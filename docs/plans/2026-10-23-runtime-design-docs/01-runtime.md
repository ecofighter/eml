# S1: eml multicore runtime architecture (round 3, final)

Tags: [V] verified (how), [I] inferred. Paths relative to /Users/arakaki/Projects/eml; "multicore.md" = docs/future/multicore.md, "OM" = docs/spec/object-model.md. Changes since r2 are marked (r3). The response table to review r2 and the S2-facing change list are at the end.

Driving fact: shared-mark RC has no unshare today (multicore.md:120; `is_unique` = rc == 1 only, OM:321/347 [V]); an object that crosses a thread pays atomic RC and loses Perceus reuse. The design keeps started tasks on one worker, crosses threads only through runtime primitives that mark on the sender side, and adds one bounded unshare (the transfer walk: objects at rc == -1 arriving at a join, receive or task start become local again).

## 1. Conclusion

### Q1 Scheduler model

- `par`: `eml_rt` fork/join nodes; per-worker Chase-Lev deque of unstarted right children; idle workers steal from the top; fork marks (schedule-independent). Waiting at join (r3): **in the VM the parent `Task` parks on the join node and the worker loop runs other work** (par children first, then Async tasks); **in native code the parent helps**, running only children whose fork depth is greater than the awaited join's depth, else it parks on the node. Progress argument for helping: let c be the shallowest unstarted child, at depth d; its forking parent is either running (progress) or waiting at its own join at depth d-1 < d and may take c; a worker blocked on a stolen child is blocked on a running worker; par children are finite; so no cycle of waits exists. Dedicated workers never steal and help only on their own joins.
- `spawn`/`Async`: **worker-affine tasks with rescue stealing** (r3 made sound, C-3/C-4). A task is placed at `spawn` (closure marked); its `k` normally only enters its worker's local structures, unmarked. Each worker has a **mailbox** = one `Mutex` over {run queue of heap-κ items, wait set, inbox}; the owner takes it once per loop iteration (batch), remote wakers take it per message, rescuers take it to steal. `Ready::Stack` items (preempted VM `Task`s) live in an owner-only queue outside the lock and are never stolen. The monitor flags a worker **stuck** when it has been running the same task (or blocked in an extern) for >= 2 ticks while its mailbox holds stealable work (run-queue items, `Place` messages, or `Wake` messages whose target is in its wait set), and unparks one idle worker. The rescuer, under the victim's mailbox lock, takes `Place` messages as its own placements, takes `Wake`d wait-set entries and run-queue items as tasks, moves each task cell's `owner` to itself (C-4), then marks every stolen κ with `share(k, sender = victim id, receiver = self)` (checked mode owner check uses the victim's id) and runs them. Sound because: every inter-task crossing marks, so a parked task's unmarked graph is private to it (invariant 8); every word in a Rust structure is an owned counted reference (invariant 9); the victim cannot touch those tasks while the rescuer holds its mailbox lock. Stolen tasks become affine to the rescuer.
- Scheduler = mechanism in `eml_rt`, semantics in `std/Async`; contract in §5.
- One pool; worker loop: mailbox batch → `Ready::Stack` queue → ready κ tasks → own par deque → steal par children → rescue (if the monitor flagged someone) → poll driver → park.
- Data parallelism in pure eml: `std/Par` combinators over the `par` primitive plus two runtime primitives (`for_mut`, `tabulate`) that give children **index ranges of one block** instead of splitting it (r3, C-6; no split/concat); unboxed scalar arrays (O-1) make fork walks O(1) and element reads RC-free; results and `-1` objects come back local via the transfer walk.

### Q2 Worker/thread model

