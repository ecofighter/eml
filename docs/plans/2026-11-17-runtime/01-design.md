# Appendix 01: the design of the runtime stages

- Status: final design, approved by the user (00 §A, UJ-17). Plans cite it as "01 §n" and by its IDs (D-xx-n, E1..E3, SC-n, G-n, S-n, P-n, H-n, U-n). 00-decisions.md wins over this file.
- Tags: [V: how] verified by the designer; [V: V5] / [V: V4] / [V: V3] verified by the named Fable verification round of the design (V5 r2 is its second round; the round records are not in the repository) and not re-run; [I] inferred. Base: main @ 4a2954c1. Rests on the user decisions UJ-1..UJ-17 (00 §A). This document replaces every earlier design of the stages it covers (the uncommitted `sched-exec` / `jit-tasks` design and the 2026-11-11 drafts of `jit-tasks` and `jit-repl`); it cites none of them. Nothing was built or run by the designer; the checks are in section 28.

## 0. Summary

- **End-state crates (19 directories, today 20).** Runtime closure: `eml_extern`, `eml_rt` (Verus), `eml_steps` (new; Verus; no `unsafe`), `eml_io`, `eml_cabi`, `eml_sched`, `eml_nrt`, and `eml_tls` / `eml_gui` behind features. Tools: the front end, `eml_lir`, `eml_core_ir`, `eml_native`, `eml_clink`. Artifact: `eml_cli`; dev: `eml_test_support`. `eml_vm` is deleted and `eml_rows` is merged into `eml_nrt`.
- **One engine.** Native code gets a checked mode, then tasks and `par`, then the REPL; then the VM is deleted. The VM's scheduler is never reshaped or moved: the new scheduler is written once, in `eml_sched`, for native code, and the VM keeps today's until it is deleted.
- **Model.** `main` is the root task; `Async.run` does not exist. A `par` child is a total function; a `Panic` crosses as a value in std; a runtime error in a child ends the run. A fork lists a job in its worker's open list; an idle worker takes the oldest job of another worker; a join runs its own job in the parent's frames or blocks until the taker sends the value. Nothing but a join waits with frames. Memory has one limit, which ends the run.
- **Stages (100 units before `native-aot`):** `engine-trust` 7 -> `async-root` 9 -> `par-total` 9 -> `task-limits` 5 -> `std-par` 6 -> `native-checked` 12 -> `sched-core` 15 -> `jit-tasks` 16 -> `jit-repl` 8 -> `vm-retire` 9 -> `sched-proofs` 4 -> `native-aot` -> `native-inline`.
- **Proved (Verus).** In `eml_steps`: the open-job list, the worker loop's step choice, the limit table, the idle protocol and the deadlock verdict as step functions; an exhaustive explorer over those functions in the tree; `sched-proofs` proves the wake-up and verdict invariant for every N. In `eml_rt`: every checked operation is "audit, then body", and a passing audit implies the body's safety premise. `rt-concurrent-proofs` stays deferred.
- **Profiler and meta mechanisms.** Section 15: one "code offset -> position" table per function; five hooks laid in the planned stages; the profiler as three later items.

## 1. The stage list

| # | Stage | Needs | Units [I] | Engines after it |
|---|---|---|---|---|
| 1 | `engine-trust` | - | 7 | VM (all), native (synchronous programs, fast mode) |
| 2 | `async-root` | - | 9 | same |
| 3 | `par-total` | - | 9 | same |
| 4 | `task-limits` | 2, 3 | 5 | same |
| 5 | `std-par` | 3 | 6 | same |
| 6 | `native-checked` | 1 | 12 | native gains checked mode and `--stats` on the programs it accepts |
| 7 | `sched-core` | 2, 3, 4 | 15 | unchanged; the new scheduler runs under `Script` in tests |
| 8 | `jit-tasks` | 2, 5, 6, 7 | 16 | native runs every program, fast and checked, at N workers |
| 9 | `jit-repl` | 8 | 8 | native REPL; native is the default; `--vm` selects the VM |
| 10 | `vm-retire` | 9 | 9 | native only |
| 11 | `sched-proofs` | 7 | 4 | - |
| 12 | `native-aot` | 10 | roadmap | |
| 13 | `native-inline` | 12 | roadmap | |

- The stages run in the listed order. Language changes (2-4) come first and are made in the VM's existing code, because both engines must agree on the language while both exist. Scheduler changes are made only in the new scheduler.
- What exists twice between `sched-core` and `vm-retire`, as the accepted cost of not touching the VM: the scheduler, the task rows' arms, the `Std.Atomic` counter table and the limit decision each have a VM implementation and a new one. No file is moved twice and no VM code is rewritten for the new design.
- What is written and later deleted: the parity harness, `native: skip` and 「VMとの違い」 (from `jit-tasks` to `vm-retire`); `GateHost`'s second implementation.
- Sizes are restated when each plan is written.

## 2. The model

### 2.1 Things

| Thing | Definition | Moves between workers |
|---|---|---|
| worker | one OS thread, one `Ctx`, one heap, one inbox, one ready queue, one open list, one I/O driver (real clock). Worker 0 runs `main`, keeps the clock, the verdict and END. The main worker is the extra one on the calling thread with a replaceable `Park` | - |
| task | an `Async` task: a registry cell and, while waiting, a continuation | never after placement |
| root task | the task that runs `main` when `main`'s row has `Async` | never |
| job | `Job { g: Marked, id, site, done }`, unstarted | until it starts |
| child | a started job, or a `par` child run in its parent's frames (left always; right when not taken) | never |
| activation | what runs one entry or one taken job to its end or next wait: frames above `drive` | never |

Isolation invariant (object-model.md, written by `sched-core`): a task's or child's local objects (positive count) are reached only by that task or child; a `par` left child runs under its parent's ID; every crossing marks or moves on the sender's thread before publication.

### 2.2 Types (final shape)

```rust
// eml_steps/src/open.rs   (verus!; vstd only)
pub struct Open<J> { joins: VecDeque<(u64, J)>, cells: VecDeque<(u64, J)>, next: u64 }   // serials strictly increase front to back in each
impl<J> Open<J> {
    pub fn push_join(&mut self, j: J) -> u64;      pub fn push_cell(&mut self, j: J) -> u64;
    pub fn take_own(&mut self, serial: u64) -> Option<J>;     // the back of `joins` has `serial` -> pop_back
    pub fn newest_join(&self) -> Option<u64>;                 // the serial at the back of `joins`
    pub fn take_detached(&mut self) -> Option<J>;             // cells.pop_front
    pub fn take_oldest(&mut self) -> Option<J>;               // the front with the smaller serial
    pub fn len(&self) -> usize; }
// eml_sched/src/job.rs
pub struct Marked(Word);                            // made only by `fork::mark(ctx, w, id)`, which shares `w` with `id`
pub enum Done { Join { worker: u32 }, Cell(CellRef) }
pub struct Job { pub g: Marked, pub id: u32, pub site: u64, pub done: Done }     // no Clone
// eml_sched/src/run.rs
pub struct Entry { pub cell: CellRef, pub worker: u32, pub kind: EntryKind, pub word: Word }
pub enum Notice { Joined { serial: u64, value: Marked }, Place { .. }, Wake(Woken), Cancel(CellRef),
                  Submit(CellRef), CancelIo(CellRef), Completion(CellRef) }
// SlotState (under the slot mutex): inbox, woken: bool, open: Open<Job>
// Slot: listed: AtomicU32 (open.len(), written under the slot lock), current: AtomicU64 (15.2 H-2), stack: (usize, usize) (H-3)
// Run: running: AtomicU32, sleepers: AtomicU32, searching: AtomicU32, idle: Mutex<Sleepers>
// eml_sched/src/fork.rs
pub unsafe fn fork<M: Mode, P: Policy>(core: &mut WorkerCore<P>, ctx: &mut Ctx<M>, g: Word, site: u64) -> Result<(u32 /* id */, u64 /* serial */), RunFault>;
pub fn take_own<P: Policy>(core: &mut WorkerCore<P>, serial: u64) -> Result<Option<Job>, RunFault>;   // 2.5: asserts strict nesting on `None`
pub fn wait_join<P: Policy>(core: &mut WorkerCore<P>, control: &Control, serial: u64) -> Result<Marked, Stop>;
pub unsafe fn join<M: Mode>(ctx: &mut Ctx<M>, right: Marked, id: u32) -> Result<Word, RunFault>;   // transfer_back, retire_worker
// eml_sched/src/exec.rs
pub trait Exec { type M: Mode; type P: Policy;
    /// この worker の核と文脈を、項の境目で、閉包の中でだけ借りる。
    unsafe fn with<R>(this: *mut Self, f: impl FnOnce(&mut WorkerCore<Self::P>, &mut Ctx<Self::M>) -> R) -> R;
    /// 項を1つ、終わりか次の待ちまで走らせる。呼ぶ間、`drive` は `this` から導いた参照もポインタも持たない。
    unsafe fn enter(this: *mut Self, call: Call, argv: *mut u64) -> Result<(), Fatal>; }
pub struct Script<M, P> { .. }                      // an `Exec` whose `enter` is a Rust closure; ordinary public API
// eml_sched/src/drive.rs
pub enum Until { Root, Stop, Idle }
pub unsafe fn drive<E: Exec>(this: *mut E, until: Until) -> Result<(), Fatal>;
// eml_sched/src/policy.rs
pub trait Policy { fn pick(..) -> usize; fn wake(..); fn budget(..) -> u32; fn at(&self, p: Point) {} }   // Normal, Sequential, Seeded
// eml_sched/src/config.rs:  Finished gains claims: u64, join_waits: u64 at jit-tasks JT-5 (00 §D D-6; schedule-dependent counts live in `Finished`, not `RunStats`)
```
- A join's state (`id`, the parent's `evv`, the parent's limit class, the serial) is in the native `Child` record. There is no join table, generation or orphan mark.
- `Job` has no depth and no limit class.
- `take_own` is a `pop_back`, `take_detached` a `pop_front`, nothing scans. An own join job that is still listed is the newest join job: children have no `Async`, `wait_join` runs nothing, and a taken job's forks nest inside its item [V: V5 R4 C-1].
- `Marked` has the only `unsafe impl Send` for a word carrier in `eml_sched`'s new files (`Word` itself is `Send` by an `unsafe impl` in `eml_rt` [V: word.rs L363], so a newtype with one constructor carries the rule).
- The ready queue is `VecDeque<Entry>` inside `WorkerCore`: owner-only by type. No queue item holds frames.
- `WorkerCore::schedule(run: Runnable)` pushes an `Entry` for `Start` / `Resume` / `ResumeIo`.

### 2.3 The loop

