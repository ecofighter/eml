# X-ruling-r1: I-1 (G-A16) and I-2 (G-A6 / G-A15 / amendment 6)

Checked against /home/user/eml at the working tree (read-only). [V] = verified by reading the cited source; [I] = inferred.

## I-1 (G-A16): concurrent `share(…, D)` / `transfer_back(…, D)` and channel items as a crossing

**Ruling: conforms, under reading (a). G-A16 and S1 amendment 10 get a clarifying edit (text below), because as written they do not name the channel crossing and leave "used" undefined. This is an editorial amendment, not a rule change; bring it to the user with the text below.**

Reason:
- Reading (a) is the one eml_rt and S1 already rely on. `share(w, X)` runs on the sender's `Ctx` under the sender's ID, and the existing `start` path (cells.rs:974-978 [V]) and `par` fork (vm.rs:1176-1191 [V]) do exactly that; S1 Q9 counts the `Place` closure as "already `share`d" without calling the spawner a user of X. "Used by one `Ctx` at a time" therefore means `with_worker`/`set_worker`/`switch_task` under the ID, which object-model.md:733-734 is the only thing it calls "ID を切り替える" [V]. S2's only use of D in that sense is `Depot::discard` in `sweep_all` (cells.rs:3295 [V]).
- Concurrent count updates on one ID from two threads are sound today [V]: `share` and `transfer_back` swap the heap's `IdCounts` copy out and update through `count_id`, which is `fetch_add`/`fetch_sub` with `Ordering::Relaxed` on `AtomicU64`s inside `Arc<Chunk>`s shared by every heap of the `Control` (control.rs:253-270, 715-721 [V]); `share` first calls `admit_worker(D)` → `enter_id(D)`, which takes the `free` mutex and `grow` takes the `chunks` mutex (control.rs:236-247, 715-721 [V]). Nothing non-atomic is touched per ID. The objects whose `owner` field is written are those reachable from the sender's `x` or from the dequeued `item`, each held by one thread only (the item leaves `items` at `reserve_recv` and the `share` precondition forbids a second path). The only ordering requirement is object-model.md:739 / `retire_id`'s doc (control.rs:297-300 [V]): every count change happens-before `retire_worker(D)`. S2 meets it: each `share`/`transfer_back` precedes (same thread) the registry section that drops its reservation, and the lock orders those sections before `settle`'s section, after which the same thread calls `close` (S2 §1 "The counts pin the channel").
- `transfer_back`'s `from == worker` check (transfer.rs:927 [V]) cannot fire: `from` = D is a `fork_worker` ID and the caller runs under a task ID or 0.

Channel items in the crossing list: yes, add them. vm.md:870 item 4 already names `push_raw`'s `share` / `pop_raw`'s `transfer_back` as an allowed crossing [V], but at today's N = 1 the items never actually change threads; after U7 they do, through `ChanCell.items`. G-A16's "new cross-thread crossings are only …" would otherwise fail a literal check.

Required text changes (S1 r3, carried into the consolidated plan):
- G-A16 → "MUST: cross-thread crossings of words are only `Place` (closure), Mail-driven `free_task` discards after the owner's last use, `Op`s (no words), and channel items through `ChanCell.items` (the existing `Depot::publish`/`receive` sites, outside the registry lock); no new `share`/`transfer_back` call site. 'Acting under' a logical ID (`with_worker`/`set_worker`/`switch_task`) is done by one `Ctx` at a time, with hand-offs ordered by the registry lock; `share(w, ID)` / `transfer_back(w, ID)` run under the caller's own ID and may run concurrently on the same depot ID from several workers, provided every such call happens-before `retire_worker(ID)` (object-model.md 「論理的な worker ID」). Check: grep set of `share`/`transfer_back` unchanged; `with_worker` is the only ID switch outside `resume`/`par`."
- S1 Q9: append "channel items (`ChanCell.items`, U7; `publish` by the sender and `receive` by the receiver, each under its own ID)".
- The one sentence S1 amendment 10 (object-model.md 「論理的な worker ID」) and S2 §5.8's Depot-table sentence both say, replacing S2's 「…同じ ID を2つの `Ctx` が同時に使うことはない」 and extending S1's: 「ID の下で動く (`set_worker` で切り替える) のは同時に1つの `Ctx` だけで、持ち手の受け渡しは run の登録表の lock で順序付ける (タスクの ID は、タスクが終わった後に mail を処理する worker が使う)。`share(値, ID)` と `transfer_back(値, ID)` は呼ぶ側が自分の ID のまま行うので、同じ預かる ID へ複数の worker が同時に預けたり受け取ったりしてよく、要るのはそれらが `retire_worker` より前に起きることだけである」.