- `N` = `available_parallelism()`; `--workers N`, `EML_WORKERS`; `N = 1` is pinned by UI tests. Workers and heaps live for the run.
- One OS thread = one worker = one `Ctx<M>` (own `Heap`, `evv`, yield state, stats, **per-worker stop word** registered in a run-level registry at creation) over the shared `CodeBase`, `Arc<Control>`, `Arc<dyn Host>`. Session roots in worker 0's `Ctx`.
- (r3, REQ-S1-5) Worker 0 runs `main`'s task on a **spawned thread** with the stack size the driver asks for (default 1 GiB reserved); `rt_thread_new(stack_size)` likewise. The **process main thread is the host thread**: it runs the driver (spawns workers, joins them, owns the signal handler) and is otherwise parked. On `rt_dedicate_main()` it becomes a dedicated worker ("worker M": its own `Ctx`/`Heap`, default OS stack, S3's `stack_limit` check guards deep native recursion) whose park is pluggable, so a macOS/Win32 toolkit loop runs on the true main thread while `main` keeps its large stack on worker 0. Nothing is ever placed on worker M except addressed work, so dedication needs no "before any spawn" condition (replaces r2's rule). `--main-dedicated` creates worker M at start.
- **Monitor thread**, one per run, started with the first worker: ticks every 10 ms while any worker runs a task, parks on a condvar otherwise (unparked by the first dispatch). Per tick: for each registered worker, if its current-task id is unchanged since the last tick → set `PREEMPT`; if unchanged for 2 ticks and its mailbox has stealable work → mark it stuck and unpark one idle worker; mirror `Control` bits into every registered stop word (so Ctrl-C reaches every thread within one tick; the signal handler itself only touches `Control`, as today); unpark all workers when `Control` bits changed (pluggable parks see them).
- Blocking: (1) inline foreign/extern call holds the worker; other tasks on it are rescued after >= 2 ticks (marked); a task's inline calls run on one OS thread for its life unless rescued; no re-entry into eml from an extern (unchanged; `run_scheduler` and `par` are VM/native built-ins, not externs, see §5). (2) `rt_offload(closure)`: blocking-pool thread (own `Ctx`/`Heap`, lazy up to cap 32, never exits); inputs marked (Unr) or moved (Lin); the result crosses back by the transfer walk, so `Lin` results are transferred, not marked (r3, S3 B-7). (3) Foreign-thread callbacks post to a worker's inbox through its `Unpark` handle.

### Q3 Preemption and fairness

- Cooperative at safe points; no signals. `Ctx::poll` keeps its shape (`depth > max_depth || bits != 0`, [V, ctx.rs:3298-3304]) but reads the per-worker word, and (r3, C-7) `stop_reason` gains `Stop::Preempt`, which the VM's `step` maps to a task switch instead of an error at every poll site (vm.rs 225-271, 560, 576): save `evv` into the `Task`, clear `PREEMPT` (owner-only store; a concurrent monitor set is harmless, it causes one extra switch), push `Ready::Stack`, dispatch next. Slice = 1 tick. Checked mode and the seeded test policy switch on a safe-point counter instead.
- Native tasks are descheduled only at Async operations.
- Fairness: (a) per worker FIFO, re-queued at the tail; (b) VM slice <= 1 tick; a native task without Async ops holds its worker until it finishes and the others are rescued after <= 2 ticks (~20 ms) at the cost of marking them; (c) driver polled once per loop iteration and at park; (d) placement round-robin over eligible workers (later: shortest run queue + wait set); (e) `par` children are bounded by grain and are preempted like any VM `Task`.

### Q4 Cross-thread memory

- Allocator: own remote-free lists (page header gains `owner: u32`, `remote: AtomicUsize`; per-heap remote list for large objects; `remote_count` atomic). Drain triggers: free list empty; `remote_count > 256` per loop iteration; before park; before leak check and recovery. Pages are not returned during the run; RSS is bounded by the sum of per-heap peaks (workers + pool), accepted (A-3) and listed in §6.
- Acceptance: any heap accepts rc < 0 blocks on any run-lifetime heap's pages, and rc > 0 blocks on other pages whose prefix owner is the current logical id. Checked mode: `word_in` = immediate | frozen | block on a registered heap's page; owner check unchanged (prefix owner == current id, rc > 0).
- Checked-mode net: live counts **per logical id in a run-level table of Relaxed atomics** (r3, C-3d); `TransferLeak` is checked at join when `N = 1` (synchronous) and at run end with all parked when `N >= 2`.
- Recovery only with all workers and pool threads parked; rc < 0 copyable; frozen-vs-heap by page owner; `Foreign`/lin/tokens stay `Unrecoverable`; pool heaps recreated like worker heaps.
- **Transfer walk** `rt_transfer(w)` (r3 cost rule, C-5): both modes descend **only through objects they flip**: rc == -1 (Acquire) → rc := 1, owner := me, continue into children; rc > 0, rc < -1, rc == 0 → stop. Cost = O(flipped objects + their direct children); a result of child-allocated local objects costs O(1). Checked mode runs a second, diagnostic-only pass over rc > 0 objects to rewrite owner ids (no semantic effect, so fast and checked reuse decisions agree). Marking mode = the existing `share` walk, extended (r3, C-6) with a **transfer mode**: starting from a lin-bit root, objects reached while every object on the path has rc == 1 are transferred (owner moved) instead of marked; the first object with rc > 1 switches the subtree to marking. Exclusive by construction (each object on the path is referenced exactly once, from its parent on the path, and the root is `Lin`).
- Soundness of the flip (A-1): invariant 9, every word held by a Rust-side structure (join node result, task cell, mailbox entries, channel buffer, driver completion) is an owned counted reference; a stolen task is removed under the victim's lock before marking; so an object at rc == -1 has exactly one reference and the walker holds it.
- Marking table (sender marks before publishing):

  | Crossing | Sender | Receiver |
  |---|---|---|
  | `par` fork | mark captures; lin roots: transfer mode | — |
  | `par` join | Release publish | transfer walk |
  | `for_mut`/`tabulate` ranges | elements with rc > 1 marked, rc == 1 left (exclusive via the slot) | — |
  | `spawn` | mark captures | transfer walk at start |
  | Async `k` at await / preemption | nothing | nothing |
  | rescue | — | rescuer marks κ (victim id as sender) |
  | channel send / receive | mark | transfer walk (S2 default on) |
  | offload submit / return | mark Unr, move Lin / pool marks Unr | transfer walk (Lin transferred) |
  | dedicated-worker message / reply | mark | transfer walk |
  | I/O buffers | `Lin`, moved | — |
  | `Lin` on any crossing | owner transfer, never marked | — |

### Q5 Thread-pinned execution

Dedicated worker + pluggable `Park`/`Unpark` behind `rt_dedicate_main()`, `rt_set_park(worker, park_fn, unpark_fn, data)`, `rt_run_pending(worker)`, `rt_thread_new(stack_size)`, `Main.run`/`Thread.run` via inbox + join node. Works whether the GUI layer is built in (Rust `Park`) or a library over FFI (C-ABI hooks). (r3, REQ-S1-2) The toolkit loop is entered **from the worker's `park`**, never from an eml extern call: `Gui.run app` installs the park, registers callbacks, and parks its own task (`rt_wait` on a quit cell); the worker loop then finds nothing ready and calls `park`, which runs `run_app`/the message loop; toolkit callbacks (`about_to_wait`, user events, modal resize loops) call `rt_run_pending(worker)` = "run ready items until none, never park, return". That nesting (loop → park → callback → loop) happens while no VM instruction is executing and `Ctx` is free, so it does not contradict "no re-entry from externs" (B-1); the fallback park = `pump_app_events` with timeout pauses eml during modal loops. Ctrl-C reaches a GUI-parked worker through the monitor's mirror + unpark.

### Q6 Order

M0 `par` a1 (one worker) → M1 single-worker scheduler → M2 multi-worker → O-1 unboxed arrays → M2b-VM → M2b-native (gated on S3) → M3 pinning/offload. VM before native for M0-M1 and M2's `eml_rt` part; native may start after M1.

### Q7 Decision table rows (multicore.md:27-40)

| # | Verdict | Change |
|---|---|---|
| 1 | change | core = workers, mailboxes, deques, park/unpark, monitor, driver hook, pool, dedicated workers, transfer walk, `for_mut`/`tabulate` range primitives, root registration; library = Async semantics, scopes, cancellation policy, channel API, `std/Par` |
| 2, 3, 4, 5 | keep | |
| 6 | change | a1 cooperative decided; order Q6 |
| 7 | change | remote-free lists + page owner id; run-lifetime heaps; drain triggers; N-heap recovery; `Raw`/unboxed array kind per O-1 |
| 8 | change | first stage `e : Row<Never>` with wait semantics; `Panic` reclassified `Never` (Q8); primary API = `std/Par`; binary `par` public; `both` kept; `spawn` places; affinity + rescue |
| 9 | keep + | invariants 8-9 added |
| 10 | change | table above; `k` unmarked unless rescued; transfer walk at join/receive/start; transfer mode under lin roots |
| 11 | change | dedicated worker + pluggable park, on request |
| 12 | keep | |

Sections: as r2, plus 「`par` の印付け」 gains the transfer mode and the range primitives; 「論理的な worker ID」 gains the run-level live-count table; 「未決」 per §6.

### Q8 The `par` API shape

- **Primary API = `std/Par` combinators; binary `par` stays public as the primitive and the divide-and-conquer form; `both` stays; no `Par` effect, no `Fut`, no syntax.**
- **Stage typing (r3, C-1).** docs/spec/types.md:17/24 fix `Panic` (and `IO`) at `Once` deliberately ("extern の関数がその場で1回だけ戻る") [V, read], so `<Panic>` does not satisfy `Row<Never>` as the spec stands, while `Int` arithmetic and `Array.get` are `Panics` rows (vm.md:226-229 [V]) and `Array.tabulate : ... -> <Panic | e> Array a` (std/Array.em:15 [V]). Decision: **reclassify `Panic` as `Never`; `IO` stays `Once`.** Justification: `Panic`'s only operation is `never panicked` (effects.md:92) and an extern that raises it aborts and never resumes; the "returns once" argument describes the extern *call*, which is not a `perform`, and the call is already excluded from yield points (core-ir.md "yield points") and from `may_yield` (effects.md:96 "`Panic` は yield しうるラベルでない"). What it touches: types.md:17 and :24 (spec text), the lattice classification of extern effects (one constant), the `σ ≤ Never` check, Kind export output for `Panic` (docs/spec/types.md 「Kind の書き出し」 snapshots, kind-2 test change), and nothing in carry-over (`Never ≤ Once`, so every `σ ≤ Once` constraint stays satisfied), `may_yield` or the `yields` marks. It is a normative type-system change and is M0's first item with its own tests. Stages: a1 = wait, a2 = cancel.
- **Surface form.** The constraint must be writable, because a declared signature without it would be more general than the inferred scheme and be rejected, and users who write helpers that forward a function into `Par.map` need it in their signatures (A-2). Decision: one new writable Kind form, a multiplicity bound on a row variable in a signature; spelling fixed in the stage plan (candidate `<never e>`, mirroring `once a -> ...`); users without signatures get it by inference. Diagnostic: a new E3xxx at the call site: "`Par.map` requires a function whose row has only `never` operations; `f` performs `IO`" (help: use `both` for debugging output).
- **std/Par surface** (`e : Row<Never>` on every `e`; `for_mut` and `tabulate` are runtime primitives over index ranges, the rest are eml over `par`):

```haskell
pub par       : (Unit -> <e> a) -> (Unit -> <e> b) -> <e> (a, b)
pub both      : (Unit -> <e> a) -> (Unit -> <e> b) -> <e> (a, b)          -- sequential twin, any row
pub map       : (a -> <e> b) -> Array a -> <e> Array b                     -- = tabulate over get
pub mapi      : (Int -> a -> <e> b) -> Array a -> <e> Array b
pub tabulate  : Int -> (Int -> <e> a) -> <Panic | e> Array a              -- primitive: children fill ranges of one MutArray, then freeze
pub fold      : (b -> b -> <e> b) -> b -> (a -> <e> b) -> Array a -> <e> b
pub for_mut   : (Int -> a -> <e> a) -> MutArray a -> <e> MutArray a        -- primitive: children update ranges in place
pub try_map   : (a -> <e> Result x b) -> Array a -> <e> Result x (Array b)
pub map_grain, fold_grain, for_mut_grain, tabulate_grain : Int -> ...
```
- **Determinism (r3, C-2, C-8).** The split tree is a function of (length, grain) only; the default grain is a constant (1024 elements) independent of `N`; adaptivity (`rt_par_stolen`) decides only whether a node's two halves run via `par` or inline, never the tree shape. So `fold`'s combine order is fixed and float results are bit-identical for any `N` and schedule. `try_map`: an `Err` at index i only suppresses forks of ranges entirely to the right of i (a CAS-min of the leftmost observed `Err` index); everything left of it is still evaluated, so the result is the leftmost `Err` after full evaluation of its left. a2 cancel follows the same rule (cancel right siblings only).
- Examples (tree sum with `par`, `mean_sq` with `fold`, stencil with `tabulate` ping-pong and `for_mut`, validation with `Fail`/`try_map`) are as in r2 §Q8 and unchanged in shape; `step`'s cost: unboxed previous buffer marks O(1) per step; boxed elements O(n) per step.
- Alternatives (`Par` effect with fork/join ops, pure-row futures, implicit-parallel `let`, binary `par` only) rejected for the reasons in r2; `Par.fork`/`Par.join` plain functions remain a §6 option.

## 2. Rationale (deltas)

- Rescue made sound by a per-worker mailbox mutex instead of lock-free structures (B-3): one uncontended lock per scheduler iteration is cheaper than building κ for a switch; Tokio-style queues are a later optimization behind the same interface.
- Task cells as `Arc<Cell>` with an `owner` field changed under the victim's mailbox lock, and senders re-checking `owner` under the destination's lock: no stranded messages after rescue (C-4), no forwarding tables.
- VM parks at join/`run_scheduler` instead of nesting (B-1): the VM `Task` is data, so parking costs nothing and removes the re-entry model, stack-order inversion and the double-level `Stop` propagation; native keeps helping/nesting because it has no alternative without stack switching, and the contract ("the caller resumes after completion") is the same.
- Range primitives instead of split/concat (C-6): one block, disjoint index ranges written through raw pointers (the `split_at_mut` discipline), no second object shape, no view with two owners.
- Transfer walk descending only through flipped objects (C-5): cost proportional to benefit; a local result costs O(1).
- `Panic` to `Never`: smallest change that makes `Row<Never>` children usable; the alternatives (a bespoke "no IO/Heap/once/multi but Panic" constraint, or non-panicking arithmetic) add a second constraint kind or a second arithmetic, both larger.

## 3. Alternatives rejected (deltas)

- Keep `Panic` at `Once` and give children non-panicking arithmetic: a second arithmetic/indexing API and a `<>`-only `par` that cannot call `Array.tabulate`; rejected.
- A row constraint "excludes `IO`, `Heap`, once/multi labels, allows `Panic`" outside the lattice: a new constraint kind in inference; rejected in favour of reclassification.
- Hand-off of a stuck worker's role to another thread (Go P/M): the stuck thread's `Heap` is in use by its task; rejected.
- Dropping rescue and relying on offload/async I/O only: head-of-line blocking for the top-priority target; rejected.
- Lock-free run queue + wait set for rescue: ABA/torn reads behind trusted rows; rejected for M2 (optimization later).
- `MutArray` split/concat as objects: copy (O(n)) or views with two owners; rejected.
- Earlier rejections unchanged.

## 4. Assumptions

- A1 (marking cost material; if false, rescue may become unconditional), A2 (await depth), A3 (safe-point density), A4 (trusted concurrent rows), A6 (recovery points quiescent), A7 (GUI loops pumpable) as before.
- A5 (r3): `poll`'s fast path is unchanged except for reading the per-worker word; `Stop::Preempt` is on the cold path; `bench/rt.sh` `poll` within noise.
- A8 (r3): transfer-walk soundness rests on invariants 8 and 9 and the Acquire read; tokens never cross `par` (reuse is frame-limited; roadmap.md:212-218).
- A9: compute-bound kernels scale; bandwidth-bound reported, not gated.
- A10 (r3): disjoint-range writes into one block from several threads are data-race-free given the owner thread does not touch the block during the primitive (it is parked/helping) and children never touch the header; a trusted row with Miri.

## 5. Implications for implementation

Invariants:

1. A started task changes worker only by rescue, which marks its κ. Only unstarted closures, payloads, results and rescued κ cross threads; every crossing marks or moves `Lin`.
2. Every Rust-side word-holding structure is a registered root of its worker's heap.
3. VM task switching only at a safe point with yield state `none`, saving `evv`; native tasks switch only at Async ops; a task returns to the loop by returning from its entry function.
4. A VM task blocked at `par` join or `run_scheduler` parks (does not hold its worker); a native frame blocked there helps (depth rule) or nests the loop.
5. Heaps live until all threads are joined and remote lists drained; recovery only with all parked.
6. `Control` is one per run; the monitor mirrors its bits into every registered per-worker stop word; the signal handler touches only `Control`; SIGINT blocked on all threads but worker 0.
7. Foreign calls run on the task's worker thread unless offloaded; offloaded calls see only marked or moved inputs.
8. All word-holding scheduler state of a worker is mutated only under that worker's mailbox lock or by its owner thread (`Ready::Stack` queue, deque owner end); the `par` deque is the only lock-free structure.
9. Every word held by a Rust-side structure is an owned counted reference, taken with `dup` or by move and released when removed.

Dispatch contract (r3):

- Per worker: `mailbox: Mutex<{ run: VecDeque<Cont>, wait: Slab<Waiting>, inbox: VecDeque<Msg> }>`, `stack_queue: VecDeque<Box<Task>>` (owner-only), `deque` (par), `park`, `current: AtomicU64` (task id, written per dispatch), `stop: AtomicU32`. `Cont = { cell: Arc<Cell>, k: Word, value: Word }`; `Waiting = { cell, k, reason }`; `Msg = Wake { cell, value } | Place { closure, scope } | Cancel { cell } | Run { closure, reply } | Completion { cell, payload }`.
- Task cell = `Arc<Cell { owner: AtomicU16, state: Mutex<State>, awaiters, scope }>`, `State ∈ { Running, Parked, Ready, Done(value), Cancelled }`; referenced from the `Lin` task handle (a `Foreign`) and from the scheduler. `owner` changes only under the current owner's mailbox lock; a sender locks the destination's mailbox, re-reads `owner`, and retries if it changed. Scopes are cells with a live counter.
- Entry functions (fixed names = ABI): `Std.Async.start_task : (Unit -> <IO, Async> a) -> <IO> Unit` = per-task handler whose `ctl` clauses store `k` (`rt_wait : Task b -> (once b -> <IO> Unit) -> <IO> Unit` consumes the `Lin` `k`) and **return normally**, `return x -> rt_done x`; `Std.Async.resume_task : (once a -> <IO> Unit) -> a -> <IO> Unit` = `k v`. Typing follows effects.md:114-126 (clause `k` is `once a -> <outer> r`, outer = `<IO>`, r = `Unit`; passing it to an extern with a `Lin` parameter is already allowed, cf. `Std.Proc.wait`).
- Worker loop: take the mailbox batch; `Cont` → fresh VM `Task` (stack from a pool) with entry `resume_task`; `Place` → `start_task`; `Ready::Stack` → continue. Native: the same two functions through the native entry.
- `run_scheduler body : <IO> a` is a **VM built-in** (r3, B-1): creates a scope, places `start_task(body')`, parks the calling `Task` as the scope's waiter (its result slot recorded), and the loop continues; when the counter reaches 0 the waiter is pushed to `stack_queue` with the result written. No nested loop, no re-entry, no Rust recursion. In native code it nests the worker loop on the machine stack (with the inversion hazard documented); the contract is identical. `par` join in the VM parks the same way.
- Cancellation (r3, B-2): `Cancel` to the owner; if the task is parked, the owner **drops `k`** (`drop k` releases the continuation; `Lin` values in its frames are finalized by drop glue; no user code runs) and sets `Cancelled`, waking awaiters and the scope. `await` of a cancelled task is typed by S2 (`Result`/`Fail`); a task observes its own pending cancellation only through an S2 op (`Async.cancelled ()`), not through a value injected into `k`.
- `par` children and `never` operations (r3, B-6): the child's `evv` is `()`; no marker evidence. EP generates, per `never` operation `E.op` in the program, `fork$E.op` (boxes the arguments into a tuple, calls `rt_child_abort(&reperform$E.op, tuple)`, aborts the child) and `reperform$E.op(tuple)` (unboxes and performs `E.op` normally). In a child, `ev_lookup[E]` with empty `evv` returns an immortal marker and `ev_clause[E.op](marker)` returns `&fork$E.op`; extern `Panic` faults in a child are routed by the VM's `failed` to `fork$Panic.panicked`. The join node stores `(reperform fn, tuple)`; after both children finish the parent applies the leftmost one, which performs in the parent's context with the parent's evidence, so `prompt$` runs as usual; the `par` call site is an unwind point. Children never touch the parent's evidence, so nothing is marked for it.

Milestones and completion criteria:

- **M0 `par` a1 (one worker).** `Panic` to `Never` + surface bound + E3xxx; `par` with `Row<Never>` wait semantics via `fork$`/`reperform$`; second `Task`, parking at join, seeded switching, fresh logical id per child, fork mark walk with transfer mode, join transfer walk (flip rule), run-level live counts + `TransferLeak`; `std/Par` eml combinators; range primitives `for_mut`/`tabulate` (single worker). Tests: all UI tests byte-identical under `sequential`/`seeded`; `RunStats` pinned under `sequential`; Kind export snapshots updated for `Panic`; negative tests: no fork marking → `WrongOwner`; no join walk → `TransferLeak`; poly-built `Lin` wrapper at rc == -1 (checked); `try_map` leftmost-`Err` test with an injected right-first schedule; runtime-error precedence; fold bit-identity across grains inline vs forked; `for_mut` on boxed elements shared elsewhere → marked, exclusive → not.
- **M1 single-worker scheduler.** Mailbox, `stack_queue`, cells, `run_scheduler` parking, `start_task`/`resume_task`, `Stop::Preempt` switch at poll sites, per-worker stop word + registry, monitor (tick, mirror, idle park), `std/Async`, driver hook, cancellation by drop. Tests: scheduler-style programs on `std/Async`; seeded stability; CPU-bound task vs ping-pong pair makes >= K round trips per slice under a test tick source; `run_scheduler` nested in a task completes in queue order (no inversion); leak check sees mailbox words; recovery after a runtime error inside `run_scheduler`; `Control::interrupt` observed within one tick on a worker that never reads `Control`; `bench/rt.sh` `poll` within noise.
- **M2 multi-worker.** Threads, mailboxes across threads, placement, rescue (lock, owner move, `share` with sender id), remote free + drains, deque + native-style helping test harness, pool (lazy, cap 32), channels, N-heap recovery, registry, SIGINT masking, Windows console handler. Tests: snapshots at N = 1; `par/` at N ∈ {2, 8} equal to N = 1 (including float folds); `async/` seeded at N = 1, order-independent ones at N >= 2; Miri: remote push/drain, deque, mailbox hand-off, rc flip, range writes; TSan over `par/`; stuck-worker test (inline blocking extern on worker 1, placements and wakes pending there) rescued within 3 test ticks including a `Place` and a `Wake`; negative: rescuer without marking → `WrongOwner` on the rescuer; deep unbalanced `par` tree at N = 2 with helping enabled terminates (liveness).
- **O-1 (object model).** Unboxed scalar arrays: accept S3's `Raw` object kind (B-11) if S3 needs the kind for Reprs; the scheduler needs only "no scan words, recovery copies bytes, `freeze`/`thaw` keep the kind". `Data`-with-raw-payload is the fallback.
- **M2b-VM.** `std/Par` grain defaults and adaptivity; count-based tests (stencil `Lin`/ping-pong shapes, `Unr` copy shape), monotone speedup `#[ignore]`.
- **M2b-native** (gated on S3): native fork/join and range-primitive entry; compute-bound kernel >= 0.6 x N at N <= 4; stencil reported.
- **M3.** Pinning interface, `rt_thread_new`, `rt_offload` with `Lin` transfer, tests as r2.

Requirements on S2: as r2 items 1-8 with these edits: ids are `Arc<Cell>` handles, not packed ids; wake/cancel go to `cell.owner` under the destination's mailbox lock; cancellation = drop of `k` + `Cancelled` state (no injected value); the Async handler is per task and its clauses return after storing `k`; `run_scheduler` parks in the VM; receive's transfer walk follows the flip rule (descends only through flipped objects).

Requirements on S3: invariants 3, 4, 7, 8, 9; native `run_scheduler`/join nest or help; `Stop::Preempt` is never observed in native code (native's merged flag check masks `PREEMPT`; the VM clears it at dispatch); the fatal-error protocol of REQ-S1-4 as mapped above; worker 0 spawned with the requested stack, worker M = host thread; range primitives need a native entry that calls `f` per index; offload returns transfer `Lin` (B-7 accepted); `Raw` kind accepted for O-1 (B-11).

## 6. Unresolved (defaults)

- Spelling of the row-variable bound (`<never e>`), and its E-code/message text — stage plan.
- Tick 10 ms; stuck after 2 ticks; overload threshold 64; pool cap 32; remote drain 256; default grain 1024.
- Placement metric (round-robin; later shortest queue).
- Page return (none; RSS = sum of per-heap peaks).
- Rescue takes all stealable items or half (default half, oldest first).
- Lock-free mailbox (none; mutex).
- `Par.fork`/`Par.join` plain functions (not provided).
- Native `run_scheduler` nesting inversion (allowed, documented; prefer calling `run_scheduler` from `main`).
- `Raw` kind vs `Data` raw payload (S3's `Raw` if needed).
- Windows blocking console reads (observed after return).
- Soft cancellation (`Async.cancelled ()`) — S2.

## 7. Checks

- Read docs/spec/types.md:12-30 (`Panic`/`IO` at `Once`, 3 writable Kinds) [V]; docs/spec/effects.md:160-170 (`var` `L` at `Once` reasoning) [V]; vm.md:226-229 (`Panics` rows) [V]; std/Array.em:15 [V]; ctx.rs `poll` [V]; control.rs `interrupt`/`clear`/`stop_bits` [V]; stdlib.md `MutArray` table [V]. Earlier checks (scheduler.em run, cell.rs orderings, OM sections, vm.rs poll sites, host.rs) stand. Not run: Verus, Miri, benchmarks.

## Answers to S3 requirements (04-native-ffi.md)

- **REQ-S1-1** (Lin offload inputs/results transferred, not marked): accepted; offload submit moves `Lin`, return uses the transfer walk (Q4 table).
- **REQ-S1-2** (GUI park may run the worker loop re-entrantly from toolkit callbacks): accepted as `rt_run_pending`, entered only from inside `park` (Q5). Consistency with B-1: the VM forbids re-entry from an extern call; `park` is not an extern, it is the loop's own idle hook, so the nesting is loop → park → callback → `rt_run_pending`, with `Ctx` free. `rt_run_pending` never parks and never runs `run_scheduler`'s nested native loop deeper than one level. eml keeps running during modal loops; the pump fallback pauses it.
- **REQ-S1-3** (`Raw` object kind 6 for unboxed scalar arrays): accepted; O-1 uses `Raw`; scheduler-side requirements (no scan words, byte copy in recovery, `freeze`/`thaw` keep the kind, fork/transfer walks O(1)) hold for it.
- **REQ-S1-4** (multi-worker fatal error protocol): accepted and mapped: `Control` gains a first-writer-wins error slot (CAS) beside the stop bits; `eml_fatal` on worker k writes it, sets the stop bit and cuts to its entry stub; the monitor mirrors the bit into every per-worker word and unparks every worker (mailbox parks, join-node parks, driver parks through `Unpark`, GUI parks through the plugin's unpark); VM workers return `Err(Stop)` from `step` as today, native workers longjmp to their entry; pool threads observe the bit when their thunk returns; the host thread joins all workers, then exits (`eml run`) or runs recovery with all parked (REPL). Worker-local structures are dropped with the heaps; no per-frame cleanup is attempted on other workers.
- **REQ-S1-5** (worker 0 on a spawned big-stack thread; `rt_thread_new` takes a stack size): accepted; the macOS tension is resolved by separating "worker 0" (main's task, spawned, 1 GiB) from "worker M" (the process main thread, dedicated on request, default stack guarded by `stack_limit`); Q2/Q5.
- Native poll facts: the merged flag check must mask `PREEMPT` (`bits & !PREEMPT`, one constant AND), since native tasks are descheduled only at Async ops; the VM clears `PREEMPT` at dispatch. Pool threads never run eml code: consistent with `rt_offload` taking only C thunks/Rust closures over marked or moved inputs.

## Response to review r2

| Finding | Verdict | Resolution |
|---|---|---|
| C-1 `Panic` is `Once` | accepted | Reclassify `Panic` as `Never` (`IO` stays `Once`); listed what it touches (types.md:17/24, classification constant, `σ ≤ Never` check, Kind export snapshots; carry-over/`may_yield`/`yields` untouched); new writable row-variable bound with a candidate spelling and a new E-code; M0's first item with tests. Alternatives (bespoke constraint, non-panicking arithmetic) rejected with reasons. |
| C-2 `fold` determinism | accepted | Tree shape = f(length, grain), grain default constant independent of N; adaptivity chooses fork vs inline only; bit-identical floats; M0/M2 tests. |
| C-3 rescue soundness | accepted | Mailbox mutex covering run queue, wait set, inbox; stuck criterion counts `Place`/`Wake` messages; rescuer drains them under the lock; monitor unparks an idle worker; `share` takes the victim id as sender for the owner check; live counts in a run-level atomic table, checked at join only at N = 1. |
| C-4 rescued task identity | accepted | `Arc<Cell>` with `owner` changed under the victim's lock; senders re-check under the destination's lock; invariant 8 restated. |
| C-5 transfer walk cost | accepted | Walk descends only through flipped (rc == -1) objects; local results cost O(1); checked-mode owner rewrite is a diagnostic pass. |
| C-6 `for_mut` cost and split representation | accepted | No split/concat: `for_mut`/`tabulate` are range primitives over one block; fork/element walk gets a transfer mode (rc == 1 under lin roots transferred, rc > 1 marked); boxed-element cost stated O(n) walk per step, no permanent sharing of exclusive elements. |
| C-7 `PREEMPT` in `poll` | accepted | `Stop::Preempt` on the cold path mapped to a switch in `step`; mirroring done by the monitor thread (not the signal handler) over a registry filled at worker/pool/recovery creation; A5 restated. |
| C-8 `try_map` early stop | accepted | Only forks entirely right of the leftmost observed `Err` are suppressed (CAS-min index); same for a2 cancel. |
| B-1 `rt_run` re-entry | accepted | VM: `run_scheduler` and join park the `Task` (built-in, no re-entry); native: nests/helps; contract identical; "no re-entry from externs" holds. |
| B-2 cancellation value | accepted | Cancel = drop `k` + `Cancelled`; no value injected; typing sketch for the clauses given. |
| B-3 concurrent queue | accepted | Mutex mailbox; `Ready::Stack` owner-only; lock-free later. |
| B-4 helping liveness | accepted | Progress argument given (shallowest unstarted child's parent can take it; waits chain to a running worker); VM does not help at all; N = 2 deep-tree liveness test. |
| B-5 epoch/same-task | accepted | `current` task id written per dispatch; monitor compares it; monitor parks while idle. |
| B-6 marker evidence | accepted | Replaced by `fork$`/`reperform$` per `never` op, empty child `evv`, VM routes extern `Panic` to `fork$Panic.panicked`. |
| A-1 flip soundness | accepted | Invariant 9 (Rust-held words are counted references); rescue removal under lock; Miri test. |
| A-2 constraint diagnostics | accepted | New E-code and message in M0. |
| A-3 RSS growth | accepted as risk | Stated: RSS = sum of per-heap peaks; page return in §6. |
| S3 B-7 `Lin` offload results | accepted | Offload return uses the transfer walk; `Lin` transferred. |
| S3 B-11 `Raw` kind | accepted | O-1 adopts `Raw` if S3 needs it; scheduler requirements are kind-neutral. |

## Changes to the S2-facing contract since r2

1. Dispatch: `run_scheduler` is a VM built-in that parks the caller's `Task` (no nested loop, no re-entry); native nests. The Async handler is still per task; `ctl` clauses store `k` via `rt_wait`/`rt_ready` and return normally.
2. Task identity: `Arc<Cell>` handles with an `owner` worker field replace packed `(worker, slot, gen)` ids; a `Lin` task handle wraps the `Arc`; messages are sent under the destination's mailbox lock with an owner re-check.
3. Mailbox: one mutex per worker over run queue (κ items), wait set and inbox; `rt_wait`/`rt_ready`/wake/cancel all go through it.
4. Cancellation: hard cancel = runtime drops `k` and sets `Cancelled`; no cancellation value is delivered to `k`; soft cancellation is an S2 op (`Async.cancelled ()`); `await` of a cancelled task is typed by S2.
5. Rescue stealing may move a parked or ready task (and its cell) to another worker; S2 must not assume a task's worker is stable across `await`.
6. Receive: the transfer walk descends only through objects it flips (rc == -1); payloads the sender keeps stay shared.
7. Offload: `Lin` results are transferred, not marked.
8. I/O handles bound to one worker's driver from first registration: unchanged. `Async.compute` unchanged.
9. `Panic` becomes a `Never` label; `IO` stays `Once`; a new writable row-variable bound exists for signatures.
10. Threads: `main` runs on a spawned worker 0 (large stack); the process main thread is the host thread, a dedicated worker only on request (`Main.run` targets it); GUI loops run inside that worker's `park` and drive tasks through `rt_run_pending`. Drivers' parks must be interruptible by `Unpark` and by the monitor (fatal-error protocol of REQ-S1-4).
