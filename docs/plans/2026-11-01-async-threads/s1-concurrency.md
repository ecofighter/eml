# Appendix S1: concurrency model for `Async` tasks on any VM worker

This is S1 r3 Part 1 as designed. Where it differs from the texts in s4-integration.md §1.4 (AM-1: `next_place` starts at 1; AM-2: `Finished.placed_tasks`; the rulings on G-A6, G-A15, G-A16, Q9 and amendments 6 and 10, quoted verbatim in x-ruling.md), §1.4 and x-ruling.md win. Unit names U1..U9/U6b here map to A2..A14 in s4-integration.md §1.3.

## 1. Conclusion

**Q1 Registry and cells**
- One run-wide `Registry` behind one mutex (`Arc<Shared { cells: Mutex<Registry>, mail: Mail }>`), created by worker 0 in `run_scheduled`, cloned into every `Scheduler`. No per-worker registries, no per-cell `Arc`, no scope lock.
- Partitioned by physical worker inside the registry, same lock: `parts[k] = { timers, timer_seq, in_flight }`. Owner-local: `Scheduler.completed` scratch buffer, `Scheduler.ready`.
- (r3) Words never leave the registry's records: `OpRecord` keeps `buf`/`stream`/`made` at all times (as today). What leaves the record is the driver `Op` (handle + `RawBuf`), which holds no words. The registry hands back words to drop as `Discards` and the caller drops them after unlocking.
- Fields added: `TaskCell.owner: u32` (physical; `TaskCell.worker` stays the logical ID); `OpRecord { host: u32, submitted: bool, cancelling: bool /*exists*/, pending_op: Option<Op>, arrival: Option<(io::Result<Done>, bool)> }`.
- Lock order (outermost first): registry → `Run.slots[k].state` → leaves: `Run.idle`, `Run.fatal`, `Mail.items`, `HandleBin.dropped`, `Control.workers.{free,chunks}` (control.rs:81-83 [V]), Host mutexes (host.rs:216, 1207-1208 [V]), driver-internal locks. eml_rt heap code takes no other lock ([V] `grep -rn "Mutex\|RwLock" crates/eml_rt/src`: driver.rs:88, host.rs, control.rs only). A holder of a slot lock or a leaf never takes the registry lock. `Mail` is swapped out under its leaf lock and processed after release, repeated until empty.

**Q2 Ownership protocol**
- `owner` written once in `Registry::start`; never changes in this stage. Owner-only: materializing `Runnable`s into `Task`s, `take_wake`, `unpark`, `expire(parts[owner])`, running-task fields, and (r3) the op phase machine (`settle_op`/`advance`) for records whose task it owns. Any worker under the registry lock: `mark`, queue removal, scope finish, `wake_cell`.
- (r3) Single-holder rule for logical IDs kept as in object-model.md: a logical ID is used by one `Ctx` at a time; hand-offs are ordered by the registry lock. Task ID X is used only by the owner's `Ctx` while the task runs or while the owner settles its records; a Mail drainer uses X only after the cell is `Finished` (the owner used X last inside `entry_finish` → `cells.finish`, under the same lock). The host of a forwarded op never acts under X: it holds no words. No eml_rt rule changes for IDs.
- Transit rule (for `async-rescue`): a `Wake`/`Cancel`/`Completion` found in slot j for a cell whose `owner` is o ≠ j is re-sent to o under the registry lock; if o == j it is processed inline (`Route::Run`). `take_wake` returns `Route::{Run(Runnable), Forward(owner), Stale}` and never asserts `owner == self`.

**Q3 Notices, inboxes, ready queue**
- `workers::Notice`: `Child(Task)`, `Joined{join,outcome}`, `Place{cell, worker, closure}`, `Wake(Woken)`, `Cancel(CellRef)`, `Submit(CellRef)`, `CancelIo(CellRef)`, `Completion(CellRef)`. `Run` is added by `main-pinning`.
- `cells::Mail` (run-wide, `Arc`, leaf lock, `pending: AtomicBool`): `MailItem::{HandleDropped(CellRef), Signal(u32), Cancel(CellRef)}`. `mark` pushes `Cancel` to Mail when `owner == self`, sends `Notice::Cancel` to the owner's slot otherwise. Mail is drained by the first collector under the registry lock; a `Cancel` owned elsewhere is forwarded. At N = 1 this is today's single FIFO (cells.rs:459-483, 1790-1815 [V]). (r3) Mail producers are worker threads (any `decref` of a `TaskHandle`; `alloc_foreign` sites are cells.rs and tls.rs only [V] grep; pool/resolver threads touch no heap) and the monitor (`Signal`, followed by `wake_all`).
- Self-targeted `Wake`/`Place` inline; cross-worker via `Run::send`.
- (r3) Ready queue unchanged: `Scheduler.ready: VecDeque<TaskId>`, `Policy` trait unchanged, `Runnable`s materialized at the owner's collect as today (`schedule` → `Task::entry`, vm.rs:147-188, 2635-2691 [V]). An un-entered entry `Task` (`TaskState::Entry{function,args,argc}`, no frames; its stack is a cleared spare) is the stealable form multicore.md:517 already allows ("まだ始まっていないタスク"), so lazy materialization is unnecessary; `async-rescue` puts the entry-state ids of `ready` under the slot lock. TaskIds, `has_children`, failure points at N = 1 are therefore unchanged.
- `Run::send` as today; may skip condvar/waker when the slot was not `parked` under the lock; `Control::notice(k)` always.
- (r3) Lock sections per collect, exactly: S1 = `take_notices` (slot). S2 = one registry section: slot notices in order, Mail swap-loop, own arrivals (`complete`: apply `driver.completions` results collected before S2 to records, `in_flight[self] -= 1` each; for records this worker owns, run `settle_op` now; for others, queue `Completion` sends), `expire(parts[self])`, `set_watched`. After S2: discards, `driver.submit` for each `Op` taken in S2, `driver.cancel`s, `Ready` pushes, `Control::settle`, `unheard`. S3 = one short registry section per `Submit::Done` result (record the arrival; if self-owned settle it now; else queue `Completion`). G-A6 is stated with these three.