`drive` is an interpreter of `eml_steps::plan::next(Facts) -> Step`. One iteration: 1 inbox; 2 a ready pass of at most the items queued when the pass began; 3 `take_detached` (after every pass); 4 only when the ready queue is empty: a foreign job (2.4); 5 I/O look (every iteration); 6 park.
- The separate main worker does 1, 2, 5, 6. A merged-seat worker 0 (REPL) does every step.
- An item is an `Entry` or a `Job`. Item open: the every-build assert (`evv == ()`, no yield in flight, no parked fault, the engine's child stack at its base), write `Slot.current`, `switch_task`. Item close: `Slot.current = NO_CELL`, switch to the worker's own ID, deliver the result.
- `Seeded` picks the next ready entry by the seed (`random % queue length`) and draws each entry's operation budget (`1 + random % 128`) from the seed; nothing is switched inside an entry. Under `Seeded` every row that reads or writes a registry cell, a channel or an atomic spends the budget (runtime.md 「操作の予算」 lists the rows).

### 2.4 Publishing, taking, sleeping, the verdict

Publish (`fork`, `compute`): lock own slot, `open.push_*(job)`, `listed += 1`, unlock; then `if sleepers > 0 && searching == 0 { rule 4 }`.

The four rules:
1. The sleeper set is `Run::idle`. It lists exactly the workers counted in `sleepers`: started workers 0..N-1 only; never the separate main worker, never a worker in `wait_join`.
2. Go idle: `sleepers += 1`, list self, scan. Took a job: unlist, `sleepers -= 1`, rule 4. Took none: park.
3. Leave the sleep for any reason: unlist, `sleepers -= 1`. With other work or a stop bit: rule 4, then that work. Otherwise `searching += 1`, scan; took a job: `searching -= 1`, rule 4; took none: `sleepers += 1`, list self, `searching -= 1`, scan again as in rule 2.
4. Pass on: `if sleepers > 0 { pop one listed worker; unlock; wake it }`.

`running` and waking:
- `running` counts the workers that are not in the sleeping form. A worker leaves `running` under its own slot lock, in the same section that decides to park (`Effect::LeaveRunning` with `Park`).
- A wake (rule 4, a notice send, `wake_all`) locks the target's slot, and if the target is in the sleeping form sets `woken`, counts the target in `running`, and rings the condvar, all in that section and before the target runs. A wake in flight is therefore already counted.
- A worker in `wait_join` stays counted in `running`.

The verdict (worker 0, `eml_steps::verdict::step`):
- (a) Worker 0 takes the sleeping form under its slot lock and leaves `running`.
- (b) It waits while `running > 0`; the worker whose `LeaveRunning` makes `running` zero wakes worker 0.
- (c) With `running == 0` it reads every `listed` without a lock (nonzero: rescan), then in one registry section reads the deadlines, the operations in drivers and the signal waiters.
- (d) Under its slot lock it re-reads its inbox, `woken`, `running`, its mail, `NOTICE` and the stop word. Only if all are quiet does it report a deadlock or advance the virtual clock.
- The verdict is a loop: a wake counts worker 0 in `running`, so after any wake, and after any step that finds something not quiet, worker 0 restarts at (a). (c) is reached only when worker 0 itself has left `running` and no other worker is counted.
- The separate main worker's park (`main_loop.rs`) performs the same `LeaveRunning` under its slot lock, with the same effect "made it zero: wake worker 0".

Rules for the shell:
- Rules 2-4 and the park are `eml_steps::idle::step(pc, seen) -> (pc, Effect)`; the verdict is `verdict::step`. The shell performs each `Effect` as one atomic operation or one lock section and reports what it saw in that section.
- A scan visits every other slot once, starting at a rotating victim, and takes `take_oldest` of the first non-empty list. A scan reads a list only under that slot's lock. `listed` is read without the lock only by the verdict, after `running` (a placement test pins the readers of `listed`).
- Lock order: slot locks are never nested; the `idle` mutex and a slot mutex are never held together.
- Fork and a join that takes its own job write no shared location but the worker's own slot.
- A worker that leaves its loop on a stop bit calls `wake_all` once. The monitor thread is started whenever workers 1..N-1 are.

Model evidence: the four rules were model-checked for 3 workers x 3 publishers (13.2 M states, no state with a listed job and every taker asleep [V: V3]); the explorer of section 14 replaces that model in the tree and adds the verdict.

### 2.5 `fork_join`

Fork (parent's context):
1. `fork`: `id = heap.fork_worker()`; `mark` the unshared roots and `g` with `id`; publish `Job { g, id, site, done: Join { worker: me } }`; the push returns the serial.
2. Push `Child { cur, class, evv, id, serial }`; `switch_task((), parent_id)`; class `Stops`; run `f` in the parent's frames.

Join:
1. `take_own(serial)`: `Some(job)` -> `switch_task((), id)`, run `g` in the parent's frames, switch back. `None` -> under the same slot lock assert in every build that `open.newest_join()` is absent or smaller than `serial`; a violation is `Internal("a join's own job is behind a newer one")`, not a wait; then `wait_join(serial)`.
2. Pop the `Child`: restore the parent's `evv` and class; `join(right, id)` (`transfer_back`, `retire_worker`); build the pair.

`wait_join`: lock own slot; loop { if `Joined { serial }` is in the inbox, remove it and return its value; read the stop word: any stop bit other than `NOTICE` -> `Err(stop)`; wait on the slot condvar }. Its predicate is exactly "that notice is in the inbox, or a stop bit other than `NOTICE` is set"; other notices and `woken` are left as they are. It runs no entry and looks at no driver: a child holds no `IO`, so nothing the wait is for comes through a driver, and on the virtual clock time cannot advance while a taken child runs (the waiter stays in `running`) [V: V4]. Wake sources: a notice send; the fatal writer (END on every stop word, then wake all); the monitor's relay for INTERRUPT / TERMINATE; the MEMORY setter's `wake_all`; a leaving worker's `wake_all`.

Taken job (taker's context): an item of `drive` with `switch_task((), job.id)`; at its end switch to the worker's own ID; `Done::Join` -> `mark` the value, send `Notice::Joined`; `Done::Cell` -> `cells::finish_job`.

Runtime errors: any runtime error in a child ends the run. In a child run in its parent's frames it is the parent's error; in a taken child the taker writes the run's fatal slot. When both children fail and the right one was taken, the reported error is the first written. Both children always run when the left one returns.

Why a taken child cannot wait on its blocked parent's worker [V: V5]: a `fork_join` child has an empty row and a `par` child `<Panic>` only; every operation that waits on or observes another task needs `IO` or `Async` (`Chan.recv` / `send` are `<IO, Async>`, `try_recv` / `try_send` / `close` and every `Std.Atomic` operation `<IO>`).

## 3. The language and runtime rules this design fixes

| # | Rule | Implemented in | Approved |
|---|---|---|---|
| E1 | a join owns its job; idle workers claim the oldest open job under the slot lock; no deque of stolen frames; a native join waits in place | `sched-core`, `jit-tasks` | UJ-9 |
| E2 | `main` is the root task; `Async.run` is removed; no wait holds frames but a join; native code has no stack switching | `async-root`; `jit-tasks` | UJ-10 |
| E3 | no block is written by several threads; `std/Par` stitches chunk results with one pure row | `std-par` | UJ-9 |
| SC-1 | a runtime error in a `par` child ends the run at once | `par-total` (VM), `jit-tasks` (native) | UJ-11 |
| SC-2 | an untaken right child runs in its parent's frames | native only; the VM keeps today's placement until deleted | UJ-11, UJ-17 |
| SC-3 | `std/Par` splits as a two-level spine | `std-par` | UJ-11 |
| SC-4 | a too-small calling thread gets its own run error | `jit-tasks` | UJ-11 |
| SC-5 | `par : (Unit -> <Panic> a) -> (Unit -> <Panic> b) -> <Panic> (a, b)` is std code over the std-private `extern fork_join : (Unit -> a) -> (Unit -> b) -> (a, b)` whose children are total; `compute : (Unit -> <Panic> a) -> <IO, Panic, Async> a` likewise. User `never` effects no longer cross `par` or `compute` | `par-total` | UJ-14 |
| SC-6 | every child runs at limit class `Stops`: the first safepoint over the limit ends the run, also during a yield or at a helper entry, where a child inside a task had up to 2 MiB of headroom before | `par-total` | UJ-14 |
| SC-7 | a join whose child was taken blocks its worker until the value arrives | native only | UJ-14, UJ-17 |
| SC-8 | memory has one limit, `max_bytes`, which ends the run; the depth limit of a task stays a catchable `ResourceLimit` | `task-limits` | UJ-14 |
| SC-9 | `Seeded` permutes ready entries and shortens budgets; no engine suspends frames; one worker loop | the new scheduler; the VM's `Seeded` keeps preempting until deleted | UJ-14, UJ-17 |
| JIT-only | native code is the default from `jit-repl`'s Close, has a checked mode, and the VM is deleted | `native-checked`, `jit-repl`, `vm-retire` | UJ-15, UJ-17 |

Rejected and closed: dropping `transfer_back` (UJ-14); all `Async` tasks on one thread; always-atomic counts; children that borrow their environment; a heap per task with copied messages; every `main` as the root task (a hello-world would link the task rows); hiding `par` (UJ-10); help-on-join now (later item, UJ-12).

## 4. `engine-trust`

Goal: every engine entry that takes a program takes `&eml_lir::Trusted`; caller-kept promises closed or itemized.
- D-ET-1: as written (P1..P5), now stated in docs/spec/lir.md 「エンジンの約束」 and docs/implementation/architecture.md 「公開の面と`unsafe`」.
- D-ET-2: the promise list is re-checked at the Close of `jit-tasks`, `jit-repl` and `vm-retire`.

- D-ET-3 (UJ-18 F-1): the guarantee covers every crate but `eml_test_support`; the runtime-layer cut is D-RL-1..D-RL-7 of the engine-trust plan (`git show 97cd579c:docs/plans/2026-11-17-engine-trust.md`; 00 §D D-9): the 49 `pub fn` of `eml_sched::cells` that take a `Ctx` become `unsafe fn` under one contract; `eml_rt::RawBuf` becomes unforgeable (an `eml_rt` change under its gate); architecture.md 「公開の面と`unsafe`」 states the classification rule and `crates/eml_cli/tests/surface.rs` keeps it (S-26, G-24).

Units: 1 plan; 2 `Trusted` and `Session::compile`; 3 VM entries; 4 native entries; 5 `extend` / `run_shown` / `Loaded` (two new tests; three by 00 §D D-8); 6 `load_with_std` and the remaining promises; 6a `cells` `unsafe fn` and the surface test; 6b `RawBuf`; 7 Close. Tests: kind 3. Spec: lir.md 「実行の前提」「エンジンの約束」, vm.md, object-model.md, architecture.md, testing.md.

## 5. `async-root`

Goal: `main` is the root task; `Async.run` is removed.
- D-AR-1: `main : Unit -> <r> Unit`, `r` a subset of `{IO, Panic, Std.Async.Async}`.
- D-AR-2: with `Async` in `main`'s row the entry is a root entry: translate emits `main$root : Unit -> <IO, Async> Unit = Std.Async.bottom main ()`; at run start worker 0 opens the root scope, makes the root cell and queues `Entry { kind: Start }` with `main$root`'s static closure; worker 0's loop ends when the root task has finished and the root scope is empty. A `main` without `Async` runs as today.
- D-AR-3: a root that ends `Io` / `Panic` gives the runtime error an uncaught failure in a synchronous `main` gives, naming `main`, with no position. `exit`, a stop bit and a signal with no waiter act on the root task as they act today on the root task of `run`.
- D-AR-4: deleted: `Async.run`, `run_raw`, `run_result`, their rows, the VM's `AsyncRunRaw` arm, the `Task` that waits for a root, pinned nested roots; in `cells.rs` what only the nested root used: `Take::Run`, the root `opener`, `WaitToken`, `Runnable::Token`, `Woken::Token`, `Registry::wait` (their only production use is the nested root wait [V: V5]).
- D-AR-5: REPL: an input whose row has `Async` runs as the root task of that input's run (the input's wrapper function plays `main`); `run_shown` takes the root cell's result.
- D-AR-6: every task starts at class `Task` with the whole depth; no shared depth budget.
- D-AR-7: the rewrite. Scope [V: V3]: 188 `.em` / `.repl` files; 29 Rust test files with 265 uses; `crates/eml_vm/tests/miri/blocking.lir`; 10 `bench/` programs; 8 check-fail files; declarations.md, expressions.md, examples.md; testing.md, status.md, benchmarks.md. A one-off script (text in the plan, not in the tree) rewrites `main () = Async.run body` and a first statement `use Async.run`, and prints every other use for manual rewrite.

Test changes: kind 1 (listed by AR-1): `nested_run`, `nested_run_cancelled`, `cancel_reaches_nested_run`, `limit_depth_nested_run`, `handle_outlives_run`, `run_on_main_returns` and `run_on_main_panics` rewritten, `memory::a_task_in_a_nested_run_is_flagged`, `workers::a_start_after_a_nested_run_places_on_another_worker`, what AR-1 finds in `pinned.rs` and the script's manual class. Kind 2: positions inside std/Async.em; positions on a rewritten line; errors that passed through `run_result`; `RunStats` snapshots of programs that called `Async.run`; transcripts. Kind 3: rewritten files with byte-identical expectations.

Units: AR-1 plan, script, lists; AR-2 entry check and `main$root`; AR-3 VM root entry and end of run; AR-4 REPL; AR-5 `.em` / `.repl` / `bench/`; AR-6 Rust-embedded programs and `blocking.lir`; AR-7 deletions (D-AR-4); AR-8 docs (stdlib.md 「`Std.Async`」「起動」「メインのworker」, vm.md 「`Async` のタスク」「待ちの2種類」, declarations.md, expressions.md, examples.md, async.md decision 2, multicore.md 「`Async.run` と入れ子」 deleted); AR-9 Close.

## 6. `par-total`

Goal: `par` and `compute` children are total; nothing aborts across a child boundary; a child's error ends the run. All in the VM's existing scheduler.
- D-PT-1: std/Par.em. The other side of a failed pair is bound by name, so a `Lin` result is dropped before the non-returning call (a wildcard is E3001 for `Lin` results [V: V4 probe p1.em; the form below passes under `--debug-heap`, p2.em]):
  ```haskell
  extern fork_join : (Unit -> a) -> (Unit -> b) -> (a, b)
  pub par : (Unit -> <Panic> a) -> (Unit -> <Panic> b) -> <Panic> (a, b)
  par f g =
    match fork_join (fn () -> try_panic f) (fn () -> try_panic g) with
      | (Ok a, Ok b) -> (a, b)
      | (Err m, r) -> panic m
      | (l, Err m) -> panic m
  ```
  A full application of `fork_join` is the `par` instruction (core-ir.md 「`par` の呼び出し」). `both` is unchanged. A total function may be passed where `<Panic>` is expected [V: V4 probe].
- D-PT-2: `compute` is std code over the job rows with `fn () -> try_panic f`, the same named-binding form; `take_job` returns the value; `Err m` is raised by `panic m`. `Panic` is in `compute`'s row also for a total child.
- D-PT-3: a child's `evv` is `()`; the parent's `evv` and limit class are saved at the fork and restored at the join by one `switch_to` (a fork can run inside a clause while the class is `Aborting`).
- D-PT-4: a child that ends with a yield in flight is `Internal("a par child ends in the middle of a yield")` in every build.
- D-PT-5: children run at class `Stops`; a soft-limit flag seen during a child stays on the cell and is delivered at the parent's next entry after the join (the sentence is deleted by `task-limits`).
- D-PT-6: the `par` instruction is not an unwind point.
- D-PT-7: in the VM, a runtime error in a child (run by its parent or as a placed `Task`) writes the run's fatal slot; the join's error record, `task_failed`'s child branch and the orphan mark are deleted. With two failing children at N >= 2 the reported error is the first written; stdlib.md 「`Std.Par`」 and vm.md 「子の誤り」 both say so.
- D-PT-8: why `try_panic` makes a child total [V: V4]: it catches `panic` and every fault with `Fault::panics` raised by a row that is not `Pure`; everything else in a child (a `Pure` row's fault, heap and internal errors, the depth limit at class `Stops`, the memory limit, a stop) ends the run; `exit` and `fail_io` need `IO`, which `fork_join`'s empty row excludes.
- D-PT-9: the cost is accepted: about +0.38 µs per empty fork (1.66 times) on the VM [V: V4]; under 0.1 % of a `GRAIN = 1024` leaf. Later item "a child result without a handler", condition: a `bench/` row of user-written `par` recursion with a cutoff below 100 work units per fork is added and is over 1.2 times today's. Gate: `bench/run.sh` rows within +0.5 % (`par_tree` has 15 forks and cannot move); `pfib 30` cutoff 2 at N = 1 is a record.

Removed: `fork$E.op` / `reperform$E.op` and `evidence/forks.rs`; the fork evidence mirror, `fork_evv`, `Forks`, `ForkTable`, one allocation and one `share` per fork; `prim child_abort`; `JobEnd::Abort`, `Joined::Abort`, `reraise_mask`; `eml_rt`'s `alloc_fork_table`, `alloc_fork_tail`, `fork_ev`, `fork_of`; `eml_lir`'s `fork_effects`, the "has no fork entry" rule and `may_fork` (unless `root_get` marking needs it); `CompileError::MissingFork`.

Test changes: kind 1 (PT-1 reads and lists each): `run/par/abort_reraised`, `leftmost_abort_wins`, `mask_inside_the_child`, `mask_on_the_call`, `never_ops_with_arguments`, `continuation_reaches_live_handler`, `rewound_continuation_reaches_live_handler` (rewritten over `Panic` or values where the fact survives, deleted where the fact is the removed rule); `eml_rt`'s fork-evidence tests (`tests/trusted.rs` L402-465, `tests/ctx.rs` L904-1035); unit tests of the orphan path. Expected unchanged: `both`, `nested`, `handlers_inside_children`, `lin_through_children`, `poly_lin_resend`, `poly_lin_resend_shared_elements`; new `run/par/lin_result_dropped_on_panic`. Kind 2: check-fail files expecting E2020 at a `par` / `compute` call (the row-mismatch code; 13 files name E2020); the uncaught child `Panic` message (the `panic` call's position inside `<std>/Par.em` instead of `reperform$Prelude.Panic.panicked`); Core IR / LIR dumps and `RunStats` of programs that fork; `par/` run-fail files with two failing children at N >= 2 (same message in both, or `-- workers: skip`).

Units: PT-1 plan and lists (reads `may_fork`'s three callers; probes E2020's replacement code); PT-2 std (`par`, `compute`) over the old instruction; PT-3 core IR: `evidence/forks.rs` deleted, `Call::Par` typed with total children; PT-4 LIR: `prim child_abort`, its unwind and verifier rules, `fork_effects`, `may_fork`; PT-5 VM: `compile/forks.rs`, the fork evidence mirror, `MissingFork`, the join's and the job's abort ends, class `Stops`; PT-6 VM: D-PT-7; PT-7 `eml_rt` (its gate): the four fork-evidence functions, their proofs (`ctx/proof.rs` L370, L413) and tests; `evv_step` only if another caller remains; object-model.md 「evidence の3つの物体」; PT-8 `eml_native`: the `Par` / `ChildAbort` forms' abort parts; `cells::JobEnd` reduced to a value; PT-9 docs and Close (`bench/rt.sh` re-measured). Spec: stdlib.md 「`Std.Par`」「`compute`」, core-ir.md, lir.md prims, vm.md 「fork」「子の終わりと join」「子の誤り」「深さ」, types.md (E2020), multicore.md 「`par` の意味」.

## 7. `task-limits`

Goal: memory has one limit; the limit classes are `Stops`, `Task`, `Aborting`.
- D-TL-1: `Aborting` has no field. The VM's `limit_hit` keeps its steps without the flag paths: the message choice of step 6 and step 8's `clear_limit` go; step 9 is "no `IO` evidence: end the run".
- D-TL-2: deleted [V: V4 grep]: the monitor's pressure role (`pressure_tick`, `choose`, `claim_pressure`, the `pressure` argument of `Monitor::start`, the second trigger of `start_monitor`); `Notice::Limit`, `MailItem::Limit`, `Post::limit`; in `cells.rs` the cell fields `limit` and the byte count, `clear_limit`, `limit_task`, `limit_forwarded`, `add_bytes`, `TaskMarks.limit` / `.bytes`; in `eml_rt/src/control.rs` `Limits.soft_bytes`, the threshold `next`, its two readers, `claim_pressure`, and `Control::taken` if no reader remains; in `eml_vm` `Policy::MEMORY`, `Memory`, `Task.counted` / `.added`, `sched.mark`, `publish`, `clear_top`, `end_slice`, the classes `Pending` and `Aborting { pending }`, `flagged`, `mark_limited`, and the slot words `current`, `top`, `bytes`, `mark` with their version counter if no reader remains (TL-1 checks); `eml_cli`'s `soft_of`; `eml_rows/src/rows.rs` L922.
- D-TL-3: kept: the monitor thread (signal relay, `wake_all` on stop bits), `max_bytes`, the depth deferral.
- D-TL-4: gate: correctness; `bench/run.sh` rows within +0.5 %; the `eml_rt` gate for TL-3.

Test changes: kind 1: `run/async/limit_memory_after_catch`, `limit_memory_caught`, `limit_memory_fails_scope`, `run-fail/async/limit_memory_uncaught`, the unit tests of the soft limit and of the monitor's pressure role (listed by TL-1). Kind 2: `limit::every_probe`'s table (what `class x yielding x row kind x beyond headroom x over real` leaves).

Units: TL-1 plan and lists; TL-2 VM, `cells.rs`, `eml_cli`; TL-3 `eml_rt` `control.rs` (its gate); TL-4 docs (vm.md 「上限と割り込み」「タスクの上限」, stdlib.md 「タスクの上限」, object-model.md 「実行の制御」 and the trust list's sentence on the soft threshold); TL-5 Close (`bench/rt.sh` re-measured).

## 8. `std-par`

- D-SP-1: API: `for_range`, `reduce_range`, their `_grain` forms, `tabulate`, `map`, `mapi`, `reduce`, `for_mut`, `try_map`, `tabulate_unboxed`, `for_mut_unboxed`; callbacks have row `<Panic>` and every function has `Panic` in its row. `par` and `both` stay public.
- D-SP-2: the range is cut into chunks of `GRAIN = 1024`, the chunks into groups of `B = ceil(sqrt(chunks))`; the top level forks `par (rest) (last group)`, a group forks `par (rest) (last chunk)`. The shape depends only on length and grain; `reduce`'s combination order is the left-nested order of this shape, stated in stdlib.md. Fork depth is `O(sqrt(n / grain))` frames (32 768 levels for 2^40 elements). JT-14 measures bytes per level natively and stdlib.md states the largest range a pinned task can pass; if it is below 2^32 elements the split takes three levels (`cbrt`) above 2^20 chunks (a `std/Par.em` change with its shape test).
- D-SP-3: a leaf builds its chunk with the sequential `tabulate`; inner nodes return a std-private rope; the caller makes an `Array (Array a)` and calls
  ```haskell
  pub extern concat : Array (Array a) -> <Panic> Array a                 -- std/Array.em
  pub extern concat : Array (UnboxedArray a) -> <Panic> UnboxedArray a   -- std/UnboxedArray.em
  ```
  `Panic` because the summed length can exceed `MAX_LEN`. First version: one new reference per element, then the outer array is released; no `eml_rt` change.
- D-SP-4: `for_mut f a` = `freeze`, `mapi`, `thaw`; `for_mut_unboxed` copies once per pass.
- D-SP-5: `try_map` evaluates every element and returns the leftmost `Err`.
- D-SP-6: no runtime entry forks over a range.
- Expected speed-up: spine shapes 0.97-0.99 N; user-written balanced recursion about 0.67 N at N = 4 and 0.73 N at N = 8 once joins block (natively) [V: V3 simulation].
- Later items: moving the words of unique chunks in `concat` (condition: the boxed cheap-map row is over 1.3 times the unboxed row); lending a `Raw` payload to a `par` tree (the unboxed `for_mut` row is over 1.3 times its sequential in-place loop per core).

Units: SP-1 plan; SP-2 the `concat` rows (VM arm, `extern_rows!` line, native lowering); SP-3 range functions and the spine; SP-4 collections; SP-5 tests (float bits across N = 1, 2, 8 and two seeds; leftmost `Err`; marking of a boxed input; the spine's shape for three lengths) and `bench/` rows with pinned `RunStats` (boxed and unboxed cheap map, `reduce`); SP-6 Close. Spec: stdlib.md 「`Std.Par`」, the array sections.

## 9. `native-checked`

Goal: native code has the VM's checked mode and `--stats` on the programs it accepts today, before it gets tasks.

- D-NC-1 (audit before inline): checked lowering emits the same inline sequence as fast lowering, preceded by one call of the audit entry for that operation. The audits:

  | Entry | Precedes | Does |
  |---|---|---|
  | `eml_rt_audit_dup(ctx, obj)` | the inline count increment | everything checked `dup` does but the count step: settles a building object, checks page / state / owner / `lin`, writes the count of a counted immortal (`count_immortal`), counts `rc_increments` |
  | `eml_rt_audit_decref(ctx, obj)` | the inline count decrement and its uniqueness branch | the same for `decref`; counts `rc_decrements` |
  | `eml_rt_audit_field(ctx, obj, i)` | a field load | page / state / owner / kind / index |
  | `eml_rt_audit_store(ctx, obj, i)` | a field store | the same; legal on the building object of the preceding `con_at` / `alloc_data` |
  | `eml_rt_audit_unique(ctx, obj)` | the uniqueness test (`load_rc` + `is_one_ref`) | page / state / owner |
  | `eml_rt_audit_header(ctx, obj)` | a header load (`load_meta`, `load_tag`, `closure_code`, `is_big_int`) | page / state |
  | `eml_rt_audit_box(ctx, obj)` | a box load (`load_box`) | page / state / kind; counts nothing |

  A failing audit takes the fatal path. For a `tobj` / `evv` word the inline sequence tests the immediate first; the audit call sits after that test, on the object path, where the fast sequence's cold call sits (NC-1 confirms against the VM's count and moves it before the test, accepting an immediate, if the VM counts immediates); the twin pins follow.
  No operation is checked or counted twice, and none loses its check: the cold call of the inline decrement gets its own entry, `eml_rt_decref_slow`, beside `eml_rt_dup_slow` (`Step::slow()` names the two, and a placement test pins that nothing else does). In the checked entry table only these two are the bodies without the top-level audit and count; a `decref` body that frees still runs the full checked `decref` on the children. `eml_rt_decref` stays the full checked operation in both tables for its direct callers, which have no inline sequence and so no audit before them (`unwind.rs` L302: the handler state at a landing point; L317: every owned word of a cleanup path; `apply.rs` L398: the last argument when an inner application aborts [V: V5 r2]). In the fast table `eml_rt_decref_slow` and `eml_rt_decref` are the same function, so fast code changes only in the symbol of one cold call.
- D-NC-2: operations whose representation differs in checked mode are calls in checked lowering: tokens (`into_token`, `con_at`, `reuse_in_place`, `drop_token`; the inline `token_of_box` / `box_of_token` sequences), allocation, the free on rc 1. These are the only sequences whose checked form is a different instruction sequence [V: V5 on data.rs L84-235, apply.rs L236-262].
- D-NC-3 (switch sites): every module of `lower/` that names `mem::load_rc`, `store_rc`, `load_field`, `store_field`, `load_meta`, `load_tag`, `closure_code`, `load_box` or `is_big_int` emits the audit through one helper in `lower/audit.rs`. The list of modules is pinned by a placement test: `rc`, `data`, `apply`, `unwind` [V: V5 grep]. `mem.rs` stays the only module that knows offsets; `Ctx<Checked>` and `Ctx<Fast>` share the `Native` prefix at offset 0, so its `Ctx` offsets serve both lowerings [V: V5].
- D-NC-4 (the table): NC-1 writes, for every lowering helper that calls a `mem::load_*` / `store_*`, the row "LIR statement or helper -> the VM's checked operations and counters -> the native checked sequence". The table also lists every direct entry call that is a heap operation (`eml_rt_decref`, `eml_rt_drop_token`, `eml_rt_free_block`, `eml_rt_release`, `eml_rt_is_last`, `eml_rt_con_at`) as "full checked operation, no audit". The units NC-5..NC-7 implement exactly that table.
- D-NC-5 (audits in `eml_rt`, Verus): for each of the seven operations `x`, `Heap::<M>::audit_x(w)` has the `requires` of today's checked operation (`wf()`, `word_in(w)`) and `ensures r is Ok ==> x_passes(w)`; `x_body(w)` has `requires M::CHECKED ==> x_passes(w)`, `!M::CHECKED ==> x_ok(w)` and proves `wf()` kept and no error from the step (a `decref` body that frees can return its children's errors); the operation `x` is `audit_x; x_body` with today's contract unchanged. `x_passes` is the run-time-checkable part (`dup_passes`, `decref_passes`, `set_passes` exist; the field, unique, header and box predicates are new). The fast precondition `x_ok` (which contains `consistent()` and `holds(w)`, facts about the whole program) is not implied by an audit and is not claimed. `word_in(w)` stays a caller premise in both modes.
- D-NC-6: `eml_nrt`'s `entries!` emits every entry generic over `M: Mode` and two tables (fast, checked); the descriptor row gains `class` (15.2 H-4). There is no `checked` cargo feature: a shipped app references no checked or audit symbol and the linker drops them as it drops unused rows; `native-aot`'s strip test adds "no `_checked` and no `eml_rt_audit_` symbol". Fallback if the runtime library's build time is the problem: the feature, declared by `eml_nrt` [I].
- D-NC-7 (`RunStats` natively): the fields `Ctx<Checked>` and `Heap` count come from them and from the audits. The three fields the VM's interpreter counts are tied to the LIR statement, as in the VM, not to an allocation or a load (the VM counts `boxes` at every box instruction, also when the result is an immediate or a constant, and `unboxes` at every unbox, also of an immediate [V: V5 r2]): checked lowering emits one count call `eml_rt_audit_count(ctx, which)` at the head of each `box` statement (`boxes`) and each `unbox` statement (`unboxes`). `unwound_frames`: the same count call inside `FnLower::cleanup`, which every non-landing exit of a function frame during an abort runs and the landing branch does not, as the VM counts one per frame popped by an abort and not the frame where it lands. Two placements are NC-1's stated decision, with these defaults: the cleanup block of `eml$apply$m` counts only when arguments remain (the VM's `ApplyRest` frame); thunk frames, which the VM does not have, do not count. NC-1 confirms each against ten `bench/` rows; a field whose native count cannot equal the VM's by construction is listed with its reason and is deleted from `RunStats` at `vm-retire` (kind 2, in that plan) instead of being approximated. `peak_stack` is bytes natively and is not compared.
- D-NC-8: `eml run --native --debug-heap` and `--native --stats`: owner checks, leak and invariant checks at the end, `TransferLeak`, census.
- D-NC-9 (rule for later stages): every inline memory access added to generated code is preceded by its audit in checked lowering through `lower/audit.rs`, with a twin CLIF pin showing the same instructions after the call.
- D-NC-10 (parity while the VM exists): the UI harness runs every `run` / `run-fail` file native code accepts in checked native mode too and compares with the checked VM: the error class of every failing run, and every field of `RunStats::fields()` but `peak_stack` on every passing run. A difference is a failure, not a re-pin.

Test changes: kind 3 for the `Mode`-generic entries; kind 2: the callee name of the inline decrement's cold call in the fast CLIF pins (`eml_rt_decref` -> `eml_rt_decref_slow`), otherwise byte-identical; new tests.

Units: NC-1 plan, D-NC-4's table, the probes; NC-2 `eml_rt`: `audit_dup` / `audit_decref` and the bodies (gate); NC-3 `eml_rt`: the field, store, unique, header and box audits (gate); NC-4 `Mode`-generic entries, two tables, `class`, `eml_rt_decref_slow`, the checked table's two body entries, the placement test on `Step::slow()`; NC-5 `lower/audit.rs`, `lower/rc.rs`, twin pins; NC-6 `lower/data.rs` (fields, stores, boxes, tokens as calls), twin pins; NC-7 `lower/apply.rs`, `lower/unwind.rs`, the count calls for `boxes` / `unboxes` / `unwound_frames`; NC-8 end-of-run checks, census, `--debug-heap`; NC-9 `--stats`; NC-10 harness (D-NC-10); the negative tests `shared_capture`, `fresh_result`; the machine helper in `eml_test_support` is created here, and `crates/eml_vm/tests/evidence.rs` and `reuse.rs` and the hand-written LIR programs of `crates/eml_lir/tests` run through it on both engines, checked; NC-11 CI lines, docs; NC-12 Close. Spec: native.md 「検査モード」 (new), object-model.md 「検査モード」「核の操作」, testing.md 「ネイティブで流す」.

## 10. `sched-core`

Goal: the new scheduler exists in `eml_sched`, complete and tested without an engine.

- D-SC-1: new crate `eml_steps`: `open.rs`, `plan.rs`, `limit.rs`, `idle.rs`, `verdict.rs`, `sleepers.rs`; all code in `verus!`, vstd only, no `unsafe`, no trust item (section 14).
- D-SC-2: `eml_sched` gains, written new against section 2: `job.rs`, `run.rs` (`Run`, `Slot`, `WorkerCore`, inbox, `Notice`; implements `cells::Post`), `fork.rs`, `drive.rs`, `exec.rs` (`Exec`, `Script`), `policy.rs`, `io.rs` (driver looks, a driver per worker on the real clock, hosting on the virtual clock through `Submit` / `CancelIo` / `Completion`, quiescence and the job budget), `clock.rs` (timers, deadlines, the virtual clock, the verdict's shell), `signals.rs` (signal waiters, virtual signals, END and the exit code, the stop procedure), `limits.rs` (the shell around `eml_steps::limit::hit`), `monitor.rs`, `pool.rs` (jobs of blocking calls), `main_loop.rs` (the main worker's loop, `Park`, the merged seat), `threads.rs` (`spawn_peers`, `separate`, `on_own`, `fold`), `recover.rs` (recovery after a fatal error, for the REPL), `counters.rs` (the `Std.Atomic` counter table). Where the VM has the same logic (`workers.rs` 2,053, `sched.rs` 1,871, `monitor.rs` 562, `jobs.rs` 281, `counters.rs` 164 lines [V: wc]) it is read, not moved.
- D-SC-3 (`cells.rs`): SC-1 lists, before the stage is approved, every `cells.rs` item the new files need and every change to it. The expected list is additions only, with `pub(crate)` on items that are not public today. First half, checked [V: V5]: the VM's scheduler files use `cells.rs` only through `pub` items (`lock`, `route`, `next_deadline`, `in_flight`, `take_submit`, `take_completion`, `cancel_hosted`, `expire`, `deadlines`, `placed`, `pinned`, `unpark`, `drain`, `sweep_all`, the `submit_*` / `take_*` functions) and implement `Post`; `Discards::run`'s `depth_offset` is passed 0 by the new code. If an item must change shape, the VM side gets a thin adapter in `eml_vm`, listed in the plan.
- D-SC-3a: new public items follow architecture.md 「公開の面と`unsafe`」; the unit that adds a safe function or `pub` field naming a `surface.rs` token (the whole identifiers `Ctx`, `Heap`, `Word`, `RawBuf`, the driver's `Op`, `IoDriver`, and `*mut`, `*const`) adds its `SAFE` / `FIELDS` entry. `Script` stays safe public API (a `Ctx` is harmless under D-RL-1); `fork`, `join`, `drive` are `unsafe fn`, `Exec`'s methods are `unsafe`, `Marked` is unforgeable. The `cells.rs` items the new files call are the `unsafe fn` of `engine-trust`'s ET-R1, so D-SC-3's "additions only" is unaffected.
- D-SC-4: across `enter`, `drive` holds no reference and no pointer derived from `this` other than `this`; `Exec::with` makes a borrow unable to leave an item boundary by type. Rule for engines (native.md): a shim calls `eml_fatal` only after every guard it holds is dropped.
- D-SC-5: `Policy::at(Point)` is the one hook mechanism: `Point` enumerates the protocol's interleaving points; `Normal` and `Sequential` inherit the empty default; tests use a hooked policy. `eml_sched`'s feature `testing` gets no new item.
- D-SC-6 (tests, all through `Script`): `claims::a_publish_between_scan_and_sleep_wakes`, `a_take_before_sleep_passes_the_wake_on`, `a_notice_woken_worker_passes_the_wake_on`, `a_searcher_that_gives_up_rescans`, `the_verdict_waits_for_a_listed_job`, `the_verdict_waits_for_a_wake_in_flight`, `the_last_sleeper_wakes_worker_0`; `par::a_join_wait_sees_a_stop` for INTERRUPT, TERMINATE, MEMORY and END on worker 0, a worker k >= 1 and the main worker; `par::a_taken_childs_error_ends_the_run`; `par::a_none_behind_a_newer_join_is_an_internal_error`; `jobs::a_job_is_not_starved_by_two_ready_tasks`, `jobs::a_cell_job_behind_a_join_job_is_found`; `workers::a_busy_worker_takes_no_foreign_job`; `pinned::a_pinned_compute_runs_off_the_main_worker`; `io::a_hosted_operation_completes_on_its_owner`, `io::quiescence_waits_for_the_job_budget`; `clock::a_timer_fires_on_the_virtual_clock`, `clock::the_clock_does_not_advance_while_a_job_runs`; `signals::a_virtual_signal_reaches_its_waiter`, `signals::end_sets_the_exit_code`; `counters::` (the VM table's tests restated); Miri `drive::` in both borrow models with an `enter` that takes `&mut *this` and calls `fork` / `wait_join`; TSan `claims::`, `par::`, `workers::`, `counters::`.
- D-SC-7 (explorer): `crates/eml_steps/tests/explore.rs` enumerates every interleaving of `idle::step`, `verdict::step` and the publish steps for 3 workers (worker 0 among them) and 3 publishes, and for the case "a worker stops for good while it has a listed job", asserting, in both cases, (1) no state with a listed job, every taker asleep and no wake in flight; (2) worker 0 never reaches the verdict's end while a job is listed, a job runs, a `Joined` notice is undelivered, or a worker is in `wait_join`; (3) every state with no enabled step is the verdict's end (no stuck state other than a reported deadlock). `#[ignore = "exhaustive"]`, a CI line in release.
- D-SC-8: end-of-run checks in every build: every `open` empty, no `Joined` in an inbox.
- D-SC-9: object-model.md gains the isolation invariant; docs/spec/runtime.md is created with 「workerのループ」「仕事の一覧」「眠りと起こし」 (the four rules, `running`, the verdict's four steps, the lock order), 「`par` の fork と join」「操作の予算」「信頼の一覧」; vm.md keeps describing the VM until `vm-retire`.
- D-SC-10: a `sched-core` commit changes no file under `crates/eml_vm` other than the adapter SC-1 lists, and no UI output or snapshot.

Test changes: none to existing tests.

Units: SC-1 plan, the `cells.rs` list; SC-2 `eml_steps`: crate, CI lines (Verus, verusfmt), `open.rs`, `plan.rs`; SC-3 `eml_steps`: `limit.rs`, `sleepers.rs`, `idle.rs`, `verdict.rs`, the explorer; SC-4 `run.rs`, `job.rs`, `Marked`, the four rules, `running` and waking; SC-5 `exec.rs`, `drive.rs`, `Script`, item open / close, `Slot.current`; SC-6 `fork.rs`, `wait_join`, taken jobs, D-SC-8; SC-7 `io.rs`; SC-8 `clock.rs` and the verdict's shell; SC-9 `signals.rs`; SC-10 `limits.rs`; SC-11 `monitor.rs`, `pool.rs`; SC-12 `main_loop.rs`, `threads.rs`, `recover.rs`; SC-13 `policy.rs` (`Seeded`, budget points), `counters.rs` with its Miri and TSan lines, the claim and join-wait counts in `WorkerCore` (the `Finished` fields at JT-5, 00 §D D-6); SC-14 Miri and TSan lines for the rest, runtime.md, object-model.md; SC-15 Close.

## 11. `jit-tasks`

### 11.1 Stacks and the worker
- Workers 0..N-1: threads of `Options.stack` (default 1 GiB); `hard = base + 1 MiB`, `real = hard + 2 MiB`.
- Main worker: the calling thread's own stack, measured once before any thread exists: `top = arch::stack_pointer()` in `run`'s frame; `stack.rs` `thread_stack_base()`; on Linux `base = max(queried, top - 8 MiB)` (the reported size is the rlimit), on macOS and Windows `base = queried`; `hard = base + 2 MiB`, `real = hard + 2 MiB`. With at most 4 MiB below `top`, or a failed query, the first start of a pinned task ends the run with `the calling thread's stack is too small for main-thread tasks` (not catchable). `execute_native` runs on a 16 MiB scoped thread. Each worker writes its bounds into `Slot.stack`.
```rust
pub struct Worker<M: Mode> { image: *const Image, by_code: Vec<usize>, by_pap: Vec<usize>,
    pub(crate) core: WorkerCore<NativePolicy>, pub(crate) cur: Current, pub(crate) limits: Limits,
    pub(crate) children: Vec<Child>, pub(crate) loaded: Arc<Loaded>,
    pub(crate) exit: Option<i64>, pub(crate) panic: Option<Word>, pub(crate) error: Option<RuntimeError> }
pub(crate) struct Child { cur: Current, class: Limit, evv: Word, id: u32, serial: Option<u64> }
```
`Current` is the cold state of the running item (its cell, its kind, its site). `split(ctx)` is the one derivation of `&mut Worker` from a `Ctx`'s `runtime` pointer. The `Worker` is boxed, never moved, and lives under a guard whose `Drop` does `ctx.set_runtime(null)`.

### 11.2 The loop
The loop is `eml_sched::drive::<NativeExec<M>>` on the thread's own stack; `NativeExec::with` is `split` inside the closure and `NativeExec::enter` is the one caller of `eml_enter`. `eml_nrt` has no loop. A synchronous `main` is the first item of `drive(Until::Root)`. Job open: push `Child { serial: None, .. }`, `switch_task((), job.id)`, `limits::set(Stops)`, `argv = [g, ()]`. Job close: assert no yield; switch to the worker's own ID; pop; deliver by `Done`. Any fatal returns `Err`.

### 11.3 Task rows, fatal errors, panics
- The task rows are written in `eml_nrt/src/rows/task.rs` (their final place) over `cells.rs` and
  ```rust
  pub struct TaskCx<'a, E: Exec> { pub core: &'a mut WorkerCore<E::P>, pub ctx: &'a mut Ctx<E::M>, pub cur: &'a mut Current, pub site: u64 }
  pub enum Aside { Fault(RunFault), Raise(cells::Raise) }     // 行が値を返す代わりにエンジンに求めること
  ```
  A body is `-> Result<ret, Aside>`. The VM's arms in `vm.rs` are read, not moved.
- Each row's entry is a `shim` (prefix `eml_x_`): `Ok(v)` returns the value; `Err(Fault(RunFault::Extern(f)))` on a row that faults sets the fault in `ctx` and returns a placeholder; `Err(Raise(Io { .. }))` sets `Fault::Raised`; `Err(Raise(Panic(m)))` parks the panic; any other `Fault(e)` is recorded and is the fatal path. An entry names exactly its own row function; nothing on the run path reads a descriptor table; no `#[used]`.
- Fatal errors: `eml_fatal` is called only from an entry's epilogue, after every guard the shim holds is dropped (`eml_nrt_join` calls it after `wait_join` returned). It cuts eml frames and thunks down to `NativeExec::enter`'s `eml_enter`; from there the fatal is a value: `drive` returns `Err(fatal)`, the worker's base turns it into a `RuntimeError` and retires the core; on the main worker it goes through `Pending::run` to the `Park`.
- Rust panics: a panic in an entry is caught by its `shim` (`FATAL_PANICKED`); a panic in a worker's base by `on_own`; on the main worker `run_ready` wraps `drive` in `catch_unwind` and returns "a worker thread panicked" as a value. No panic unwinds through generated code or `Park` / toolkit frames.
- `exit` under a task entry or a job: the item close sees the abort with `worker.exit == Some(n)`, does `yield_finish`, `evv_set(UNIT)`, `cells::end_with_exit`, `end_run()` when not worker 0, and returns the fatal `Stopped(Ended)`.

### 11.4 `fork_join` natively
```
call eml_nrt_fork(ctx, g)                 ; shim: 2.5 fork; push Child { serial: Some(s), .. }; limits::set(Stops)
l  = call eml$apply$1(ctx, f, Unit)
g2 = call eml_nrt_left(ctx)               ; shim: assert no yield; take_own(s): Some -> switch to the child's ID, return g; None -> the nesting assert of 2.5, return Unit
if g2 is a box: r = call eml$apply$1(ctx, g2, Unit) else r = Unit
pair = call eml_nrt_join(ctx, l, r, tag)  ; shim: assert no yield; taken: r = wait_join(s), a stop is the fatal `Stopped`; restore the parent; fork::join; build the pair
```
A native function value is always an object, so the test is one compare [V: V3]. No cleanup path and no unwind point: nothing leaves the sequence but a value or the fatal path. Nothing interleaves between a fork and its join on a worker, so `children` is strict.

### 11.5 Limits
- `eml_nrt/src/limits.rs`: `STACK_HARD_MARGIN = 1 << 20`, `LIMIT_HEADROOM_BYTES = 2 << 20`; `pub(crate) fn set(w, ctx, class)` is the one writer of `stack_limit`: `Stops` and `Task` -> `real`, `Aborting` -> `hard`. Callers: `drive`'s item open, the three `par` services, `eml_nrt_safepoint`, `eml_nrt_failed` (`set(class.aborting())`), `eml_nrt_landed` (`set(class.landed())`).
- `eml_nrt_safepoint(ctx, function, defers) -> action`:
  1. `poll(0)`: a stop other than `NOTICE` -> fatal `Stopped(stop)`. `NOTICE`: if a yield is in flight leave the bit; else `take_notice(core.index)`, poll again, collect with one OS look.
  2. `SP >= stack_limit` -> `ACTION_CONTINUE`.
  3. `eml_steps::limit::hit(Probe { class, yielding, site: if defers { Defers } else { Entry }, beyond_grace: SP < hard, over_real: SP < real })`: `Stop` -> fatal `DepthLimit`; `Stay` -> `ACTION_CONTINUE`; `Raise { message }` -> `set(Aborting)`, then if `ev_find(io_effect)` finds a handler: park `Fault::Raised { kind: IO_RESOURCE_LIMIT, message }` and return `ACTION_RAISE`; if none: `set(Task)`, fatal `DepthLimit`.
- `lower/safepoint.rs`: on `ACTION_RAISE`, `call eml$raise(ctx, me, ENTRY_SITE, PURITY_EFFECTFUL)` and then the function's `leave` cleanup. `ACTION_UNWIND` is deleted. `hit` returns `Stay` while yielding and `Stop` while `Aborting`, so no unwinding action starts in either.
- A clause entry reached with class `Task` below `real` takes the raise path: the `ResourceLimit` replaces the failure being raised (the clause's cold block calls `eml$raise` again; its `leave` releases its entry-owned evidence and message).
- The `n < m` arm of `eml$apply$m` passes `defers = 1`; the service then returns only `ACTION_CONTINUE` or does not return, so the arm needs no cleanup path.
- Class at creation: `main`, jobs and children `Stops`; task entries `Task`.

### 11.6 Blocking C, run orchestration, CLI
- Blocking C: `Image.c: Slice<CRow>`, `CRow { function: *const CFunction, call: CCall, dtor: Option<Dtor> }`, built by `eml_native` at `load`; datum kind `DATUM_C` for `c:<Module.f>` holds the row's address (hence the edge `eml_nrt -> eml_cabi`). Two `shim` entries in `eml_nrt/src/c.rs`: `eml_x_c_submit(ctx, cfn, argv) -> int` (the token) and `eml_x_c_take(ctx, cfn, token, wake, argv, out) -> unit`; their bodies `submit` / `take` are in `eml_nrt/src/rows/blocking.rs`; the wait between them is the `AsyncParkIo` row. `JobSource::C(index)` is derived from the row address at run time. A blocking call runs on a pool thread's own stack.
- Run (`eml_nrt::run::run`): (1) on the calling thread, before any thread exists: `control.begin_run()`, `Run::new`, the registry, the load-time constants and every data slot, the main worker's `Ctx`, the `Park` and its waker, the stack measurement of 11.1; (2) `threads::separate`: worker 0 behind a gate, the calling thread's two signals masked, the main worker's loop on the calling thread; (3) each worker's base makes its `Ctx` and `Worker`, runs `drive` (`Until::Root` on worker 0, `Until::Stop` on peers, `Until::Idle` under `main_loop` on the main worker), retires, asserts the `Ctx` detached; (4) on the calling thread after both joins: settle the result with the run's fatal slot, fold the heaps, end checks, census, build `Finished`; one local function holds the only `clear_end` call of `eml_nrt` and runs on both paths. `Options.masked` is deleted.
- CLI: `--workers` / `EML_WORKERS`, `--gui` / `EML_GUI`, `--debug-heap`, `--stats` are accepted with `--native`. `Session::compile_native` loses the `unsupported` call; `eml_native::{Unsupported, Form, unsupported}` are deleted.
- `GateHost` gets its second implementation (on the new `WorkerCore`); the trait is deleted at `vm-retire`.

### 11.7 「VMとの違い」 until `vm-retire`
1. No word-count depth natively: the limit is the thread's stack. A UI file whose outcome depends on depth carries `-- native: skip` naming this item. `ui::limit_depth_native` runs the files that carry `-- max-depth:` natively on an 8 MiB stack and compares stdout, the error's class and the exit code with the VM's fast run.
2. Scheduling: the VM places a right child by inbox, its parent waits as data, and `Seeded` preempts inside an entry; native code lists jobs, blocks at a taken join and switches only between entries. UI outputs do not depend on the schedule by construction (every `par/` and `async/` file has one expected output at N = 1, 2, 8 and two seeds [V: V5]); the files outside the comparison are the ones that carry `-- workers: skip` (2 today).
Outputs are compared on every other file, and the schedule-free `RunStats` fields (the set testing.md names) at every worker count and seed. The section is deleted by `vm-retire`.

### 11.8 `rt-concurrent-proofs`
Stays deferred (14.4). In this stage: the placement test of the three heap-store sites; CLIF pins that the inline `dup` / `decref` store is dominated by the local-range compare, in fast and checked lowering; a repeated native run of the `par/` set at N = 8, checked, with the census, as an `#[ignore]` test CI runs.

### 11.9 Units
| # | Goal | Tests | spec |
|---|---|---|---|
| JT-1 | Plan; roadmap; `heap_stores_have_three_sites`; CLIF pins; the per-file test list of 11.10 confirmed | new | roadmap |
| JT-2 | `Worker<M>`, `split`, `NativeExec` (`with`, `enter`), `drive` instantiated with `Normal`, `Sequential`, `Seeded` | Miri `drive::` with a `Script` that calls the shipped entries; placement tests; `the_frame_chain_reaches_eml_enter` | native.md 「エントリ」 |
| JT-3 | `limits.rs`, `stack.rs`, `Slot.stack`, safepoint, `landed`, `failed` | 2: `the_stack_limit_has_one_writer`; new | native.md 「安全点」「タスクの上限」 |
| JT-4 | Lowering: `ACTION_RAISE`, `defers`, the `n < m` arm; `ACTION_UNWIND` deleted | 2: CLIF pins; new `apply::the_fewer_arm_never_raises`, `limits::a_raise_at_a_clause_entry_nests` | native.md 「安全点」 |
| JT-5 | `Image.tasks`, `Loaded`, `run.rs` at one worker, the main worker and `Park`, the root entry; the machine helper of NC-10 extended to run configurations; `run.rs`, `api.rs`, `spawns.rs` of 11.10 on both engines | 3; 1: `masked` hook tests; 2: `spawns.rs` site list; new `pinned::` | native.md 「Image」「実行の始めと終わり」「メインのworker」 |
| JT-6 | Task rows (`rows/task.rs`), their entries; `Done::Cell` items | new `tasks::`, `tasks::fatal::`; Miri row scripts | native.md 「タスク」 |
| JT-7 | Lowering of task rows; harness honours clock / signal / gui marks; `tasks.rs`, `chan.rs`, `roots.rs`, `memory.rs` on both engines | 2: snapshots; 1: two `unsupported::` tests | native.md 「VMとの違い」 |
| JT-8 | `eml_nrt_fork` / `left` / `join`; `Done::Join` items | new `par::` (inline, taken, stop during the wait, record checks) | native.md 「`par`」 |
| JT-9 | Lowering of the `par` instruction; `par.rs` on both engines | 2: snapshots, CLIF pin; 1: `unsupported::par_is_rejected` | native.md 「`par`」 |
| JT-10 | Blocking C (`rows/blocking.rs`, `c.rs`); `Unsupported` deleted; `ffi.rs`, `ffi_abandon.rs` on both engines | 1: `unsupported.rs`; 2 | native.md 「Cの関数の呼び出し」 |
| JT-11 | Workers 1..N-1; native taking; `workers.rs`, `seat.rs`, `signal.rs`, `io.rs`, `pinned.rs` on both engines | new `workers::`, `workers::fatal::`, `par::a_join_wait_with_a_compute_and_a_task` | native.md 「止める手順」 |
| JT-12 | CLI: lift the usage errors | 1: listed | - |
| JT-13 | Harness: native `par/` / `async/` at 2 and 8 workers and under the two seeds, checked and fast, with census; parity (11.7); `ui::limit_depth_native`; the N = 8 stress test | new; 2 | testing.md |
| JT-14 | CI lines (TSan, ASan on native task tests); bench rows: native `pfib 30` cutoff 2 at N = 1 (lock share; over 15 % of a fork schedules the CAS claim as a later item), `pfib` at N = 1, 2, 4, 8, balanced recursion at N = 4 (help-on-join's condition is read here), bytes per spine level | - | testing.md, benchmarks.md |
| JT-15 | docs/future pruned; promise re-check (D-ET-2) against docs/spec/lir.md 「エンジンの約束」 | - | - |
| JT-16 | Fable whole-branch review; Close | - | benchmarks.md |

A form is accepted natively only when everything its files need at one worker is in place. A test of a switched file that asserts the VM's placement, preemption or `Seat` internals is listed at that unit and stays VM-only until deleted.

### 11.10 The fate of every file of `crates/eml_vm/tests` (20,722 lines [V: wc])

| File | Lines | Fate |
|---|---|---|
| `evidence.rs`, `reuse.rs` | 533, 241 | both engines at NC-10; moved to `crates/eml_native/tests` at VR-2 |
| `run.rs`, `api.rs`, `spawns.rs` | 422, 1,104, 134 | both engines at JT-5; moved at VR-2 (`spawns.rs`'s crate list becomes `eml_sched`, `eml_nrt`, `eml_test_support`) |
| `tasks.rs`, `chan.rs`, `roots.rs`, `memory.rs` | 724, 600, 260, 292 | both engines at JT-7 (they run programs, so they need the lowering); moved at VR-2 |
| `par.rs` | 973 | both engines at JT-9; placement asserts VM-only, deleted at VR-2 |
| `ffi.rs`, `ffi_abandon.rs` | 1,171, 431 | both engines at JT-10; moved at VR-2 |
| `workers.rs`, `seat.rs`, `signal.rs`, `io.rs`, `pinned.rs` | 949, 331, 438, 2,015, 1,460 | both engines at JT-11; `workers.rs`'s tests of `placed` / `preemptions` / `Seeded` preemption VM-only, deleted at VR-2 |
| `machine.rs` | 1,229 | both engines in `jit-repl` (it tests appending and session roots); moved at VR-2 |
| `bench.rs` | 1,914 | stays on the VM; at VR-2 moved and re-pinned on checked native runs (kind 2) |
| `scaling.rs` | 1,739 | `#[ignore]` scaling tests; at VR-2 each test is re-pointed to native code or deleted, listed (kind 1) |
| `miri.rs`, `miri/` | 722 | its 15 programs run natively checked (D-VR-1 (i)); what it checks under Miri is replaced by the `Script` row scripts of JT-6; deleted at VR-2, each test listed with its replacement (kind 1) |
| `codegen.rs`, `calls.rs`, `structs.rs`, `tags.rs`, `variants.rs`, `data.rs`, `apply.rs` | 788, 525, 576, 25, 163, 653, 277 | bytecode-specific; at VR-2 deleted where a CLIF pin or UI file states the same fact, else restated on native code, each listed (kind 1) |
| `main.rs`, `common/`, `ir/` | 33 | `main.rs` deleted; helpers and fixtures move with their users |

Which tests of `api.rs`, `run.rs`, `machine.rs` name VM-only surface is [I]; JT-1 and the `jit-repl` plan list them. Unit tests in `crates/eml_vm/src` end with the crate; VR-1 lists any whose fact no other test states.

## 12. `jit-repl`, `vm-retire`, `sched-proofs`, `native-aot`, `native-inline`

### 12.1 `jit-repl`
- D-JR-1: the REPL's machine lives on a session thread; code is appended by name under "same name, same code"; session roots and the debug-build comparison of appended code are kept.
- D-JR-2: recovery after a fatal error is `eml_sched::recover`; the reset also empties `children` and the worker's `open`. `c_links` and `renumbered` are written in `eml_nrt/src/rows/blocking.rs`.
- D-JR-3: an `Async` input runs as the root task of its run; one worker, merged seat.
- D-JR-4: `eml repl --debug-heap` runs checked native code.
- D-JR-5: at Close `eml run` and `eml repl` use native code; `--vm` selects the VM until `vm-retire`; `--native` is accepted and ignored until then.
- `machine.rs` runs on both engines; the transcripts pass on both before the default flips.

Units (8): plan; session thread and `NativeMachine::extend`; session roots; recovery; `run_shown` and the root entry for inputs; checked REPL; transcripts and `machine.rs` on both engines; Close with the default switch.

### 12.2 `vm-retire`
Goal: delete `eml_vm`; merge `eml_rows` into `eml_nrt`; one engine in every document.
- D-VR-1: precondition, run at VR-1 and recorded in the plan; if one item fails the stage does not start: (a) every UI file, seed and worker count passes checked native and fast native, the files of 11.7 listed with their native expectations; (b) every checked-mode negative test fires natively with the VM's error class; (c) the transcripts pass natively; (d) the Miri suite through `Script` names every row section and every `drive` path; (e) ASan and TSan lines are green on native tests; (f) the four CI targets pass; (g) the fast-mode census passes on every `run` file at 1, 2 and 8 workers; (h) every CLIF pin of an inline sequence has its checked twin; (i) the hand-written LIR programs of `crates/eml_lir/tests` and `crates/eml_vm/tests/miri.rs` run natively checked; (j) every test switched in `jit-tasks` and `jit-repl` passes on `NativeMachine`.
- D-VR-2: tests: 11.10's table. `bench.rs`'s snapshots are re-pinned on checked native runs (kind 2: the fields of D-NC-7 expected unchanged; `peak_stack` in bytes; any field D-NC-7 listed for deletion goes).
- D-VR-3: `ui::run` runs each file checked native then fast native (a native run of a small file takes 10-20 ms in the dev build [V: V5]); `ui::*_native`, parity, `native: skip`, `Engine`, `--native`, `--vm` go. Seed runs: `async/` only (two `par` children never interleave at one worker under any policy, so `par/` seed runs equal the `Sequential` run).
- D-VR-4 (depth): `max_depth` (words) is replaced by `Options.stack` (bytes); every `-- max-depth:` mark becomes `-- stack:` (8 files carry one mark each at 4a2954c1 [V: `grep -rln 'max-depth' tests crates/eml_cli/tests/repl`]; an earlier count of 29 was wrong). A test may depend on the class of a depth error, never on the depth reached.
- D-VR-5: `Finished` loses `preemptions`, `placed`, `placed_jobs`; `rows::call`, `SHARED`, `GateHost`, `eml_sched`'s feature `testing` and `cells.rs`'s `#[cfg(test)]` helpers behind it are deleted; `extern_rows!` and `entries!` become one list `rows!` (a line gives the row's body, its ABI form and its `class`); `eml_nrt/src/constants.rs` and the moved `rows/values.rs` are merged.
- D-VR-5a: VR-5 moves the `eml_rows` paths of `surface.rs`'s `SAFE` by 13.3's file map; VR-6 makes every `pub` item of `cells.rs` that no crate but `eml_sched` names `pub(crate)` (architecture.md 「公開の面と`unsafe`」 step 3), checked by `grep -rn '<name>' crates/eml_nrt/src` being non-empty for each remaining `pub` item; `test_ctx` and `stream_on` go with the `testing` feature.
- D-VR-6: docs: vm.md's scheduler, limits, I/O and run sections are merged into docs/spec/runtime.md; its instruction and frame sections are deleted; native.md loses 「VMとの違い」; multicore.md, architecture.md, testing.md, CLAUDE.md, crates/eml_rt/CLAUDE.md, benchmarks.md (`bench/run.sh` on native code) follow.
- Stated losses: a second implementation; whole-run Miri (15 small runs); a deterministic word-count depth; an engine on a host that forbids executable memory (`native-aot` is the answer); targets without a Cranelift backend.

Units: VR-1 plan and precondition run; VR-2 test moves and deletions; VR-3 harness, CLI, depth marks; VR-4 delete `eml_vm`, its CI lines (the `counters::` Miri and TSan lines now name only `eml_sched`); VR-5 move `eml_rows` into `eml_nrt` by 13.3's map, `nix/crate-graph.sh` tables; VR-6 `rows!`, the deletions of D-VR-5; VR-7 docs/spec (runtime.md, vm.md deleted); VR-8 bench tables; VR-9 Close.

### 12.3 `sched-proofs`
Section 14, P-4b. Units: 1 plan and the invariant on paper; 2 the model over `idle::step`, `verdict::step` and the publish steps; 3 the inductive proof; 4 Close (roadmap item 「協調的な配置のVerusの証明」's wake-up half closed).

### 12.4 `native-aot`, `native-inline`
The roadmap items, with: the runtime library is built from `eml_nrt` (features `tls`, `gui`); the strip test of D-NC-6; the symbol rule of 15.2 H-5; `native-inline` follows D-NC-9.

## 13. Crate structure

### 13.1 The end state

```
eml_cli ─────────────┬───────────────────────────────┐
  front end          │                               │
  (syntax, hir,      eml_native ── eml_clink ──┐     │
   types, core_ir)   │    │                    │     │
        │            │  eml_lir                │     │
 ─ ─ ─ ─┼─ ─ ─ ─ ─ ─ ┼ ─ ─ tools above, runtime closure below ─ ─
        │            eml_nrt ──(tls)── eml_tls │     │
        │            │  │ │ └─(gui)── eml_gui  │     │
        │            │  │ └────────── eml_cabi ┘     │
        │            │  eml_sched ── eml_io ─────────┘
        │            │  │   └── eml_steps
        └──────────  eml_rt          eml_extern  (used by both strata)
```

| Crate | Stratum | Holds | Edges (workspace) |
|---|---|---|---|
| `eml_extern` | runtime | the built-in table, user C function descriptions; no `unsafe` | - |
| `eml_rt` | runtime | object model, `Ctx`, `Control`, the driver and Host traits; Verus | - |
| `eml_steps` | runtime | the scheduler's decision kernels; Verus; no `unsafe`, no trust item | - |
| `eml_io` | runtime | drivers, real hosts | `eml_rt`; `eml_tls` under `tls` |
| `eml_tls`, `eml_gui` | runtime, behind features | as today | - |
| `eml_cabi` | runtime | C code pointers (`Bridge`, `Dtor`, `CCall`, `Linked`) | `eml_extern` |
| `eml_sched` | runtime | cells, `Run` / `Slot` / `WorkerCore`, `drive`, `fork`, I/O, clock, signals, limits, monitor, pool, main loop, counters, run interface types, `Exec`, `Script` | `eml_io`, `eml_rt`, `eml_steps` |
| `eml_nrt` | runtime | `rows/` (one list `rows!`: bodies, entries fast and checked, descriptors; `task`, `blocking`, `handle`, `tls`, `gui`, `values`), the stubs, `Image`, `Worker`, `NativeExec`, the run driver, census | `eml_cabi`, `eml_extern`, `eml_rt`, `eml_sched`; `eml_tls`, `eml_gui` under features |
| `eml_clink` | tool | C linker, bridges, libloading | `eml_cabi`, `eml_extern` |
| `eml_lir`, `eml_core_ir`, front end, `eml_diagnostics` | tool | as today | as today |
| `eml_native` | tool | code generator, JIT host | `eml_cabi`, `eml_clink`, `eml_extern`, `eml_lir`, `eml_nrt`, `eml_rt` |
| `eml_cli` | artifact | as today, one engine | gains `eml_nrt` (it forwards `tls` / `gui` / `gui-native` to it); loses `eml_vm`, `eml_rows` |
| `eml_test_support` | dev | `Session` wrapper, `RecordingHost`, fixtures, the machine helper | - |

Decisions and reasons:
- **`eml_vm` deleted.** JIT-only.
- **`eml_rows` merged into `eml_nrt`.** Its reason to exist was two consumers. With one, a row's body and its entry are one line of one list, and `rows::call`, `SHARED` and the test tying two lists together go.
- **`eml_cabi` stays.** Merging it would remove one 73-line leaf and add a tool edge (`eml_clink -> eml_nrt`) onto the whole runtime closure and its features for `Linked`, which only tools consume.
- **`eml_sched` stays a crate.** Three properties are cargo-enforced by it and would be grep rules in a merged crate: (1) it cannot name `eml_fatal`, `eml_enter` or any entry, so no scheduler frame is under the fatal longjmp; (2) it has no `Extern`, no `extern "C"` and no asm, so Miri and both sanitizers run all of it (`grep 'extern "C"\|eml_fatal' crates/eml_sched/src` is empty today [V: V5]); (3) the `tls` / `gui` features never reach it.
- **`eml_steps` is new and separate from `eml_rt`.** A crate fully inside `verus!` follows `eml_rt`'s precedent; `eml_rt`'s gate (Miri budget, `bench/rt-gate.sh`) is about the heap and should not run for scheduler kernels.
- **`eml_clink`, `eml_io` stay.** `eml_clink`'s reason (own Cranelift flags and libloading outside the code generator's placement rules) does not depend on the VM; `eml_io` has per-OS backends with their own sanitizer lines and a tool-only feature.
- Rejected: one runtime crate; `eml_sched` + `eml_rows` merged (rows need `eml_extern`, `tls`, `gui`, which `eml_sched` must not reach).
- Dead-stripping does not depend on crate boundaries: it needs no `#[used]`, no descriptor table on the run path and no dispatch over `Extern`, which stay rules with their grep [V: V5].

### 13.2 Features (end state)

| Crate | Features |
|---|---|
| `eml_nrt` | `tls`, `gui`, `gui-native` (it has the cfgs) |
| `eml_io` | `tls` (tool-only terminator) |
| `eml_gui` | `native` |
| `eml_cli` | `types` < `core` < `run`; `tls`, `gui` (default), `gui-native`; the only crate that forwards |
| `eml_test_support` | stage features |
| `eml_sched`, `eml_steps`, `eml_native`, `eml_clink`, `eml_cabi` | none: hooks are `Policy::at`; `Script` is ordinary public API; `load_unverified` goes with `eml_vm` (a test that needs IR the verifier rejects is deleted or restated as a verifier test, listed by VR-2) |

### 13.3 Moves per stage

| Stage | Crate change | `nix/crate-graph.sh` |
|---|---|---|
| `sched-core` | new `eml_steps`; edge `eml_sched -> eml_steps`; new files in `eml_sched` | EDGES; `expect_tree`: `eml_steps` reaches no workspace crate; the `eml_sched` check keeps its list |
| `jit-tasks` | `eml_nrt/src/rows/task.rs`, `rows/blocking.rs` born in place (`eml_nrt/src/rows.rs` is the module root); edge `eml_nrt -> eml_cabi` | EDGES |
| `vm-retire` | `eml_vm` deleted; the map below; features move to `eml_nrt`; `eml_sched`'s and `eml_vm`'s `testing` deleted; edge `eml_cli -> eml_nrt` | EDGES, FEATURES, REEXPORTS rewritten; closure checks use `eml_nrt/tls`, `eml_nrt/gui-native`; the three `eml_vm` checks and the `load_unverified` check deleted; new: `grep -rn 'eml_fatal\|extern "C"' crates/eml_sched/src` empty |

The `vm-retire` file map (no target exists before the move):

| From `crates/eml_rows/src/` | To `crates/eml_nrt/src/` |
|---|---|
| `rows.rs` | `rows/list.rs` |
| `constants.rs` | `rows/values.rs` (merged with `eml_nrt/src/constants.rs` in VR-6) |
| `ffi.rs` | `rows/handle.rs` |
| `tls.rs` | `rows/tls.rs` |
| `gui.rs` | `rows/gui.rs` |
| `lib.rs` | deleted; its `mod` lines go to `rows.rs` |

Never moved: the VM's `workers.rs`, `sched.rs`, `monitor.rs`, `jobs.rs`, `counters.rs`, `ffi.rs`, `hosts.rs` and the task arms of `vm.rs` (deleted with the crate); `eml_vm`'s capture fixtures go to `eml_test_support` only if a moved test needs them.

### 13.4 Rules restated for CLAUDE.md and architecture.md (at `vm-retire`)

- Three strata: the runtime closure (`eml_rt`, `eml_steps`, `eml_extern`, `eml_io`, `eml_cabi`, `eml_sched`, `eml_nrt`, plus `eml_tls` and `eml_gui` behind features); the tools (the front end, `eml_lir`, `eml_core_ir`, `eml_native`, `eml_clink`); the artifact (`eml_cli`). No edge from the runtime closure to a tool.
- `eml_sched` names no `Extern`, entry, stub or `extern "C"`; `eml_fatal(` appears only in `eml_nrt`'s entry macro arms and `enter.rs`.
- Re-exports: a crate re-exports from another workspace crate only types of its own public signatures; `eml_nrt`'s `__rt` stays the one exception.
- Features: 13.2; only `eml_cli` forwards.
- `#[cfg(test)]` code in the `src/` of `eml_sched`, `eml_nrt`, `eml_io`, `eml_steps` names no tool crate and not `eml_test_support`.
- Dead-strip: nothing on `eml_nrt`'s run path reads a descriptor table; no `#[used]`; no dispatch over `Extern` (`grep -rn '#\[used\]' crates/eml_nrt/src` empty; `rows::call` does not exist).
- Public surface: architecture.md 「公開の面と`unsafe`」: a safe function of any crate but `eml_test_support` reaches no undefined behaviour; a function is safe, carried by a type with establishing constructors, `pub(crate)`, or `unsafe fn` with `# Safety`, in that order; `crates/eml_cli/tests/surface.rs` pins the safe `pub fn` and `pub` fields that name runtime memory (trait methods are read by hand at the D-ET-2 re-checks).
- `eml_steps`: all code inside `verus!`; `cargo verus verify -p eml_steps`, verusfmt, tests, clippy; `src/trust.rs` pins "no `external_body`, no `assume_specification`, no `unsafe`". No bench gate of its own.
- The crate table, the Commands block (no `--vm`, no `eml_vm` lines, the `eml_steps` Verus line, the explorer line) and the Testing bullets (UI files run checked native then fast native; no `native: skip`) are rewritten in the commit that deletes the crate.

## 14. Proofs

### 14.1 What is proved, where, when

| # | Property | Form | Crate | Stage | Cost [I] |
|---|---|---|---|---|---|
| P-1 | `Open`: serials strictly increase in each deque; `take_own(s)` returns the job pushed with `s` or `None` and removes only it; `take_own(s)` is `None` implies `s` is not in `joins` or a join with a larger serial is; `take_detached` returns only `push_cell` jobs, oldest first; `take_oldest` returns the smaller front; every operation changes the multiset of listed jobs by exactly its result | Verus over `VecDeque` (the pinned vstd specifies `index`, `len`, `push_back`, `pop_back`, `pop_front`; a probe crate verified `push_join`, `take_own` and the completeness clause [V: V5]) | `eml_steps` | `sched-core` SC-2 | ~250 lines |
| P-2 | `plan::next(Facts) -> Step`: `Foreign` only with an empty ready queue and never on the separate main worker; `Detached` is offered after every ready pass; a pass is bounded by the count at its start; `Park` only with nothing else; total | Verus, finite case analysis | `eml_steps` | SC-2 | ~150 lines |
| P-3 | `limit::hit(Probe) -> Hit`: total; class `Stops` over `real` stops; no unwinding action while yielding or `Aborting`; a `Defers` site never gets a raise | Verus, finite case analysis; the test keeps three example rows chosen from the VM's table, so a drift between the two copies shows | `eml_steps` | SC-3 | ~150 lines |
| P-4a | The wake-up protocol and the verdict, bounded: the three assertions of D-SC-7 | exhaustive explorer in Rust over the shipped `idle::step`, `verdict::step` and publish steps | `eml_steps/tests` | SC-3 | ~400 lines |
| P-4b | The same for every N and any number of publishes | Verus: a spec state machine whose `next` is the specs of `idle::step`, `verdict::step`, `Sleepers` and `Open`; an inductive invariant; `init ==> inv`, `inv && next ==> inv'`, `inv ==> safe`. The Close states which of the three assertions the invariant covers | `eml_steps/src/proto.rs` | `sched-proofs` | 4 units; if the invariant is not found within the stage, P-4a stays the standing check and the item returns to the roadmap |
| P-5 | For each of the seven audited operations: `audit_x` returns `Ok` implies `x_passes`; `x_body` under `x_passes` (checked) or `x_ok` (fast) keeps `wf()` and returns no error from the step; `x = audit_x; x_body` keeps today's contract | Verus `ensures` (D-NC-5) | `eml_rt` | `native-checked` NC-2, NC-3 | 2 units with the `eml_rt` gate |
| P-6 | `Sleepers` (the listed set): no duplicates; `enlist` / `delist` / `pop` are set operations | Verus | `eml_steps` | SC-3 | ~80 lines |

By types, not proofs: a job is consumed at most once (`Job` is not `Clone`; `Open` returns it by value), exactly once at a successful end (D-SC-8); every word that crosses at a fork is marked (`Marked`); the ready queue is owner-only.

How the proofs are tied to the code (the `dup_step` pattern of object-model.md 「遷移の関数」): the shell calls the proved functions and does what they return. `drive` executes `plan::next`'s `Step`; the idle path executes `idle::step`'s `Effect` (`IncSleepers`, `DecSleepers`, `IncSearching`, `DecSearching`, `Enlist`, `Delist`, `Scan`, `PassOn`, `LeaveRunning`, `Park`, `Work`; a wake is "lock the target's slot, set `woken`, count it in `running`, ring"; the main worker's park performs `LeaveRunning` too); worker 0's verdict executes `verdict::step`, restarting at its first step after any wake; the safepoint's cold path executes `limit::hit`'s `Hit`.

### 14.2 Proofs that leave or move

| Proof | From | To | Stage |
|---|---|---|---|
| fork evidence (`alloc_fork_table`, `alloc_fork_tail`, `fork_ev`, `fork_of`) | `eml_rt` | deleted | `par-total` PT-7 |
| the soft threshold's glue (`claim_pressure`, `next`) | `eml_rt` `control.rs` | deleted; the trust list's sentence shortened | `task-limits` TL-3 |
| the limit decision | `eml_vm/src/vm.rs` `limit_hit`, a table test | `eml_steps::limit::hit`, proved; the VM keeps its copy until deleted | `sched-core` |
| the checks of checked `dup`, `decref`, field, store, uniqueness, header, box | inside the checked operations of `eml_rt` | factored into `audit_x` with P-5 | `native-checked` |
| the wake-up argument | a Python model outside the tree | P-4a, then P-4b | `sched-core`, `sched-proofs` |

### 14.3 Trust after `vm-retire`

| Layer | Checked by | Trusted |
|---|---|---|
| `eml_rt` | Verus; Miri `trusted::`; TSan | T0..T18 unchanged, with T18's `jmp` written by the stub only and the soft-threshold glue gone |
| `eml_steps` | Verus | T9 only (the tools) |
| `eml_sched` shell | hook tests through `Policy::at`; TSan; Miri (both borrow models) through `Script`; every-build end checks | ST-1: each `Effect` and each `Step` is performed as one atomic operation or one lock section, and the facts fed back are read in that section. ST-2: the model assumes sequential consistency; `sleepers` and `searching` are `SeqCst`; `running` is written under slot locks (`AcqRel`) and read `Acquire`; a scan reads a list only under that slot's lock; `listed` is read without the lock only by the verdict, after `running`. ST-3: `std::sync::Mutex` / `Condvar` and thread park. Written in runtime.md 「信頼の一覧」 |
| `eml_nrt` rows and entries | Miri through `Script`; ASan; UI tests checked and fast | the `shim` rule (no borrow or guard across `eml_fatal`); the three asm stubs |
| generated code | checked lowering: an audit before every inline access whose pass implies the body's safety premise `x_passes` (P-5), twin CLIF pins, the census in fast mode | Cranelift; the lowering's use of offsets (`mem.rs`); `word_in(w)` for every word it passes; `Trusted`'s premises (engine-trust P1), from which the fast preconditions `x_ok` follow |
| C functions | - | the program's `extern "C"` declarations |

### 14.4 `rt-concurrent-proofs`: still deferred

A concurrent proof is a rewrite of `eml_rt`'s sequential model, and a lending token for other heaps' objects moves the trust into ghost form without reducing it; one engine does not change that. What this design adds narrows what T0 carries: the isolation invariant is written; the crossings of a fork are `Marked`; generated code keeps three heap-store sites with dominance pins in both lowerings; native `par/` runs checked at N = 2 and 8 from `jit-tasks`' first `par` commit and repeatedly at N = 8. Evidence wording: TSan sees the shared Rust code, not generated code. Review condition: generated code gets a fourth heap-store site or an inline shared-count or cross-worker operation; or TSan / Miri finds a T0 / T14 error; or a native run at N >= 2 fails the census or the checked run without a logic cause; or stealing of frameless tasks is scheduled. The roadmap item 「協調的な配置のVerusの証明」 is split: its wake-up half is `sched-proofs`; its placement half ("the owner is decided once at `start`") stays with `rt-concurrent-proofs`.

Not proposed: a VerusSync refinement proof of the scheduler's shell; the Kind-solver proof of docs/future/verification.md (unchanged; `eml_steps` is the precedent for its leaf crate).

## 15. The profiler and other meta-level mechanisms

### 15.1 One table serves four consumers

D-MT-1: the unit of position information in native code is a per-function table "code offset -> `Loc`", built from Cranelift source locations that the lowering sets from LIR positions. `Image`'s function record already has the name, the code range and the fault sites [V: image.rs L88-95]; the table is one more slice beside them. Consumers: the profiler's line attribution, runtime-error backtraces, "the user's position for an error inside std", and debugger line tables for AOT binaries. Positions enter LIR on calls only (`Call`, `apply`, in the trailing `@"file":l:c` form `extern` and `par` already have); a sample or frame inside a function is attributed to the function's definition line or its innermost call site.

### 15.2 Hooks the planned stages lay down

| Hook | Stage | What |
|---|---|---|
| H-1 frame chain | exists; JT-2 | frame pointers in all generated code and the stubs [V: abi.rs L26]. Test `the_frame_chain_reaches_eml_enter`: from generated code the chain reaches `eml_enter`'s frame record, whose return address is inside `NativeExec::enter`. Rust frames beyond it have no frame pointers on x86_64 Linux and Windows today (no `force-frame-pointers` in the workspace [V: grep]); "reaches the thread base" is the profiler stage's test, with its build flag. No stack switch and no asm but the stubs keeps the chain one per thread |
| H-2 task attribution | SC-5 | `Slot.current: AtomicU64`, written by `drive` at item open and close: a task cell, `NO_CELL`, or a job tag with the forking worker. Only in the new `run.rs` |
| H-3 stack bounds | JT-3 | `Slot.stack = (base, top)`, written once by the worker thread; a sampler reads frame records only inside it, so a garbage frame pointer cannot make it read unmapped memory |
| H-4 entry classes | NC-4 | the descriptor row's `class`: `Alloc`, `Free`, `RcSlow`, `Share`, `Reuse`, `Audit`, `Row`, `Task`, `Service`. Tool-side only |
| H-5 symbols | exists; `native-aot` | name-keyed symbols, `NativeMachine::symbols()`; `native-aot` decides the public symbol of a function as an injective escape of its LIR name, written in native.md |
| H-6 protocol points | D-SC-5 | `Policy::at(Point)`: the tests' hook now, a tracer's later |
| H-7 site table shape | exists | `site -> (function, Loc)` for extern statements; D-MT-1's table generalises it without changing it |

Not laid now, and nothing is redone later: the frame-pointer build flag for the runtime's Rust code; positions on LIR calls (additive to the IR text).

### 15.3 Plans (sizes are [I] and bind nothing now)

| Item | What | Needs | Size | When |
|---|---|---|---|---|
| `perf` map | `EML_PERF_MAP=1` writes `/tmp/perf-<pid>.map` from `symbols()` at each load (Linux) | H-5 | 1 unit | later item; any time after `jit-tasks` |
| `native-prof` | a sampler in the runtime closure that writes raw records `(worker, Slot.current, pcs[..64])` to a file, and `eml prof report` in the tool that resolves them by symbols and entry classes into folded stacks and tables per function, per entry class, per task. Sampling per OS: a directed timer signal whose handler walks its own frame chain (Linux), thread suspend and thread state from the monitor thread (macOS, Windows). The runtime side never resolves names, so AOT binaries stay strippable | H-1..H-5; the frame-pointer build flag; an `unsafe` boundary (reading another thread's stack) that goes to a Fable advisor first | 8 units | stage after `native-aot` (the JIT half needs only `vm-retire`) |
| `native-lines` | positions on LIR calls; D-MT-1's table; the fault path records up to 64 return addresses when a fault is parked; the report prints `at f (file:l:c)` lines and the nearest user frame for an error inside std; `eml prof report` gains lines | D-MT-1 | 5 units | later item after `vm-retire`; closes roadmap 「実行時エラーの backtrace」 and the std-position item of 「診断」 |
| AOT debug info | `.eh_frame` / compact unwind from Cranelift's unwind info and `.debug_line` from D-MT-1's table, ELF and Mach-O | `native-aot`, `native-lines` | 3 units | later item; condition: someone debugs an AOT binary with lldb / gdb and frame pointers are not enough |
| `RunStats` | counts work, natively from `Ctx<Checked>` and the audits; schedule-free fields stay schedule-free; `claims` / `join_waits` are in `Finished`. The profiler measures time and is read beside `--stats` | - | in `native-checked` | planned |
| tracing | a `Policy` that records `Point`s with a timestamp to a ring per worker | H-6 | 2 units | later item; condition: a scheduling problem that counters and the profiler cannot explain |
| REPL meta commands | `:load`, `:reload`, `:type` are `Session` queries and need nothing from the runtime; `:stats <input>` is available in a `--debug-heap` session | `jit-repl` | 1 unit | later item |
| top-level values with `Panic` | evaluated once at load, before any thread (step 1 of `run`, where load-time constants are built), natively; a `Panic` there is a run error naming the value. There is no interpreter to evaluate at compile time, so "a compile error for a body that panics" is given by the tool running the initialisers natively under `eml check --eval`, or not at all. Constant folding of pure externs in Core IR stays its own roadmap item | step 1 of `run` | roadmap | unchanged item, restated |
| symbol naming | unchanged: `f:<name>`, `c<k>:<name>`, `v:<name>`, `s:` / `i:` / `x:` data, `e:` / `m:` effects, `c:` C rows, `eml$apply$<m>`, `eml$pap$<m>$<r>`, entries `eml_rt_*`, `eml_x_*`, `eml_nrt_*`, C `eml_c$` / `eml_cd$`. The merge renames no symbol | - | - | - |
| Rust-side generation | three lists become two: `eml_extern`'s `table!` (types and effects, for the front end) and `eml_nrt`'s `rows!` (body, ABI form, fast and checked entry, descriptor with `class`), tied by the existing const check | - | in `vm-retire` VR-6 | planned |

Dropped, with reasons:
- Debugging JIT code through the GDB / LLDB JIT interface: it needs an in-memory object file per load; the checked mode, the REPL and backtraces cover the use.
- Variable- and type-level debug info: Reprs, unboxed leaves and evidence passing make locations unfaithful to the source.
- Windows line tables (CodeView / PDB): no writer worth carrying; Windows gets symbols and frame pointers.
- Positions on every LIR statement: every pass and both IR texts would carry them for line granularity inside straight-line code.
- A built-in sampler UI: the report is text and folded stacks.
- Reflection and runtime type descriptors: evidence is decided by monomorphisation and there are no dictionaries; `deriving` and the REPL's printer are compile-time. Nothing in docs/ plans it.
- User macros and user-defined deriving: not planned in docs/; `deriving (Eq, Ord, Show)` stays compiler-internal.
- The roadmap's speculative JIT (copy-and-patch, trace JIT "from bytecode"): its premise is the bytecode; deleted from the roadmap at `vm-retire`.
- A separate compile-time interpreter for Core IR.

## 16. Soundness

| # | Invariant | Enforced by | Test |
|---|---|---|---|
| S-1 | A job is consumed exactly once; its words are counted references; every word crossing at a fork is marked before publication | `Job` not `Clone`; P-1; `Marked` | `claims::`; TSan; checked native `par/` at N = 2, 8 |
| S-2 | A join runs only its own job or takes only its own value; a `None` from `take_own` means the job was taken | P-1 with the completeness clause; the every-build nesting assert; `wait_join` by serial; serials never reused | `par::`, `par::a_none_behind_a_newer_join_is_an_internal_error` |
| S-3 | No taker sleeps while a job it may take is listed | the four rules as `idle::step`; P-4a, P-4b; ST-1, ST-2 | the `claims::` hook tests; the explorer |
| S-4 | Only workers 0..N-1 take foreign jobs, and only with an empty ready queue; the separate main worker runs no job at its loop | P-2 | `workers::a_busy_worker_takes_no_foreign_job`, `pinned::a_pinned_compute_runs_off_the_main_worker` |
| S-5 | A `compute` job starts after at most one bounded ready pass when nobody takes it | P-2 | `jobs::` |
| S-6 | The verdict and the virtual clock never fire while a job is listed, a job runs, a wake is in flight, a `Joined` notice is undelivered or a worker waits at a join | a wake counts its target in `running` under the target's slot lock; `LeaveRunning` under the own slot lock; `verdict::step`'s four steps as a loop; `listed` read after `running`; a join waiter stays in `running`; P-4a assertions (2) and (3) | `claims::the_verdict_waits_for_a_listed_job`, `the_verdict_waits_for_a_wake_in_flight`, `the_last_sleeper_wakes_worker_0`; `clock::`; virtual-clock `par/` at N = 2 |
| S-7 | `wait_join` leaves only with its value or a stop, and every stop reaches it | 2.5's predicate and wake sources | `par::a_join_wait_sees_a_stop` (four bits, three seats) |
| S-8 | A successful run ends with every `open` empty and no `Joined` in an inbox | every-build check | unit tests |
| S-9 | No yield is in flight at a child boundary | every-build asserts; the type of `fork_join` | `par::` |
| S-10 | A child's runtime error ends the run, from any worker | the fatal slot | `par::a_taken_childs_error_ends_the_run` |
| S-11 | The fatal longjmp cuts only eml frames, thunks and the detecting entry's frame down to `NativeExec::enter`'s `eml_enter`; no `eml_sched` frame and no guard is under it | `eml_sched` cannot name `eml_fatal` (crate edge); `eml_fatal(` only in the macro arms and `enter.rs`; the shim rule | `fatal::`, `tasks::fatal::in_a_par_child_three_deep` |
| S-12 | Across `enter`, `drive` holds no reference and no pointer derived from `this` other than `this` | `Exec::with` is a closure; placement test | Miri `drive::`, both models |
| S-13 | The `children` stack is strict | 11.4; asserts at item open and in the services | `par::` record checks |
| S-14 | `stack_limit` has one writer; no unwinding action while yielding or aborting; `Defers` sites never get a raise | grep; P-3 | `the_stack_limit_has_one_writer`, `apply::the_fewer_arm_never_raises` |
| S-15 | Pinned eml code stays above `hard` of the measured stack; a too-small thread starts no pinned task | 11.1 | `pinned::a_deep_pinned_task_gets_resource_limit`, `pinned::a_small_main_thread_ends_the_run` |
| S-16 | No panic unwinds through generated code or `Park` / toolkit frames | `shim`, `on_own`, `run_ready` | `pinned::a_panic_in_the_loop_is_an_error_value` |
| S-17 | A `Ctx` is detached when it leaves its thread; `runtime` is null or valid | assert; the guard | TSan `workers::`; `workers::a_panicking_worker_leaves_no_runtime` |
| S-18 | Constants and data slots exist before any thread of the run | step 1 of `run` | native 2 / 8 UI runs |
| S-19 | Generated code has three heap-store sites; an inline count store is dominated by the local-range compare, in both lowerings | placement test; CLIF pins and twins | the tests; the N = 8 stress run |
| S-20 | No object is written by two threads; `std/Par` has one extern, `fork_join` | std export test | `grep -n 'extern' std/Par.em` |
| S-21 | One root scope per run; the run ends only with the root done and the scope empty | D-AR-2 | `async/` at N = 1, 2, 8 |
| S-22 | Every engine entry that takes a program takes `&Trusted` | `engine-trust` | grep |
| S-23 | Native code suspends no frames: an activation ends or waits as a continuation before its worker runs anything else, except inside `wait_join` | no switch primitive; `drive` is the only caller of `Exec::enter` | placement test |
| S-24 | In checked mode every inline memory access of generated code is preceded by an audit whose pass implies the safety premise of the access (`x_passes`), and every direct entry call that is a heap operation is the full checked operation; nothing is checked or counted twice | D-NC-1, D-NC-3, D-NC-4; P-5; the placement test on `Step::slow()` | twin pins; the placement test on `lower/audit.rs`; checked UI runs; D-NC-10's counter parity; the fast census |
| S-25 | Slot mutexes are never nested; the `idle` mutex and a slot mutex are never held together; a list is read only under its slot lock | `Effect`s are single sections; the placement test on `listed` | TSan; `claims::` |
| S-26 | No safe function of a crate other than `eml_test_support` reaches undefined behaviour, given the three premises of 「保証」 | architecture.md 「公開の面と`unsafe`」; `Trusted`, `Loaded`, `RawBuf`, the `cells` contract | `surface::` |

## 17. Cost model

| Path | Cost |
|---|---|
| fork and join, child not taken (native) | 3 services, 2 direct `eml$apply$1`, `fork_worker`, `mark(g)`, two slot-lock pairs, three `switch_task`, `transfer_back`, `retire_worker`, the pair; no `eml_enter`; in std two `try_panic` and two `Result` boxes. Lock share measured at JT-14 |
| take by another worker | 1 lock on the victim (`pop_front`); one `eml_enter`; end: `mark`, 1 inbox lock and a ring |
| join wait | the worker blocks on its condvar; no switch, no reservation |
| wake-ups | at most one per publish, only with a sleeper and no searcher; one pass-on per take and per notice-wake while another worker sleeps |
| the kernels | `plan::next`, `idle::step`, `verdict::step` are branch-only functions inlined into the shell; `Open` operations are O(1) |
| entry safepoint | unchanged: 2 loads, SP, 2 compares, 1 branch |
| task switch (one wait and wake) | the row (1 registry lock), item close, a pick from an unlocked queue, item open (1 lock), 2 `switch_task`, 2 `limits::set`, 1 stop-word load, 1 `eml_enter` round trip (about 42 instructions) |
| checked mode | one audit call per inline access; fast mode pays nothing (separate lowering, separate table) |
| root task | one handler per run; a synchronous `main` pays nothing |
| data-parallel | one fork per 1024 elements; per element the leaf loop, then in `concat` one count pair and 8 bytes |
| first fork over an unshared boxed input | one visit per object on the forking thread |
| memory | one thread stack per worker; a waiting task holds no stack |
| the VM until deleted | unchanged but for `par-total` (+0.38 µs per empty fork) and the deletions |

Forgone: a lock-free claim; help-on-join; moving unique chunks in `concat`; in-place `Raw` updates; inline task rows; a `par` for children that cannot panic; a VM that runs the new scheduler.

## 18. Conformance criteria

| ID | MUST / MUST NOT | Applies | Check |
|---|---|---|---|
| G-1 | MUST: every public engine function that takes a program takes `&Trusted` | engine-trust on | grep for `&Program` in the engine surfaces |
| G-2 | MUST NOT: `Async.run`, `run_raw`, `run_result`, `WaitToken` exist | async-root on | `grep -rn 'Async\.run\b\|run_raw\|run_result\|AsyncRunRaw\|WaitToken' crates std tests bench docs/spec docs/implementation` empty |
| G-3 | MUST: `fork_join`'s children are total | par-total on | `grep -rn 'child_abort\|ChildAbort\|fork\$\|reperform\$\|fork_ev\|fork_of\|alloc_fork_\|ForkTable\|MissingFork\|reraise' crates/*/src std` empty |
| G-4 | MUST: the limit classes are `Stops`, `Task`, `Aborting`; the monitor has no memory role | task-limits on | `grep -rn 'pressure\|mark_limited\|Pending\|soft_bytes\|claim_pressure\|limit_task\|clear_limit\|limit_forwarded\|add_bytes' crates/*/src` empty |
| G-5 | MUST: `std/Par`'s functions are eml over `par`; the split depends only on length and grain | std-par on | `grep -n 'extern' std/Par.em` shows `fork_join` only; float `reduce` bit-equal at N = 1, 2, 8 and two seeds |
| G-6 | MUST: every module of `lower/` that names one of the nine `mem::` access helpers of D-NC-3 emits the audit through `lower/audit.rs`, and checked lowering emits the same instructions after it; `eml_rt_dup_slow` and `eml_rt_decref_slow` are named only by `Step::slow()` | native-checked on | the two placement tests; twin CLIF pins |
| G-7 | MUST: checked native and the checked VM agree on error classes and on every `RunStats` field but `peak_stack` at one worker, and on the schedule-free fields at every worker count and seed | native-checked to vm-retire | the harness (D-NC-10, JT-13) |
| G-8 | MUST: `eml_steps` is wholly inside `verus!` with no `unsafe`, `external_body` or `assume_specification`; the scheduler takes the decisions of P-1..P-3, the idle protocol and the verdict from it | sched-core on | `cargo verus verify -p eml_steps`; `trust.rs`; placement tests: `Open`'s operations, `plan::next`, `idle::step`, `verdict::step`, `limit::hit` each have one caller module in `eml_sched` |
| G-9 | MUST: an unstarted job exists only in an `Open`; MUST NOT: placement of a child by inbox or a field `Run.in_flight` in `eml_sched`'s new files (`Registry::in_flight(k)`, the count of operations in a driver, is not this) | sched-core on | grep over `crates/eml_sched/src/{run,job,fork,drive}.rs` |
| G-10 | MUST, until help-on-join is designed: `Exec::enter` is called only by `drive`; MUST NOT: asm other than the entry stubs, a stack switch, thread-locals in `eml_nrt` / `eml_native` | jit-tasks on | placement tests; `grep -rn 'strand\|Stacks\|CreateFiber' crates` empty |
| G-11 | MUST: one worker loop, `eml_sched::drive`; MUST NOT: a loop over ready items in `eml_nrt` | sched-core on | `grep -rn 'fn drive' crates/eml_sched crates/eml_nrt` has one hit |
| G-12 | MUST NOT: a `sched-core` commit change a file under `crates/eml_vm` other than the adapter SC-1 lists, a UI output or a snapshot | sched-core | `git diff --stat` |
| G-13 | MUST: generated `par` code is the sequence of 11.4 with no unwind point | jit-tasks | CLIF pin |
| G-14 | MUST: 「VMとの違い」 lists exactly the two items of 11.7; every `native: skip` names item 1; outputs are compared on every file but those with `-- workers: skip` | jit-tasks to vm-retire | read; a harness check on the mark's text |
| G-15 | MUST: the `Park` is entered only from the main worker's idle loop; a fatal or a panic crosses toolkit frames only as a value | jit-tasks | `pinned::` |
| G-16 | MUST: `stack_limit` has one writer; the safepoint's hot path is unchanged | jit-tasks | grep; CLIF pin |
| G-17 | MUST: the end of a native run runs on the calling thread after both joins; one `clear_end` call site in `eml_nrt` | jit-tasks | grep; `pinned::` |
| G-18 | MUST: every commit passes `cargo nextest run`; a form is accepted natively only when everything its files need at one worker is in place; a test that reaches `eml_fatal` is in a module named `fatal` | all | unit order; sanitizer jobs |
| G-19 | MUST: nothing on `eml_nrt`'s run path reads a descriptor table; no `#[used]`; no dispatch over `Extern` | jit-tasks on | grep |
| G-20 | MUST: `eml_vm` is deleted only after D-VR-1's run is recorded in the plan; every file of `crates/eml_vm/tests` has the fate of 11.10 or a listed change of it | vm-retire | the plan |
| G-21 | MUST: the end-state crate graph is 13.1; `eml_sched` names no entry, stub or `extern "C"` | vm-retire on | `nix/crate-graph.sh --check` with the tables and greps of 13.3 |
| G-22 | MUST: the hooks H-1..H-4 and H-6 exist with their tests | their stages | `the_frame_chain_reaches_eml_enter`; a test that reads `Slot.current` and `Slot.stack` from another thread; a test over the descriptor rows' `class` |
| G-23 | MUST: a scan reads a list only under its slot lock; `listed` has the readers the placement test pins | sched-core on | the placement test |
| G-24 | MUST: `surface::` passes at every commit from `engine-trust`'s ET-R1 on | engine-trust on (every later stage) | `cargo nextest run -p eml_cli --test integration -E 'test(/^surface::/)'` |

## 19. Topics

| Topic | Answer |
|---|---|
| REPL | D-AR-5, 12.1; one worker, merged seat; native from `jit-repl`, checked with `--debug-heap` |
| GUI `Park` | Served whenever the main worker's queue is empty; not while a pinned task runs, a join wait included; never re-entered |
| Signals | Handlers set stop bits; the monitor relays; `wait_join` reads the stop word after every wake |
| Cancellation | Unchanged; a `compute` job runs to its end; a task inside `par` sees the bit at its next wait. stdlib.md 「`Std.Par`」: "inside a task, `par` keeps the task's worker until the join; use `compute` to keep serving" |
| `compute` | Publishes a `Done::Cell` job; a pinned caller's job is never run by the separate main worker (stated with its price in stdlib.md) |
| `RunStats` | From `Ctx<Checked>` and the audits. Marking, IDs, `transfer_back`, `retire_worker` at every fork and join: schedule-free. `claims`, `join_waits` are in `Finished` |
| `Seeded` | Shared policy code; natively from `jit-tasks`; the VM's keeps preempting until deleted. It gives no reproducible cross-worker interleaving; the hook tests and TSan carry the protocol |
| The parity harness | From `jit-tasks` to `vm-retire`; from `native-checked` it also compares checked runs |
| `eml_test_support` | Gains the machine helper (NC-10, extended at JT-5), loses `Engine` (`vm-retire`); `Script` is in `eml_sched`, so `eml_sched`'s Miri run does not build the compiler |
| Gates | `eml_rt` units: its own gate. VM stages: `bench/run.sh` +0.5 %. Native: `bench/native.sh` is a record until `vm-retire`, then `bench/run.sh` runs native code and its gate applies |

## 20. What earlier designs had that this one does not

| Mechanism | Why it is gone |
|---|---|
| Strands, stack switching asm, fibers, a home stack, spare stacks, the `raw` entry form, `Extern::is_gui()`, the C rule for runtime-allocated stacks | E2: no wait holds frames but a join |
| `Async.run` nesting, `BuriedRun`, a token for a waiting root, `Aside::WaitRoot` | E2 |
| Fork evidence, `fork_evv`, `Forks`, `child_abort`, a re-raise slot and service, `Aside::Reraise`, abort records at a join | SC-5 |
| `Limit::inherit`, a class per job, `Pending`, `flagged`, `mark_limited`, the monitor's pressure role | SC-6, SC-8 |
| `Items`, `Carried`, held items and engine notices in the shared queue types | jobs and joined values are engine-neutral |
| A method-free `Exec` and a loop per engine; `Seeded` refused natively | SC-9: one loop |
| A Chase-Lev deque, `par-steal`, `native-par`, helping at a join | E1; help-on-join is a later item |
| Moving the VM's worker table, monitor and task arms into `eml_sched` / `eml_rows` byte-identically; the VM gate on three task rows | UJ-17: the VM's scheduler is not moved |
| A join table, generations, orphan marks, `JoinEnd::Error`, `fork::discard` | SC-1; joins keyed by a per-worker serial |
| `Options.masked` | the end of a native run is on the calling thread |

## 21. Amendments against the repository's settled decisions ("entry: old -> new")

The amendment list (A-1..A-40, "entry: old -> new" with the approving decisions) is kept in one place, 00-decisions.md §B, because most of its "old" entries name plan files that this group deletes. The IDs A-n are cited from there.

## 22. Open items with defaults

| # | Item | Default | When |
|---|---|---|---|
| U-1 | `cells.rs` serves the old and the new scheduler by additions only | yes (D-SC-3); else the adapter | SC-1, before approval |
| U-2 | `RunStats` fields equal between checked VM and checked native | equal with D-NC-1's "no double count" and D-NC-7's sources [I]; exceptions listed and deleted at `vm-retire` | NC-1 |
| U-3 | Lock share per fork natively | over 15 % schedules the CAS claim as a later item | JT-14 |
| U-4 | The inductive invariant of P-4b | found within 4 units; else P-4a stands | `sched-proofs` |
| U-5 | `async-root` manual class and Rust kind-1 list | AR-1 | AR-1 |
| U-6 | Linux `pthread_getattr_np` on the process main thread | queried once per run | JT-3 |
| U-7 | E2020's replacement code at `par` / `compute` calls | ordinary row mismatch | PT-1 |
| U-8 | Build time of the runtime library with both entry tables | acceptable; else the feature of D-NC-6 | `native-aot` |
| U-9 | Which tests of `api.rs`, `run.rs`, `machine.rs` are VM-only | listed | JT-1, the `jit-repl` plan |

## 23. Responses to the fifth verification round

| # | Verdict | Where |
|---|---|---|
| V-1 | Accepted | D-NC-5, P-5, S-24, 14.3's row: a passing audit implies `x_passes`, the body's safety premise; `x_ok` and `word_in` stay premises. 2 `eml_rt` units (NC-2, NC-3) |
| V-2 (a) | Accepted | D-NC-1's seven audits (`header`, `box` added); D-NC-3: `lower/audit.rs` and the pinned module list `rc`, `data`, `apply`, `unwind`; G-6 |
| V-2 (b) | Accepted | D-NC-1: the checked table's two cold entries (`eml_rt_dup_slow`, `eml_rt_decref_slow`) are bodies without the top-level audit and count |
| V-2 (c) | Accepted | D-NC-1: the audit settles and counts immortals; `audit_store` is legal on a building object |
| V-2 (d) | Accepted, decided | D-NC-7 (as corrected by V5 r2: count calls per `box` / `unbox` statement and in `FnLower::cleanup`); a field that cannot be equal is deleted at `vm-retire`, not approximated. D-NC-10 compares `RunStats::fields()` |
| V-3 | Accepted | 2.2 (`running`, `woken`), 2.4 (`running` and waking; the verdict's four steps), `idle::step`'s `LeaveRunning`, `verdict::step`, D-SC-7's second assertion with worker 0 among the three, P-4b's Close statement, ST-2, S-6, runtime.md 「眠りと起こし」 |
| V-4 | Accepted | D-SC-2's file list (`io.rs`, `clock.rs`, `signals.rs`, the split of limits / monitor + pool / main loop), D-SC-6's tests, 15 units; section 1 states what exists twice |
| V-5 | Accepted | `Finished.claims` / `join_waits` (2.2; added at JT-5, 00 §D D-6); `preemptions`, `placed`, `placed_jobs` leave `Finished` at `vm-retire` (D-VR-5) |
| V-6 | Accepted | 13.3's file map; `rows/task.rs`, `rows/blocking.rs`; the edge `eml_cli -> eml_nrt` |
| V-7 | Accepted | 11.9 (files switched in JT-5, JT-7, JT-9, JT-10, JT-11; NC-10; `jit-repl`), 11.10 (every file's fate), `eml_sched/src/counters.rs` in SC-13 with its CI lines |
| V-8 | Accepted | 2.4 "a scan reads a list only under that slot's lock"; ST-2; G-23; S-25 |
| V-9 | Accepted | P-1's completeness clause; `Open::newest_join`; the every-build assert in 2.5 / `eml_nrt_left`; S-2 |
| V-10 | Accepted | `WaitToken` and its relatives go in AR-7 (D-AR-4, G-2); G-9's pattern is the field `Run.in_flight`; D-TL-2 deletes the VM's `current` / `top` and H-2 is only in the new `run.rs`; native seed runs in JT-13; P-3's example rows come from the VM's table; section 1's Needs column |
| V-11 | Accepted | 11.7 item 2 (the skip class is `-- workers: skip`, 2 files); the "over 20" fallback is gone; G-7, G-14 |
| R4 B-1 | As ruled (refuted) | 2.5's last paragraph records why |
| R4 B-2 | Accepted | H-1: `the_frame_chain_reaches_eml_enter`; "reaches the thread base" belongs to the profiler stage |
| R4 B-3 | Accepted | D-SC-3 (the list before approval; `pub(crate)`), D-SC-10, G-12 |
| R4 B-4 | Accepted | section 1 |
| R4 C-1 | As ruled | V-9's clause and assert |
| R4 C-2 | As ruled | ST-2 |
| R4 C-3 | Accepted | `eml_cabi` stays (13.1); 19 directories |
| R4 C-4 | As ruled | 11.7; V-7's early switching |
| R4 C-5 | Accepted | D-PT-7: the sentence in stdlib.md 「`Std.Par`」 and vm.md 「子の誤り」 |
| R4 C-6 | Accepted | G-7 (fields), G-14 (outputs) |
| R4 A-1 | Accepted | 15.3's heading: sizes are [I] and bind nothing now |
| R4 A-2 | Accepted | D-SC-3; ST-2 |

V5's second round (on the previous revision of this document):

| # | Verdict | Where |
|---|---|---|
| V2-1 | Accepted | D-NC-1: `eml_rt_decref_slow` is the inline decrement's cold call; only it and `eml_rt_dup_slow` are bodies in the checked table; `eml_rt_decref` stays the full checked operation for its three direct callers; the placement test on `Step::slow()`; D-NC-4 lists the direct heap-operation calls; G-6, S-24; the fast pins' callee name is kind 2 |
| V2-2 | Accepted | D-NC-7: one count call at the head of each `box` and `unbox` statement in checked lowering; `audit_box` only checks |
| V2-3 | Accepted | D-NC-7: the count inside `FnLower::cleanup`; `eml$apply$m`'s cleanup block (default: counts only when arguments remain) and thunk frames (default: no count) are NC-1's stated decision |
| V2-4 | Accepted | 2.4: the verdict restarts at (a) after any wake; the main worker's park does `LeaveRunning`; D-SC-7 assertion (3); P-4a, P-4b, S-6 |
| V2-5 | Accepted | 11.9, 11.10: the four task files switch at JT-7, `par.rs` at JT-9; the machine helper is created at NC-10 and extended at JT-5 |
| wording | Accepted | P-5 and D-NC-5 say "no error from the step"; D-NC-1 states where the audit sits relative to the immediate test |

Earlier rounds, for the record (their required changes are all in the text above): V3 gave the four rules, the loop order, `listed` for the verdict, the spine split, the `async-root` scope, the main-thread geometry and the measurements at JT-14; V4 gave the named bindings of `par`, audit-before-inline, the `eml_rt` units of `par-total` and `task-limits`, SC-6's wording, `Exec::with`, `wait_join`'s wake sources and the budget points.

## 24. Decisions taken by the user that this document implements

UJ-9 (E1, E3), UJ-10 (E2), UJ-11 (SC-1..SC-4), UJ-12 (help-on-join later), UJ-13 (8 MiB cap on Linux only; proofs deferred), UJ-14 (SC-5..SC-9; `transfer_back` kept), UJ-15 (JIT-only), UJ-16 (process), UJ-17 (the five decisions of the previous revision with V5's changes: the VM's scheduler untouched; `native-checked` first; the crates of section 13; the proofs of section 14; the profiler cut of section 15). No decision is open.

## 25. Per-stage runnable state (CI green at every commit, tests within 1 s)

| Stage | What keeps the tree runnable |
|---|---|
| 1-5 | the VM runs everything; native runs what it accepts; parity as today |
| `native-checked` | additive: a second lowering and table; fast lowering's CLIF pins are byte-identical |
| `sched-core` | additive: outside `crates/eml_steps` and the new files of `eml_sched`, only `eml_sched/src/lib.rs`, `cells.rs` (additions), the workspace manifest, CI and the listed adapter change; the explorer is `#[ignore]` with its CI line |
| `jit-tasks` | each form is accepted natively in the unit that completes it at one worker; each switched test file runs on both engines from its unit; workers above 1 are a usage error until JT-11 / JT-12 |
| `jit-repl` | the default flips in the Close commit, after the transcripts pass on both engines |
| `vm-retire` | VR-2 and VR-3 make every test independent of `eml_vm` before VR-4 deletes it; VR-5 is a mechanical move by the map |
| `sched-proofs` | proofs only |

## 26. Changelog r4 -> r5

- Self-contained: the model, every stage and the comparison with earlier designs are written here; no earlier revision or plan file is cited.
- Audits (V5 V-1, V-2): seven audits instead of five; an audit does everything the checked operation does but the step; the checked table's cold entries are bodies; `lower/audit.rs` with a pinned module list (`rc`, `data`, `apply`, `unwind`); P-5 restated to `x_passes`; native sources for `boxes`, `unboxes`, `unwound_frames`; `native-checked` 10 -> 12 units.
- Verdict (V-3): `running`, `woken`, `LeaveRunning`, counting at the wake, `verdict::step`, the explorer's second assertion, ST-2.
- `sched-core` (V-4, V-7): `io.rs`, `clock.rs`, `signals.rs`, `main_loop.rs`, `threads.rs`, `counters.rs`; 10 -> 15 units.
- `jit-tasks` (V-7): machine-API test files switch to both engines in the unit that enables them; every file of `crates/eml_vm/tests` has a fate (11.10); native seed runs; 17 -> 16 units.
- `Finished.claims` / `join_waits` instead of `RunStats` fields (V-5).
- Crates: `eml_cabi` stays (19 directories); the `vm-retire` file map; `rows/blocking.rs`; the edge `eml_cli -> eml_nrt` (V-6, R4 C-3).
- `Open::newest_join`, P-1's completeness clause, the nesting assert (V-9).
- `WaitToken` deleted in `async-root`; the VM's `Slot.current` deleted in `task-limits`; G-9's pattern; G-12's adapter exception; G-23 (V-10, V-8, R4 B-3).
- 11.7: the skip class is the two `-- workers: skip` files (V-11).
- H-1's test stops at `eml_enter`'s frame record (R4 B-2).
- Section 21: the full amendment list with approving decisions. Total 94 -> 100 units.

## 26b. Changelog r5 -> r6

- `eml_rt_decref_slow` added as the cold call of the inline decrement; `eml_rt_decref` keeps its full check for direct callers (V2-1).
- `boxes` / `unboxes` counted per LIR statement by a count call; `audit_box` counts nothing; `unwound_frames` counted in `FnLower::cleanup`, with two placements left to NC-1 with defaults (V2-2, V2-3).
- The verdict is stated as a loop; the main worker's park leaves `running`; the explorer's third assertion (V2-4).
- Test switches moved to JT-7 and JT-9; the machine helper is born at NC-10 (V2-5).
- Two wordings (the step's error; the audit and the immediate test). Nothing else changed.

## 27. Things the plans must confirm first

1. SC-1: the `cells.rs` list (U-1).
2. NC-1: D-NC-4's table, the counter probes (U-2), the two `unwound_frames` placements, and where the audit sits relative to the immediate test.
3. PT-1: `may_fork`'s callers; E2020's replacement.
4. TL-1: `Control::taken`'s readers; the VM's `current` / `top` readers.
5. JT-1 and the `jit-repl` plan: VM-only tests of the switched files (U-9).

## 28. Additional checks

1. [V: read] the verification rounds V4 and V5 in full; the user decisions (UJ-17); the earlier decision record of the 2026-11-11 group (its section H); the previous plan's sections on ready items, the task rows' context, the limit table, threads and the main loop, the native task entries, fatal errors and panics, native limits, blocking C, run orchestration, CLI and harness, the cost table and later items (their substance is inlined in sections 10, 11 and 17); CLAUDE.md; `nix/crate-graph.sh`; architecture.md 「crate の依存関係」; crates/eml_rt/CLAUDE.md; object-model.md 「`unsafe` の核」 rows T0..T18; roadmap 「進め方」, 「段の列」 and the items named in section 21; the engine-trust design doc under docs/future (deleted at the Close; now docs/spec/lir.md 「エンジンの約束」) and verification.md; `cells.rs` L1-60, L1303-1322.
2. [V: wc / ls / grep] lines per crate and per file of `crates/eml_vm/tests` (11.10); the files of `crates/eml_rows/src` and `crates/eml_nrt/src` (13.3's map has no existing target; `eml_nrt/src/rows.rs` exists and stays the module root); `RunStats` has 23 fields; 2 UI files carry `-- workers: skip`; the pinned vstd's `VecDeque` specs include `push_back`, `pop_back`, `pop_front`, `len`; `unsafe impl Send for Word` (word.rs L363); no `force-frame-pointers` in the workspace; `preserve_frame_pointers` in `abi.rs` L26.
3. [I], confirmed where named: U-1, U-2, U-9; every size; P-4b's feasibility; that a count inside `FnLower::cleanup` matches the VM's `unwound_frames` for function frames, and the two placements D-NC-7 leaves to NC-1; the per-OS sampling mechanisms of 15.3; the build time of two entry tables.
4. Not done: no build, test, Verus run or prototype by the designer (V5 ran a Verus probe for P-1 and timed native runs). Not read: `eml_vm`'s scheduler files, `lower/*.rs`, multicore.md, testing.md's sanitizer sections, the Sonnet review itself (V5's rulings were used).
5. No repository file was changed; no background process was started.
