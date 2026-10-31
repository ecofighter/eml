# S4 (r3, final integration): S1 r3 + S2 r3 + S3 r2 merged into one code-level design and one unit list for `worker-threads`

Tags: [V] means verified, with the method given; [I] means inferred. Paths are relative to /home/user/eml at commit 0f9cd9b.

Sources:
- S1 = S1-response-r3, including the user's amendments X2–X7 and the G-S1 criteria of its §8.
- S2 = S2-response-r3, including the G-S2 criteria of its §8.
- S3 = S3-response-r2, amended by S2 r3's `check_lin_parts` text.
- "W*n*" = the units of §5.

Change log against r2 is at the end.

## 1. Conclusion

### 1.1 Interface decisions (S1's needs mapped onto the S2 API)

| S1 needs | Decision |
|---|---|
| Drain the remote lists | `Heap::reclaim` / `Heap::reclaim_if_due` (S2). W3 adds safe wrappers `Ctx::reclaim(&mut self)` and `Ctx::reclaim_if_due(&mut self) -> bool`, because `heap_mut` is `unsafe`. |
| Run-wide ID table, `Heap` signatures kept | `Control.workers: WorkerIds`. It lives as long as the `Machine` and is reset only by `recover` (S2, S1 C3). `fork_worker`/`retire_worker`/`set_worker` keep their signatures. A worker heap starts with ID 0 (`Heap::with_control`, unchanged, admission is idempotent), and ID 0 is its **idle ID**: no eml code runs under it on worker k. In fast mode every ID is 0 [V: heap/mod.rs:963-965]. That `reclaim` changes no count is a checked test (W6). |
| Recovery over N heaps | `Ctx::recover(&mut self)` on Ctx 0, with heaps 1..N-1 alive and reclaimed (G-S2-11). `Machine.others: Vec<Ctx<M>>` keeps them until recovery has run; then they are dropped. |
| Observing a remote free | `Stats.remote_frees` (counted by the freer) and `Stats.reclaimed` (counted by the owner), checked mode, read through `Heap::stats()`, not part of `RunStats` (S2 G-S2-14). W12 asserts `ctx0.heap().stats().remote_frees >= 1` after a run whose parent drops a list the child allocated (§1.5). `Finished.placed: u64` stays and counts children placed on another worker. |
| Page registration | Internal to S2 (`fresh_page`/`dealloc_page` → `Control.pages`, a three-level `PageMap`). Nothing in the VM. |
| Page walks reclaim first (G-S2-4) | `Heap::check_invariants`, `leaks` and `live_objects` take `&mut self` and call `reclaim` first. W3 adds `Ctx::check_invariants(&mut self)` and `Ctx::leaks(&mut self)`. `Machine.check_heap` becomes `fn(&mut Ctx<M>, &mut [Ctx<M>]) -> Result<(), HeapError>`. |
| `Control::check_owned(&self, &[&Heap<M>])` (G-S2-4 exception) | It cannot reclaim. Precondition: every heap has had a forced `reclaim` after the last push, and no thread touches any heap during the call. In checked mode it returns `NotQuiescent` if any sink has `count != 0`, `large != 0` or `pushing != 0`. The VM meets the precondition with the forced reclaims of §1.3. |
| Foreign children in checked walks (G-S2-13) | `check_reach`, `freezable` and S3's `check_lin_parts` judge an edge into another registered heap's page through `foreign_lend`, under the same rc/`lin` conditions. `UnknownPage` is returned for an unregistered page. `leaks`/`live_objects`/`owned_entries` count own pages only, and `check_owned` sums its recount over all heaps. `check_heap(ctx0, &mut others)` therefore accepts valid N ≥ 2 states. |

### 1.2 The idle/placement protocol (all workers)