**Q4 Placement**
- `start_raw`/`start_in_raw`: `owner = next_place.fetch_add(1, Relaxed) % N` when `sched.workers.places()`, else self; `run_raw` on the caller; tasks never move.
- Lazy spawn: the two arms begin with `if sched.workers == Workers::Wanted { return Err(SPAWN_WORKERS) }` before any read. Verified mechanism: `run_tasks` captures `pc` before `step` (vm.rs:559-566 [V]), `task_failed` resets `task.pc` (vm.rs:1900-1907 [V]), `vm::run` → `Exit::Spawn` (vm.rs:488-491 [V]), `run_scheduled` re-calls `vm::run` with the same `task` (lib.rs:909-930 [V]). (r3) Coverage of contexts: the VM has no nested loop (no extern re-enters the VM: [V] `grep -n "nested\|入れ子" vm.rs` finds only the par-cancel comment; `Async.run` parks the caller instead of nesting, vm.rs:2083-2096 [V]); `Wanted` exists only in worker 0's `Scheduler` (k ≥ 1 are `Peer`, sched.rs:431-453 [V]); a `par` child cannot call `start_raw` because its row is `<never e>` and every `Async`/`Chan` extern is `<IO>` (`Once`) (std/Par.em:6, std/Async.em:324 [V]; multicore.md decision 8 [V]). So the sentinel is only ever raised inside worker 0's single `run_tasks`, from `main`, from an entry `Task`, or from a `Task` resumed after a nested `Async.run` — all the same loop frame. Refused spawn → `Denied` → `places()` false (lib.rs:923-927 [V]).
- At N ≥ 2 placement is race-dependent: output order and `RunStats` are not reproducible per seed; `RunStats` compared only at N = 1; `places()` stays a bool until `main-pinning` makes it a mask.

