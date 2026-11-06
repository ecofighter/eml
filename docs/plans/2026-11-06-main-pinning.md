# main-pinning: the main worker, pinned tasks and its park

- Created: 2026-11-06
- Base: branch `plan/sched-cooperative` at 94551e9f (`Close sched-cooperative`; PR #34, not yet merged; this branch is stacked on it, decided with the user). The file:line references below were taken at f693bf2d; locate code by content.
- Changes in sched-cooperative after this plan was written: `Scheduler.current_woken`, the `placed_owner` re-routing, `Registry::cancel_task` and `route_limit` are gone (a task's owner is written once at `start`; inbox notices are debug-asserted to name the receiver's own task, or the waiting `Task`'s worker for a token wake); `eml run --stats` no longer prints `preemptions`; the monitor starts at `begin_run` for real-clock, injected-driver and N >= 2 runs, otherwise when the first `Async` task is queued under a watched policy.
- Status: plan finalized (no unresolved item needs the user)

## 1. Goal and success criteria

- Goal: make the thread that owns the `Machine` (the process main thread in `eml run`) a **main worker**. It is an extra, eager, dedicated worker that runs *pinned* tasks and whose wait is a replaceable `Park`. A later toolkit stage (Qt 6 / WinUI 3 1.2 / AppKit, winio-style) drives its event loop through this park.
  - `main` keeps running on worker 0, which keeps every run-management duty.
  - `Async.start_on_main` / `Async.run_on_main` start pinned tasks; tasks those tasks start are pinned too.
  - The design is native-first: one loop shape for every worker. MAIN's task loop is the VM counterpart of native `eml_run_pending`.
- Success criteria:
  - A pinned task's code runs on the thread that called `execute`. A test records the thread through the Host.
  - Pinned tasks never run elsewhere. `par` children and `compute` jobs of a pinned task are placed like any other.
  - A custom `Park` that misbehaves cannot stall the clock, the deadlock verdict or the end of the run (G-4).
  - Every existing test passes with only the listed test changes. No `eml` process is left after a run.
  - eml_rt's Verus gate passes with `Control` holding N+1 words.
  - docs/ describe the main worker, and docs/future, the roadmap and the platform floor are updated (T5).

## 2. Scope

- In scope:
  - The separate seat (`Seat::Separate`): `execute` runs `Machine::new`/`load`/epilogue on the caller and spawns worker 0 inside the run.
  - MAIN as slot N: idle loop, task loop (`drive`), peer accounting, its own I/O driver on the real clock.
  - Pinned tasks: the `start_main_raw` extern, `Async.start_on_main`/`run_on_main`, inheritance.
  - `Park`/`Pending`/`Progress`/`ParkFactory` with `DefaultPark` only.
  - eml_rt `Control` with N+1 words.
  - The Windows 8 MiB main-thread stack for `eml.exe`.
  - The spec, implementation and future docs, the roadmap, the platform floor, CLAUDE.md and CI lists.
- Out of scope:
  - Every toolkit, `Std.Gui`, OS parks, `Pending::io()` / `IoDriver::pollable` (all in the toolkit stage `gui-toolkit`).
  - A GUI in `eml repl` (roadmap item "REPL GUI").
  - General dedicated workers (`rt_thread_new`, roadmap item).
  - Native code generation; this stage only fixes the native rules in docs/future/native.md.

## 3. Background context

- After `sched-cooperative`:
  - Workers 1..N-1 start at run start.
  - `start` places on workers 1..N-1 in rotation, and worker 0 runs `main`.
  - A task never leaves its worker; there is no rescue and no VM preemption.
  - The monitor relays signals, wakes workers and watches memory pressure.
- `eml_vm::execute` (`crates/eml_vm/src/lib.rs:259-300`) spawns `eml-worker-0` (8 MiB) and runs `execute_here` there: `Machine::new`, `load`, `run`, `close`. The caller masks SIGINT/SIGTERM (`eml_io::Masked`) and joins. `eml repl` and `Machine::run` run the VM on the calling thread.
- `run_scheduled` (`lib.rs:960-1170`) builds `Run`, `Registry` and `Scheduler` per run on the thread that calls it. Worker 0's `idle_wait` (`workers.rs:1111-1240`) advances the virtual clock, gives the deadlock verdict and hosts I/O on the virtual clock (`io_worker`, `workers.rs:433-441`). Peers wait in `peer_wait` (`workers.rs:1243-1268`) through `park_idle`/`join_idle` (`workers.rs:740-835`).
- `Control::new` (`crates/eml_rt/src/control.rs:383-409`) ensures `word_count() == limits.workers`. Verus-verified; gate in `crates/eml_rt/CLAUDE.md`.
- `Box<dyn IoDriver>` is not `Send` (a compile error E0277 confirmed this during design): a driver lives and dies on the thread that made it.
- `Async.start` is `start_raw (bottom f)`. `bottom` and the raw rows are not `pub`, and `Async` is abstract (`std/Async.em:9`, `:343-348`). The row is `AsyncStartRaw => holding(&[true], kinded("Std.Async.start_raw", &[TObj], &[Poly], Obj, Lin, Effectful))` (`crates/eml_extern/src/lib.rs:1248`).
- A design prototype on a clone of f693bf2d checked the separate-seat shape: a real parked MAIN thread, worker 0 spawned inside `run_scheduled`, the driver created on worker 0's thread, END raised on every exit of worker 0, and `Control` with N+1 words.
  - The whole workspace passed, 4697 tests, with no test or snapshot edited.
  - Its first run hung every `eml run`: worker 0 lowered END (`clear_end`) before MAIN read it. Only the Machine owner may lower END, after the joins.
  - The `Control` edit passed `cargo verus verify -p eml_rt`: `2102 verified, 0 errors` and `1201 verified, 0 errors`.
- `rust-toolchain`/`build.rs`: `cargo:rustc-link-arg-bin=eml=...` changed the `eml` binary's stack without affecting test binaries (checked on macOS with the macOS flag). CI sets `RUSTFLAGS`, which would silently replace `.cargo/config.toml` rustflags, so the setting goes in `build.rs`.
- Platform floor (user decision): Windows 10 1809+, WinUI 3 1.2 (Windows App SDK), Linux 6.12, Qt 6, macOS 26. CI's `macos-15` and `ubuntu-24.04` runners are below it; nothing in this stage needs a floor feature.

## 4. Prerequisites

| Needed | For | How to check | If missing |
|---|---|---|---|
| `sched-cooperative` closed | all | `git log --oneline \| grep 'Close sched-cooperative'` → one line; `git grep -n RescueAlways -- crates` → nothing | stop; do that stage first |
| Nix devShell with nextest, Verus, verusfmt, insta | all; T1 gate | `verus --version` | stop and ask |
| `.#cross` shell | T2b Windows type-check | `nix develop .#cross -c rustc --print target-list \| grep -c x86_64-pc-windows-msvc` → 1 | mark the line CI-only |
| Linux CI (ASan, TSan, Windows) | CI-only lines | the PR's CI | none |

## 5. Decisions

### 5.1 Long-term guidelines (changing these requires re-examination)

Names:
- **worker 0** runs `main`.
- **MAIN** is the main worker.
- **Machine owner** is the thread that calls `execute` or `Machine::run`.
- **A** is `Seat::Separate` (`execute`; later the native JIT/AOT entries).
- **B** is `Seat::Merged` (`eml repl`, `Machine` default).

| ID | Rule (MUST / MUST NOT) | Why | How to check |
|---|---|---|---|
| D-1 Main worker identity | MAIN MUST be the Machine owner's thread; in B worker 0 doubles as MAIN | toolkits bind to the process main thread, and `eml run`/AOT `main()` own the Machine there | `run_on_main_runs_on_the_caller_thread`; REPL transcript |
| D-2 Entry worker | Worker 0 MUST run `main`, MUST be a spawned thread in A (VM 8 MiB; native 1 GiB reservation), and MUST keep: virtual-clock advance, deadlock verdict, hosting every worker's I/O on the virtual clock, the Machine's driver (made, quiesced and dropped on its thread in A), spawning and joining workers 1..N-1, deciding END, owning the monitor | run management must not sit behind a replaceable wait (D-4) | `io_worker` unit test; `Scheduler::new` only inside `entry_body` (compile-enforced by `Box<dyn IoDriver>: !Send`) |
| D-3 Dedication | MAIN MUST NOT be returned by `next_owner` or `pop_idle`, MUST NOT join `idle`, and MUST NOT receive `par` children or `compute` jobs by placement | GUI latency must not depend on unrelated work | `placement_never_picks_the_main_worker`, `the_main_worker_never_joins_idle` |
| D-4 Liveness separation | No run-wide progress duty (clock, verdict, END, spawn/join, other workers' I/O hosting, monitor) MAY sit on a worker whose park is replaceable or that runs pinned tasks | a toolkit modal loop or a pinned extern must not stall the run | `a_park_that_never_runs_pending_still_ends_main`; `idle_wait` has no park hook |
| D-5 Pinning | A task is pinned iff `start_main_raw` started it or a pinned task started it (nested `Async.run` roots included). Pinned tasks are placed on MAIN. `par` children and `compute` jobs do not inherit. "Never moves" follows from `sched-cooperative` | thread-bound state lives in pinned tasks; parallel work must leave MAIN | `children_of_a_pinned_task_stay_on_the_main_worker`, `par_/compute_in_a_pinned_task_is_placed_on_a_worker` |
| D-6 MAIN is a peer in the accounting | MAIN parks through the peer protocol without `join_idle`. `Run.holds` is stored with Release before the `running` decrement, and read with Acquire after `running` at worker 0's two verdict sites. A `holds` change while parked is stored, and when it becomes false MAIN wakes worker 0. Before parking, MAIN checks `SlotState.woken` or a non-empty inbox under the slot lock | worker 0 advances the clock only when everyone sleeps, and the last parker wakes it; MAIN needs nothing new | `the_main_worker_parking_last_wakes_worker_0`, `a_wake_before_the_main_worker_parks_keeps_it_awake`, `a_holding_park_defers_the_deadlock_verdict*` |
| D-7 Park contract | See D-7 below the table | the clock and the verdict do not depend on the park (D-4), so the contract is "serve your own wake" | T4 tests |
| D-8 Native | `eml_run_pending` MUST NOT longjmp or unwind past its own frame; each entry goes through `eml_enter`; a nested native loop on MAIN parks as a peer in `Parked` | toolkit frames sit between the park and eml code | native stages' review (`jit-effects`) |
| D-9 Signals | Unix: only worker 0 is unmasked. The Machine owner masks SIGINT/SIGTERM from right after spawning worker 0 until it has joined it | `EINTR` must reach worker 0's blocking reads | `spawns::every_thread_but_worker_0_is_masked` |
| D-10 Shutdown | Worker 0 decides END and joins its peers. No thread MAY wait on MAIN. The Machine owner joins worker 0 after its own MAIN loop returned, then folds every `Ctx` (MAIN's last). MAIN quiesces and drops its own driver on its thread before returning. `Control::clear_end` MUST be called only by the Machine owner after both joins. END does not interrupt a running pinned task: MAIN reads it at its next safe point, or when its extern or park returns | process exit leaves through the main thread; the prototype hung when worker 0 lowered END | `exit_in_main_ends_a_run_with_a_sleeping_pinned_task`, `a_fatal_error_on_worker_1_ends_a_run_with_a_sleeping_pinned_task`, `a_panic_on_worker_0_ends_a_separate_run` |
| D-11 Configuration B | B MUST stay today's run: `entry_body` inline, no `Park`, no second thread; `Control` word N unused | the REPL has one thread and no toolkit (the REPL GUI is a roadmap item) | REPL transcripts unchanged |
| D-12 `eml_io` toolkit-free | `eml_io` MUST NOT reference any toolkit, and this stage does not change it | crate layering | `git diff --stat <base>..HEAD -- crates/eml_io` → empty |
| D-13 Counts | `--workers N` = N workers for `main` and tasks; `Finished.workers` excludes MAIN; `Finished.pinned_tasks` counts pinned starts; `Control` has N+1 words | unchanged CLI semantics | `control_keeps_a_word_for_the_main_worker`, workers tests |
| D-14 Virtual clock | The rule is unchanged (advance only while every worker, MAIN included, sleeps); the host is worker 0 | determinism | timer tests pass unchanged |
| D-15 eml_rt gate | eml_rt changes pass `crates/eml_rt/CLAUDE.md`'s gate in the same commit | Verus | T1 |
| D-16 Vocabulary | No `--main-dedicated`, `rt_dedicate_main`, `Notice::Run` or `Std.Gui` after this stage; `rt_thread_new` only as a roadmap item | deleted designs | T5 grep |
| D-17 Seat and park are Machine options | `Seat` and the park factory MUST be `RunConfig`/`Machine` options, not private to `execute`. The roots refusal MUST be worded by its cause: "code reaching session roots cannot run with a main worker on another thread" | a future asyncio-style REPL runs `Machine::run` repeatedly on the main thread with a separate seat | `a_separate_run_refuses_session_roots` checks the message |
| D-18 MAIN is a peer in I/O | On the real clock MAIN makes its own driver on first need (`io_worker(N) = N`) and hosts the handles its tasks make. On the virtual clock worker 0 hosts (`io_worker(N) = 0`). `hosts_apart` keeps its formula `Real && slots.len() > 1`. `DefaultPark` waits on MAIN's driver | without preemption, a worker running CPU work delays every I/O it hosts, so MAIN's I/O must not depend on worker 0's schedule | `io_worker` unit test (N = 3 Separate: Virtual `[0,0,0,0]`, Real `[0,1,2,3]`); `a_pinned_listener_serves_a_worker_0_client` |
| D-19 Time base persists | `Machine.run_clock` is created at the first run after `new`, `with_clock` or `with_sched`, and kept across runs in both seats | repeated runs (REPL, a future GUI REPL) see one time base | `a_separate_machine_runs_twice` |

D-7, the park contract:
- `Park` replaces only MAIN's blocking primitive. Every wake rings `Slot::ring`.
- Latching: a ring before `wait` begins makes that `wait` return or call `run()`.
- `run()` is `Busy` unless the innermost loop on the thread is `Parked` (three-state guard).
- The default park is MAIN's own driver wait when MAIN has a driver (real clock; `timeout` = MAIN's earliest deadline), else the slot condvar (virtual clock: `None`; worker 0 wakes MAIN for due timers).
- A park MUST also serve MAIN's driver readiness. `DefaultPark` does by waiting on the driver; a toolkit park will do so through `Pending::io()`, which the toolkit stage adds.
- A park calls `run()` or returns when rung or when `holds()` changed, and returns after `Stopping`.
- `Progress = { Wait(Option<Duration>), Stopping, Busy }`.

Kept in the repository:
- D-1..D-4, D-6, D-7, D-10, D-11, D-13, D-14, D-17, D-18, D-19: docs/spec/vm.md 「メインの worker」 and 「メインの worker の待ち」 (T2b, T3, T4), and docs/future/multicore.md (T5).
- D-5: stdlib.md 「`Std.Async`」 (T3).
- D-8: native.md (T5).
- D-9: vm.md 「上限と割り込み」 (T2b).
- D-12, D-15, D-16: CLAUDE.md and architecture.md (T2b, T5).

Rejected alternatives (do not relitigate):
- MAIN hosts run management, i.e. the clock, the verdict and the driver move to the main thread. Then a toolkit park has to cooperate for time to pass, and a pinned extern stalls everyone's virtual-clock I/O.
- A dedicated control thread: one more thread per run for what worker 0 already does.
- Decentralised clock and verdict: a new protocol for no gain.
- MAIN also runs `main`: native `main` needs a big stack, and `--workers 1` would serialise the GUI and `main`.
- Dedicating the main thread lazily (`rt_dedicate_main`, `--main-dedicated`): nothing to dedicate.
- MAIN without a driver, worker 0 hosting its I/O: no preemption means worker 0's CPU work would delay pinned I/O.
- `Std.Gui.run_on_main` in this stage: the user decided `Std.Async` hosts it and `Std.Gui` comes with the toolkit.
- An egui/winit plugin: replaced by a thin toolkit over Qt/WinUI/AppKit.
- Input-hook REPL GUI: superseded by the asyncio pattern.
- A shutdown timeout for a stuck park: nondeterministic tests; the second-signal `_exit` exists.

Revisit when: the toolkit stage cannot integrate an OS loop through `Park` plus `Pending::io()`; or a second thread-bound library appears (general dedicated workers).

### 5.2 Native accounting (kept in docs/future/native.md by T5)

| Structure | Native form | This stage |
|---|---|---|
| Task cell and registry | the shared `cells.rs` core through `extern "C"` entries | `TaskCell.pinned`; the `start_main_raw` row |
| Continuation | a parked task holds `k`; `resume_task k w` | unchanged; `k` never leaves its worker |
| Ready queue and inbox | per-worker queue and `Notice` inbox | MAIN's queue is slot N; `Slot::ring` also rings the park waker |
| `Async.run` nesting | native nests the worker loop | a nested loop on MAIN parks as a peer in `Parked` (D-8) |
| Join helping | native helps its own deeper forks | MAIN helps only its own joins |
| `eml_enter`/`JmpBuf`/`eml_fatal` | asm stub, nested JmpBufs | `eml_run_pending(ctx) -> Progress`: each entry via `eml_enter`; a fatal inside returns `Stopping` to the park and never longjmps past toolkit frames |
| `Ctx` fields read by native code | stop-word pointer, `stack_limit`, `jmp`, `worker`, … | MAIN's `Ctx` is `Ctx::for_worker(.., N)` with stop word N; `stack_limit = OS stack base + 2 MiB` |
| AOT `main()` | spawns worker 0 (1 GiB); `main` there | builds `Control` (N+1), spawns worker 0, runs MAIN's loop with the default park, joins, folds, returns the exit code |
| I/O | per-worker driver on the real clock; forwarding on the virtual clock | MAIN is one more such worker; its park waits on its driver |

### 5.3 Short-to-mid-term design

#### D-30: Thread roles

| Work | A | B |
|---|---|---|
| `Machine::new`, `load`, `run_entry` prologue and epilogue (fold, `settle`, `end_check`, `sweep_all`, `reclaim_all`, checks, `close`) | caller (Machine owner = MAIN's thread) | caller |
| `entry_body`: Machine driver creation/quiesce/drop, `Scheduler::new`, `begin_run`, eager spawn, `vm::run` of `main`, clock, verdict, virtual I/O hosting, END, joining peers | spawned `eml-worker-0` (8 MiB), unmasked | caller (today) |
| MAIN: per-run `Ctx::for_worker(control, host, N)`, `Scheduler::worker(policy.for_worker(N), .., N, ..)`, pinned tasks, `Park`, own driver on the real clock | caller, masked from after the spawn until the join | caller = worker 0 |
| workers 1..N-1 | spawned by worker 0 inside `entry_body`'s scope | same |

`Run::new(workers, seat, bell, clock, counters)`:
- `slots = workers + (separate as usize)`;
- `main() = if separate { workers } else { 0 }`;
- slot `main` starts `parked = true` when separate, with `running = 1`, `live = 1` and `next_place = 1`;
- accessors `workers()`, `main()` and `separate()`;
- `Registry::new(generations, run.slots.len())`;
- `spawn_workers` iterates `1..run.workers()`, and both tests in `begin_run` use `run.workers() >= 2`;
- `relay`, `wake_all` and `pressure_tick` cover every slot.

`hosts_apart` (`Real && slots.len() > 1`) and `io_worker` (`if Real { k } else { 0 }`) keep their formulas. The literal 0 stays at `park_idle`'s `wake(0)`, `idle_wait`'s `k != 0` and `entry_finish`'s `index != 0`.

#### D-31: `execute`, `run_scheduled`, `entry_body` (shape checked by the prototype)

```text
execute(program, linked, config, host):
  machine = AnyMachine::new(config.clone().with_seat(Separate), host); load; run; close   // all on the caller

Machine::run_entry (both seats):
  if Separate && (code.tables.uses_roots || ctx.roots().len() > 0)
      -> Internal("code reaching session roots cannot run with a main worker on another thread")
  if Separate && self.driver.is_some()
      -> Internal("a separate run makes its driver on worker 0")
  run_clock = self.run_clock.get_or_insert_with(|| self.clock.start()).clone()   // D-19; with_clock/with_sched reset run_clock and driver
  B: driver = self.driver.take().or_else(|| run_clock.driver(seed))   (today's error mapping)
  A: driver = None
  (result, scheduled, driver_back) = run_scheduled(..); self.driver = driver_back   // None after an A run

run_scheduled(code, ctx, task, policy, start, counts):
  run = Arc(Run::new(N, seat, ..)); registry = Registry::new(gens, run.slots.len()); schedule_signals
  cells = Arc(Shared::new(registry)); roots_free = !uses_roots && roots().len() == 0
  B: return entry_body(code, ctx, task, policy, start.driver, args, counts)
  A: main_ctx = Ctx::for_worker(control, host, N); main_base = HeapWork::of(main_ctx.heap().stats())
     main_sched = Scheduler::worker(policy.for_worker(N), run, control, N, main_ctx.heap().worker(), max_depth, cells)
     park = main_park_factory.map_or(DefaultPark, |f| f()); run.slots[N].park_waker.set(park.waker())   // T4
     thread::scope(|scope| {
        guard = Shutdown { control, run, armed: true }          // a caller panic -> END + wake_all; the scope joins worker 0
        worker = Builder::new().name("eml-worker-0").stack_size(WORKER_STACK)
                   .spawn_scoped(scope, || { let (r, s, d) = entry_body(code, ctx, task, policy, None, args, counts); drop(d); (r, s) })
                   // spawn Err(e): guard.armed = false; return (Err(Fault{Driver(e), "<worker 0>"}), Scheduled::empty(cells), None)
        _blocked = eml_io::Masked::new()                          // D-9
        main_done = catch_unwind(|| main_work(code, &mut main_ctx, &mut main_sched, &mut park, &mut main_counts))
        (result, scheduled) = worker.join().unwrap_or_else(resume_unwind)
        drop(guard)
     })
     control.clear_end()                                          // D-10: only here, after both joins
     fold MAIN: Retired { ctx: main_ctx, counts, preemptions, peak_stack, base: main_base, left, kept_pending } pushed last into scheduled.others
     if main_done panicked: set_fatal("<main worker>") (done inside), leak main_ctx (ManuallyDrop), payload into scheduled.panic
     (result, scheduled, None)

entry_body(code, ctx, task, policy, driver, args, counts) -> (Result, Scheduled<M>, Option<Box<dyn IoDriver>>):
  shutdown = Shutdown { armed: args.separate }        // FIRST statement: every exit below ends the run in A
  driver = driver.or_else(run.entry_driver(seed))      // Err(e) -> return (Err(code.error(Driver(e), task)), Scheduled::empty(cells), None)
  sched = Scheduler::new(policy, driver, run, control, task.worker, max_depth, cells); sched.begin_run(roots_free)
  thread::scope(|scope| { eager spawn; shutdown.armed |= !handles.is_empty(); result = vm::run(..); drop(shutdown); join peers })
  fold peers, settle(result, run.take_fatal()), quiesce the Machine driver (today's lib.rs:1092-1104), end_check, sweep_all,
  stats, reclaim_all if workers > 0 || separate, return (result, Scheduled { .. without driver }, sched.driver.take())
  // B only: clear_end after this returns when workers > 0 (today's rule)
```

- `Scheduled` loses its `driver` field. The driver is returned beside it, because a `Box<dyn IoDriver>` cannot cross to worker 0 inside a value. `Scheduled::empty(cells)` carries `generations = cells.lock().next_generations()` and zeros.
- In A the caller appends MAIN's heap to `others`; `at_run_end`, `check_heap` and `close` see it like a peer heap (checked mode: leak and owner checks).
- A worker 0 panic unwinds its closure. `shutdown` drops (END + `wake_all`), `join()` returns `Err`, and the caller calls `resume_unwind` (today's behaviour).
- A MAIN panic is caught in `run_scheduled`: `set_fatal`, END, `wake_all`, leak `main_ctx`, and put the payload into `scheduled.panic`. The Machine is poisoned as today.
- `exit n` in a pinned task: `entry_finish` (index N ≠ 0) raises END, and worker 0 ends `Abandoned` with the code (the peer path).
- Interrupt and terminate set bits in all N+1 words. Relay → `wake_all` → MAIN sees the stop word at its next safe point or wait.
- `Machine::new`, `load` and the epilogue run on the caller. The LIR passes and the VM codegen do not recurse with IR size (architecture.md:320); the frontend already runs on the caller. A caller with Rust's default 2 MiB stack suffices for `execute`, and `eml.exe` reserves 8 MiB for the frontend (D-33).

#### D-32: MAIN's loop

- T2b (idle): `main_work = loop { main_wait()?; if stopping(control, N) { break quiet }; if a notice or a ready item exists → internal error "a task reached the main worker" }`. On exit: `left_notices`, `retire_driver` (quiesce MAIN's own driver on MAIN's thread, record completions and `kept_pending`), `exit_worker(N)`.
- T3 (tasks): `main_work = loop { drive()?; main_wait(park)? }`.
- `drive(code, ctx, task, sched, counts)`:
  - Read the stop word; exit quietly on END, INTERRUPT, TERMINATE or MEMORY.
  - Call `switch_away(.., Some(TaskState::Ready))`. The idle placeholder `Task` is parked at the `ENTRY` slot as `work_loop` does for peers (`vm.rs:858-882`).
  - `Err(HOST_IDLE)` → return `Ok(())`. Other faults → `task_failed`.
  - Then `run_tasks`: `Err(Fault{HOST_IDLE})` → `Ok(())`; `Ok(())` → internal error "a pinned task reached the end of a run"; `Err(e)` → `Err(e)`.
- `switch_away` at `sched.idle_wait()?` (`vm.rs:1933`), when `sched.main_loop` (set only on MAIN's scheduler in A): `if sched.current != ENTRY { resume::<R>(code, ctx, task, sched, ENTRY, keep)?; } return Err(HOST_IDLE);`.
  - `const HOST_IDLE: VmFault = VmFault::Internal("the main worker has nothing to run")` sits next to `CHILD_FAILED`.
  - `task_failed` returns `Err(RuntimeError::Fault { fault: HOST_IDLE, function: String::new(), at: None })` at the top of its loop.
  - Every `switch_away` call site returns its `Result` into a step or `task_failed` (`vm.rs:866, 1785, 1803, 1827, 1866, 2384, 2570, 3832`). Natively the loop simply returns when the queue is empty.
- `main_wait(park)`:
  - `holds = park.holds(); park_main(N, holds)`. `park_main` stores `Run.holds` with Release, then sets `parked = true` and `running -= 1`; the last parker wakes worker 0. Under the slot lock it first checks `woken || !notices.is_empty()`; if either holds, it clears `woken` and returns false.
  - If MAIN did not park, return.
  - `timeout` = Real with a driver ? its earliest deadline − now : `None`.
  - Loop:
    - `guard = Parked; r = park.wait(timeout, &mut pending); guard = Outside`.
    - Propagate `pending.stop`; `r?` → `VmFault::Driver`.
    - `if take_wake(N) || !parked(N) { unpark; return }`.
    - `h = park.holds(); if h != holds { holds = h; run.holds.store(h, Release); if !h { run.wake(0) } }`.
  - A park that returns with nothing new keeps MAIN parked: no accounting change.
- `Pending::run`, with the guard `Outside | Parked | Running | Stopped`:
  - `Outside` or `Running` → `Busy`.
  - `Stopped` → `stopped_calls += 1; debug_assert!(stopped_calls < 64, "Park::wait ignored Progress::Stopping")`, then `Stopping`.
  - `Parked` → `guard = Running; unpark(N)`, then loop:
    - `drive()`; on `Err` → `stop = Some(e); guard = Stopped; return Stopping`;
    - `park_main(N, holds)`; if woken → `unpark; continue`;
    - `guard = Parked; return Wait(own timeout)`.
- `Pending::default_wait` = the T2b primitive (MAIN's driver `wait(deadline)` when it has a driver, else the condvar). `DefaultPark::wait` = `pending.default_wait()`.
- `MainPending<'a>` reborrows `code, ctx, task (placeholder), sched, counts`. It is built per `park.wait` and dropped before `sched` is touched again (a sketch of this borrow shape type-checked during design). The `Park` is a local `Box<dyn Park>` in the A branch, passed down as `&mut dyn Park`.

#### D-33: API, rows and stack

```rust
pub enum Seat { Merged, Separate }   // RunConfig::with_seat, Machine::with_seat
pub trait Park {
    /// Block until the waker rings, `timeout` passes, or the park chooses to return. May call `pending.run()`.
    fn wait(&mut self, timeout: Option<Duration>, pending: &mut dyn Pending) -> std::io::Result<()>;
    fn waker(&self) -> Option<Arc<dyn IoWaker>>;
    /// The toolkit has pending external waits: worker 0 reports no deadlock while MAIN is parked with this true.
    fn holds(&self) -> bool { false }
}
pub trait Pending { fn run(&mut self) -> Progress; fn default_wait(&mut self) -> std::io::Result<()>; }
pub enum Progress { Wait(Option<Duration>), Stopping, Busy }
pub type ParkFactory = Arc<dyn Fn() -> Box<dyn Park> + Send + Sync>;   // RunConfig::with_main_park, Machine::with_main_park
```

- Slot and run fields:
  - `Slot.park_waker: OnceLock<Arc<dyn IoWaker>>`, set once per run before `main_work`.
  - `Slot::ring(&self)` = `cv.notify_one()` + the driver waker if any + the park waker if any. `wake` and `send` call it after releasing the slot lock.
  - `Run.holds: AtomicBool`. Worker 0's two verdict sites in `idle_wait` treat `holds` like pending work: they sleep on the condvar instead of returning `Err(Deadlock)`.
- `api.rs` asserts `send::<ParkFactory>(); sync::<ParkFactory>();`.
- `TaskCell.pinned: bool`, set by `Registry::start` from a `pinned` argument.
  - Inheritance: `start_raw`, `start_in_raw` and `run_raw` read the starter's `pinned` inside the registry lock they already hold.
  - `Registry.pinned_tasks: u64` → `Finished.pinned_tasks`.
  - `place_owner(sched, pinned) = if pinned { run.main() } else if places() { next_owner() } else { me() }`. `run_raw` roots stay on the caller.
- `std/Async.em`: put the two functions after `compute`, and the extern at the end of the extern list, so that the snapshot positions `:75:9` and `:171:25` do not move.

  ```
  pub start_on_main : (Unit -> <IO, Panic, Async> a) -> <IO, Async> Task a
  start_on_main f = start_main_raw (bottom f)
  pub run_on_main : (Unit -> <IO, Panic, Async> a) -> <IO, Panic, Async> a
  run_on_main f = await (start_on_main f)
  extern start_main_raw : (Unit -> <IO, Async> Unit) -> <IO> Task a
  ```
- eml_extern: add `AsyncStartMainRaw => holding(&[true], kinded("Std.Async.start_main_raw", &[TObj], &[Poly], Obj, Lin, Purity::Effectful))` next to `AsyncStartRaw`. `is_async` includes it.
  - Counts: 94 → 95 externs, `Std.Async` 36 → 37 rows, held rows 18 → 19 (declarations.md:121, stdlib.md:35).
  - VM arm `AsyncStartMainRaw`: like `AsyncStartRaw` (`vm.rs:2530-2546`) with `pinned = true` and owner `run.main()`. In B it is `start_raw` with owner 0 and the bit.
- Windows stack: `crates/eml_cli/build.rs` emits `cargo:rustc-link-arg-bin=eml=/STACK:8388608` when the target env is `msvc` and `CARGO_FEATURE_RUN` is set.
  - The Windows-only test `cli::eml_exe_reserves_an_8_mib_main_stack` reads `SizeOfStackReserve` from the PE header: `e_lfanew` at 0x3C, PE32+ magic 0x20B, a u64 at `e + 24 + 72` == 8388608.
  - Fallback if Windows test threads overflow: set `RUST_MIN_STACK` in CI.

#### D-34: Tests

Files: `crates/eml_vm/tests/seat.rs` (T2b) and `pinned.rs` (T3, T4), declared in `tests/main.rs`.
- Every test runs on the virtual clock unless it says Real, finishes in under 1 s alone, and makes no wall-clock assertion.
- A recording Host in `tests/common` wraps `CaptureHost` and records `(line, ThreadId)` per `print_line`. The patterns exist: `RaisingHost` was in tests/preempt.rs before `sched-cooperative` (recreate it in common), and `BlockingHost` is in tests/workers.rs.

## 6. Implementation constraints and conventions

- Each task is one commit that passes `cargo nextest run`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` on its own.
- docs/spec changes go into the implementing commit. docs/future, the roadmap and the benchmark tables change at the close (T5).
- Japanese for docs and comments through PLaMo (CLAUDE.md); this plan's English texts are the content. No space between Japanese and Latin text in edited paragraphs.
- Test hygiene for local runs:
  - an outer `timeout 600` or less, and targeted `-E` filters while iterating;
  - after any run, `ps -axo pid,command | grep 'target/debug/eml ' | grep -v grep | awk '{print $1}' | xargs kill -9`; zsh does not word-split `$VAR`.
  - A wrong END order shows up as hung `eml run` processes, not as failures.
- Only the listed test changes are allowed. Any other diff in a test, a snapshot or `RunStats` stops the stage for a plan amendment. Suspect MAIN's empty heap in `peak_objects` first; if it shows, exclude a heap that ran no task from the max.
- No `cfg` in `vm.rs`; OS code stays in `eml_io` and `eml_cli` (D-12).
- The plan IDs never appear in code or docs.

## 7. Implementation tasks

| ID | Task | Based on | Depends on | Parallelizable |
|---|---|---|---|---|
| T0 | Commit this plan | — | sched-cooperative closed | no |
| T1 | eml_rt: `Control` with N+1 words | D-13, D-15 | T0 | no |
| T2a | Vocabulary, byte-identical | D-30 | T1 | no |
| T2b | Separate seat with an idle MAIN | D-1, D-2, D-3, D-6, D-9, D-10, D-11, D-17, D-18, D-19, D-30, D-31, D-32, D-33 | T2a | no |
| T3 | Pinned tasks and MAIN's task loop | D-5, D-32, D-33, D-34 | T2b | no |
| T3b | UI files and REPL transcript | D-5 | T3 | no |
| T4 | Park | D-4, D-6, D-7, D-8, D-33 | T3b | no |
| T5 | Close: docs/future, roadmap, platform floor, CI lists, benchmarks | all | T4 | no |

Let `B0` be the test count after `sched-cooperative` closed (`cargo nextest run` → `B0 passed`); record it before T1. Recorded at 94551e9f: B0 = 4649 passed, 21 skipped.

### T0: the plan
- Work: add this file as `docs/plans/2026-11-06-main-pinning.md`.
- Completion: `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` → passes.

### T1: eml_rt `Control` with N+1 words
- Targets: `crates/eml_rt/src/control.rs`, `docs/spec/object-model.md`.
- Work:
  - `Control::new` requires `1 <= limits.workers < usize::MAX` and ensures `word_count() == limits.workers + 1`.
  - `stop_words` and `byte_counters` take `limits.workers + 1`.
  - Doc comments: word N is MAIN's in a separate run and unused in a merged run.
  - `Ctx::for_worker(.., k)` keeps requiring `k < word_count()`.
  - Fix the doc comments and `heap/tests/control.rs:417` that state the old count.
  - New unit test `control_keeps_a_word_for_the_main_worker` (`Limits { workers: 3 }` → 4 words).
- Spec: object-model.md 「実行の制御」, worker-word row: "`Control::new` makes `limits.workers + 1` words and `taken` counters: words 0..N−1 for the workers that run `main` and tasks, word N for the main worker of a separate run (unused in a merged run)."
- Completion criteria:
  - `cargo verus verify -p eml_rt` → both summary lines `0 errors`.
  - `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` → exit 0.
  - The rest of `crates/eml_rt/CLAUDE.md`'s gate.
  - `cargo nextest run` → `B0 + 1 passed`, 0 failed.
  - clippy and fmt → exit 0.

### T2a: vocabulary, byte-identical
- Targets: `crates/eml_vm/src/{workers.rs, sched.rs, cells.rs, lib.rs, monitor.rs}`.
- Work:
  - `pub enum Seat` (only `Merged` used).
  - `Run::new(workers, seat, ..)` with `workers()`, `main()` and `separate()`.
  - `Run::entry_driver(seed)`.
  - The `slots.len()` rows of D-30.
  - `Registry::new(.., slots.len())`.
  - Rename `sched::MAIN` (the `TaskId` constant) → `ENTRY`.
  - `Slot::ring` (no park waker yet).
- Test changes: kind 3 — the `Run::new` call sites in src unit tests gain `Seat::Merged` (about 27 sites).
- Completion criteria:
  - `cargo nextest run` → `B0 + 1 passed`.
  - `git diff -U0 HEAD~ -- crates/eml_vm/src | grep -E '^[-+].*(assert|@")'` → no output.
  - `git diff --stat HEAD~ -- 'crates/*/tests' '*.snap'` → empty.
  - clippy and fmt → exit 0.

### T2b: separate seat with an idle MAIN
- Targets:
  - `crates/eml_vm/src/{lib.rs, workers.rs, sched.rs}`;
  - new `crates/eml_cli/build.rs`;
  - `crates/eml_vm/tests/{main.rs, seat.rs, common}`;
  - `crates/eml_cli/tests/cli.rs`;
  - docs (below).
- Work:
  - D-30 and D-31 in full: `execute`, `RunConfig::with_seat`, `Machine::with_seat`, `run_clock: Option<RunClock>` (D-19), the two refusals, the `run_scheduled` split, `entry_body` with `Shutdown` armed first, MAIN's `Ctx`/`Scheduler::worker` and fold, `clear_end` only after both joins, and the `reclaim_all` condition.
  - D-32's idle `main_work`, `park_main` and `exit_worker(N)`.
  - D-18: MAIN's own driver on first need through the peer path.
  - D-33's `build.rs`.
- New tests:
  - Unit:
    - `a_separate_run_adds_a_parked_main_slot`;
    - `placement_never_picks_the_main_worker`;
    - `io_worker_gives_the_main_worker_its_own_driver_on_the_real_clock` (Separate N = 3: Virtual `[0,0,0,0]`, Real `[0,1,2,3]`; Merged Real `[0,1,2]`);
    - `the_main_worker_never_joins_idle`;
    - `the_main_worker_parking_last_wakes_worker_0`;
    - `a_wake_of_the_parked_main_worker_counts_it_once`;
    - `a_wake_before_the_main_worker_parks_keeps_it_awake`.
  - `seat.rs`:
    - `a_separate_run_refuses_session_roots` (checks the D-17 message);
    - `a_separate_machine_refuses_an_injected_driver`;
    - `a_separate_machine_runs_twice` (Real; both runs Ok; the second `Async.now` ≥ the first);
    - `a_panic_on_worker_0_ends_a_separate_run` (a Host whose `print_line` panics on `"boom"`: `execute` panics on the caller with that payload, the scope returns, and a following `execute` on a fresh Machine works);
    - `worker_0_is_a_spawned_thread_in_a_separate_run` (recording Host: the `main` line's thread ≠ the test thread).
  - cli.rs (`#[cfg(windows)]`): `eml_exe_reserves_an_8_mib_main_stack`.
- Existing tests: no expected-value change (prototype: 4697/4697 unchanged).
- Spec and docs (same commit):
  - vm.md 「上限と割り込み」 :400, replace with: "`eml run` calls `eml_vm::execute`. `execute` builds the `Machine`, loads the program and runs on the calling thread (the Machine owner; in `eml run` the process main thread), which is the main worker during the run (「タスクとスケジュール」の「メインの worker」). `main` runs on worker 0, a thread `eml-worker-0` with an 8 MiB stack that `run_scheduled` spawns and joins inside the run. The caller masks SIGINT and SIGTERM with `eml_io::Masked` from just after spawning worker 0 until it has joined it; it does not mask earlier, because a new thread inherits its creator's mask. The frontend compile (`eml_cli::Session`) and `Machine::load` run on the caller before worker 0 exists, so a signal then is taken by the handler as a bit and read at the first safe point. If worker 0 cannot be spawned, the run ends with the driver error (`the I/O driver failed: {e}`) in function `<worker 0>`. `eml repl` spawns no worker 0 and runs the VM on its own thread (`Seat::Merged`)."
  - vm.md 「タスクとスケジュール」, new `### メインの worker` after the policies: "A separate run (`Seat::Separate`: `execute`, or a `Machine` with `with_seat(Separate)`) has one more worker than `Limits::workers`: the main worker, slot N, on the thread that owns the `Machine`. It exists from the start of the run with its own `Ctx` (`Ctx::for_worker(.., N)`) and starts parked. It never joins `idle`, is never chosen by `next_owner` or `pop_idle`, makes its own I/O driver on the real clock when it first needs one, like workers 1..N-1 (on the virtual clock worker 0 hosts its I/O), and carries no run-wide duty: the clock, the verdict, END and the monitor stay with worker 0. It waits through the peer protocol without `idle`: under its slot lock it sets `parked`, decrements `running` and, as the last parker, wakes worker 0. Before it returns, it quiesces and drops its driver on its own thread like workers 1..N-1. In a merged run (`eml repl`, the `Machine` default), worker 0 is the main worker and none of this applies."
  - vm.md 「時計と driver」:
    - :536 add: "In a separate run the main worker is a host like workers 1..N-1: on the real clock it makes its own driver on first need and binds the handles its tasks make; on the virtual clock worker 0 hosts its I/O. Worker 0 makes the Machine's driver on its own thread in a separate run (`Run::entry_driver`), and the `Machine` keeps none after the run. The time base `run_clock` persists across runs in both seats. An injected driver (`with_driver`) requires a merged run."
    - :545 (the quiesce sites): add "and the main worker, before it returns".
  - vm.md 「誤り、止めること、終わり」 :352-353 add: "In a separate run, worker 0 raises END on every exit of its body (a `Shutdown` guard armed from its first statement) so that the main worker wakes. The Machine owner joins worker 0 only after its own main-worker loop returned, folds the main worker's `Ctx` last, and only then lowers END (`Control::clear_end`). No thread waits on the main worker. Code reaching session roots cannot run with a main worker on another thread (`Seat::Separate` refuses it with an internal error)."
  - vm.md 「`RunStats`」 :1153: "`workers` excludes the main worker; the main worker's heap is folded like a peer heap".
  - architecture.md:322-323, replace the `execute` bullet: "`lib.rs`'s `execute` builds a `Machine` with `Seat::Separate`, loads and runs on the calling thread, which is the main worker during the run (docs/spec/vm.md 「メインの worker」). `run_scheduled` spawns worker 0 (`eml-worker-0`, `WORKER_STACK` 8 MiB) inside the run; the caller masks SIGINT and SIGTERM until it has joined worker 0. The entry worker's body (`entry_body`: the driver, `Scheduler::new`, `begin_run`, eager spawn of workers 1..N-1, `vm::run`, END, joins, quiesce) runs inline on the caller in a merged run. Drivers: on the real clock one per worker, the main worker included, made on first need; on the virtual clock only worker 0's. A caller thread with Rust's default stack (2 MiB) suffices for `execute`: the LIR passes and the VM codegen do not recurse with the size of the IR; `eml.exe` reserves 8 MiB for the frontend (`crates/eml_cli/build.rs`, `/STACK`)." 「CLI と lib API」: `Seat`, `with_seat`.
  - testing.md:594: "Stack overflow: in `eml.exe` the main thread reserves 8 MiB through `crates/eml_cli/build.rs` (`/STACK:8388608`, checked by `cli::eml_exe_reserves_an_8_mib_main_stack`); the command body is not moved to another thread, because the process main thread is the main worker. Elsewhere fix the recursion."
  - CLAUDE.md:97 (the eml_vm row) and :122: "`eml run` loads on the calling thread, which is the main worker (an extra parked worker with a replaceable `Park`), and runs `main` on a spawned worker 0 that keeps the clock, the deadlock verdict, the Machine driver and END; on the real clock every worker, the main worker included, makes its own I/O driver when it first needs one".
- Left to the implementer: the shape of `Scheduled::empty` and the `EntryArgs` struct; summing MAIN's `RunCounts` into `counts`.
- Completion criteria:
  - `cargo nextest run` → `B0 + 1 + 12 passed` (13 on Windows), 0 failed, and no `TIMEOUT` or `SIGKILL` line.
  - `ps -axo command | grep 'target/debug/eml ' | grep -v grep` → nothing.
  - `git diff --stat HEAD~ -- 'crates/*/tests' '*.snap'` → only `crates/eml_vm/tests/main.rs`, `seat.rs`, `common/*` and `crates/eml_cli/tests/cli.rs`.
  - `cargo nextest run -j 1 -p eml_vm --test integration -E 'test(seat::)'` → every line `PASS [ 0.` (under 1 s).
  - `cargo clippy --all-targets -- -D warnings` and `cargo clippy -p eml_cli --no-default-features --features types -- -D warnings` → exit 0.
  - `nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings` → exit 0.
  - `cargo nextest run -p eml_vm --test integration -E 'test(spawns::)'` → passes. The spawn site stays in lib.rs.
  - CI-only: the Windows job passes the PE test.
- Pitfalls:
  - END must be lowered only by the Machine owner after both joins. The prototype hung when worker 0 did it.
  - `Shutdown` must be armed at `entry_body`'s first statement, or an early error (a driver creation failure) leaves MAIN parked and `eml run` hangs.

### T3: pinned tasks and MAIN's task loop
- Targets:
  - `crates/eml_extern/src/lib.rs`, `std/Async.em`;
  - `crates/eml_vm/src/{vm.rs, cells.rs, sched.rs, lib.rs, workers.rs}`;
  - `crates/eml_core_ir/tests/externs.rs`, `crates/eml_cli/tests/ui.rs`;
  - `crates/eml_vm/tests/{main.rs, pinned.rs}`;
  - `.github/workflows/ci.yml`;
  - docs.
- Work: D-32's `drive`, `HOST_IDLE` and `main_loop`; D-33's pinning, the row, the std functions and the VM arm; `Finished.pinned_tasks`.
- New tests (`pinned.rs`):
  1. `run_on_main_runs_on_the_caller_thread`: N = 1; the pinned line is on the test thread, the `main` line is not.
  2. `children_of_a_pinned_task_stay_on_the_main_worker`: N = 2.
  3. `par_in_a_pinned_task_is_placed_on_a_worker`: `placed ≥ 1`.
  4. `compute_in_a_pinned_task_is_placed_on_a_worker`: `placed_jobs == 1`, with `main` idle.
  5. `exit_in_a_pinned_task_ends_the_run`: the exit code.
  6. `exit_in_main_ends_a_run_with_a_sleeping_pinned_task`.
  7. `a_fatal_error_on_worker_1_ends_a_run_with_a_sleeping_pinned_task`: checked mode; no `TransferLeak`.
  8. `run_on_main_in_a_merged_machine_runs_on_worker_0`: B.
  9. `a_pinned_timer_advances_the_virtual_clock`: N = 1; pinned `sleep 20`, `main` `sleep 10`; virtual order.
  10. `a_pinned_timer_fires_on_the_real_clock`: Real, N = 1; no time assertion.
  11. `a_pinned_listener_serves_a_worker_0_client`: Real, N = 1.
  12. `a_socket_moves_from_a_pinned_task_to_worker_0`: Real, N = 1.
  13. `an_error_in_main_ends_a_run_with_a_pinned_read_in_flight`: Real, N = 1, checked; ASan in CI. The shutdown order is: END reaches MAIN; MAIN quiesces its driver on its thread; the caller joins worker 0; the fold frees the lent buffer.
  14. `a_pinned_task_closing_a_socket_wakes_a_sleeping_worker_0`: Real, N = 1. A worker-0 task reads until EOF on a socket worker 0 accepted; the pinned task holds the other end and drops it. No wall-clock assertion.
  15. `pinned_tasks_are_counted`: `Finished.pinned_tasks`.
  - Unit: `a_pinned_start_is_placed_on_the_main_worker` (registry level, Merged and Separate).
- Test changes:
  - Kind 2. Scope: "add `Std.Async.start_main_raw` to the extern-table expectations": `crates/eml_core_ir/tests/externs.rs`, the `(&[Poly], Lin)` arm (about :633) and the held list (about :706).
  - Kind 3: `crates/eml_cli/tests/ui.rs` `Finished { .. }` literals gain `pinned_tasks: 0`.
- Spec and docs (same commit):
  - stdlib.md 「`Std.Async`」, new `### メインの worker`: "`start_on_main f` starts `f` as a task on the main worker (the thread that owns the `Machine`: the process main thread in `eml run`, worker 0 in `eml repl`) and returns its handle; `run_on_main f` is `await (start_on_main f)`. Tasks a pinned task starts are pinned too; `par` children and `compute` jobs are not. Pinned tasks never move. Thread-bound resources (GUI toolkits) are used only from pinned tasks."
  - stdlib.md 「extern の境界」: 18 → 19 held rows, and add the `start_main_raw` row (closure held).
  - declarations.md:121: 95 externs, `Std.Async` 37 rows.
  - vm.md 「`Async` のタスク」 after the placement sentences: "`start_main_raw` places on the main worker and marks the task pinned; a task started by a pinned task (`start_raw`, `start_in_raw`, a nested `run_raw` root) is pinned too; `par` children and `compute` jobs are not. `Finished.pinned_tasks` counts pinned starts."
  - vm.md `### メインの worker`, add: "Pinned tasks run there. The loop `drive` takes notices, runs the ready queue until it is empty and returns to the wait; the VM leaves its task loop at the empty-queue point with the internal signal `HOST_IDLE`, so that no VM state is on the Rust stack while the main worker waits. On END, INTERRUPT or TERMINATE the main worker exits like a peer; a running pinned task reads the stop word at its next safe point."
  - vm.md 「`RunStats`」: `pinned_tasks`.
  - status.md 「実装したもの」: the main worker and pinned tasks.
  - testing.md 「AddressSanitizer」 and 「ThreadSanitizer」, the ASan/TSan lines in CLAUDE.md and ci.yml: add `seat:: pinned::`.
- Completion criteria:
  - `cargo nextest run` → `B0 + 13 + 16 passed` (+1 on Windows), 0 failed.
  - `cargo nextest run -j 1 -p eml_vm --test integration -E 'test(pinned::)'` → each test under 1 s.
  - `cargo nextest run -p eml_hir -p eml_core_ir` → passes.
  - `test ! -e std/Gui.em` → exit 0.
  - `ps … | grep 'target/debug/eml '` → nothing.
  - `nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml` → exit 0.
  - CI-only (Linux): `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=address CARGO_TARGET_DIR=target/asan cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_vm --test integration -- seat:: pinned::` → `test result: ok`, and the TSan line with `seat:: pinned::` → ok. A report there is a real race in the new code: fix it before merging.

### T3b: UI files and REPL transcript
- Targets:
  - `tests/ui/run/async/run_on_main_returns.em`, `run_on_main_nested_start.em`;
  - `tests/ui/run-fail/async/run_on_main_exit.em`, `run_on_main_panics.em`;
  - `crates/eml_cli/tests/repl/async/run_on_main.repl`. At the REPL top level, wrap the call in `Async.run`.
- Each `async/` file follows testing.md 「seed で流す」 (`spin 40` after a start; output order independent of the schedule).
- Completion criteria:
  - `EML_UI_FILTER=run_on_main cargo nextest run -p eml_cli --test integration -E 'test(=ui::run) | test(=ui::run_fail)'` → passes (including the 2- and 8-worker and seed runs).
  - `cargo nextest run -p eml_cli --test integration -E 'test(repl::)'` → passes.
  - `cargo insta pending-snapshots` → only the new files.

### T4: Park
- Targets:
  - new `crates/eml_vm/src/park.rs`;
  - `lib.rs` (`with_main_park` on `RunConfig` and `Machine`, the re-exports);
  - `workers.rs` (`Run.holds`, `Slot.park_waker`, `main_wait` through the park, the two verdict reads);
  - `vm.rs`/`sched.rs` (`MainPending`);
  - `tests/api.rs`, `pinned.rs`;
  - docs.
- Work: D-7, D-32's park path and D-33's traits.
- New tests (`pinned.rs` and unit):
  1. `a_main_park_runs_on_the_caller_thread`: Virtual and Real.
  2. `a_park_that_returns_at_once_changes_nothing`: output and `Finished.stats` equal those of the default park.
  3. `a_holding_park_defers_the_deadlock_verdict` (Real) and `a_holding_park_defers_the_deadlock_verdict_on_the_virtual_clock`.
  4. `a_park_that_ignores_stopping_fails_in_debug_builds`: `#[should_panic]`.
  5. `a_park_that_never_runs_pending_still_ends_main`: D-4.
  6. `an_interrupt_reaches_a_main_worker_in_a_custom_park`: Real, N = 2.
  7. `a_pinned_socket_read_completes_under_the_default_park`: Real; no custom-park test does pinned I/O.
  - Unit: `run_pending_enters_only_from_a_parked_wait`, `stopping_is_sticky_until_the_wait_ends`, `a_notice_to_the_parked_main_worker_rings_its_park_waker`.
  - `api.rs`: the `ParkFactory` `Send + Sync` assertions.
- Spec and docs (same commit):
  - vm.md, new `### メインの worker の待ち`: the `Park`/`Pending`/`Progress`/`ParkFactory` contract of D-7; the three-state guard and the sticky `Stopping` (a debug assertion on the 64th call); latching; MAIN stays parked across early returns; `Run.holds` with its ordering and its two reads in `idle_wait`; `Slot::ring`; "a `Park` must also wait on the main worker's driver; `DefaultPark` does; a toolkit park does so through `Pending::io()` (toolkit stage)".
  - architecture.md 「CLI と lib API」: `with_main_park`, `Park`.
  - status.md, new `### メインの worker` under the known limits:
    - "(1) END does not interrupt a running pinned task: the run ends when the task reaches a safe point or its extern or toolkit callback returns; a park that stays in a modal loop, or ignores `Progress::Stopping` in a release build, delays only the final exit."
    - "(2) On the virtual clock, time advances only while every worker including the main worker sleeps; a running pinned task holds the clock and the deadlock verdict like any running task; on the real clock only the verdict waits."
    - "(3) No `EINTR` reaches a synchronous read on the main worker."
    - "(4) Only `DefaultPark` exists; a replacement park must also wait on the main worker's I/O driver, which needs `Pending::io()` (toolkit stage); until then, pinned I/O under a custom park does not complete."
- Completion criteria:
  - `cargo nextest run` → `B0 + 29 + 11 passed` (+1 on Windows), 0 failed (8 `pinned.rs` tests and 3 unit tests).
  - `git diff --stat <T0 commit>..HEAD -- crates/eml_io` → empty (D-12).
  - `cargo nextest run -j 1 -p eml_vm --test integration -E 'test(pinned::)'` → each under 1 s.
  - `ps … | grep 'target/debug/eml '` → nothing.
  - CI-only: the ASan and TSan lines → ok.

### T5: close the stage
- Purpose: the `Close main-pinning` commit, in the order of roadmap 「進め方」.
- Work:
  - docs/future/multicore.md:
    - 確定した設計判断:
      - row 1: "専用の worker" → "the main worker";
      - row 6: add "the main worker" to the order of spreading parallelism;
      - row 11: "thread-bound resources are confined to pinned tasks on the main worker, whose wait is a `Park`";
      - row 13: "`main` runs on worker 0, a spawned thread with a big stack; the thread that owns the `Machine` (the process main thread in `eml run`) is the main worker: an extra dedicated worker whose wait can be a toolkit loop; in the REPL worker 0 is the main worker";
      - row 17: add "the Machine owner joins worker 0 after its own main-worker loop and exits with the code".
    - 「スレッドと worker」:
      - Host paragraph → "The Machine owner is the thread that calls `execute` or `Machine::run`: it loads before the run and folds after it, and during the run it is the main worker."
      - The 専用の worker and メインの worker paragraphs → "The main worker is an extra, eager, peer-shaped worker (slot N) on the Machine owner's thread. It runs the pinned tasks (`Async.start_on_main`), is never a placement target, has its own I/O driver on the real clock like every worker, carries no run-wide duty, and its wait is a `Park` that a toolkit may replace (ffi.md 「GUI」). It has its own `Ctx` and heap and runs on the OS default stack. General dedicated workers (`rt_thread_new`) are a deferred roadmap item."
      - Delete `rt_dedicate_main` and `--main-dedicated`; replace "`Session::run`" with "`eml_vm::execute`".
      - Signal bullet: "the Machine owner masks from spawning worker 0 to joining it".
    - 「worker のループ」: "the main worker does not steal; its wait is its `Park`, and toolkit callbacks call `run_pending`".
    - 「`Async` のタスク」: drop `Run` from the notice list.
    - 「固定したタスク」, rewrite: "Pinned tasks are the tasks started by `Async.start_on_main` and their descendants (nested `Async.run` roots included); they run on the main worker and never move; the `par` children and `compute` jobs of a pinned task are placed like any other. Work leaves the main worker through `compute`, channels and the toolkit's own posting. The main worker's wait is a `Park`; a toolkit loop calls `run_pending` (`Pending::run`) from its callbacks; `run_pending` is `Busy` unless called from the park, runs until nothing is runnable, re-parks and returns `Wait(timeout)` or `Stopping`. The clock, the verdict and END never depend on the park."
    - 「止める手順」 4-v: "the Machine owner joins worker 0 after its own main-worker loop returned, lowers END, folds all heaps and exits with the code; no thread waits on the main worker; a pinned task in a long extern or a modal toolkit loop delays only the final exit."
    - 「VM の並列化の備え」: constraint 8 "no run-wide progress duty sits on a worker whose wait is replaceable or that runs pinned tasks"; constraint 9 "the main worker is a peer in I/O and in the idle accounting; only its wait is replaceable". Keep "writers set every stop word; the monitor only wakes", which resolves the native.md:127 wording.
    - 「テスト」, add: "main worker: pinned tasks run on the Machine owner's thread (recording Host) and never move; a park that never calls `run()` still lets `main` finish with today's output; a holding park defers the verdict on both clocks"; TSan list with `seat::` and `pinned::`.
  - docs/future/native.md:
    - #9: "…The main worker runs on the OS default stack of the process main thread (`stack_limit = stack_base + 2 MiB`); `eml` and AOT binaries reserve 8 MiB on Windows (`/STACK:8388608`)."
    - 「呼び出し規約とエントリ」, add: "`eml_run_pending(ctx) -> Progress` is the main worker's loop entry for toolkit callbacks: it takes the inbox, enters each ready entry through `eml_enter`, re-parks and returns `Wait(timeout)`, `Stopping` or `Busy` (when called outside the park). It never longjmps or unwinds past its own frame; a fatal error inside an entry returns through `eml_enter`'s `Fatal` and becomes `Stopping`."
    - 「安全点とスタック」 :92: "worker threads (worker 0 and workers 1..N-1) reserve 1 GiB; the main worker runs on the OS default stack".
    - 「並列の実行」 :127: "writers set every stop word; the monitor only wakes" in place of "監視のスレッドがビットを各 worker の止める語に写し"; add "the main worker is an extra dedicated worker: it helps only its own joins, its nested loops park as peers, a fatal error under `eml_run_pending` returns to the park, and it has its own I/O driver like every worker".
    - 「AOT と配布」, add: "AOT `main()` builds `Control` (N+1 words), spawns worker 0 with the stack reservation, runs the main worker's loop with the default park, joins worker 0, folds and returns the exit code."
    - 「未決」 :160: keep the default; add "the process main thread is never worker 0 in a native binary".
    - The 5.2 native accounting table as a short subsection under 「並列の実行」.
  - docs/future/async.md: :214 "`Gui.next_event` completes through a `Pending` wake from the main worker's park"; :381 "`Async.start_on_main` is the one named-worker primitive".
  - docs/future/ffi.md:
    - #12: "the GUI is a Rust toolkit crate wrapping Qt 6 (Linux), WinUI 3 1.2 (Windows) and AppKit (macOS); widgets are the toolkit's own, `!Send`, and touched only on the main worker".
    - #13: "the toolkit loop is the main worker's `Park`; it waits on the main worker's I/O driver through `Pending::io()` (a kqueue or epoll fd as a run-loop or socket-notifier source; IOCP through a helper thread that posts a wake); the loop never hosts the clock or the verdict".
    - 「拡張」: `Session::run` → `eml_vm::execute`; gating by the `Std.Tls` pattern.
    - 「GUI」, rewrite with winio-style per-OS integration:
      - Win32: `MsgWaitForMultipleObjectsEx` + `PeekMessage`, woken by `PostMessage` from the park waker.
      - AppKit: `nextEventMatchingMask` / a `CFRunLoop` source signalled by the waker.
      - Qt: `processEvents(WaitForMoreEvents)` with a posted event.
      - WinUI: `Application::Start` owns the pump, and `DispatcherQueue` enqueues `run_pending`.
      - Common rules: `run_pending` from callbacks; the modal rule (a modal loop entered inside the park keeps eml running; one entered from an extern pauses eml until it returns); `Gui.open` installs the park; `Std.Gui` is created there; it references `Async.run_on_main`/`start_on_main`; external events arrive through a `Pending` wake added in that stage; CI uses a fake park and a stub backend; the platform floor "WinUI 3 1.2 (Windows App SDK), Qt 6, AppKit on macOS 26".
      - Add a REPL GUI paragraph (the `python -m asyncio` pattern: the main thread owns the `Machine` with `Seat::Separate` and the toolkit park; the line editor runs on a thread and submits inputs; output goes through rustyline's `ExternalPrinter`; preconditions: root objects reachable from pinned closures, a between-runs park on `Machine`).
      - :158: "GUI tasks and their children never leave the main worker; it receives no task by placement".
    - 「却下した案」, add: egui + winit + wgpu (non-native widgets, a GPU dependency, winit owns the loop and takes no foreign source); the GUI loop as an `IoDriver` implementation; the input-hook REPL GUI.
    - 「未決」: replace the `pump_app_events` item with the modal rule.
  - docs/future/roadmap.md:
    - Delete this stage's entry from 「段の列」.
    - `gui-plugin` → `gui-toolkit`: "a Rust toolkit crate over Qt 6, WinUI 3 1.2 and AppKit on the main worker's park; adds `Pending::io()`; creates `Std.Gui`. Prereq: `main-pinning`. Design: ffi.md 「GUI」".
    - `jit-effects` prereqs: `jit-externs` and `main-pinning` (drop `gui-plugin`); add "`eml_run_pending` and the main worker's fatal path".
    - The `spawn_here` item (~:427): "named placement exists only as `Async.start_on_main`".
    - Add to 「その後の項目」, each with 「後に回した理由」 and 「見直す条件」:
      - REPL GUI (asyncio pattern; preconditions: root objects reachable from pinned closures, a between-runs park on `Machine`, the editor on a thread with `ExternalPrinter`; until then GUI in `eml repl` is unsupported);
      - general dedicated workers (`rt_thread_new`): revisit when a second thread-bound library appears;
      - removal of the pidfd `ENOSYS` fallback (`eml_io` `sys/unix.rs:309-318`, stdlib.md:1597), since Linux ≥ 6.12;
      - CI runners below the platform floor (`macos-15`, `ubuntu-24.04`): move them when a stage needs a floor feature.
  - Platform floor:
    - docs/overview.md 「確定した設計判断」「コンパイラと実行系」 gets a row 「対応するOS」: Windows 10 1809 or later, Linux 6.12 or later, macOS 26 or later; later stages may rely on them; link to testing.md.
    - testing.md 「OS ごとのテスト」: one paragraph naming the floor, noting that CI's `macos-15` and `ubuntu-24.04` images are below it and that no test depends on a floor feature yet.
    - ffi.md 「GUI」 names the toolkit floor.
  - docs/overview.md:93: add "the main worker on the Machine owner's thread runs the tasks started with `Async.start_on_main`"; the 用語 "worker" row adds the main worker.
  - docs/README.md: vm.md entry add 「メインの worker」; multicore.md entry "(スレッドと worker、メインの worker、スケジューラ、…)".
  - benchmarks.md: refresh the three tables from a clean tree.
  - Delete `docs/plans/2026-11-06-main-pinning.md`.
- Completion criteria:
  - `git grep -n -e rt_dedicate_main -e main-dedicated -e main_dedicated -e 'Notice::Run' -- . ':!docs/plans'` → no output.
  - `git grep -n 'Std.Gui' -- . ':!docs/plans'` → only docs/future (ffi.md, roadmap `gui-toolkit`, async.md).
  - `git grep -n rt_thread_new -- . ':!docs/plans'` → only the roadmap's deferred item.
  - `git grep -n -e '専用の worker' -- docs CLAUDE.md ':!docs/plans' ':!docs/reports'` → only the roadmap's general-dedicated-workers item.
  - `git grep -n 'host のスレッド' -- docs ':!docs/plans'` → no output.
  - `git grep -n 対応するOS docs/overview.md` → one hit.
  - `git grep -n -e 'WinUI 3 1.2' -e 'Qt 6' docs/future/ffi.md` → hits in 「GUI」.
  - `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` → passes.
  - `nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml` → exit 0.

## 8. Execution order

T0 → T1 → T2a → T2b → T3 → T3b → T4 → T5, strictly sequential; each commit is green on its own.

## 9. Verification plan

- Baseline (before T1): `timeout 600 cargo nextest run` → record `B0 passed`, 0 failed.
- Overall (after T4):
  - `cargo nextest run` → `B0 + 40 passed` (+1 on Windows), 0 failed, no `TIMEOUT`.
  - `ps -axo command | grep 'target/debug/eml ' | grep -v grep` → nothing.
  - `cargo verus verify -p eml_rt` → 0 errors.
  - The clippy, feature-combo and cross-target clippy lines of CLAUDE.md → exit 0.
  - CI green (Miri, ASan, TSan, Windows).
- Manual checks by the user: on macOS, `eml run` of a program that prints from `Async.run_on_main` behaves like any other run (stdout, exit code, Ctrl-C ends it with 130). No GUI exists yet.

## 10. Risks and unresolved items

| ID | Item | Impact | How / when | Default |
|---|---|---|---|---|
| R-1 | The `drive`/`HOST_IDLE`/Park code was not prototyped as one build with `sched-cooperative` | T3, T4 | the implementer; the listed tests | a failure outside the listed tests stops the stage for an amendment |
| R-2 | Windows: `/STACK` through `build.rs` and test-thread stacks are unmeasured | T2b | CI | set `RUST_MIN_STACK` in CI if test threads overflow |
| R-3 | `peak_objects` may count MAIN's empty heap | T2b | the T2b run | exclude a heap that ran no task from the max |
| R-4 | A pinned task in a long extern or a toolkit modal loop delays the final exit | users | documented limit (status.md, T4) | second Ctrl-C `_exit` |
| R-5 | Names: `Seat::{Merged, Separate}`, `Run::main()`, `ENTRY`, 「メインの worker」, MAIN's thread keeps the caller's name, `placed_tasks` counts pinned starts placed from another worker | all | as written | as written |

## Appendix: main points settled in review

- The main thread first also hosted run management (clock, verdict, driver) → moved back to worker 0 (D-2, D-4). A toolkit loop must never be able to stall the run.
- MAIN without an I/O driver, with worker 0 hosting its I/O → withdrawn. With no preemption, worker 0's CPU work would delay pinned I/O (D-18).
- Worker 0 lowering END before MAIN read it hung every `eml run` in the prototype → only the Machine owner lowers END, after both joins (D-10).
- A driver creation failure before `Shutdown` was armed hung `eml run` → `Shutdown` is armed at `entry_body`'s first statement.
- `IoDriver` is not `Send` → the Machine driver is made on worker 0's thread in A, and an injected driver requires B.
- `Std.Gui` in this stage → no; `Async.start_on_main`/`run_on_main`, and `Std.Gui` comes with the toolkit (user decision).