## I-2 (G-A6, G-A15, amendment 6): the per-worker `hold`, and "heap allocation under the lock"

### (1) The hold at end of run

**Ruling: the guideline needs a small amendment. S2's debug assertion at task switches is necessary but not the end-of-run check G-A15/amendment 6 ask for; add the hold to amendment 6 and one assertion at worker exit.**

Reason:
- Amendment 6 enumerates the worker-local structures that own counted words and says `sweep_all` empties them before the leak check. The hold is such a structure (`Hold::Recv.item`), lives in each worker's `Scheduler`, and `sweep_all` (worker 0, after join) cannot reach it. S2's correctness argument is that the hold is `None` at every safepoint (adjacency, S2 §7 disassembly), and `END` is handled only at a safepoint or idle wait (S1 Q8 step 2), so it is empty at exit. That argument is sound [I, from S2 §7 + S1 Q8], but the guideline's check must be able to observe it: the switch-time assertion never runs on a worker that exits without switching again.
- Error/`exit` runs: dropping the `Scheduler` with a `Word` in the hold leaks nothing in Rust (`Word` is `Copy`) and recovery discards the heaps; amendment 6 already allows this.

Required changes:
- S1 amendment 6 (multicore.md:336) → append 「channel の予約の hold も worker ごとの構造で、`Recv` の品物の語を持つが、予約から確定までに安全点がないので、安全点では必ず空である。run の終わりの確かめは、`sweep_all` が空にするのではなく、各 worker が抜けるときに空であることを確かめる」.
- G-A15 → add to the per-worker exit sequence "… final collect counting `left`, **`debug_assert!(sched.hold.is_none())`**, quiesce, …" and to worker 0's `main` `Finish` registry-state check "and `hold.is_none()` on worker 0". Code site: where `done` is produced for `Retired` (workers.rs:434-443 [V]) and in `run_scheduled` before `sweep_all`. Check column: the assertion plus S2's existing `switch_away`/`preempt` assertions.
- S2 §5.3 "Hold guard" lists the two new sites; no other S2 change.

### (2) G-A6 "no heap allocation under the lock"

**Ruling: G-A6 means the eml heap (`Ctx`/`Heap` calls), not Rust allocations. S2 conforms. G-A6's wording gets a clarifying edit.**

Reason:
- The exception list ("what today's `start`/`advance`/`settle_op` already do") names eml-heap operations: `start` forks an ID and `share`s (cells.rs:974-978 [V]), `advance`/`settle_op` allocate `made` objects under `with_worker` (cells.rs:2482-2500 [V]). Under the "Rust allocations too" reading S1's own registry would violate G-A6: `timers.insert` (BTreeMap, cells.rs:717, 1209, 2272 [V]), `slots.push` (:789 [V]), `signal_waiters[..].push` (:1240 [V]) and `senders`/`receivers.push_back` (:3166-3170 [V]) all run under the registry lock, and S1 keeps them there. The criterion's purpose (G-A6 Why: bounded, lock-held sections with no heap walk and no `Ctx` re-entry) is served by excluding `Ctx`/`Heap` calls; collection growth is bounded and takes no further lock (allocator only).
- S2's sections push to `VecDeque`s and a reused `Vec<Woken>` and make no `Ctx`/`Heap` call under the lock (S2 §1, §8 G-A6 [V, by text]); `share`/`transfer_back`/`fork_worker`/`retire_worker` are outside.

Required text change:
- G-A6 → replace "no heap allocation under the lock except what today's `start`/`advance`/`settle_op` already do" with "no `Ctx`/`Heap` call under the registry lock except what today's `start` (`fork_worker`, `share`) and the owner's `advance`/`settle_op` (allocation of `made` objects under `with_worker`) already do; Rust collection growth (`Vec`, `VecDeque`, `BTreeMap`) under the lock is allowed and is not a heap allocation in this sense".

## Additional checks

- control.rs:205-332, 632-739 (`IdCounts`, `WorkerIds`, orderings); heap/share.rs:2308-2440 and heap/transfer.rs:891-1001 (count swap-out, `admit_worker`, `from == worker`); heap/mod.rs:875-883 (`admit_worker` → `enter_id`); cells.rs:3246-3307 (`Depot`), 974-978, 2482-2500, 717/789/1209/1240/2272/3166-3170; vm.rs:1176-1191, 1782-1784, 2710; workers.rs:62-79, 434-443; object-model.md:730-742; vm.md:857-876.
- No scratch experiment was needed; nothing in /home/user/eml was modified.