**Q5 Drivers per worker** (r3: protocol rewritten; the host never touches words)
- `Run::io_worker(k) = if Virtual { 0 } else { k }`; Real: lazy driver on k at first need; Virtual: worker 0 only; `with_driver` → `Denied` (all such tests run at `workers = 1` [V] grep).
- Run-wide time base `RunClock { Real{epoch}, Virtual(Arc<AtomicU64>), Injected }`; eml_io `RealDriver::new_at(epoch)`, `TestDriver::new_shared(seed, clock)`.
- Division of labour: the **owner** builds every `Op` (lending `RawBuf` from `buf`/stream `rest` exactly where `submit_*`/`advance` do it today), runs the phase machine, allocates `made` objects and commits bytes — all on its own `Ctx` under `with_worker(record.worker)`, under the registry lock, as today on worker 0 (cells.rs:2353-2520 [V]). The **host** only moves `Op`s into its driver and arrivals out of it. `RawBuf` therefore crosses threads inside `Op`: eml_rt `driver.rs` adds `unsafe impl Send for RawBuf` with the rule "created by the lender, used by the driver's thread; the lent object is kept alive and unmoved by the record until the arrival is recorded" (replaces vm.md:483's "driver を持つスレッドから出さない"; `driver.rs` is outside `verus!`, no `external_body`, no trust row; gate = `cargo verus verify -p eml_rt`, `verusfmt --check`, clippy, fmt).
- Record states (under the registry lock): `Idle` (no op) → `Queued` (`pending_op = Some(op)`, `submitted = false`) → `InDriver` (`submitted = true`, `pending_op = None`, counted in `in_flight[host]`) → `Arrived` (`arrival = Some(..)`, not counted) → settled by the owner (`phase` advances; may go back to `Queued` for the next step) → `Done`.
- Submit of a step, owner w, record host h:
  1. w under the lock: `pending_op = Some(op)`; if h == w take it back out at once and after unlock call `driver.submit`; else after unlock `send(h, Submit(op_cell))`.
  2. h on `Submit(c)` in S2: `op = pending_op.take()`, `submitted = true`, `in_flight[h] += 1`, `cancel_now = cancelling`; after S2: `driver.submit(key, op)`; if `Pending` and `cancel_now` → `driver.cancel(key)`; if `Submit::Done(r)` → S3: `arrival = Some((r,false))`, `in_flight[h] -= 1`, `submitted = false`; if w == h settle now else `send(w, Completion(c))` if `waiter.is_some()` (else the owner's next `op_ready` sees `arrival`).
  3. (r3, B-1 invariant) `cancelling` and `submitted` are read and written only inside registry sections, and the host reads `cancelling` in the very section that sets `submitted = true`. `cancel_io(c)` by the owner: `if submitted { if host == self { driver.cancel after unlock } else { send(host, CancelIo(c)) } } else { cancelling = true }`. Because the canceller's section and the host's section are totally ordered by the lock, exactly one of them observes the other's flag: either the canceller sees `submitted` and sends `CancelIo`, or the host sees `cancelling` and cancels right after `submit`. The owner-side `park_op`/`unpark`/`expire` call `cancel_io`; `cancelling` stays set for the `Cancelled` wake value as today.
  4. Arrival on h: before S2, `driver.completions(&mut completed)`; in S2 for each key: `arrival = Some((result, cancelled))`, `in_flight[h] -= 1`, `submitted = false`; owner == h → `settle_op` now (in S2); else `send(owner, Completion(c))` if `waiter.is_some()`.
  5. Owner on `Completion(c)` / `op_ready` / `park_op` with `arrival.is_some()`: `settle_op` under the lock → `advance` (today's code; a re-submit goes through step 1) → when `Done`, `wake_op` as today.
- (r3, B-2) Word ownership invariants: (I1) record words are always in the record; only `Op`s (no words) and raw results travel. (I2) A record in `InDriver` is freed by nobody: `take_op` requires `Done` (cells.rs:2647 [V]); `free_task`/`retire_worker(X)` happens only for a `Finished` task, which has no un-taken `OpId` (`OpId` is `Lin`, `io_wait` waits for the arrival even when cancelled, vm.md:795 [V]); `sweep_all` runs only after every driver of the run has been quiesced and its arrivals recorded (Q8), so every record is `Idle`/`Queued`/`Arrived`/`Done` then, and `sweep_all` discards their words under `record.worker` before tasks, as today (cells.rs:1826-1855 [V]). (I3) A `Queued` op whose host never submits it (run ends first) is dropped with the record; the `RawBuf` is dead but unused. (I4) Error/`exit`: each worker quiesces its own driver before returning its `Ctx` (so no kernel write outlives the heap), the registry is dropped untouched after join; the `Op` values in records are dropped with it. Who frees what: owner → words at `take_*`; worker 0's `sweep_all` → leftover words at run end; nobody frees mid-flight.
- Handle binding: `Bind::{Bound{host, handle, bin}, Unbound(RawSocket)}`; `listen`/`connect` bound to their host; `accept` results unbound until first `submit_*` (U6b, last, gated); until U6b accepted streams are bound to the accepting host.
- (r3) `in_flight` accounting: incremented in the section that sets `submitted`, decremented in the section that records the arrival (`Submit::Done` or completion). A driver error inside `submit` is a `Submit::Done(Err)` (vm.md:479 [V]); an error inside the owner's `advance` is a `CellFault` → run error (as today) → the run ends and the registry is dropped; `pending_all` is read only by the deadlock decision, which cannot run while the run is ending.

**Q6 Virtual clock and idleness**
- One virtual clock, advanced only by worker 0 at `others == 0` to the global minimum; owners of due partitions are woken; no spin because `Run::wake(k)` increments `running` (workers.rs:162-192 [V]).
- Deadlock decision (worker 0): (a) under its slot lock `parked = true`, `others = running.fetch_sub(1) - 1`, release; (b) `others != 0` → wait; (c) registry: `d_all`, `pending_all = Σ in_flight`, `listening = relay && signal waiters non-empty`; release; (d) (r3) re-take the slot lock: if `woken || !notices.is_empty() || running.load(Acquire) != 0 || mail.pending.load(Acquire) || control.worker_bits(0) & NOTICE != 0` → release and go to (b) after one more collect; else if `d_all.is_none() && pending_all == 0 && Run.in_flight == 0 && !listening` → unpark self, `Deadlock`; else wait. Mail producers are running workers (→ `running > 0`) or the monitor (→ `wake_all` → `woken`), and `mail.pending` closes the remaining window between (c) and (d).
- Waits as r2 (k ≥ 1 driver or condvar; worker 0 Virtual `driver.wait(d_all)` at `others == 0`, else condvar; Real `driver.wait(wait_deadline(d_0))`).

**Q7 Signals** — as r2: Mail `Signal`, `notice` on all, `wake_all`; first collector delivers under the lock; `unheard` → bits in all words, reason by `poll` precedence (control.rs:787-800 [V]); `set_watched` under the lock; `Run.relay`.

**Q8 Run, end, drop, failure, exit, fatal, recovery**
- `Async.run` caller parks as `WaitingCell(root)`; `Waiter::Token{token, owner}`; `Woken::Token` routed.
- `TaskHandle` drop → Mail; drainer frees a `Finished` cell (discards after unlock under `with_worker(X)` on the drainer's `Ctx` (r3 A-2: the same helper the host used to use, cells.rs:3311 [V]), a non-finished cell gets `detached` + `mark`.
- `exit` on k ≥ 1: `end_with_exit`, `control.end_run()`, `wake_all` (gap verified workers.rs:494-520 [V]).
- (r3) End of a successful run:
  1. `main`'s `Finish` (worker 0): drain own slot + Mail; under the registry lock assert no task cell with `phase != Finished` and no record in `InDriver`/`Queued` owned by a finished task (else `a task is left at the end`); release. Justification as r2 (root scope closes inside the last member's `finish` under the lock; members include `Placed` cells; `Lin` `OpId`s force every take). Then `END` + `wake_all` is issued by `run_scheduled` after `vm::run` returns `Exit::Finished` (as today, `Shutdown::drop`).
  2. Each worker k ≥ 1 reacts to `END` only at a safepoint or in its idle wait, never inside a collect (so its post-unlock discards are complete). Its exit sequence: final `take_notices` + one registry section (`Place`/`Child`/`Wake` with `Route::Run` → `left = true`; `Joined`/`Completion`/stale are ignored); `driver.quiesce()`; `driver.completions` → record arrivals for its records (`in_flight[k] = 0` afterwards); `kept_pending = driver.pending() > 0`; drop the driver; return `Retired { left, kept_pending, .. }`.
  3. Worker 0 after join: `quiesce` its own driver and record its arrivals (moved here from `finish` so that it happens after every k stopped sending `Submit`s); `kept_pending` → poisoned; any `left` → `a task is left at the end`; `sweep_all`; `reclaim_all`; Host flush; checks. G-A15 lists this order.
- Error/`exit` runs: registry dropped untouched after join; each worker quiesces its driver before returning its `Ctx`. Recovery order unchanged; REPL N = 1.

**Q9 Logical worker IDs and RC crossing** — crossings: `Place` closure (already `share`d), Mail-driven `free_task` discards under X after the owner's last use (ordered by the lock), `Op`s with `RawBuf` (no words). (r3) No word is ever touched by a thread other than the one acting as X's single holder; the host-side allocation path of r2 is gone, so the r2 probe is no longer load-bearing (it remains evidence that an ID's counts are shared across heaps, control.rs:253-263 [V]).

**Q10 Monitor and preemption** — `notice` on all workers after a relay; otherwise unchanged.

**Q11 Spec text** — §5 table + design-doc amendments (r3: adds multicore.md:115, :160, :336, :439 and vm.md:483).

**Q12 Commit order** — U1 run clock → U2 shared registry, `owner`, `Woken`, `Discards`, record states (N = 1 identical) → U3 unified notices + Mail + k ≥ 1 collect/idle + deadlock re-check → U4 Counter (S2) → U5 placement + spawn-at-first-start + `exit`-on-k + `async/` subset at N = 2/8 → U6a Real drivers on k, `Op` forwarding, end-of-run order → U7 Chan (S2) → U8 signals → U9 measurement + docs → U6b unbound accept (gated).

## 2. Rationale

- Single run lock, owner takes `k`, Mail, Virtual on one driver, run-wide clock, `exit` gap: as before (cells.rs:658-680, roadmap.md:31, vm.md:876, multicore.md:127/157, vm.md:495-496, sys/unix.rs:460, workers.rs:494-520 [V]).
- (r3) Owner-side phase machine: the only operations that need another thread are `driver.submit/cancel/completions`, and those need no words — only an `Op`. Keeping all heap work on the owner preserves eml_rt's single-holder rule without any eml_rt ID change, removes the body hand-off (B-2) and the concurrent-ID question (C-1), and keeps `advance` exactly as today. The price is one extra `Submit` hop per re-submitted step for cross-host records, which are rare in Real mode after U6b and only a test-time cost in Virtual mode.
- (r3) `RawBuf: Send` rather than a host-side lend: the alternative (host reads the `Bytes` header to compute the pointer) would make the host touch an object under X. A pointer-plus-length moved inside `Op` is the minimal crossing; safety rests on the record keeping the object alive and unmoved, which is the same obligation vm.md:483 already states for the lender.
- (r3) No `Ready::Run`: the entry `Task` already is the un-started, frameless, movable form; adding a second representation changed TaskIds and the `Policy` trait for no gain.
- (r3) Mail-pending in the deadlock re-check: closes the only window the "notices imply running" argument does not cover.
- (r3) Quiesce of worker 0's driver after join: a k could still have a `Submit` in flight to worker 0 while `main` finishes; quiescing before join could miss it and then record `kept_pending` wrongly or drop the arrival.

## 3. Alternatives considered and rejected

- Per-worker registries; `Wake` carrying `k`; all notices through the slot; per-worker test drivers; `cv.wait_timeout`; inline `unpark` at `mark`; `Run` notice now: as before.
- (r3) Host-side settle with the record body outside the lock (r2): racy hand-off, two `Ctx`s under one ID, new eml_rt rule; rejected.
- (r3) Per-record logical ID for in-flight words (host allocates under a record ID, `transfer` at hand-off): correct but adds `fork_worker`/`retire_worker` per op and a transfer per arrival; the owner-side machine needs none of it.
- (r3) `Ready::Run` lazy materialization: changes TaskIds and `Policy`; replaced by "entry-state `Task`s are the stealable form".

## 4. Assumptions made

- A4 [I]: real backends can accept without associating/registering the new socket (U6b only); IOCP fallback: accepted streams bound to the accepting host on Windows.
- A7 [I] (new): `RawBuf` pointers computed by `lend`/`lend_room` remain valid while the object is held by the record and not moved; true today for the same reason (vm.md:483 [V]); the only new element is the thread of use. If a backend asserted thread affinity of buffers (none does: epoll/kqueue/IOCP take raw pointers), the `Op` would be rebuilt on the host from `(usize, cap)`, which is the same thing.
- A8 [I]: `with_worker` can be used on any worker's `Ctx` for an ID forked on another heap (the probe showed `set_worker` + allocation + free + retire across two heaps work in checked mode; `with_worker` is `switch_task` twice).

## 5. Implications for implementation

### Types and fields (crates/eml_vm)

```rust
// cells.rs (imports stay std + eml_rt)
pub(crate) struct Shared { pub cells: Mutex<Registry>, pub mail: Mail }
pub(crate) struct Mail { items: Mutex<Vec<MailItem>>, pending: AtomicBool }
pub(crate) enum MailItem { HandleDropped(CellRef), Signal(u32), Cancel(CellRef) }
pub(crate) struct Registry { slots, free, stats, counters /* until S2 */, parts: Vec<Part>, signal_waiters: [Vec<CellRef>; 2], unheard: u32, relay: bool }
pub(crate) struct Part { timers: BTreeMap<TimerKey, Due>, timer_seq: u64, in_flight: usize }
pub(crate) struct TaskCell { owner: u32, worker: u32, ... }
pub(crate) struct OpRecord { host: u32, submitted: bool, pending_op: Option<Op>, arrival: Option<(io::Result<Done>, bool)>, kind, phase, worker, buf, stream, made, waiter, timer, cancelling, timed_out }
pub(crate) enum Waiter { Task(CellRef), Token { token: WaitToken, owner: u32 } }
pub(crate) enum Woken { Task { owner: u32, cell: CellRef, wake: i64, op: Option<CellRef> }, Token { owner: u32, token: WaitToken } }
pub(crate) enum Route { Run(Runnable), Forward(u32), Stale }
pub(crate) struct Discards(Vec<(Word, u32)>);          // dropped by the caller after unlock, via with_worker(id)
pub(crate) struct IoWork { submits: Vec<(CellRef, Op)>, cancels: Vec<CellRef> }   // driver calls to make after unlock
impl Registry {
    fn wake_cell(&mut self, cell, wake, op) -> Result<Option<Woken>, CellFault>;
    fn take_wake(&mut self, w: &Woken, me: u32) -> Result<Route, CellFault>;
    fn queue_op(&mut self, c: CellRef, op: Op) -> Result<(), CellFault>;                 // owner: pending_op = Some
    fn take_queued(&mut self, c: CellRef) -> Result<(Op, bool /*cancel_now*/), CellFault>; // host: submitted = true, in_flight += 1
    fn arrive(&mut self, c: CellRef, r: io::Result<Done>, cancelled: bool) -> Result<Option<u32 /*owner if not me*/>, CellFault>; // in_flight -= 1
    fn cancel_io(&mut self, c: CellRef, me: u32) -> Result<CancelPath, CellFault>;       // Flag | Local | Remote(host)
}
// workers.rs
pub(crate) enum Notice { Child(Task), Joined{..}, Place{cell, worker, closure: Word}, Wake(Woken), Cancel(CellRef), Submit(CellRef), CancelIo(CellRef), Completion(CellRef) }
pub(crate) struct Run { ..., next_place: AtomicUsize, clock: RunClock, relay: bool }
struct Retired { ..., kept_pending: bool, left: bool }
// sched.rs: Scheduler { cells: Arc<Shared>, ready: VecDeque<TaskId> /*unchanged*/, driver: Option<Box<dyn IoDriver>>, completed: Vec<Completion>, .. }
// driver.rs: enum RunClock { Real { epoch: Instant }, Virtual(Arc<AtomicU64>), Injected }
```
eml_rt `driver.rs`: `unsafe impl Send for RawBuf {}` + doc rule (gate: verus verify, verusfmt, clippy, fmt; no trust row). eml_io (U1): `RealDriver::new_at`, `TestDriver::new_shared`, `TestDriver::clock`. U6b: `RawSocket`, `Done::Accepted`, `IoDriver::adopt`. `Finished` gains `placed_tasks: u64` (real meaning: tasks placed on a worker other than the spawner's, like `placed`).

### Protocols

Routing: as r2, with `Route::Run` for forward-to-self. Owner collect: S1/S2/S3 as in Q3. Submit/cancel/arrival/settle: Q5 steps 1-5. Idle wait: Q6 (a)-(d). End of run: Q8 1-3.

### Units and completion criteria (no wall-clock bounds)

- U1 Run clock. `cargo nextest run` passes; `bench::` snapshots unchanged; `RealDriver::new_at(Instant::now() - 10 s).now() >= 10 s`; two `TestDriver::new_shared` over one `Arc`: `a.wait(Some(7))` then `b.now() == 7 ms`.
- U2 Shared registry + `owner` + `Woken`/`Route` + `Discards` + record states (`queue_op`/`take_queued`/`arrive`, still all on worker 0). Suite passes; `cargo insta test --check` no change; `bench::` unchanged; `the_task_cells_import_only_std_and_the_runtime` passes; unit test `a_cancel_and_a_submit_are_ordered_by_the_lock`: interleaving A (cancel section before `take_queued`) → `take_queued` returns `cancel_now == true`; interleaving B (after) → `cancel_io` returns `Remote(host)`/`Local`; both are table-driven on a `Registry` with a `NoDriver`, no threads.
- U3 Notices/Mail/k ≥ 1 collect + idle + deadlock re-check. Suite passes; TSan `workers::`+`signal::`; `a_handle_dropped_under_the_registry_lock_does_not_deadlock`; `mail_keeps_arrival_order`; `a_mail_item_pending_at_the_deadlock_check_defers_the_verdict` (push `MailItem::Cancel` between steps (c) and (d) via a test hook on `Mail` and assert the run continues instead of `Deadlock`).
- U4 Counter (S2).
- U5 Placement. `placement_rotates_over_every_worker`; `the_first_start_starts_the_workers` (`Finished.workers == 1`, `placed_tasks >= 1`); `a_start_after_a_nested_run_starts_the_workers` (`Async.run` inside a task, then `start` after it returns; same assertions); `exit_in_a_task_on_another_worker` (exit code 7); `ui::` at N = 2/8 for the 36 start/timer `run/async` files and the 5 start-only `run-fail/async` files listed in r2 §5, minus S3's opt-outs.
- U6a Real drivers + `Op` forwarding + end order. `an_echo_between_two_workers_completes` (Clock::Real, listener task on worker 0, client placed on worker 1, N = 2; asserts bytes and `workers == 1`); (r3 A-1) `a_forwarded_submit_is_settled_by_its_host`: Virtual, N = 2, a task forced onto worker 1 reads from a `localhost` peer; asserts the program's output and `Finished.placed_tasks >= 1`; since G-A9's debug assertion guarantees worker 1 has no driver in Virtual mode, a completed read proves forwarding — no new field; `net_*` UI at N = 2/8; `an_op_record_left_on_worker_1_is_swept`; `a_notice_after_the_last_collect_is_counted_as_left` (test hook sends `Place` to k after `END`; run ends with `a task is left at the end`); ASan + TSan on `-p eml_vm --test integration -- workers:: signal:: io::`.
- U7 Chan (S2) + `chan_*` at N ≥ 2. U8 Signals + `signal_*` at N ≥ 2; `a_pending_signal_reaches_a_waiter_on_worker_1`.
- U9 Measurement (Linux, `bench/net.sh` `conn-self-32`, `busy-self`, `echo` at N = 1, 2, 4, recorded in the plan) + docs (spec table, amendments below, CLAUDE.md:94/117, testing.md:417, ui.rs comment, status.md:36, overview.md:89, architecture.md:317, benchmarks.md tables).
- U6b Unbound accept, gated by U9 (echo does not scale while `conn-self-32` does); eml_rt gate as above; `RawSocket: Send` asserted; `Done` matches in the test driver.

### Spec sentences that become false (verified line numbers at 2c8de78)

As r2's table, plus (r3): vm.md:483 「`RawBuf` は … `Send` ではなく、driver を持つスレッドから出さない」 → 「`RawBuf` は `Send` で、貸す側のスレッドで作り、driver を持つスレッドで使う。貸した物体は、受け付けが `Done` を返すか完了を記録に当てるまで、記録が生かして動かさない」.

### Design-doc amendments (U9; rough Japanese)

1. multicore.md:124 → 「受け箱は worker ごとに 1 つの mutex で、知らせの列を守る。実行待ちは持ち主だけが触る。まだ始まっていないエントリの `Task` (フレームを持たない) は、`Place` の知らせと同じく動かせる形なので、救済を入れるときは実行待ちのそうした項と知らせの列を受け箱の lock の下に置き、持ち主はループの 1 回ごとに lock を 1 回取ってまとめて取り、救済する worker は盗むときに取る。横取りした VM のタスクは盗まない。セルと登録表は run の lock の下にある」. Reason: the entry `Task` is already the frameless movable form (multicore.md:517); no second queue representation is needed.
2. multicore.md:126 (last two sentences) and **:160** (r3) → 「持ち主の worker は run の登録表の lock の下でだけ変わる。知らせを送る側はその lock の下で持ち主を読んで宛先の欄に積む。持ち主が変わった後に古い欄に残った知らせは、取り出した worker が登録表の lock の下で今の持ち主へ回し、自分が今の持ち主ならその場で処理する」. Reason: owner and send are one critical section; the transit window is handled by forwarding.
3. multicore.md:157 → 「盗めるものは、実行待ちのまだ始まっていないエントリの `Task`、`Place` の知らせ、待ちの集まりにある待ちを宛先とする `Wake` の知らせ (継続はセルにある) である。救済は登録表の lock と詰まった worker の欄の lock の下で行い、受けたセルの `owner` を自分にし、継続から届く物体に `share(k, 元の worker)` で印を付ける」.
4. multicore.md:128-129 → 「時間の輪は run の登録表の worker ごとの区画で、持ち主だけが項を替える。仮想の時計は run に 1 つで、全 worker が空きのときに worker 0 だけが進め、満ちた区画の持ち主を起こす」.
5. (r3) multicore.md:**115** → add 「`Async` のタスクは `start` で置ける worker を順に回して置き、`Place` の知らせで運ぶ」 (the VM-before-deque paragraph currently speaks of `par` only).
6. (r3) multicore.md:**336** → 「worker ごとの構造 (受け箱、実行待ち、I/O の driver の保留の表) の語は数えた参照で、run の間は構造が持つ。漏れの検査は run の終わりに `sweep_all` が構造を空にしてから行い、回復は誤りの run の構造をヒープに触れずに捨てる。worker の間で共有する構造の語は run の登録表に載せる」. Reason: the VM registers no roots for these; today's `Task.args`/`k` in cells are already Rust-held counted references dropped by `sweep_all`.
7. (r3) multicore.md:**439** → keep 「`k` は worker の局所にとどまるので、印を付けない」 and add 「救済だけが例外で、盗む側が `share(k, 元の worker)` で印を付ける (決定 10)」 so :439 and amendment 3 agree.
8. async.md:244 → 「ハンドルを縛った worker (host) は driver の操作を出し入れするだけで、語には触れない。完了は host の driver に届き、host が記録に当ててから持ち主へ `Completion` を送り、持ち主が自分の `Ctx` で記録の歩を進める。中身の語はタスクの論理的な ID のままで、印は付けない」. (r3: replaces r2's host-side settle.)
9. async.md:249-250 → add 「`Clock::Virtual` の run では host はいつも worker 0 である」.
10. (r3) object-model.md 「論理的な worker ID」: no rule change; add one sentence 「ID の持ち手の受け渡しは run の登録表の lock で順序付ける (タスクの ID は、タスクが終わった後に mail を処理する worker が使う)」.
Consequences: `async-rescue` — slot lock around entry-state ids of `ready` + notices, owner change under the registry lock, forwarding present; `task-limits` — `TaskCell` counters under the registry lock; `main-pinning` — `Notice::Run`, placeable mask, dedicated worker excluded from `next_place`; native runtime — same `Shared` and slots; its ready queue holds only frameless entries; its `Async.run` nests the loop (multicore.md:138).

## 6. Unresolved items (defaults)

- `ChanCell` reservation vs re-validation (S2): default re-validate + "retry" code.
- Opt-out mark spelling (S3): default `-- workers: skip (<reason>)`.
- Skip waker when not parked: default yes; measure at N = 2.
- `next_place` start: 0.
- U6b in-stage or follow-up: by U9's gate.
- (r3, C-6) Virtual N ≥ 2 `net_*` files over the 1 s budget: default opt them out with the mark and keep their coverage in `io::` Rust tests; S3 measures with `cargo nextest run --profile timing -j 1`.

## 7. Additional checks

- As r1/r2 (dossiers, design docs, spec sections, code sites; spec line numbers by `sed -n`; scratch build; two `--workers 2` baseline runs; two-heap checked-mode probe `s1_probe_alloc_under_a_foreign_id_on_another_heap` passed and was reverted).
- (r3) Read: vm.rs grep for nested loops (none); multicore.md:113-116, 158-161, 334-338, 437-441; eml_rt driver.rs:1-45 (`RawBuf` definition, no `Send` impl; module outside `verus!`, no `external_body`); vm.rs:147-188 (`Task::entry` is frameless until entered); `alloc_foreign` call sites (cells.rs, tls.rs only); check-fail tests list (no `io_in_par_child` test exists; the refusal follows from `par`'s `<never e>` row and `IO : Once`).

## 8. Conformance criteria

- **G-A1** MUST: one `Registry` per run behind one `Mutex` in `Arc<Shared>`; no cell field atomic or separately locked. Check: grep `Mutex|Atomic` in cells.rs → `Shared.cells`, `Mail`, `HandleBin` only.
- **G-A2** MUST: lock order registry → slot → leaves (enumerated in Q1); `Mail` swapped out before processing; `Drop` impls take only leaves. Check: `a_handle_dropped_under_the_registry_lock_does_not_deadlock`; review; TSan `workers::`.
- **G-A3** MUST: `owner` written only by `start`; owner-only: materialization, `take_wake` (returns `Route`, no equality assert), `unpark`, `expire(parts[owner])`, running-task fields, `settle_op`/`advance` of its records. Any worker: `mark`, queue removal, `wake_cell`. Check: grep callers.
- **G-A4** MUST: the only `Word`s in `workers::Notice` are `Child(Task)`, `Joined.outcome`, `Place.closure`; `Submit` carries a `CellRef` and the `Op` travels inside the record (`pending_op`). Check: enum definitions.
- **G-A5** MUST: self `Wake`/`Place` inline; self `Cancel` via Mail; `ready`/`Policy`/TaskId assignment unchanged; N = 1 stdout/stderr/files/exit/`RunStats` byte-identical to 2c8de78. Check: `cargo insta test --check`; `mail_keeps_arrival_order`.
- **G-A6** MUST: per collect, S1 (slot) once, S2 (registry) once, plus S3 (registry) once per `Submit::Done`; driver calls, discards, `Ready` pushes outside registry sections; no heap allocation under the lock except what today's `start`/`advance`/`settle_op` already do. Check: `collect` review; `IoWork`/`Discards` are the only carriers out of S2.
- **G-A7** MUST: placement rule, `run_raw` on caller, no moves, sentinel first in both start arms, `Denied` → self. Check: `placement_rotates_over_every_worker`, `the_first_start_starts_the_workers`, `a_start_after_a_nested_run_starts_the_workers`.
- **G-A8** MUST: VM time only via `Run::now_ms_*`; drivers on the run's time base; `with_driver` → `Denied`. Check: grep `\.now()` in crates/eml_vm/src.
- **G-A9** MUST: Virtual → driver only on worker 0 (debug assert `k ≥ 1 ⇒ driver.is_none()`); Real → lazy on k, `Slot.waker` once; every driver quiesced, its arrivals recorded, then dropped on its own thread before the `Ctx` returns; `kept_pending` from any worker poisons. Check: assert; `Retired.kept_pending`.
- **G-A10** MUST: the host never touches words: it calls `take_queued`/`arrive` and the driver only; `cancelling`/`submitted` are read and written only in registry sections and the host reads `cancelling` in the section that sets `submitted`; `in_flight[h]` changes only in those two sections; words of a record never leave it. Check: `a_cancel_and_a_submit_are_ordered_by_the_lock`; grep for `buf`/`stream`/`made` access outside owner paths; TSan `io::` at N = 2.
- **G-A11** MUST (U6b): `listen`/`connect` bound to host; `accept` unbound until first submit; before U6b, bound to the accepting host. Check: `Bind`; echo test.
- **G-A12** MUST: virtual time advanced only by worker 0 at `others == 0` to the global minimum, then owners woken; deadlock declared only by worker 0 after the slot-lock re-check that follows the registry read and that also reads `running`, `mail.pending` and its `NOTICE` bit. Check: `a_timer_on_worker_1_advances_the_virtual_clock`; `a_mail_item_pending_at_the_deadlock_check_defers_the_verdict`; deadlock UI tests at N = 2/8.
- **G-A13** MUST: `HandleDropped`, `Signal`, self `Cancel` via Mail; any worker drains under the registry lock with `with_worker(X)` for discards; monitor never takes the registry lock, sets `NOTICE` on all; `set_watched` under the lock; `unheard` raises bits in all words. Check: monitor.rs has no `Shared.cells` access; `signal::` at N = 2 exit codes 130/143.
- **G-A14** MUST: a worker k ≥ 1 originating `Stopped(Ended)` calls `Control::end_run()` before `wake_all`. Check: `exit_in_a_task_on_another_worker`.
- **G-A15** MUST: end of a successful run = registry-state check in `main`'s `Finish` → `END` + `wake_all` → each k: final collect counting `left`, quiesce, record arrivals, drop driver, return `Ctx` → join → worker 0 quiesces its own driver and records arrivals → `left`/`kept_pending` handling → `sweep_all` → `reclaim_all` → Host flush → checks; a worker handles `END` only at a safepoint or idle wait. Check: `run_scheduled` review; `an_op_record_left_on_worker_1_is_swept`; `a_notice_after_the_last_collect_is_counted_as_left`.
- **G-A16** MUST: new cross-thread crossings are only `Place` (closure), Mail-driven `free_task` discards after the owner's last use, and `Op`s (no words); no new `share`/`transfer_back`; a logical ID is used by one `Ctx` at a time with hand-offs ordered by the registry lock. Check: grep set unchanged; `with_worker` is the only ID switch outside `resume`/`par`.
- **G-A17** MUST (S2): `Counter` lock-free `SeqCst`; channel cells in the run registry; cross-worker wake = `Woken`; woken side re-validates. Check: S2 design; `chan::` at N = 1 unchanged.
- **G-A18** MUST (S3): TSan `workers::`, `signal::`, `io::`, Mail-drain race; `async/` at N = 2/8 per the unit enable lists minus opt-outs; `RunStats` only at N = 1; N = 1 snapshots untouched; timing profile run for the Virtual N ≥ 2 `net_*` files. Check: testing.md; `workers_runs_cover_par_and_async`.
- **G-A19** MUST: `RawBuf` is `Send` with the documented lender/driver-thread rule; an `Op` holding a `RawBuf` is created only by the record's owner from the record's own words and is submitted only by the record's host. Check: eml_rt driver.rs; grep `RawBuf {` sites in cells.rs.
- **G-A20** MUST: amendments 1-10 written in U9; roadmap's `async-rescue` entry cites amendments 1-3. Check: U9 diff.