**State**
- `Run.running: AtomicUsize`, `Run.in_flight: AtomicUsize`.
- `Run.idle: Mutex<Vec<u32>>`, used as a set.
- Per slot:
  - `state: Mutex<SlotState { notices: Vec<Notice>, woken: bool, parked: bool }>` (the worker's inbox lock in G-S1-7);
  - `has_notices: AtomicBool`;
  - `cv: Condvar`;
  - `waker: OnceLock<Arc<dyn IoWaker>>`;
  - `switches: AtomicU64`.

**Invariants**
- **I1** `running` equals the number of started workers whose slot has `parked == false`. It changes only together with a `parked` flip, under that slot's lock.
- **I2** A slot with pending `notices` or `woken` set has `parked == false`. Every push and every wake unparks the slot.
- **I3** `in_flight` equals the number of `Child` notices sent whose `Joined` has not yet been consumed, orphans included.
- **I4** `idle` holds no duplicates. `k ∈ idle` means k inserted itself after finding its notices empty and has not been popped since. A placer sends one `Child` per pop.

**Consequences**
- `running == 0` implies that no worker executes and that no notice is pending in any slot. So worker 0 may then advance virtual time.
- `in_flight == 0` together with worker 0's notices being empty implies that no `Joined` can still arrive. So the deadlock check is sound.

**Transitions**

| Point | Effect |
|---|---|
| Run start | `running = 1`; slot 0 has `parked = false`. |
| Spawn (worker 0, before spawning) | For k in 1..N: slot k gets `parked = true`; `idle = [1..N-1]`. `running` is unchanged, so a new worker starts parked and idle. |
| `unpark(k)` | Under slot k's lock: `if parked { parked = false; if running.fetch_add(1) == 0 { run.bell.ring() } }` |
| `send(k, n)` | 1. Lock k. 2. Push; set `has_notices = true`; `unpark(k)`. 3. Unlock. 4. `control.notice(k)`. 5. `cv.notify_one()`. 6. `waker.wake()` if a waker is set. |
| `wake(k)` | As `send`, but sets `woken = true` and pushes nothing. |
| Idle wait, k ≥ 1 | (a) Lock k. If there are notices or `woken`: take/clear them, unlock, return.<br>(b) Unlock. Lock `idle`, insert k if absent, unlock.<br>(c) Lock k. If there are notices or `woken`: take/clear, unlock, remove k from `idle`, return. Otherwise set `parked = true`, do `prev = running.fetch_sub(1)`, unlock.<br>(d) If `prev == 1`: `wake(0)`.<br>(e) Lock k; `while parked { cv.wait }`; take/clear; unlock; remove k from `idle` if still present. |
| Idle wait, worker 0 | Steps (a) and (b) as above. Then, under lock 0 with no notices and no `woken`:<br>1. If `deadline.is_none() && pending() == 0 && in_flight == 0 && !listening()`: unlock and return `Err(Deadlock)`.<br>2. Otherwise set `parked = true` and decrement `running`, giving `r`.<br>3. If the clock is `Virtual`, `r == 0`, and there is a deadline or a pending op: unlock and call `driver.wait(deadline)`.<br>4. If the clock is `Virtual` and `r > 0`, or if nothing is pending and there is no deadline (so `in_flight > 0`): `while parked { cv.wait }`.<br>5. Otherwise: unlock and call `driver.wait(wait_deadline(deadline))`.<br>6. After any `driver.wait`: lock 0, `unpark(0)`.<br>7. `Ok(false)` counts as `Deadlock` only if `in_flight == 0` and no notices are pending; otherwise loop.<br>8. Remove 0 from `idle`. |
| Placement (a fork while `Workers::Started`) | `if let Some(k) = idle.pop()`: `in_flight += 1` (before the send); `placed += 1`; `send(k, Child(task))`. The `Task` is not `add_task`ed on the placing worker, and `join.right = PLACED` (`usize::MAX`). Worker k assigns its own `TaskId` when it consumes the notice. |
| Consuming `Joined` (on the parent's worker) | Orphan: `take_join` and drop the outcome untouched. Otherwise: set `right_outcome = Some(o)`, then `wake_parent`. In both cases `in_flight -= 1` (Release). |
| Worker exit (END, stop or fatal) | Lock k; `if !parked { parked = true; running -= 1 }`; unlock. |

Lock order: `idle`, `fatal` and every slot lock are leaves; no thread ever holds two of them at once. `collect` does one Acquire load of `has_notices` per task switch.

### 1.3 Call sequences

**Worker k ≥ 1 startup** (scoped thread `eml-worker-k`, 8 MiB stack, started with `eml_io::spawn_scoped_masked`):
1. `Ctx::<M>::for_worker(control, host, k)`; `ctx.word = control.word(k)`.
2. `Scheduler::worker(policy.for_worker(k), Arc::clone(&run), k, max_depth)` with `driver: None` and an empty `Registry` that is never read. Every `cells` access sits behind `driver: Some`, and `driver_mut()` on k ≥ 1 returns `VmFault::Internal` (G-S1-11).
3. Idle wait from step (e).

**Before every idle wait:** `ctx.reclaim_if_due()` (S2's reclaim point 3).

**A placed child ends on worker k** (in `child_done`, when `boundaries` is empty after the pop and `task.placed = Some(p)`):
1. `rest = ctx.switch_task(UNIT, task.worker, task.depth_offset)`, then `decref(rest)`.
2. `ctx.switch_task(UNIT, 0, 0)`.
3. `run.send(p.home, Notice::Joined { join: boundaries[0], outcome })`.
4. `switch_away(None)`.

Step 1 now comes before publishing; today's code does the opposite [V: vm.rs:1222-1227]. The slot mutex provides the happens-before edge that `retire_worker` needs (G-S2-10).

**On the parent's worker:** consume `Joined` (see the table in §1.2). Later, `complete_join` → `join_outcomes` → `transfer_back(w, child)` → `retire_worker(child)`.

**Run end, in `run_scheduled` on worker 0, after the trampoline loop** (G-S1-8):
1. If workers were started: `control.end_run()`, then `run.wake_all()`.
2. The scope joins every handle and collects `WorkerExit<M> { ctx, counts, preemptions, peak_stack }`.
3. `control.clear_end()`.
4. `result = settle(result, run.fatal.take())`. Any recorded fatal replaces worker 0's own result, including an `exit`'s `Stopped(Ended)`. The exit code is then 1.
5. `sched.driver.quiesce()`; `kept_pending` handling as today.
6. **Reclaim all heaps**: a forced `reclaim` on Ctx 0 and on every Ctx k.
7. Return `Scheduled<M> { …, others, workers, placed, worker_counts, worker_peaks }`.

**`run_entry` after `run_scheduled`:**
1. On a shown run with `Ok`, copy the result (`copy_string` takes and releases its reference; the release may be a remote push into heap k). If workers were started, **reclaim all heaps again**. This is ruling (i): `shown` does not deny workers, and `check_owned`'s precondition holds after the copy.
2. Read `ctx_k.stats()` and `ctx_k.heap().stats()` (the last reclaim has run). Fold them into `Machine.worker_work` (`HeapWork`: sum the counts, max of `peak_objects`) and `Machine.worker_ctx` (`CtxStats` sum). Add `worker_counts` to `counts`.
3. **Host flush**, `ctx.flush()`, before any check or recovery (G-S1-8).
4. **Success:**
   1. `check_finished` on Ctx 0, and `finished()` on every other Ctx.
   2. `check_heap(ctx0, &mut others)`. In checked mode: `check_invariants` on each heap (foreign-child rule), then `control.check_owned(&[all heaps])`.
   3. `at_run_end(&mut others)`. In checked mode, a non-empty `leaks()` on any heap 1..N-1 gives `RuntimeError::Leak`.
   4. `drop(others)`.
5. **Error or abandon:**
   1. `self.others = others`.
   2. `run_recovering` → `recover()`: driver `quiesce`; `old = heap_work()`.
   3. `ctx.recover()`, which reclaims itself (G-S2-4). It copies from any registered page; here it copies nothing, because a spawning run has no roots.
   4. `self.others.clear()`.
   5. `earlier = old; worker_work = 0; copying = …`.
   6. `check_heap(ctx, &mut [])`.

   This is G-S2-11's order.
6. `Unrecoverable` errors, broken errors and poisoned Machines keep `others` until `Machine::drop`, which drops in the order `driver = None` → `others.clear()` → `ctx`.
7. The host joins worker 0, unblocks the signals, and maps the exit code as today.

**`RunStats` (S1 C12):**
- `heap_work() = earlier + now(ctx0) + worker_work − copying`.
- `run_stats` adds `worker_ctx`.
- `preemptions` is a sum; `peak_stack` is a max.
- The Registry fields come from worker 0 only.
- `Stats.remote_frees`/`reclaimed` are not in `RunStats`.

### 1.4 Thread structure

**Monitor (G-S1-6).** `begin_run` starts the monitor if `driver_kind != Virtual || limits.workers ≥ 2`. If it fails to start, worker 0's state becomes `Workers::Denied` at run start (N = 1), and `wait_deadline` caps waits at 10 ms. No mid-run branch exists.

**`Workers` state.** Worker 0's `Scheduler` holds `enum Workers { Wanted, Started(u32), Denied }`, decided at every run start:
- `Wanted` requires all of: `limits.workers ≥ 2`, the monitor running, `!self.code.uses_roots`, and `ctx.roots().is_empty()`. Otherwise the state is `Denied`.
- `CodeBase.uses_roots` is set by `emit` when it lowers `Prim::RootGet` or `RootSet` (emit.rs:705-708). It is ORed on every `load` and never cleared. The code cannot change during a run.
- These are exactly G-S1-2's conditions. vm.md 「タスクとスケジュール」 states them, and the roadmap's deferred item "REPL の複数の worker" states the limit (S1 r3 text).

**Trampoline.**
- Signatures: `vm::run<R>(…) -> Result<Exit, RuntimeError>` with `pub(crate) enum Exit { Finished, Spawn }`.
- `run_tasks` handles `Err(fault) if fault == SPAWN_WORKERS => { task.pc = pc; return Ok(Exit::Spawn) }`.
  - `SPAWN_WORKERS` is a `const VmFault::Internal` sentinel, like `CHILD_FAILED`.
  - `VmFault: PartialEq, Eq` [V: error.rs:10].
- The first statement of `par` is `if sched.workers == Workers::Wanted { return Err(SPAWN_WORKERS) }`. It runs before any read, `fork_worker`, `share`, `switch_task`, `safepoint` or `tick`, so no counter moves.
- `step` sets `task.pc = pc + 1` before dispatch [V: vm.rs:479, :521], so rewinding the pc gives an exact re-execution.
- `run_scheduled` wraps a loop in `std::thread::scope`:
  - On `Exit::Spawn`: `spawn_workers` (slots parked and idle first), then `sched.workers = Started(n)`, then call `vm::run` again.
  - On `Exit::Finished` or `Err`: break.
- The re-executed `Par` finds `idle = [1..N-1]`, so the first fork of a spawning run always places its child.

**Notices and data structures.**
- `workers::Notice { Child(Task), Joined { join: JoinId, outcome: Outcome } }`.
- `cells::Inbox` (`Signal`, `HandleDropped`, `Cancel`) stays with worker 0's `Registry`.
- `Task.placed: Option<Placed { home: u32, at: Pc }>` is used in three places:
  - `resume` of an `Unstarted` task reads `at` from it;
  - `child_done` takes the placed-end path when it is `Some`;
  - `task_failed` sends `Joined { boundaries[0], Outcome::Error }`.
- `JoinNode.orphan: bool`: `cancel` keeps a placed, unresolved node as an orphan instead of freeing it.

**Shutdown on worker k.**
- A stop bit (INTERRUPT, TERMINATE, MEMORY or END, seen in `poll` or after a wake): `run.wake_all()`, then exit quietly.
- Any other `stops_the_run` error: write `fatal` (first writer wins), then `end_run` + `wake_all`, then exit.
- On exit: run the worker-exit row of §1.2; the `Task` is dropped without unwinding and the `Ctx` is returned.
- Panic: the worker body runs in `catch_unwind(AssertUnwindSafe)`, with the `Ctx` in a `ManuallyDrop` outside it. On a panic: `mem::forget` the `Ctx`, set `fatal = Internal("a worker thread panicked")`, `end_run`, `wake_all`. Worker 0 then sets `poisoned` and calls `resume_unwind` after the scope.
- `stop_reason` order: INTERRUPT > TERMINATE > MEMORY > END (→ `Stop::Ended`) > NOTICE > PREEMPT > DepthLimit.

**Monitor details.**
- `Monitor::start(control, registry_inbox, run: Arc<Run>, doorbell, preempt)`.
- Per-slot `switches` counters and per-slot tickers; `preempt(k)` per worker.
- `relay` pushes `Signal` to the Registry inbox, sets `notice(0)`, and calls `wake_all` when there are new signals or `stop_bits() != 0`.
- With a doorbell: `doorbell.wait((preempt && running > 0).then_some(until_next))`; `unpark` rings `run.bell` on the 0→1 transition of `running`.
- Without a doorbell: `park_timeout(TICK)` on every tick (G-S1-6 as amended).
- `wait_deadline` is uncapped if and only if a monitor runs or the clock is `Virtual`.

**`Run` lifetime.**
- A fresh `Arc<Run>` is created per `run_scheduled`; the monitor is stopped by `Watcher::drop` before `run_scheduled` returns. Nothing survives a run.
- `WorkerIds` and `PageMap` live in `Control` (Machine lifetime), not in `Run` (S2 Cf-7).

### 1.5 CLI, harness, tests that cross crates

**CLI.**
- `eml run --workers <N>` takes a value of at least 1 (`value_parser!(u32).range(1..)`); `repl` has no such flag.
- Otherwise `EML_WORKERS` is read; a bad value exits with code 2 and the message `error: EML_WORKERS must be a positive integer`.
- Otherwise `available_parallelism()` is used, falling back to 1 on error.
- `default_limits().workers == 1` and `repl_limits().workers == 1`.

**`eml_vm::execute`.**
- Inside `thread::scope`: `let builder = thread::Builder::new().name("eml-worker-0".into()).stack_size(WORKER_STACK);` then `builder.spawn_scoped(s, …)`.
- `let _blocked = eml_io::Masked::new();` (`Masked` becomes `pub`).
- Join; re-raise a panic with `resume_unwind`.
- A spawn error becomes `RuntimeError::Fault { fault: VmFault::Driver(e.to_string()), function: "<worker 0>".into(), at: None }`.
- `eml_io::spawn_scoped_masked(scope, builder, body)` contains `builder.spawn_scoped(scope, body)`. The `CaptureHost` reader thread (hosts.rs:331) moves to `spawn_masked`.

**Spawn allow-list test** (`crates/eml_vm/tests/spawns.rs`).
- Scope: every `.rs` file under `crates/{eml_vm,eml_io,eml_cli}/src`, except paths containing `/tests/` and files named `tests.rs`. Each file is cut at its first `#[cfg(test` or `#[cfg(all(test`.
- Matches: `(\w+)(\(\))?\s*\.\s*(spawn|spawn_scoped)\(` and `thread::spawn\(`, keyed by (file, receiver.method).
- Allowed receivers: `command` (`Command::spawn`) and `backend` (`Backend::spawn` → `spawn_masked` [V: unix.rs:618, iocp.rs:736]).
- Exact expected multiset after W9 (any other key fails):
  - `eml_io/src/driver/pool.rs` `backend.spawn` 1
  - `eml_io/src/driver/resolve.rs` `backend.spawn` 1
  - `eml_io/src/mask.rs` `builder.spawn` 1
  - `eml_io/src/mask.rs` `builder.spawn_scoped` 1
  - `eml_vm/src/hosts.rs` `command.spawn` 1
  - `eml_vm/src/lib.rs` `builder.spawn_scoped` 1
- On today's tree, the same script finds the entries above minus the two new ones, plus `eml_vm/src/hosts.rs thread::spawn( 1` [V].

**UI harness.**
- `execute_in(.., workers)`.
- `assert_workers_agree` runs `par/` at N ∈ {2, 8}: checked mode, `Sched::Sequential`, `Clock::Virtual`, `assert_agrees(.., compares_stats(checked, &ran))`.
- For `run/` it also asserts `finished.workers == N-1 && finished.placed >= 1`. This is deterministic per §1.4.
- `run_fail()` does this only when `fast_mode_reports`. `async/` is never run with workers.
- Budget [V]: today `par/` takes 0.26 s for 4 runs per file and the whole of `ui::run` takes 5.7 s on this container. The two extra runs add about 0.13 s.

**Remote-free observation (G-S2-14).**
- `crates/eml_vm/tests` cannot read `ctx0`, because `Machine` exposes no heap statistics and adding a test-only accessor is forbidden.
- So the assertion is a unit test in `crates/eml_vm/src/workers.rs` (`#[cfg(test)]`): `the_parent_frees_a_child_result_remotely`.
  - It loads a LIR program written as text (`eml_lir::parse`; eml_lir is already a dependency of eml_vm) into `Machine::<Checked>` with `Limits { workers: 2, .. }`.
  - In the program, the right child of a `par` builds a fresh list and the parent drops it after the join.
  - After `run`, it asserts `machine.ctx.heap().stats().remote_frees >= 1` and `finished.placed == 1`.
- The integration test `workers::a_child_runs_on_another_worker` keeps the checks on output, `placed` and stats. See §9.

**TSan.** One CI job `tsan` (ubuntu-24.04) using the `.#miri` shell, with `RUSTFLAGS=-Zsanitizer=thread` and `TSAN_OPTIONS=halt_on_error=1`, in two steps:
- `cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_rt trusted::`
- `… -p eml_vm --test integration -- workers:: signal:: preempt::`

## 2. Rationale

- **Send/Sync.** The cross-thread pieces were verified by a compiled probe [V: r1 §7]: `RuntimeError`, `Outcome`, `JoinNode`, `Notice` and `Ctx<Fast>` are `Send`; `Run` is `Sync`; scoped closures over `&CodeBase<M>`, `Arc<Run>`, `HostRef`, `&Program` and `&RunConfig` compile.
- **Trampoline.** It keeps `&Scope` out of `Scheduler`, so the 33 signatures stay unchanged. Re-execution is exact.
- **Outcome inside the notice.** `JoinNode`s stay single-threaded, and the slot mutex provides G-S2-10's edge.
- **Unparking on send (I2).** A queued notice always counts as running, so virtual time advances only when nothing runs and nothing is queued. This holds without also requiring `in_flight == 0`; that extra test would freeze virtual time during a child that waits on nothing.
- **The roots gate** (the ruling on X6/X7, adopted in S1 r3 G-S1-2):
  - `transfer_back` flips objects in place without copying them [V: transfer.rs:894-900].
  - So a root set after a join may point into heap k's pages, and heaps 1..N-1 are dropped at the end of the run.
  - Roots exist only in code lowered for the REPL [V: translate/program.rs:601, emit.rs:705-708].
  - The gate is therefore the rule that keeps the drop at run end sound.
- **Why `shown` is not a gate condition** (ruling (i)):
  - A shown result does not survive the run: `copy_string` takes and releases its reference [V: heap/ctx/io.rs:183].
  - `run_entry` copies the result before the flush, before the checks and before the drop [V: lib.rs:559-567].
  - The only effect at N ≥ 2 is ordering: the release can be a remote push into heap k after the forced reclaim. That would make `check_owned` return `NotQuiescent` and would leave the stats non-final.
  - So a second forced reclaim after the copy is enough, and the gate condition is dropped.
  - r2's rationale ("a kept shown result pointing into a dropped heap") was wrong and is removed.
- **`settle`.** `run_entry` reads `Stopped(Ended)` as an `exit` abandon [V: lib.rs:574-595]. A fatal error on any worker means no heap can be trusted, so it must win.
- **Waking on a quiet stop.** `charge` only raises bits, and a doorbell monitor running `Sequential` sleeps without a timeout [V: monitor.rs:136].
- **Instruction gate.**
  - Cachegrind's `I refs` is deterministic run to run [V: r2 §7].
  - `loop` and `fib` free at most 2 blocks each (`--stats`: `rc_decrements` 1 for both [V]), so the free-path cost permitted by G-S2-9 does not show in them.
  - `list.em` (100,001 frees [V]) and `par_tree` (7,977 [V]) show that cost, and their deltas are recorded.

## 3. Alternatives considered and rejected

- **`Scheduler<'s, P>` holding `&Scope`:** changes the lifetime of 33 signatures.
- **S1 r2's `Run.joins` mutex:** a lock on every fork and join even at N = 1.
- **Keeping `shown.is_none()` with its true reason (ruling (ii)):** it adds a denial that G-S1-2 does not list, to avoid one reclaim pass that costs O(pages) only on shown runs with workers.
- **Requiring `in_flight == 0` before advancing virtual time:** it stalls virtual time; I1/I2 already cover the case it guards.
- **Persisting heaps 1..N-1 across runs, or compacting with `recover` after success:** rejected by the X6 ruling.
- **A test-only `Machine::heap_stats()`:** forbidden by the rule against test-only API; the unit test in `src/` reads `machine.ctx` instead.
- **Lazy drivers on workers k ≥ 1:** not needed in this stage (G-S1-11; S1 r3 roadmap text).
- **A bare `.spawn(` count:** it cannot tell process spawns from thread spawns.

## 4. Assumptions made

- **B1 [I].** Verus accepts `Ctx.word: Arc<AtomicU32>` together with `Control.words: Box<[Arc<AtomicU32>]>`. Fallback: `Box<[AtomicU32]>` indexed by the worker number, the shape S1 verified [V: S1 §7], gated by the IR check in W8.
- **B2 [I].** A shared `Arc<dyn Host>` holds the output buffers, so one `ctx0.flush()` covers every cloned `HostRef`. `par` children cannot print.
- **B3 [I], pinned by tests in W3/W6.** S2's checked remote push decrements `count[owner]` when it frees, and `reclaim` changes no count and no work counter.
- **B4 [I].** TSan raises no false positives on std `Mutex`/`Condvar` built with `-Zbuild-std`.
- **B5 [I].** The `with_driver` tests listed in W10 keep their outcome. A change in any of them requires a plan amendment before W10 lands.
- **B6 [I].** A LIR text program with `par` can be written for the `src/` unit test. `crates/eml_vm/tests/miri.rs` runs small LIR programs the same way.

## 5. Implications for implementation

### Constraints

- **C1'** The protocol of §1.2 (I1–I4) is normative. Code comments cite the vm.md section written in W12.
- **C2'** `Scheduler` gains `run: Arc<Run>`, `index: u32`, `workers: Workers` and `driver: Option<Box<dyn IoDriver>>`. The 31 worker-0 driver sites become `driver_mut()` calls [V: count].
- **C3'** `Policy::for_worker(&self, k: u32) -> Self`: `Seeded` keeps its seed and returns `Seeded::new(splitmix64(seed ^ u64::from(k)))`; the other policies return `self`.
- **C4'** `Control` additions (W8):
  - `END = 4`, set in the per-worker words only.
  - `end_run()` and `clear_end()` write all words.
  - `begin_run()` clears `PREEMPT|NOTICE|END` in all words.
  - `clear()` zeroes all words.
  - Per-word accessors: `preempt(k)`, `take_preempt(k)`, `notice(k)`, `take_notice(k)`, `worker_bits(k)`, `word(k)`.
- **C5'** Compile-time asserts (G-S1-12):
  - in `src/workers.rs`: `Run: Sync`, `JoinNode: Send`, `Outcome: Send` (added in W10) and `Notice: Send` (added in W12);
  - in `tests/api.rs`: `Finished: Send` (added in W11).
- **C6' Miri budgets (S2 r3).**
  - Nine new `trusted` tests:
    - W3: (a), (b), (c), (e);
    - W4: (d), plus S3's `a_lin_part_moves_to_another_thread_and_back`;
    - W6: (f), plus `reclaim_changes_no_owner_count_and_no_work_counter`;
    - W7: (g), extended.
  - Tree Borrows budget: 220 + 9 × 5 = 265 s. Stacked Borrows budget: provisionally 220 s, re-measured on a quiet 4-CPU container.
  - Each unit that adds `trusted` tests edits crates/eml_rt/CLAUDE.md and testing.md 「Miri」 with its measurement.
  - W7 writes the final numbers. This corrects S2 r3's "W6": Miri (g) lands in W7.
- **C7' Linux instruction gate (IR)**, for G-S1-4, G-S2-9 and G-S2-12 (fast mode).
  - Build: `cargo build --release -p eml_cli`.
  - For F ∈ {empty, loop, fib, par_tree, list}, run `nix shell nixpkgs#valgrind -c valgrind --tool=cachegrind --cache-sim=no --cachegrind-out-file=/dev/null target/release/eml run --sched sequential --workers 1 bench/F.em` and read `I refs`. Omit `--workers 1` before W14.
  - Gate on every IR unit: `|Δ(loop − empty)| / (loop − empty) ≤ 0.1%` and `|Δ(fib − empty)| / (fib − empty) ≤ 0.1%`, both against the parent commit.
  - Record only (in the commit message), on every IR unit: `Δ(par_tree − empty)` and `Δ(list − empty)`. On W2/W3 (the free path) the `list` delta must not exceed +2 instructions per free, i.e. +200,002 for 100,001 frees (G-S2-9).
  - Baseline on 0f9cd9b [V]: empty 19,796,867; loop 196,045,081; fib 114,540,917; par_tree 41,767,918. `list` is measured first in W2.
  - S1 r3's exception ("free-path commits record `fib`") never applies, because `fib` frees no more than 2 blocks. Gating `fib` everywhere is stricter than G-S1-4 and still conforms to it.
- **C8' PageMap measurement (S2 r3, I-1)**, done in W4.
  - Before the map: `user` time of the Tree Borrows `trusted::` command.
  - After: the same.
  - Per-test increase = Δuser ÷ (number of checked `trusted` tests). It must be < 0.2 s.
  - Otherwise: shrink `leaf` to 256 words and add a fourth level in the same unit.
  - The numbers go in the commit message.

### Gates

**S** (every unit):

| Command | Expected |
|---|---|
| `cargo nextest run` | `0 failed` |
| `cargo clippy --all-targets -- -D warnings` | exit 0 |
| `cargo fmt --check` | exit 0 |

**G** (eml_rt units; crates/eml_rt/CLAUDE.md):

| Command | Expected |
|---|---|
| `cargo verus verify -p eml_rt` | last line `verification results:: <n> verified, 0 errors`, with no `warning:` lines |
| `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` | exit 0 |
| `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" /usr/bin/time -p cargo miri test -p eml_rt trusted::` | pass, with `user` ≤ the Stacked Borrows budget in crates/eml_rt/CLAUDE.md |
| the same with `-Zmiri-tree-borrows` | pass, with `user` ≤ the Tree Borrows budget (265 s from W7) |

**X** (VM units):

| Command | Expected |
|---|---|
| `nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings` | exit 0 |
| `cargo clippy -p eml_cli --no-default-features --features types -- -D warnings` | exit 0 |
| `cargo nextest run --profile timing -j 1` | `0 failed` |

**IR:** C7'. It applies to every code unit W1–W13.

### Units

One commit each. Every unit leaves `cargo nextest run` green. S applies to all.

| # | Unit (source) | Depends | Gates | Files | Spec/doc in the same commit | Tests by kind | Completion beyond the gates |
|---|---|---|---|---|---|---|---|
| W0 | Plan `docs/plans/<date>-worker-threads.md` (this design, the S1–S3 decisions, the kind lists) | — | — | plan only | — | — | `git show --stat HEAD` lists only that file |
| W1 | S3 commit 1: R1 in `share` | W0 | G, IR | heap/share.rs, heap/tests/share.rs, tests/ui/run/par/poly_lin_resend_shared_elements.em | OM 「`share`」 (bullet :259 replaced, row :331), 「報告する誤り」; testing.md:104, :136; status.md:142 bullet removed; roadmap.md:31 sentence removed | Kind 2: `an_rc2_object_under_a_resent_lin_object_keeps_the_senders_owner` → `…_is_an_error`. New: `resending_after_marking_an_rc2_node_under_the_lin_part_is_ok` (both modes); UI `par/poly_lin_resend_shared_elements.em` | `cargo nextest run -p eml_rt -E 'test(/an_rc2_object_under_a_resent_lin_object_is_an_error$/) \| test(/resending_after_marking_an_rc2_node_under_the_lin_part_is_ok$/)'` → 3 run, 3 passed. Before the commit, `git status --porcelain` lists exactly one new `.snap` (`integration__ui__run@par__poly_lin_resend_shared_elements.em.snap`). `grep -rn "「\`share\`」の制限\|前の持ち主のまま残る\|前の持ち主に残る" docs crates` → no output |
| W2 | S2 U1: page head split, `RemoteSink`, ownership by sink, T6/T14 `page_sink_of`; `RemoteSink: Sync` asserted | W0 | G, IR | eml_rt alloc.rs, heap/*, trust.rs, lib.rs | OM 「ページ」「生存の列挙」「凍結する」「`unsafe` の核」 | Kind 2: `TRUSTED` rows | `cargo nextest run -p eml_rt -E 'test(trust::)'` → 6 passed. `git diff --stat HEAD~1 -- crates/eml_vm/tests/bench.rs` → empty |
| W3 | S2 U2: remote free, `Stats.remote_frees`/`reclaimed`, `reclaim`/`reclaim_if_due`, `Heap::over_budget`, `Drop` reclaims, `pushing`/`NotQuiescent`, walks take `&mut self` and reclaim first; VM wrappers `Ctx::{reclaim, reclaim_if_due, check_invariants, leaks}`; `Machine.check_heap: fn(&mut Ctx<M>)` | W2 | G, IR | eml_rt alloc.rs, heap/*, remote.rs, ctx; eml_vm lib.rs | OM new 「別のスレッドからの解放」, 「実行の制御」 note, T14 row; testing.md 「Miri」「単体テスト」; crates/eml_rt/CLAUDE.md budgets; multicore.md 「別のスレッドからの解放」 | Kind 2: Miri budgets; `Stats` gains two fields. Kind 3: `let mut heap` for walks; `over_budget` callers. New: Miri (a)(b)(c)(e), where (a) asserts `stats_B.remote_frees == n` and `stats_A.reclaimed == n`; unit `reclaim_changes_no_work_counter` (both modes: after remote frees, `reclaim` changes only `live`/`quarantined`/`reclaimed`) | `cargo nextest run -p eml_rt -E 'test(the_size_of_heap_error_stays_at_twelve_bytes) \| test(reclaim_changes_no_work_counter)'` → 3 passed |
| W4 | S2 U3: foreign acceptance, `ForeignLend`, three-level `PageMap` in `Control` with `UnknownPage`, the foreign-child rule in `check_reach` and `freezable` (`child_ok` gains `foreign(c) && foreign_alive(c)`), T0 restated, T15 `PageMap`, `PageMap: Send + Sync` asserted; S3's threaded Miri test | W3, W1 | G, IR, C8' | eml_rt heap/core.rs, walk.rs, control.rs, remote.rs, tests/trusted.rs, lib.rs | OM 「核の操作」「報告する誤り」「検査モード」; OM:394 removed; multicore.md:327 | New: Miri (d), `a_lin_part_moves_to_another_thread_and_back` | `cargo nextest run -p eml_rt -E 'test(trust::)'` → 6 passed. C8' numbers in the commit message, per-test increase < 0.2 s |
| W5 | S3 commit 2: `check_lin_parts`, using S2 r3's pop-step text verbatim (`UnknownPage`; own page / region page / other registered page via `foreign_lend`; freed or token → `UseAfterFree`; already in `seen`, or rc ≠ 1 → `SharedReachesLocal`; the `seen` set replaces the `blocks.len()` bound) | W4 | G, IR | eml_rt heap/walk.rs, heap/tests | OM:668 bullet (S3 text) | New: `check_invariants_finds_an_rc2_object_in_a_lin_part` (checked) | `cargo nextest run -p eml_rt -E 'test(/check_invariants_finds_an_rc2_object_in_a_lin_part$/)'` → 1 passed |
| W6 | S2 U4: `WorkerIds` in `Control`, `check_owned` (the exception and precondition of §1.1), `retire_worker` precondition, `recover` resets the table, `WorkerIds: Send + Sync` asserted; VM: checked `check_heap` = `check_invariants` + `check_owned(&[heap])` | W5 | G, IR | eml_rt workers.rs, control.rs, recover.rs, lib.rs; eml_vm lib.rs | OM 「論理的な worker ID」; multicore.md 「論理的な worker ID」 | Kind 3: recount calls → `check_owned`. New: Miri (f); trusted `reclaim_changes_no_owner_count_and_no_work_counter` (checked, two threads: B remote-frees blocks owned by ID 3 on A's pages; after the join, A records `count[3]`, `count[0]` and the `Stats` work fields, calls `reclaim`, and asserts they are unchanged, `live` is down by n and `reclaimed` is up by n) | `cargo nextest run -p eml_rt -E 'test(reclaim_changes_no_owner_count_and_no_work_counter)'` → 1 passed (one checked `#[test]`) |
| W7 | S2 U5: `recover` copies from any registered page; the drop-order rule. Final Miri budgets | W6 | G, IR | eml_rt ctx/recover.rs; crates/eml_rt/CLAUDE.md; testing.md 「Miri」 | OM 「回復」 ("nothing persists on heaps 1..N-1 because a run with session roots never spawns"); multicore.md:328 | New: Miri (g), extended with `check_invariants` → `Ok` over a shared object on heap 0 whose `lin` child is on heap 1 (G-S2-13) | G with Tree Borrows ≤ 265 s and the measured Stacked Borrows budget written down |
| W8 | S1 U1: `Limits.workers` (default 1), `Control.words`, the per-index API, END, `end_run`/`clear_end`, `stop_reason` END, `Ctx.word`, `Ctx::for_worker(control, host, k)` | W7 | G, IR | eml_rt control.rs, heap/ctx.rs; 29 `Limits {..}` literals in 12 files | OM 「実行の制御」 | Kind 2: `Control` call shapes in eml_rt and monitor tests. Kind 3: `Limits` literals. New: `every_word_sees_interrupt`, `end_run_sets_end_in_every_word_and_stop_stays_clear`, `stop_reason_puts_end_below_memory` | IR gate (the G-S1-4 check) |
| W9 | S1 U2: `execute` runs on `eml-worker-0` with the host masked across the join; `eml_io::Masked` made `pub`; `spawn_scoped_masked`; the `CaptureHost` reader masked; spawn allow-list test | W0 | X, IR | eml_vm lib.rs, hosts.rs; eml_io mask.rs, lib.rs; eml_vm tests/spawns.rs, tests/main.rs | vm.md 「上限と割り込み」 (worker-0 thread, host mask, spawn rule) | New: `spawns::every_thread_but_worker_0_is_masked` (the §1.5 multiset) | `cargo nextest run -p eml_vm --test integration -E 'test(spawns::) \| test(signal::)'` → pass. `cargo nextest run -p eml_cli --test integration -E 'test(cli::)'` → pass |
| W10 | S1 U3: `Run` (created per run, N slots), `Slot`, the §1.2 protocol for worker 0 alone, the `has_notices` load in `collect`; monitor on `Arc<Run>` (per-slot switches and tickers, `wake_all`, doorbell tick gating, no-doorbell ticking); the monitor starts if `driver_kind != Virtual \|\| limits.workers ≥ 2`, otherwise `Workers::Denied`; `wait_deadline` uncapped iff a monitor runs or the clock is `Virtual`; `Scheduler.driver: Option`; `const _` asserts for `Run`, `JoinNode`, `Outcome` | W8, W9 | X, IR | eml_vm sched.rs, monitor.rs, vm.rs, workers.rs (new) | vm.md 「タスクとスケジュール」 (monitor start rule; doorbell-gated tick), 「時計と driver」 (cap only without a monitor; stop latency ≤ one tick without a doorbell) | Kind 2: sched.rs `begin_run_starts_the_monitor_only_for_a_real_run_with_a_doorbell` → `begin_run_starts_the_monitor_for_every_sleeping_driver`; sched.rs `wait_deadline_caps_only_drivers_that_sleep_without_a_relay` → `wait_deadline_caps_only_without_a_monitor`; api.rs `an_injected_driver_waits_at_most_10_ms_at_a_time` → `an_injected_driver_waits_until_its_deadline`. Expected unchanged (B5): api.rs `a_real_run_with_a_doorbell_has_a_monitor_from_its_start`, `an_interrupt_wakes_a_sleeping_worker`, `the_default_clock_is_virtual`; io.rs `a_cancelled_net_write_wait_raises_cancelled`, `a_read_cancelled_during_its_transfer_raises_cancelled`, `a_spent_budget_looks_at_the_os_and_goes_on_alone`, `one_task_of_finished_ops_never_waits_on_the_driver`; machine.rs `a_driver_left_pending_poisons_the_machine`, `a_failed_run_quiesces_its_driver_before_recovery`, `a_failing_driver_breaks_the_run`, `every_run_quiesces_its_driver`; signal.rs `a_relayed_signal_without_a_waiter_stops_the_run`, `an_injected_driver_does_not_listen_for_os_signals`. New: `monitor::tests::the_monitor_sleeps_while_every_worker_is_parked`, `monitor::tests::a_monitor_without_a_doorbell_wakes_a_parked_worker_on_a_stop_bit` | `cargo nextest run -p eml_vm --lib -E 'test(monitor::) \| test(sched::)'` → pass. `grep -rn "WAIT_CAP_MS" crates/eml_vm/src` → hits only in `wait_deadline`, the constant and its comment |
| W11 | Spawn infrastructure: the `Workers` state and gate (roots, `uses_roots`, monitor); `SPAWN_WORKERS` and `Exit` trampoline; `spawn_workers` (slots parked and idle); the `run_worker` loop (idle only); worker-exit rows (stop + `wake_all`, `fatal`, panic); `end_run`/join/`clear_end`; `settle`; `Ctx` return; heaps 1..N-1 dropped after the join (empty here); `Finished.workers` | W10 | X, IR | eml_vm lib.rs, sched.rs, vm.rs, workers.rs, code.rs, compile/emit.rs; tests/workers.rs, tests/main.rs, tests/api.rs | vm.md 「タスクとスケジュール」 (workers; the spawn conditions of G-S1-2 and that a session with roots runs on one worker; shutdown order; panic) | Kind 3: `Finished {..}` literals (ui.rs:368, 409; lib.rs:589, 609); `Scheduler::new`/`Monitor::start` signatures. New `workers::`: `a_run_without_par_starts_no_worker`, `the_first_fork_starts_the_workers`, `code_with_roots_starts_no_worker` (also when a root-using program is appended to a Machine that spawned before), `an_interrupt_ends_a_run_with_idle_workers`. Unit: `workers::tests::a_fatal_wins_over_any_result_of_worker_0` (`settle` table: `Ok`, `Stopped(Ended)`, `Stopped(Interrupted)`, `Err(Deadlock)` × `Some(heap fault)`/`None`). api.rs: `Finished: Send` | `cargo nextest run -p eml_vm --test integration -E 'test(workers::)'` → 4 passed. `cargo nextest run -p eml_vm --lib -E 'test(workers::)'` → pass |
| W12 | Placement and the N-heap run end: `Notice::{Child, Joined}`, `send`/`wake`/unpark, the `idle` set, `in_flight`, the placed-end reorder, `Task.placed`, `JoinNode.orphan`, remote `task_failed`, the deadlock and virtual-time rules; forced reclaim after the join and again after a shown copy; flush; `check_heap(ctx0, &mut others)` with `check_owned` over all heaps; leak check of heaps k; `Machine.others` and the recovery/drop order; checked assert `sched.shared_roots == ctx.roots().len()` at placement; `Finished.placed`; `Notice: Send` assert | W11 | X, IR | eml_vm vm.rs, sched.rs, workers.rs, lib.rs; tests/workers.rs, tests/signal.rs | vm.md 「`par` の fork と join」 (「fork」: placement; 「子の終わりと join」: `Joined`; 「子の誤り」: S1 r3's replacement text), 「タスクとスケジュール」 (I1–I4), 「時計と driver」 (deadlock with `in_flight`; virtual time only when no worker runs), 「誤り、止めること、終わり」 (the run end over N heaps: reclaim, flush, checks or recovery), 「追記」 (heaps 1..N-1 kept until recovery), 「並列化への備え」 item 4 | New `workers::`: `a_child_runs_on_another_worker` (checked, `placed ≥ 1`, output and stats equal to N = 1); `a_shown_result_from_a_child_is_released_before_the_checks` (checked `run_shown` at N = 2, the right child builds the string; Ok); `an_interrupt_stops_a_child_on_another_worker`; `an_interrupt_reaches_worker_0_parked_in_join_wait`; `an_interrupt_reaches_a_parked_worker_without_a_doorbell`; `exit_abandons_a_child_on_another_worker`; `a_heap_error_in_a_placed_child_ends_the_run`; `a_depth_error_in_a_placed_child_is_reported_at_join`; `recovery_after_an_error_drops_n_heaps`; `a_timer_on_worker_0_and_a_child_on_worker_1_agree`; `nested_children_place_on_eight_workers`. Unit `workers::tests::the_parent_frees_a_child_result_remotely` (G-S2-14). `signal::` gains N = 2 variants of the interrupt and terminate tests | `cargo nextest run -p eml_vm --test integration -E 'test(workers::)'` → 15 passed. `cargo nextest run -p eml_vm --lib -E 'test(workers::tests::the_parent_frees_a_child_result_remotely)'` → 1 passed. `cargo nextest run -p eml_vm --test integration -E 'test(par::) \| test(miri::)'` → pass |
| W13 | `RunStats` aggregation: `worker_work`, `worker_ctx`, worker `RunCounts`, sum/max | W12 | X, IR | eml_vm lib.rs; tests/workers.rs | vm.md 「`RunStats`」 (sum the counts, max the peaks) | New: `workers::stats_at_two_workers_equal_one_worker` | `cargo nextest run -p eml_vm --test integration -E 'test(workers::) \| test(bench::)'` → pass with the bench snapshots unchanged (16 `workers::`) |
| W14 | S1 U6: CLI `--workers`, `EML_WORKERS`, `available_parallelism` | W13 | X | eml_cli main.rs, lib.rs; tests/cli.rs, tests/api.rs | stdlib.md 「起動」 | New: `cli::workers_flag_is_accepted_and_rejected`, `api::repl_limits_run_one_worker` | `timeout 20 cargo run -p eml_cli -- run --workers 2 tests/ui/run/par/both.em` → stdout byte-identical to `--workers 1`, exit 0. `EML_WORKERS=0 cargo run -p eml_cli -- run tests/ui/run/par/both.em; echo $?` → `2`. `cargo run -p eml_cli -- run --workers 0 tests/ui/run/par/both.em; echo $?` → `2`. `cargo run -p eml_cli -- repl --workers 2; echo $?` → `2`. `timeout 20 cargo run -p eml_cli -- run --debug-heap --workers 8 tests/ui/run/par/nested.em; echo $?` → `0` |
| W15 | S1 U7: UI `par/` at N = 2 and 8 | W14 | X | eml_cli tests/ui.rs | testing.md 「UI テスト」, 「seed で流す」 | New in-file: `ui::workers_runs_cover_par_only`. No snapshot changes | `EML_UI_FILTER=/par/ cargo nextest run -p eml_cli --test integration -E 'test(=ui::run) \| test(=ui::run_fail)'` → 2 passed. `cargo insta test -p eml_cli --check` → nothing pending. `cargo nextest run --profile timing -j 1 -p eml_cli --test integration -E 'test(=ui::run) \| test(=ui::run_fail)'` → pass. Wall time of `ui::run` before/after in the commit message (expected +0.2 s or less) |
| W16 | S1 U8 + S2 U7: CI `tsan` job | W12, W4 | — | .github/workflows/ci.yml; CLAUDE.md command line | testing.md 「CI」, new 「ThreadSanitizer」 | — | `nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml` → exit 0. `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=thread TSAN_OPTIONS=halt_on_error=1 CARGO_TARGET_DIR=target/tsan cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_rt trusted::` → exit 0. The same with `-p eml_vm --test integration -- workers:: signal:: preempt::` → exit 0 |
| W17 | Close (roadmap 「進め方」 steps 1–9) | W0–W16 | — | docs only | Spec sweep. `bench/scripts.sh`, then the `bench/run.sh`, `bench/rt.sh` and `bench/scripts.sh` tables on macOS (G-S2-9: local free +≤ 2 instructions per free; `dup`/`decref` rows unchanged), plus a heading comparing `par_tree` at `--workers 1` and `2`. status.md. architecture.md (`workers.rs`, the worker-0 thread, lazy workers, `spawn_scoped_masked`, no driver on k ≥ 1). roadmap: the stage removed; S1 r3's `async-threads`/`par-steal` texts; deferred items (S1 r3's three, plus S2's dirty-page index and `rt-concurrent-proofs`). multicore.md (S1 r3 §5 line list; S2 invariant 5, 「テスト」). overview/README if changed. docs/plans removed | Kind 2: benchmarks.md tables | `grep -rn "worker-threads" docs/spec docs/implementation` → no output. `ls docs/plans` → no worker-threads file |


### Test changes by kind (whole stage)

- **Kind 1 (pass/fail):** none.
- **Kind 2 (expected value):**
  - W1: the S3 rename.
  - W2–W6: `TRUSTED` rows.
  - W3, W4, W6, W7: Miri budgets.
  - W3: `Stats` gains `remote_frees` and `reclaimed`.
  - W8: `Control` call shapes.
  - W10: the two sched.rs unit tests and api.rs `an_injected_driver_waits_at_most_10_ms_at_a_time`.
  - W17: benchmarks.md tables.
  - `citations.rs` snapshots, only if a cited heading changes.
- **Kind 3 (mechanical):**
  - W8: `Limits` literals.
  - W11: `Finished` literals.
  - W3: `let mut` for walks and the `over_budget` callers.
  - W6: recount → `check_owned`.
  - W3: two-heap tests → `with_control(shared)`.
  - W10/W11/W15: `Scheduler::new`, `Monitor::start` and `execute_in` signatures.
  - W10: `driver_mut()` at 31 sites.

### Check against S1 r3 §8 and S2 r3 §8

| Criterion | Where it is met |
|---|---|
| G-S1-1 | W12: `Notice` has two kinds; `Registry` access only behind `driver: Some`. |
| G-S1-2 | W11: gate and tests `a_run_without_par_starts_no_worker`, `the_first_fork_starts_the_workers`, `code_with_roots_starts_no_worker`. `Finished.workers`. |
| G-S1-3 | W8: `raise` loops; `every_word_sees_interrupt`. |
| G-S1-4 | W8 (`Ctx.word`); IR on W1–W13. |
| G-S1-5 | W9: worker 0, host mask, allow-list. `signal::` at N = 2 in W12. |
| G-S1-6 | W10: start rule, cap, gating, two monitor tests. W12: the two interrupt tests. |
| G-S1-7 | W12: §1.2 (`running` changes only under the slot = inbox lock); `a_timer_on_worker_0_and_a_child_on_worker_1_agree`. |
| G-S1-8 | W12: join → reclaim (again after a shown copy) → flush → checks/drop or recovery. Workers k never write `Control::end`. Its three tests are in W12. CLI exit codes unchanged (W9, W14). |
| G-S1-9 | W12: placement assert. W14: `repl_limits` test. W11: `code_with_roots_starts_no_worker`. |
| G-S1-10 | W13 + W15. |
| G-S1-11 | W10/W11: no driver on k ≥ 1, `driver_mut()` internal error. Nothing else listed is created. |
| G-S1-12 | W10, W12 (`const _`), W11 (`Finished: Send`). |
| G-S2-1, -2 | W2/W3 (Miri (e)). |
| G-S2-3 | W3 (Miri (b)). |
| G-S2-4 | W3 (walks); W6 (`check_owned`); VM forced reclaims in W12. |
| G-S2-5 | W1/W5 + W15 (checked N = 2/8). |
| G-S2-6 | W3. |
| G-S2-7 | W2–W7 rows; budgets per C6'. |
| G-S2-8 | `RemoteSink` W2, `PageMap` W4, `WorkerIds` W6; asserts in eml_rt lib.rs. |
| G-S2-9 | IR record on W2/W3 (`list` +≤ 2 per free); `bench/rt.sh` at W17. |
| G-S2-10 | W6; VM edge W12. |
| G-S2-11 | W7; VM order W12. |
| G-S2-12 | W4; IR for fast mode. |
| G-S2-13 | W4 (`check_reach`, `freezable`); W5 (`check_lin_parts`); W7 (Miri (g) extended); W15 (checked UI at N = 2). |
| G-S2-14 | W3 (Miri (a)); W12 (unit test; see §9). |

## 6. Unresolved items (default if work proceeds)

- **U1** Verus and `Arc<AtomicU32>` in `Ctx` (B1). Default: try it in W8, fall back to the index form.
- **U2** Worker 0 in the idle set. Default: eligible (S1 r3 C3).
- **U3** `miri::par_on_two_workers`. Default: added in W12 if it fits 10 s under Tree Borrows; otherwise a roadmap note in W17.
- **U4** `--stats` printing `workers`/`placed`. Default: not printed.
- **U5** `REMOTE_RECLAIM`. Default: 256.
- **U6** IR threshold 0.1%. Default as stated; widen it only with measured noise recorded in the plan.

## 7. Additional checks

- [V] `--stats --sched sequential` on a release build of 0f9cd9b:
  - empty: `rc_decrements=0 allocations=1`
  - loop: `rc_decrements=1 allocations=2`
  - fib: `rc_decrements=1 allocations=2`
  - par_tree: `rc_decrements=7977 allocations=7978 shared_objects=3851`
  - list: `rc_decrements=100001 allocations=100002`
- [V] Cachegrind `I refs` on the same build: empty 19,796,867; loop 196,045,081; fib 114,540,917; par_tree 41,767,918. Each was identical across repeated runs.
- [V] `copy_string` takes and releases its argument (heap/ctx/io.rs:183). The result is copied at lib.rs:567, before the flush (lib.rs:570-572).
- [V] `Machine` has no public accessor for heap statistics (`pub fn` list, lib.rs:276-399). Hence G-S2-14's VM observation is a `src/` unit test.
- [V] The spawn multiset and the timings are from r2 §7, still on 0f9cd9b.
- Read in full: X-conformance-r1, X-ruling-r1, S1-response-r3 §5 and §8, S2-response-r3 §1 (r3 parts), §5, §8, §10.
- Scratch clone: `/tmp/claude-0/-home-user-eml/e8305f47-7239-534e-8be7-5a0da86d7258/scratchpad/wt/scratch-S4`. Its probe edits were reverted. No background task is running.

## 9. Open items for the guideline owner

- **G-S2-14 Check wording.** It says "S4 W12's `a_child_runs_on_another_worker` asserts `ctx0.heap().stats().remote_frees >= 1`". That integration test cannot read `ctx0` without a test-only public accessor, which the project rules forbid. Implemented as the `src/` unit test `workers::tests::the_parent_frees_a_child_result_remotely` in W12. Proposed wording: "… a W12 unit test in `crates/eml_vm/src/workers.rs` asserts …".
- **S2 r3 "final budget numbers written by W6".** Miri (g) lands in W7, the ninth `trusted` test, so W7 writes the final numbers. This is a wording fix only.

Every other X item is settled by S1 r3 / S2 r3 and is no longer listed.

## Change log (r2 → r3)

- **Ruling on `shown`:** option (i). `shown.is_none()` is no longer a gate condition. On a shown run with workers, all heaps are reclaimed a second time after the result copy, and the stats are read after that reclaim. The §2 rationale is corrected. `a_shown_run_starts_no_worker` is replaced by `a_shown_result_from_a_child_is_released_before_the_checks` (W12). The gate now equals G-S1-2.
- **Cf-5:** `loop` and `fib` are gated at 0.1% on every IR unit, because each frees no more than 2 blocks [V]. The free-path cost is recorded on `list` (+≤ 2 instructions per free on W2/W3) and on `par_tree`. IR now covers W1–W13. The `empty` baseline was added.
- **Cf-2:** `Stats.remote_frees`/`reclaimed` (S2 r3) arrive in W3, with Miri (a) asserting them. The VM observation is the W12 unit test `the_parent_frees_a_child_result_remotely`. `Finished.placed` is kept.
- **S2 r3 folded in:**
  - foreign-child rule (G-S2-13) in W4 (`check_reach`, `freezable`), in W5 (`check_lin_parts`, using S2's text) and in W7 (Miri (g) extended);
  - the `check_owned` exception and precondition in §1.1, §1.3 and W6;
  - budgets: Tree Borrows 265 s, Stacked Borrows provisional 220 s, final numbers in W7;
  - the three-level `PageMap` and its measurement obligation (C8', W4);
  - `WorkerIds`/`PageMap` in `Control` (§1.4).
- **Unit order:** S3 commit 2 moved after S2 U3 (now W5), so that `check_lin_parts` is written once with the final rule.
- **G-S1-8 order** (S1 r3): join → reclaim → flush → checks/recovery. Flush now precedes the checks inside `run_entry`.
- **§9:** reduced to two wording items. X1–X7 are settled.
- **Correctness check:** the full G-S1/G-S2 cross-check table was added to §5.
