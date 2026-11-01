# Appendix S2: cross-worker `Std.Chan` and atomic `Std.Atomic.Counter`

S2 r2 Part 1. Unit names U4/U7 map to A5/A6/A9 in s4-integration.md §1.3; its A2 (placement start) is replaced by s4-integration.md §1.4 (AM-1). Paths under the design work directory no longer exist.


Paths are under /home/user/eml unless noted. [V] = verified (how is stated); [I] = inferred. Line numbers are at 2c8de78. (r2) marks what changed from r1. Part 1 is the full deliverable; Part 2 is the response table to S2-review-r1.

# Part 1: deliverable

## 1. Conclusion

**Counter (U4)**
- New module `crates/eml_vm/src/counters.rs` defines `Counters`. The table leaves `Registry` and `cells.rs`.
  - Values live in an append-only array of `AtomicI64`, split into geometric buckets: bucket b holds `64 << b` slots, there are 26 buckets, about 4.29e9 slots in total. Each bucket is a `OnceLock<Box<[AtomicI64]>>`.
  - `len: AtomicUsize` hands out indices.
- Operations:
  - `new`: `len.fetch_add(1)`, then `get_or_init` on the bucket, then `store(n)`.
  - (r2) `add`/`load`/`store`: one `OnceLock::get` (an Acquire load) plus one `SeqCst` op. They do not read `len`, take no lock, and keep no per-worker cache. Slots never move.
  - (r2) Only a `new` that opens a bucket allocates and zeroes it. Concurrent `new`s that need the same bucket wait inside `get_or_init` until it is ready. `add`/`load`/`store` never wait.
- Ownership:
  - `Machine.counters: Arc<Counters>` is cloned into `Start`, then into `Run.counters` for each run, and every `Scheduler` reaches it through `sched.run.counters`.
  - Nothing moves back at the end of a run, so values written by error, `exit` and poisoned runs stay. Recovery never touches the table, and REPL inputs share it.

**Chan (U7)** (r2: redesigned so that G-A6 holds with no amendment)
- Channel cells stay in the run registry. Each channel instruction takes **one** registry-lock section. No `ctx`/heap call is made under the lock: `share`, `transfer_back`, `fork_worker` and `retire_worker` all run outside it.
- The reserve → commit gap is handled by reservation counts in `ChanCell` plus a per-worker **hold** that carries the reserved data across the gap: `Scheduler.hold: Option<Hold>`, with `enum Hold { Send { chan: CellRef, depot: u32 }, Recv { chan: CellRef, depot: u32, item: Word } }`.
  - **Send.**
    1. `reserve_send` (locked) returns 0 when there is room. It does `sends += 1` and holds `Send { chan, depot: D }`.
    2. `push_raw` takes the hold and runs `share(x, D)` outside the lock.
    3. One locked section then does `sends -= 1`, pushes the item and wakes receivers.
  - **Receive.**
    1. `reserve_recv` (locked) returns 0 when there is an item. It removes the front item, does `recvs += 1`, holds `Recv { chan, depot: D, item }`, and wakes a sender if there is now room.
    2. `pop_raw` takes the hold and runs `transfer_back(item, D)` outside the lock.
    3. One locked section then does `recvs -= 1`, counts `channel_ops` and settles. If the channel is released, `retire_worker(D)` runs after the unlock.
- The counts pin the channel:
  - Room: `items.len() + sends < cap + receivers.len()`.
  - Receivable: `!items.is_empty()`.
  - Drained for receivers: `closed && sends == 0`.
  - Released (`settle`) only when closed, `items` empty, `sends == 0` and `recvs == 0`. So D is retired only after every `share`/`transfer_back` on D. Each of those runs before the locked section that drops its reservation, and the lock orders all of them before `settle`'s section, which is the happens-before `retire_worker` needs (object-model.md:739 [V]).
- No safepoint, scheduler switch or catchable failure lies between `reserve_*` and `push_raw`/`pop_raw` [V: disassembly in §7]. So the hold is used by one task without interruption, a per-worker hold is enough, and nothing can be stranded in it except in runs that end in error. A debug assertion `sched.hold.is_none()` at every task switch guards this.
- The extern rows, the extern table and the code of `std/Chan.em` do not change; only comments change.
- Wakes:
  - The channel `wake(t, chan)` checks `Parked && wait == Cell(chan)`, then calls S1's `wake_cell(t, WOKE, None)` and returns a `Woken`.
  - The VM arm routes every `Woken` by S1's rule while still holding the lock: owner == self → `take_wake` inline (the `Runnable` is scheduled after unlock); otherwise `Run::send(owner, Notice::Wake(w))`.
  - The woken side re-validates through the unchanged `send_loop`/`recv_loop`.
  - `park_chan`'s ready branch is unreachable at N = 1. At N ≥ 2 it is the lost-wake guard.
- Two deliberate semantic additions, each pinned by a test:
  - **(S-a) A send reserved before `close` still lands.** Receivers wait for it instead of reporting "closed and empty". The push that takes `sends` to 0 on a closed channel wakes every waiting receiver.
  - **(S-b) Transient over-capacity.** If a receiver that opened a cap-0 slot is cancelled after a send reserved that slot, the item still lands. The channel then holds more than `cap + receivers` until the next receive.
  - Neither can happen at N = 1.
- Logical IDs:
  - Nothing acts under D except `Depot::discard` in `sweep_all`, which runs on worker 0 after the join.
  - `share(x, D)` and `transfer_back(item, D)` run on the sender's and receiver's own `Ctx` under their own task IDs, on objects only they hold. They change D's Relaxed atomic counts, and the reservations pin D.
  - The single-holder rule holds without any eml_rt change.
- RunStats: no field changes. N = 1 values stay identical (§2).

## 2. Rationale

- **Reservations instead of re-validate + retry (S1's default overridden).**
  - `push_raw` holds its argument and returns `Unit`, and `pop_raw` returns a bare `a` (stdlib.md:1189-1197 [V]).
  - A held argument is either consumed or borrowed by the row. A consumed value can come back to eml code only as the result of another row (declarations.md:126 [V]). So a push that refuses and keeps its value needs a second "give it back" extern plus a stash, and a pop that may find nothing needs a sum-typed result. That means new rows, a new table entry, changes to `externs.rs` tests, and an allocation or an extra extern on every refused or fast operation.
  - Reservations change only `ChanCell`, the VM arms and `Scheduler.hold`. vm.md:876 already sketches them [V].
- **Hold in `Scheduler`, not in the VM `Task`.**
  - In every instance the gap contains only `load_raw`, `int_eq`, `switch_enum` and, on the send side, `box_int`/`box_const` [V: §7].
  - Safepoints are only at `Call`, `Ret`, `TailCall`, `TailSelf`, `ApplyRest`, `CallN`/`RetN` and the `Apply`/`Clause` paths (vm.rs:593-653 [V]; vm.md:152 [V]). `async_op`'s channel arms never collect or switch [V: vm.rs:2256-2304]. So no other task runs on this worker in the gap at any N, and no `Seeded` or `Normal` switch can fall inside it.
  - A `box_int` allocation failure is not catchable: `step_failed` maps only integer arithmetic and array/string/bytes ops to `Panic`; everything else is `Err(fault)` (vm.rs:2782-2810 [V]). So the run ends, the `Scheduler` is dropped untouched, and the hold's word is reclaimed by recovery like any other word of a failed run.
  - A per-worker field adds nothing to every `Task` (including `par` children). The debug assertion catches a future std change that breaks adjacency.
- **G-A6 kept.** The heap walk moves out of the lock. Each section only edits `ChanCell` queues and counts and routes wakes. Rust bookkeeping allocations (queue growth, `Vec` growth in the reused `woken` scratch) are of the same kind S1's own registry operations make. I read G-A6's "heap allocation" as eml-heap work: its exception list names `start`/`advance`/`settle_op`, which allocate `Foreign`s. The r1 `mem::take` of the receiver queue is replaced by `pop_front` in a loop, so no queue buffer is freed under the lock.
- **N = 1 identical** [I, by construction plus §7's adjacency check].
  - Reserve and commit are adjacent, so `sends` is 0 at every other instruction, and `recvs` is 0 except inside a receive's own gap.
  - Heap effects happen in the same order: `share` before the item is queued, `transfer_back` after it is dequeued, `fork_worker` before the cell is allocated, `retire_worker` after release. ID issuance therefore follows the same sequence.
  - The sender wake moves from `pop_raw` to `reserve_recv`. No `Ready` push lies between the two arms, so the ready-queue order is unchanged.
  - `channel_ops`/`channel_waits`/`channels_swept` are counted at the same events.
- **Lost-wake invariants** [I, transition by transition]:
  - (a) Room and waiting senders ⇒ a sender's `Woken` is in flight, or a woken sender is runnable.
  - (b) An item or drained, and waiting receivers ⇒ the same for a receiver.
  - Preserved by each transition:
    - `park_*` registers only when not ready, checked in the same section.
    - Reserves only consume room or items.
    - `reserve_recv` wakes a sender when room appears.
    - `park_recv` wakes a sender when room appears.
    - `push_raw` wakes the first receiver, or all of them when the channel becomes drained.
    - `close_raw` wakes everyone.
    - `unpark` only removes a waiter.
  - The reviewer independently failed to break these (S2-review-r1, "Protocol result").
- **Counter.** Lock-free `SeqCst` (G-A17). Buckets never move. A missing bucket is the out-of-range signal, so the hot path does not touch `len`'s cache line, which every `new` writes.

## 3. Alternatives rejected

- **Re-validation with a retry code**, or a fused send/receive returning an `Int` that consumes `x` only on success: held arguments cannot be consumed conditionally (declarations.md:126 [V]), so a refused value needs a returning row and a stash. That costs the same hold machinery plus new extern rows.
- **Fused op returning a sum type:** allocates on every fast path.
- **(r1 design) `share`/`transfer_back` under the lock:** a heap walk under the global lock, and it needs a G-A6 amendment. Replaced by the hold.
- **Stashing the hold in the VM `Task`:** unnecessary given verified adjacency, and it costs memory for every task.
- **`reserve_recv` reserving `items[recvs]` in place** (the review's sketch): commits would have to remove a specific queue position. Taking the item out at reserve is simpler, and `recvs` still pins `settle`.
- **Counter:** a registry lock or `Mutex`/`RwLock` (a lock per `add`; violates G-A17); a per-worker chunk cache in the `WorkerIds` style (more code); fixed preallocation (an arbitrary limit).
- **Claiming a waiting receiver at `reserve_send` (to avoid S-b):** the receiver can still be cancelled before the push lands, so S-b is not avoided.

## 4. Assumptions

- A1 [I]: S1 U2 provides:
  - `wake_cell(cell, wake, op) -> Result<Option<Woken>, CellFault>`: any worker; clears `wait`, leaves `k`.
  - `take_wake(&Woken, me) -> Result<Route, CellFault>`: owner only.
  - The routing rule: send under the registry lock. S1's amendment 2 puts the owner read and the send in one critical section.
  - S1 r1:141-142 and r3 §5 describe these [V, read].
- A2 [I]: S1 U5 placement is `owner = (next_place.fetch_add(1)) % N` with a start value s ∈ {0, 1} that consolidation decides. `run_raw` places on the caller. At N = 2, any two consecutive `start`s from one task land on workers {0, 1}, one each, for either s. The tests below rely only on this. S1 U5 adds `Finished.placed_tasks`.
- A3 [V, CLAUDE.md platform list]: all targets are 64-bit.
- A4 [V, object-model.md:737-741; heap/mod.rs:917-922]: D's counts are Relaxed atomics in `Control`. `share`'s check-mode `enter_id` takes only `Control`'s leaf mutex. `retire_worker` relies on its caller for happens-before. `transfer_back` and `with_worker` work on objects of other heaps, as `par` join does.
- A5 [V, std/Chan.em:144-153]: the raw channel externs are not `pub`. `std/Chan.em` is their only caller.
- A6 [V, lib.rs:189]: `RunConfig::default()` uses `Clock::Virtual`. The virtual clock advances only when every worker is idle (workers.rs test `a_timer_on_worker_0_and_a_child_on_worker_1_agree` [V]; S1 G-A12).

## 5. Implications for implementation

### 5.1 Counter: `crates/eml_vm/src/counters.rs` (new)

```rust
//! `Std.Atomic.Counter` の表 (docs/spec/vm.md の「`Counter` の表」)。
use std::sync::OnceLock;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering::SeqCst};

const FIRST: usize = 64;   // 塊 b は FIRST << b 個
const BUCKETS: usize = 26; // 合わせて FIRST * (2^26 - 1) 個
const FULL: VmFault = VmFault::Internal("the counter table is full");
const OUT: VmFault = VmFault::Internal("a counter index is out of range");

pub(crate) struct Counters { len: AtomicUsize, buckets: [OnceLock<Box<[AtomicI64]>>; BUCKETS] }
impl Default for Counters { /* len 0; std::array::from_fn(|_| OnceLock::new()) */ }

fn locate(i: usize) -> Option<(usize, usize)> {
    let j = i.checked_add(FIRST)?;
    let b = (j.ilog2() - FIRST.ilog2()) as usize;
    Some((b, j - (FIRST << b)))
}

impl Counters {
    pub(crate) fn push(&self, n: i64) -> Result<i64, VmFault> {
        let i = self.len.fetch_add(1, SeqCst);
        let (b, o) = locate(i).ok_or(FULL)?;
        let bucket = self.buckets.get(b).ok_or(FULL)?
            .get_or_init(|| (0..FIRST << b).map(|_| AtomicI64::new(0)).collect());
        bucket[o].store(n, SeqCst);
        i64::try_from(i).map_err(|_| FULL)
    }
    // 添字の範囲は塊があるかで確かめ、`len` は読まない。作っていない塊の添字は表の外である
    fn slot(&self, c: i64) -> Result<&AtomicI64, VmFault> {
        let (b, o) = usize::try_from(c).ok().and_then(locate).ok_or(OUT)?;
        self.buckets.get(b).and_then(OnceLock::get).and_then(|s| s.get(o)).ok_or(OUT)
    }
    pub(crate) fn add(&self, c: i64, n: i64) -> Result<i64, VmFault> { Ok(self.slot(c)?.fetch_add(n, SeqCst).wrapping_add(n)) }
    pub(crate) fn load(&self, c: i64) -> Result<i64, VmFault> { Ok(self.slot(c)?.load(SeqCst)) }
    pub(crate) fn store(&self, c: i64, n: i64) -> Result<(), VmFault> { self.slot(c)?.store(n, SeqCst); Ok(()) }
}
```

Notes:
- `locate` returns `None` instead of overflowing for large indices, which `slot` reports as `OUT`.
- An index inside a bucket that exists but beyond the issued `len` reads 0. Checked programs cannot produce such an index, and safety is unaffected.
- The r1 shape was compiled and run standalone [V: §7]. The r2 change only drops the `len` read from `slot`.

Wiring:
- lib.rs: `Machine.counters: Arc<Counters>` (:329), `Arc::default()` at :377.
- lib.rs: `Start.counters = Arc::clone(&self.counters)` (:593).
- lib.rs: delete :619, `Scheduled.counters` (:838), `sched.cells.counters = …` (:893), and :1011.
- workers.rs: `Run.counters: Arc<Counters>`; `Run::new(workers, bell, counters)`.
- (r2) Every `Run::new` caller [V, grep]:
  - lib.rs:881 passes `start.counters`.
  - These pass `Arc::default()`: sched.rs:622, vm.rs:3610, monitor.rs:214, :261, :297, :322, workers.rs:669, :736.
- vm.rs:2305-2325: the arms call `sched.run.counters.{push, add, load, store}` and take no registry lock.
- cells.rs: delete `Registry.counters` (:668-669, :699) and `Registry::counter` (:847-853).

### 5.2 Chan: types and `Registry` functions (cells.rs)

```rust
pub(crate) struct ChanCell {
    cap: u32, closed: bool, items: VecDeque<Word>,
    /// `reserve_send` が 0 を返し、まだ `push_raw` していない送りの数
    sends: usize,
    /// `reserve_recv` が列から取り、まだ `pop_raw` していない品物の数
    recvs: usize,
    senders: VecDeque<CellRef>, receivers: VecDeque<CellRef>, depot: Depot,
}
impl ChanCell {
    fn can_send(&self) -> bool { self.items.len() + self.sends < self.cap as usize + self.receivers.len() }
    fn drained(&self) -> bool { self.closed && self.sends == 0 }
    fn released(&self) -> bool { self.drained() && self.items.is_empty() && self.recvs == 0 }
}
/// 予約から確定までの間、worker が持つもの。間に安全点も切り替えもない (docs/spec/vm.md の「channel」)
pub(crate) enum Hold { Send { chan: CellRef, depot: u32 }, Recv { chan: CellRef, depot: u32, item: Word } }
pub(crate) enum Parked { Now(Runnable), Waiting }
```

`Depot` gains `pub(crate) fn id(&self) -> u32`.

Function contracts. Every function runs under the registry lock and takes no `ctx`, except `free_chan`. `out: &mut Vec<Woken>` is `Scheduler.woken`, reused and cleared by the arm.

| fn | behaviour |
|---|---|
| `new_chan(cap, depot: Depot) -> i64` | `cap` clamped as today; `sends`/`recvs` 0; alloc. |
| `reserve_send(c, me, yields, hold) -> Result<i64>` | `spend` (3 as today). Then: freed → 2; `closed` → 2; `can_send()` → `sends += 1`, `*hold = Some(Send{chan, depot: depot.id()})`, 0; else 1. |
| `reserve_recv(c, me, yields, hold, out) -> Result<i64>` | `spend`. Then: freed → 2; items non-empty → `item = items.pop_front()`, `recvs += 1`, `*hold = Some(Recv{chan, depot, item})`, and if `can_send()` wake `senders.pop_front()` into `out`, 0; `drained()` → 2; else 1. |
| `push_raw(chan, x, out) -> Result<()>` | Live or `Internal("an item is pushed to a freed channel")`; `sends.checked_sub(1)` or `Internal("an item is pushed without a reserved send")`; `items.push_back(x)`; `channel_ops += 1`. Then: if `drained()`, wake every receiver (loop `pop_front`); else wake one. `x` is already published (§5.3). |
| `pop_raw(chan) -> Result<Option<Depot>>` | Live or `Internal("an item is popped from a freed channel")`; `recvs.checked_sub(1)` or `Internal("an item is popped without a reserved receive")`; `channel_ops += 1`; `settle`. |
| `close_raw(c, out) -> Result<Option<Depot>>` | Freed or closed → `None`. Else `closed = true`, wake receivers then senders (FIFO, `pop_front` loops) into `out`, `settle`. |
| `settle(cell) -> Result<Option<Depot>>` | If `released()`, `take_chan` and return its `Depot` for the caller to `close` after unlock. |
| `wake(t, chan) -> Result<Woken>` | Check `Parked && wait == Some(Cell(chan))`, else `Internal("a channel waiter is not waiting")`. Then `wake_cell(t, WOKE, None)?.ok_or(same Internal)`. |
| `park_send/park_recv(c, me, k, out) -> Result<Parked>` | `suspend`, `parks += 1`, `channel_waits += 1`, cancel `point` → `Now(Resume{CANCELLED})`. Ready test: Send = freed \|\| closed \|\| `can_send()`; Recv = freed \|\| items non-empty \|\| `drained()`; ready → `Now(Resume{me, k, WOKE})`. Otherwise store `k`, `wait = Cell(c)`, queue the task; a receiver then wakes `senders.pop_front()` if `can_send()`. Returns `Waiting`. |
| `free_chan(ctx, cell, depth_offset)` (`sweep_all`) | As today (discard items under `with_worker(D)`, `retire_worker`, `channels_swept += 1`), plus `debug_assert!(sends == 0 && recvs == 0)`. Sound because `sweep_all` runs only at the end of a successful run, after the S1 G-A15 registry-state check: every task cell is `Finished` and `main` is at `Finish`. No task can be inside a reserve→commit gap there, because the gap has no switch. Error, `exit`, `kept_pending`-poisoned and "task left" runs do not reach `sweep_all` (vm.md:610 [V]; S1 G-A15). |
| `unpark`, `Wait::Cell(c)` | Unchanged: remove the task from both queues, `Resume{CANCELLED}` on its owner. |

### 5.3 VM arms (vm.rs:2256-2304) and the `Scheduler`

Two new `Scheduler` fields: `hold: Option<Hold>` and `woken: Vec<Woken>`.

Arm shape: the arm owns the guard. `let mut reg = sched.shared.cells.lock()…`. Field-disjoint borrows give `&mut sched.hold`, `&mut sched.woken`, `&sched.run` and `sched.index`. After the cell call, the arm runs `route(&mut reg, &sched.run, sched.index, &mut sched.woken, &mut runs)`, which is S1's routing step. Then `drop(reg)`, then `schedule` each `runs` entry in order.

- `ChanNewRaw`: `let depot = Depot::open(ctx)` (outside the lock); lock; `new_chan(cap, depot)`.
- `ChanReserveSend`: lock; `reserve_send(.., &mut sched.hold)`; `debug_assert!` that the hold was `None` before a 0 result.
- `ChanReserveRecv`: as above, with `out`.
- `ChanPushRaw`:
  1. `let Some(Hold::Send{chan, depot}) = sched.hold.take()` and `chan == decode(c)`, else `Internal("an item is pushed without a reserved send")`.
  2. `let x = Depot::of(depot).publish(ctx, x)?` (outside the lock).
  3. Lock; `push_raw(chan, x, out)`; route; unlock; schedule.
- `ChanPopRaw`:
  1. Take `Hold::Recv{chan, depot, item}` (else `Internal("an item is popped without a reserved receive")`).
  2. `let x = Depot::of(depot).receive(ctx, item)?` (outside the lock; the item has been out of the queue since the reserve and has no other reference, which meets `Depot::receive`'s precondition, cells.rs:3279-3280 [V]).
  3. Lock; `let gone = pop_raw(chan)?`; unlock.
  4. `if let Some(d) = gone { d.close(ctx)? }`; `task.set(dst, x)`.
- `ChanCloseRaw`: lock; `close_raw(c, out)`; route; unlock; schedule; close any returned `Depot`.
- `AsyncParkSend/Recv`: lock; `park_*`; route; unlock; schedule `Now(r)` as today; `switch_away` as today.
- Hold guard: `debug_assert!(sched.hold.is_none())` at the head of `switch_away` and in `preempt` before switching.
- Ordering for `retire_worker(D)`: each `share`/`transfer_back` on D is sequenced before its own thread's locked section. That section precedes, in lock order, the section where `released()` became true. `close` runs after that section on the releasing thread.

### 5.4 Paths when the waiting task's owner (o) differs from the acting worker (a)

- **`push_raw` on a, receiver r owned by o.** Under the lock: r is removed and `wake_cell` runs (r stays `Parked`, `wait = None`, `k` remains in the cell). `Notice::Wake(Woken{owner: o, cell: r, WOKE})` goes to o. On o's collect, `take_wake` produces `Resume{WOKE}`, and r retries `reserve_recv`. If the item is already taken, r re-parks at the back of the queue.
- **`reserve_recv` on a, waking sender s on o.** Symmetric.
- **`park_recv` on o, waking sender s on p.** Routed to p.
- **`close_raw` on a.** One `Woken` per waiter, each routed to its owner, receivers first.
- **Cancel of a waiting r, marked on any worker.** `Notice::Cancel` goes to o (or the Mail if o is self), and o runs `unpark` under the lock.
  - If r is still queued, it is removed and resumed with `CANCELLED`.
  - If a wake cleared `wait` first, `unpark` does nothing. r resumes with `WOKE` and retries without a cancel check; if the channel is still empty, `park_*`'s cancel point returns `CANCELLED`.
  - The lock orders the two; whichever clears `wait` first wins (vm.md:725).
- **Lost or unreserved wakes.** A woken task holds no reservation and re-runs `reserve_*`. A channel with reservations cannot be released. A released channel makes `reserve_*` return 2.
- **Stale `Woken`.** None for channels in this stage: `k` is taken only by the owner's `take_wake`, and `unpark` ignores `wait == None`.
- **`sweep_all`.** Worker 0 after the join; `with_worker(D)` discards items allocated on any heap (remote frees, then `reclaim_all`).
- **`Lin` items.** The sender `share`s to D on its own heap. The receiver `transfer_back`s from D after taking the item out, as `par` join does across heaps [V: vm.md:874 already sanctions this crossing].

### 5.5 RunStats

No field added, removed or redefined. N = 1 values are identical (§2). The ready branch of `park_chan` counts `parks`/`channel_waits` at N ≥ 2, where stats are not compared (G-A18).

### 5.6 Tests

**Existing `chan::` Rust tests (crates/eml_vm/tests/chan.rs, 11 [V])**: all stay N = 1 Sequential and are not run at N = 2/8. Each one pins either `RunStats` or a specific interleaving driven by `spin`/`yield_now` on one worker, which real parallelism does not reproduce. Their order-independent behaviours are covered at N ≥ 2 by the UI files and the new tests below.

| test | N ≥ 2? | reason |
|---|---|---|
| `send_waits_for_room_and_recv_waits_for_items` | no | asserts `(tasks, parks, channel_waits, channel_ops, channels_swept)` |
| `waiting_senders_are_woken_in_order` | no | FIFO order of waiters set up by start order on one worker |
| `waiting_receivers_are_woken_in_order` | no | same |
| `close_wakes_every_waiter` | no | asserts `channel_waits == 3` |
| `a_cancelled_waiting_sender_leaves_the_queue_and_frees_its_value` | no | asserts waits and cancels |
| `a_woken_receiver_waits_again_when_the_item_was_taken` | no | pins a fast-path race outcome |
| `a_sender_woken_for_a_cancelled_receiver_waits_again` | no | pins the interleaving and stats |
| `a_woken_receiver_receives_before_its_cancellation_is_raised` | no | pins the interleaving |
| `a_refused_value_is_not_marked` | no (no tasks; nothing to gain) | `shared_objects` |
| `the_fast_paths_spend_the_budget` | no | parks per budget |
| `the_channels_left_at_the_end_are_swept` | no | `channel_ops`, `channels_swept` |

**New `cells.rs` unit tests** (top-level `#[test]`, no threads; tasks started with S1's `owner` argument; `test_ctx()` only where a `Depot` is opened or closed):

| name | assertions |
|---|---|
| `a_reserved_send_holds_its_room_until_the_push` | cap 1: `reserve_send` == 0, hold `Send`; a second == 1; `push_raw`; `reserve_send` == 1; `reserve_recv` == 0 |
| `a_reserved_receive_takes_the_item_out_of_the_queue` | cap 2, two items: `reserve_recv` == 0, hold `Recv` with the first item; `items.len() == 1`; `recvs == 1`; `pop_raw` → `Ok(None)` |
| `a_commit_without_a_reservation_is_an_internal_error` | `push_raw` with `sends == 0` and `pop_raw` with `recvs == 0` → the two `Internal` texts |
| `a_close_before_a_reserved_push_keeps_receivers_waiting_for_the_item` | cap 1: `reserve_send` 0; `close_raw` → `None`, empty `out`; `reserve_recv` == 1; `push_raw`; `reserve_recv` == 0; `pop_raw` → `Some(depot)`; `depot.close(ctx)` is `Ok`; `reserve_recv` == 2 (S-a) |
| `the_last_reserved_push_into_a_closed_channel_wakes_every_receiver` | **cap 2**: two `reserve_send` 0; close; receivers r1 (owner 0), r2, r3 (owner 1) park → `Waiting`; first `push_raw` → `out == [r1]`; second → `out == [r2, r3]` with owner 1 (S-a) |
| `a_channel_is_released_only_after_the_last_reserved_receive` | cap 1, one item: `reserve_recv` 0; `close_raw` → `None`; `pop_raw` → `Some` |
| `a_wake_for_another_owner_leaves_the_continuation_in_the_cell` | r (owner 1) parks; `reserve_send` + `push_raw` → `out == [Woken::Task{owner: 1, cell: r, wake: WOKE, op: None}]`; r `Parked`, `wait == None`, `k.is_some()`; `take_wake(&w, 1)` → `Route::Run(Resume{r, WOKE})` |
| `a_cancel_after_the_wake_leaves_the_task_woken` | as above, then `unpark(r)` → nothing, `cancels` unchanged |
| `a_cancel_before_the_wake_takes_the_receiver_out_of_the_queue` | r parks; `unpark(r)` → `Resume{CANCELLED}`; cap 0 `reserve_send` == 1 |
| `a_receiver_cancelled_after_a_reserved_send_overfills_the_channel_until_the_next_receive` | cap 0, r parks; `reserve_send` 0; `unpark(r)` → `CANCELLED`; `push_raw` → empty `out`; `items.len() == 1 > cap + receivers.len() == 0`; `reserve_send` == 1; a new receiver's `reserve_recv` == 0 takes it; then `reserve_send` == 1 until a receiver parks, then 0 (S-b) |
| `park_returns_at_once_when_another_worker_changed_the_channel` | `reserve_recv` == 1; then `reserve_send` + `push_raw`; `park_recv` → `Now(Resume{WOKE})`; `channel_waits == 1` |

**New integration tests**, crates/eml_vm/tests/chan.rs, section 「worker をまたぐ channel」.
- Helper `run_at(src, workers) -> (String, Finished)`: runs checked (`with_debug_heap(true)`) and fast with `Limits { workers, .. }` and the default `Clock::Virtual`, asserts both `Ok` with equal stdout, and returns the checked `Finished`.
- (r2) Pinning: each N = 2 test starts its tasks as **one consecutive pair** from the root task and makes no other `start`. By A2, exactly one of the pair lands on worker 1 for either start value of `next_place`, so `placed_tasks == 1` is deterministic.
- `Async.sleep_ms 1` in the root makes "every task has parked" deterministic: the virtual clock advances only when all workers are idle (A6).
- No test asserts `RunStats` or time.

| name | N | program | assertions |
|---|---|---|---|
| `producers_on_both_workers_feed_the_root_task` | 2 | cap 1; producers P1 (sends 1..=100) and P2 (101..=200) started as a pair; the root receives exactly 200 items and sums them, then closes | stdout `"20100\n"`; `workers == 1`; `placed_tasks == 1` |
| `parked_receivers_on_both_workers_are_woken_by_the_root` | 2 | cap 0; receivers R1, R2 started as a pair; root `Async.sleep_ms 1` (both parked); root sends 1 and 2; awaits both; prints `List.sort` | `"[Some 1, Some 2]\n"`; `placed_tasks == 1` |
| `lin_items_cross_workers_and_come_back` | 2 | echo tasks E1, E2 (pair); each receives a `MutArray` from `inbox` (cap 0), writes 7 at index 1, sends it to `outbox` (cap 2), and puts one more `MutArray.make 2 1` into an unclosed `left` channel (cap 2); the root sends two arrays, receives two, prints the two index-1 values sorted | `"7 7\n"`; both runs `Ok` (no `TransferLeak`, no leak; `left` swept on worker 0); `placed_tasks == 1` |
| `receivers_cancelled_on_both_workers_leave_the_queue` | 2 | cap 0; in `Async.scope`, a pair of `spawn_in` receivers each `let _ = Chan.recv c`; scope body `Async.sleep_ms 1` (both parked), then `cancel_scope s`; after the scope, `Chan.try_send 1 c` | `"Full 1\n"`; `placed_tasks == 1` |
| `every_item_reaches_one_receiver_on_eight_workers` | 8 | cap 2; 3 producers × 100 (1..=300); 3 consumers each `Atomic.add 1 n` per item; the root awaits producers, closes, awaits consumers | `"300 items, sum 45150, counted 300\n"` |
| `counters_add_atomically_across_workers` | 4 | 4 tasks: each runs 1000 × `Atomic.add 1 c` (no yields) and creates 50 counters `Atomic.new k` (k = 1..50), returning the sum of their `load`s | `"4000 5100\n"` |

**`counters.rs` unit tests** (`#[cfg(test)] mod tests`):
- `locate_numbers_every_slot_once`: 0..100_000 map to consecutive offsets in consecutive buckets.
- `push_hands_out_consecutive_indices`.
- `an_index_outside_the_table_is_out_of_range`: -1, 64 with only bucket 0 created, and `i64::MAX` → `OUT`.
- `add_wraps_around`.
- `concurrent_pushes_and_adds_agree`: 4 threads × 70 pushes and 200 `add 1` on one counter. The indices are distinct, every value reads back, and the counter ends at 800. Small enough for Miri.

**UI files** (classification for S3; read [V]):
- Order-independent at N = 2/8: `run/async/chan_cancel_points`, `chan_cancelled_sender`, `chan_close`, `chan_many`, `chan_merge`, `chan_mutarray`, `chan_rendezvous`, `chan_reply`, `chan_send_recv`, `chan_unclosed`, `chan_woken_retry`, `counter`, and `run-fail/async/chan_deadlock`. Their outputs hold for any placement; with S3's `placed_tasks > 0` harness check, files with at least one `start` satisfy it under either start value at N = 2/8 [I].
- `signal_instead_of_deadlock` belongs to S1's signal group (U8).
- `run/std/std_chan`, `std_atomic` and the REPL transcripts `async/chan_stale`, `async/counter` stay at N = 1 and must not change.

**Sanitizers** (S3 owns the CI diff):
- TSan: `-p eml_vm --test integration -- chan::` and `-p eml_vm --lib counters::`.
- Miri: `-p eml_vm --lib -- counters::` (both settings).

**Measurement program** (r2, C-4): `bench/async/chan_pingpong.em`.
- 4 independent pairs. Each pair is `ping`/`pong` over two cap-0 channels for 20,000 rounds; `pong` sums what it receives and closes its reply channel when `ping` closes.
- `main` prints the sum of the four `pong` results: `800040000`.
- Not in bench/'s top level. bench.rs's `every_program_in_bench_has_a_test` lists the bench directory; other subdirectories (`net/`, `numeric/`) already exist [V: ls]. [I] that the listing ignores subdirectories; check it in U7, and if not, place the file next to S3's Linux measurement programs instead.

### 5.7 Units

**U4 Counter** (after S1 U3). Completion:
1. `cargo nextest run` → all pass.
2. `cargo nextest run -p eml_vm --lib -E 'test(counters::)'` → 5 pass.
3. `cargo insta test --check` → no pending snapshots.
4. `grep -n "counters\|fn counter" crates/eml_vm/src/cells.rs` → no output.
5. `grep -n "Mutex\|Atomic" crates/eml_vm/src/cells.rs` → only S1's set (G-A1).
6. TSan: `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=thread TSAN_OPTIONS=halt_on_error=1 CARGO_TARGET_DIR=target/tsan cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_vm --lib counters::` → pass.
7. Miri: `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --lib -- counters::`, then the same with `-Zmiri-tree-borrows` added → pass.
8. `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → clean.

**U7 Chan** (after S1 U5 and U6a). Completion:
1. `cargo nextest run` → all pass.
2. `cargo insta test --check` → no pending snapshots.
3. `git diff <U7 base> -- crates/eml_vm/tests/chan.rs` → only additions.
4. `cargo nextest run -p eml_vm --test integration -E 'test(chan::)'` → 17 pass.
5. `cargo nextest run -p eml_vm --lib -E 'test(cells::)'` → the 11 new tests and all existing ones pass.
6. `for i in $(seq 20); do cargo nextest run -p eml_vm --test integration -E 'test(chan::)' || exit 1; done` → 20 passing runs.
7. `cargo nextest run -p eml_cli --test integration -E 'test(ui::)'` → passes with the 13 files enabled at N = 2/8.
8. TSan (U4's env): `cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_vm --test integration -- chan::` → pass.
9. `cargo nextest run --profile timing -j 1 -p eml_vm --test integration -E 'test(chan::)'` → no SLOW.
10. `grep -n "heap_mut\|ctx" crates/eml_vm/src/cells.rs`, restricted to the channel section (the functions after `impl Registry {` for channels up to `Depot`) → only `free_chan`.
11. `grep -n "Runnable::Resume" crates/eml_vm/src/cells.rs`, in the channel section → only `park_chan`'s two self-resumes.
12. Clippy and fmt clean.
13. Measurement (Linux, recorded in the plan, no gate on the unit): `cargo build --release -p eml_cli`, then `for n in 1 2 4; do for i in 1 2 3; do EML_WORKERS=$n /usr/bin/time -f "$n %e %U %S" target/release/eml run bench/async/chan_pingpong.em; done; done`.
    - Every run prints `800040000`.
    - The medians of wall and user time per N go into U9's benchmarks.md heading (S3's measurement section).
    - Decision rule: if the N = 4 wall-time median is above the N = 1 median, U9 adds the numbers to roadmap.md's existing deferred item 「ロックフリーの受け箱と channel」 under its 見直す条件, as the measured contention. The stage does not block on it.

### 5.8 Spec and doc text (rough Japanese)

stdlib.md 「`Std.Chan`」:
- New bullet: 「送り手と受け手は別の worker で動いてよい」.
- New bullet (S-a, S-b): 「閉じる前に枠を確かめた送りは、閉じた後に積まれることがある。そのとき受け手は閉じて空とせずに待ち、品物を受け取る」; 「容量 0 で枠を開いていた受け手が、送りが枠を確かめた後に取り消されると、品物は次の受け手が来るまで channel に残る」.
- :1198 code 0 → 「0 (予約した。続けて確定する)」.
- :1199 append 「0 を返した `reserve_*` は枠か品物を1つ予約し、続く `push_raw` か `pop_raw` が確定する。間に `Int` の比べのほかを挟まないのは、間に安全点も失敗の地点も置かず、予約を持ったまま切り替えたり抜けたりしないためである」.

stdlib.md 「`Std.Atomic`」:
- :1258 → 「表の項は `SeqCst` の atomic で、どの worker のタスクも lock を取らずに `add`、`load`、`store` を行う。`add` は `fetch_add` である」.

vm.md:
- :604: add 「送りの予約の数と、予約で取り出してまだ受け取っていない品物の数」.
- :606 → 「`Counter` の表 `Counters` は登録表の外にあり、`Machine` が `Arc` で持って run の表 `Run` に渡す。表は `AtomicI64` の塊の並びで、塊 b は 64 << b 個の項を持ち、使い始めるときに1回だけ作って動かさない。`new` は添字を atomic に取る。`add`、`load`、`store` は lock を取らない。回復でも戻さず、誤りや `exit` で終わった run が書いた値も残る。`Counter` は放さない」.
- :690 `new_raw`: 「`Depot::open` で預かる ID を出してから、登録表の lock の下でセルを作る」.
- :691 reserve:
  - 「積めれば送りの予約を数え、channel の番号と預かる ID を worker の hold に置いて 0」.
  - recv: 「品物があれば先頭を列から取って hold に置き、取り出した数を1つ数え、積めるようになれば送り手の先頭を1つ起こして 0。閉じていて送りの予約がなければ 2」.
- :692 push_raw → 「hold の預かる ID に lock の外で `publish` で預け、lock の下で送りの予約を下ろして積み、`channel_ops` を数える。閉じていて送りの予約が 0 になれば待っている受け手をすべて、そうでなければ先頭を1つ起こす。hold か予約がなければ内部の誤り (`an item is pushed without a reserved send`)」.
- :693 pop_raw → 「hold の品物を lock の外で `receive` で受け取り、lock の下で取り出した数を下ろして `channel_ops` を数え、`settle` を呼ぶ。放した channel の預かる ID は lock を外してから `close` する」.
- :694 close_raw: 「放すときの `close` は lock の外」.
- :698 (Counter) → 「`Std.Atomic` の4行は run の表の `Counters` を読み書きし、登録表の lock を取らない。`new n` は次の添字を取ってその項に `n` を置く。`add n c` は `fetch_add` で足し、足した後の値を返す。どれも `SeqCst` である。添字が作った塊の外なら内部の誤り…」.
- 「channel」:
  - :733 → 「積めるかは `items.len() + sends < cap + receivers.len()` で決める」.
  - New bullet 「予約と hold」: 「`reserve_*` が 0 を返すと予約を数え、確定に要るもの (channel の番号、預かる ID、受け取りでは列から取った品物) を worker の hold に置く。予約から確定までの命令には安全点も切り替えも捕まえる失敗もないので、hold は worker に1つで足り、ほかのタスクに渡らない。予約は確定まで channel を放させないので、`publish` と `receive` を lock の外で行っても、`settle` の `retire_worker` はそれらの後に起きる」.
  - :734 wake → 「…持ち主の worker へ `Woken` を送る (自分が持ち主ならその場で継続を取り出す)」.
  - :735 → 「…受け手への `Woken` が届く途中か…」.
  - :736 → 「ほかの worker が速い道と `park_*` の間に channel を変えうるので、この道で `Woke` を返す。worker が1つなら届かない」; the receive ready test → 「品物があるか、閉じていて送りの予約がない」.
  - New bullets for S-a and S-b (text as in stdlib.md).
  - :739 → 「閉じた後に列に待ち手がいるのは、送りの予約が残っていて受け手が待つときだけである。`settle` は、閉じて空で予約のない channel を放す」.
  - Table: rows `reserve_send` が 0 (送りの予約 +1 / なし), `reserve_recv` が 0 (品物 −1、取り出した数 +1 / 積めるなら送り手の先頭を1つ); `push_raw` (送りの予約 −1、品物 +1 / 受け手の先頭、閉じて予約 0 なら全員); `pop_raw` (取り出した数 −1 / なし。続けて `settle`).
  - Depot table: 「今の worker」 for `publish`/`receive` 「送り手 (lock の外)」 / 「受け手 (lock の外)」, `close` 「どれでも (lock の外)」, plus the sentence: 「預かる ID の下で動くのは `sweep_all` の `discard` だけで、同じ ID を2つの `Ctx` が同時に使うことはない」.
- :876 → present tense: 「channel のセルは run の登録表にあり、送り手は lock の下で積む。…予約の数を `ChanCell` に、確定に要るものを worker の hold に持つ…」.

Other docs and comments:
- architecture.md:316: 「`Counter` の表は `counters.rs`」.
- roadmap.md:344: drop 「`Async` のタスクが…atomic にする」.
- (r2) eml_extern lib.rs:253 comment → 「登録表ではなく、run をまたいで `Machine` が持つ `Counters` の表の添字の即値」.
- std/Chan.em:54-55 → 「予約と確定の間は `Int` の比べだけにする。間に安全点も失敗の地点も置かず、予約と hold を持ったまま切り替えたり抜けたりしないためである (docs/spec/vm.md の「channel」)」.
- std/Chan.em:144-146 → 「0 なら予約したので続けて確定し」.
- std/Atomic.em:4 → drop 「登録表を使う」.

Kinds: all comment and doc changes are mechanical. The new tests are additions. No expected value changes.

## 6. Unresolved items (defaults)

- Lock contention of two sections per channel operation (C-4). Default: measure in U7 step 13 and record in U9. The decision rule there feeds the existing roadmap item; there is no gate.
- S-b over-capacity. Default: accept, document, and test.
- `counter.em` at N = 2/8. Default: enabled in U7 with `chan_*`.
- Bucket count 26. Default: keep; `Internal("the counter table is full")` beyond that. `new` at a bucket boundary allocates and zeroes up to 8 × (64 << b) bytes (for example 8 MiB around 2^20 counters). That memory is outside `max_bytes`, as the `Vec` is today. Fine for "数の少ない共有の状態".

## 7. Additional checks

- r1 reads as before. Additionally read:
  - vm.rs:585-660 (safepoint sites), vm.rs:2782-2810 (`step_failed`).
  - vm.md:152 (safepoints); declarations.md:126 (held arguments).
  - The `chan.rs` assertions (grep `assert`).
  - eml_extern lib.rs:253; `Run::new` callers (grep).
- (r2) Disassembly of the gap [V].
  - Setup: a copy of the scratch repo at /tmp/claude-0/-home-user-eml/ff77671a-e89e-5f5c-8ef2-c41b31f88c75/scratchpad/at/s2-repo with its own target dir (`target-s2`). A probe test `s2_probe_chan_disasm` was appended to its crates/eml_vm/tests/scaling.rs and run with `cargo test -p eml_vm --test integration s2_probe_chan_disasm -- --nocapture`. Output: /tmp/claude-0/-home-user-eml/ff77671a-e89e-5f5c-8ef2-c41b31f88c75/scratchpad/s2/disasm.txt.
  - `send_loop@[Int]` 39-47: `async reserve_send`, `load_raw`, `int_eq`, `switch_enum`, `box_int`, `async push_raw`.
  - `recv_loop@[Int]` 95-102: `async reserve_recv`, `load_raw`, `int_eq`, `switch_enum`, `async pop_raw`.
  - `try_send` 172-180 and its second instance 561-569: same, with `box_const`.
  - `try_recv` 241-248 and 601-608: same.
  - No call, ret, tail or apply lies in any gap.
- r1's standalone `Counters` run [V]: /tmp/claude-0/-home-user-eml/ff77671a-e89e-5f5c-8ef2-c41b31f88c75/scratchpad/s2/c.rs, output "ok".
- /home/user/eml was not modified.

## 8. Conformance with S1

- G-A1: no `Mutex`/`Atomic` in cells.rs; `Counters` lives in counters.rs; `ChanCell` fields are plain.
- G-A2: arms take registry → slot (`Run::send`). No heap call under the registry lock. `fork_worker`/`retire_worker` (`Control` leaf) run outside it.
- G-A3: any worker does queue removal and `wake_cell`; only the owner's `take_wake` takes `k`; `park_*`/`spend`/`unpark` touch only the running task's own cell.
- G-A4: wakes travel as `Notice::Wake(Woken)`; items never travel in notices; the hold is worker-local.
- G-A5: N = 1 identical (§2, adjacency verified in §7); the 11 `chan::` tests are untouched.
- G-A6: kept unamended. One registry section per channel instruction; `publish`/`receive`/`fork_worker`/`retire_worker` and `schedule` are outside it; no eml-heap allocation under the lock.
- G-A7: tests depend only on "two consecutive starts at N = 2 cover both workers", so they hold for either start value.
- G-A13: channel-wait cancellation goes through S1's `Notice::Cancel`/Mail → owner `unpark`.
- G-A15: `free_chan` stays in `sweep_all` after the join; the debug assertion is justified in §5.2.
- G-A16: no new `share`/`transfer_back` site (`Depot::publish`/`receive`). The channel crossing is the existing one (vm.md:874). The only `with_worker(D)` is in `sweep_all`.
- G-A17: `Counter` lock-free `SeqCst`; channel cells in the run registry; cross-worker wakes are `Woken`; the woken side re-validates.
- G-A18: chan/counter UI files classified; `RunStats` asserted only at N = 1; TSan targets `chan::` and `counters::`.
- G-A20: no amendment from S2 (the r1 G-A6 amendment is withdrawn).

