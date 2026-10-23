> 付録。par-core の設計の担当が書いた最終版で、英語のまま残す。本文 (docs/plans/2026-10-24-par-core.md) と食い違う所は本文が優先する。違いは本文の 5.3 にまとめた。文中の `par-core/probe/` は `docs/plans/2026-10-24-par-core/probe/` を指す。

# S2 response r2: `eml_rt` heap operations for `par` on one worker

Scratch clone `<work dir>/S2-repo` (base ffc04d6b, reverted after each experiment). Baselines I measured:
- Verus: `1043 verified, 0 errors`, 37 s wall / 157 s user.
- `cargo nextest run -p eml_rt`: 637 tests.
- `size_of::<HeapError>() == 12`; a `{u32,u32}` variant keeps 12, a `{u32,u64}` variant makes it 16.
- Miri `trusted::`: 108 tests. `/usr/bin/time -p -o` reports user 19.8 s (Stacked Borrows) and 24.4 s (Tree Borrows). The test harness prints "finished in 64 s" for both, so treat the `user` figure as the budget baseline, with that caveat.

Changes from r1, in short:
- `share` and `transfer_back` no longer use the owner field as a visited marker. A checked-only set of addresses (`HashSetWithView<usize>`) replaces it. This removes the freshness precondition (C-4), the `perm_wf` strengthening (B-2), and the reliance on shared-object owners.
- Every pass that only rewrites owners has a stack of its own, and nothing pushed there ever changes rc. Fast and checked modes therefore make the same rc changes by construction (C-1).
- IDs are recycled at `retire_worker`, so the count table stays bounded (C-5).
- Counts are checked at runtime by an exact recount in `check_invariants` (B-1).
- `transfer_back` takes `from` (A-1).
- Names follow S1, except `share`, which stays one op.
- `Ctx::evv_swap` is added.

## 1. Conclusion

**Q1: transfer form of `share`**
- Keep one op: `pub unsafe fn share(&mut self, w: Word, receiver: u32) -> Result<(), HeapError>`. Fork, `RootSet` (receiver == worker) and later senders all call it. I disagree with S1's split into `fork_share` + `share`: both would run the same walk.
- The walk has three stacks: *mark* (the root starts here), *move*, and *own*. The own stack is used in checked mode only. A node n popped from any stack is handled as follows:
  - Not live → `UseAfterFree` (checked). rc == 0 → stop. lin with rc < 0 → `LinShared` (checked) / stop (fast).
  - **mark or move stack, rc > 0**:
    - If lin, or (popped from the move stack and rc == 1): *move*. In checked mode, if `seen` already contains n → `LinReachedTwice`; otherwise insert n. Set owner := receiver (ghost only in fast mode). Push the children to the move stack.
    - Otherwise: *mark*. Set rc := `mark_step(rc)` and push the children to the mark stack.
  - **mark or move stack, rc < 0, not lin**: fast mode stops. Checked mode, if rc == -1, handles n as an own-stack node; otherwise it stops.
  - **own stack** (checked; never writes rc):
    - rc == -1, not lin: if `seen` contains n → stop; otherwise insert n and push the children to the own stack.
    - rc > 0 and (lin or rc == 1): if seen → stop; otherwise insert n, set owner := receiver, and push the children to the own stack.
    - Anything else → stop.
- **Mode agreement (C-1).** Only mark and move expansions change rc. Each depends only on rc, the lin bit and which stack it came from. Pushes to the mark and move stacks come only from mark and move expansions, and those are identical in both modes. Checked mode adds only own-stack visits, which never write rc and never push to the mark or move stacks. Checked mode can stop early only with an `Err`, which means a violated precondition. So on `Ok`, the rc results are identical, and so are the reuse decisions. The `share` postcondition `node_post` keeps stating the rc part with no dependence on the mode.
- **What the own stack is for.** A non-lin shared parent at rc -1 can hold a lin-local child: a poly `(a, FileIn)` or an EP continuation. When such a parent is sent a second time (nested `par`), fast mode stops at the parent, as today. In checked mode the lin child and its rc-1 descendants change owner to the receiver, so the receiver's correct use of them does not raise `WrongOwner`.
  - Objects with rc > 1 below such a lin child are left with the sender's owner. A receiver touching them gets `WrongOwner`.
  - That report is real: fast mode would leave a local object reachable from two workers. At N = 1 this is harmless. In worker-threads it is a latent race of today's rule.
  - Recorded as R7.
- **Moved regions (C-3).** A move starts at every lin object the walk reaches, not only at the walk root.
  - Soundness argument: a live lin object is reachable through exactly one reference path. Linearity forbids duplicating a `Lin` value. Kinds propagate, so a container of a `Lin` value is itself `Lin`, and a poly container is `Lin` at its use site. Checked mode enforces this with `LinDup` and with `seen`, which catches a second arrival at a lin object.
  - The rc-1 descendants of a lin object are reachable only through it. Moving them gives the receiver nothing that the left child can still reach, unless the lin object itself is reachable twice, and that is excluded already.
  - Today's code moves every lin object reached, wherever it sits (share.rs:1185-1199 [V]). This extends the same rule one level further: the rc-1 region below each such object moves with it.
  - multicore.md's "根" means the root of a moved region. U3 rewrites that sentence, and the plan lists it as a design refinement for approval.
- **Termination.**
  - Checked mode: each expansion either changes rc (a mark) or adds an element to `seen` (a move or an own visit). The measure is |{rc > 0}| + |nodes − seen|.
  - Fast mode: as today, through `well_formed`. Its uniqueness clause widens from "lin ∧ rc > 0" to "rc == 1": such a node has at most one incoming edge and is not the root when it has one. It holds in every `consistent()` heap, since rc == |refs| ≥ indeg.
- No freshness precondition on `receiver`. `receiver == worker` is allowed: a move is then a no-op owner write. S1's `RootSet` needs this.
- **Cost.** Fast mode: unchanged order, O(objects marked or moved + their direct children). Checked mode adds the own-stack visits: rc -1 non-lin objects reachable from the value, plus lin and rc-1 objects below them, each visited once per call (`seen`).
  - Divide and conquer that hands a structure of n objects down d levels of forks: O(n·d) hash operations in checked mode.
  - For UI-test sizes (n ≤ 10⁴, d ≤ 14) that is ≤ 1.4·10⁵ inserts, a few ms [I]. The 1 s per-test rule is not at risk. C-2's "O(1) today" is true only because today's checked mode is wrong in this case.

**Q2: transfer back**
- `pub unsafe fn transfer_back(&mut self, w: Word, from: u32) -> Result<(), HeapError>`. Here `from` is the child's ID. This differs from S1's signature, which has no `from`. The reason is A-1: `from` turns a silent rewrite into `WrongOwner`. S1 has `JoinNode.right_worker`.
- The walk has two stacks: *flip* (the root starts here) and *own* (checked only).
  - Not live → `UseAfterFree` (checked). rc == 0 → stop.
  - **flip stack**: rc == -1 → set rc := 1 and owner := worker, and push the children to the flip stack. Otherwise, fast mode stops and checked mode handles n as an own-stack node.
  - **own stack** (checked; never writes rc):
    - rc > 0: owner == worker → stop. owner == from → set owner := worker and push the children to the own stack. Any other owner → `Err(WrongOwner { owner, worker })`.
    - rc == -1, not lin: if seen → stop; otherwise insert n and push the children to the own stack.
    - Anything else → stop.
- **Mode agreement.** Flips happen only on the flip stack, and the flip stack receives pushes only from flips. The rc results are therefore identical in both modes. The flipped set is exactly the set of nodes reachable from the root through nodes that were rc == -1 at the start. That holds even for malformed graphs: a flip writes rc 1, so a second arrival stops.
- **Termination.**
  - Fast mode: |{rc == -1}| decreases with every flip. No uniqueness precondition is needed.
  - Checked mode adds two more decreasing terms: |{rc > 0 ∧ owner == from}| for rewrites, and |nodes − seen| for passes. A flip never increases the first term, because flipped nodes get owner := worker and `from ≠ worker`.
  - `from == worker` is a caller error. Checked mode returns `Err(WrongOwner { owner: from, worker })` before it changes anything. Fast mode ignores `from`.
- Cost: fast mode is O(flipped + their direct children). Checked mode adds the child-owned objects reachable from the root, plus rc -1 objects reached through them (A-3: a result that wraps a large rc -1 structure costs O(size) in checked mode only).
- **What `WrongOwner` still catches (A-1).**
  - A missing fork mark is reported when the child dups, decrefs or writes the unmarked object, as before.
  - If the child only passes such an object through, untouched, to its result, `transfer_back` meets it with owner == worker and leaves it. That is right, since the object is the parent's. The omission goes undetected there, as it does today.
  - An object owned by a third ID now raises `WrongOwner` at the join instead of being rewritten.

**Q3: IDs, counts, `TransferLeak`** (names follow S1)
- `pub fn fork_worker(&mut self) -> u32`:
  - Checked mode: pop the most recently retired ID from `free_ids: Vec<u32>`, or else take `next_worker` and increment it. `next_worker` starts at 1; 0 is the base ID.
  - When `next_worker == u32::MAX`, call `too_many_workers()`, a new external_body panic helper. It is a T5 entry listed next to `too_many_acquires`.
  - Fast mode: return 0 and touch no state; IDs have no effect there.
- `pub fn retire_worker(&mut self, id: u32) -> Result<(), HeapError>`:
  - Checked mode: `owned[id] > 0` → `Err(TransferLeak { worker: id, live: min(owned[id], u32::MAX) as u32 })`. The ID is then not recycled.
  - Otherwise push `id` onto `free_ids` and return `Ok`.
  - Fast mode: `Ok`.
- Recycling is safe once marker ownership is gone: an ID is reused only when `owned[id] == 0`, i.e. no live local object is owned by it. Owners stored on shared objects are never read.
- The table stays bounded: `owned.len() ≤ next_worker`, and `next_worker` ≤ 1 + the maximum number of simultaneously live IDs (C-5).
- `set_worker(id)`: unchanged except in checked mode, where `if id >= next_worker { next_worker = id + 1 }` keeps `fork_worker` from handing out an ID that a test chose. Precondition: tests use small IDs. The VM uses only 0 and IDs from `fork_worker`.
- `owned: Vec<u64>` (checked only). It grows by pushing zeros up to `id + 1` on first use. Updates use `wrapping_add` and `wrapping_sub`, so there is no panic path; vstd specifies both, and `Stats` already uses them in verified code [V].
- **Invariant (I-own)**, checked mode, for every ID i: `owned[i]` equals the number of blocks in state Live, Building, Token or Dying that have rc > 0 and prefix owner i. Positions past `owned.len()` count as 0.
  - It is not part of `wf`; Verus does not prove it.
  - `Heap<Checked>::check_invariants` checks it exactly at runtime. A new enumeration `owned_entries()` in walk.rs (verified, read-only, shaped like `leak_entries`) yields the rc > 0 blocks and their owners. `check_invariants` (outside `verus!`, as allowed) recounts and returns `Err(OwnedMiscount { worker })` on the first difference, after the existing reach checks.
  - The VM already runs `check_invariants` after every checked run (eml_vm lib.rs:149 [V]). So every checked UI test also checks the counts.
- **Transition sites (B-1).** These are every write of a prefix owner and every rc sign change of a mortal block [V, grep of `prefix_first_put`, `init_prefix`, `token_build`, `set_owner`, `mark_freed`, `free_box` callers]:
  1. alloc wrapper, mod.rs ~896 (`init_prefix`), rc ≠ 0: `owned[worker] += 1`. This covers every allocation, including `string_push`/`string_concat` reallocation and the `con` paths.
  2. `free_box`, mod.rs:2838, the only checked free point: rc > 0 at that moment → `owned[prefix owner] -= 1`. A local last `decref` leaves rc 1 in memory. A shared free leaves 0 (`rc_down_shared`) or -1 (`is_last` + `free_block`) [V `release_ref`]. Its callers are mod.rs:2811, release.rs:654/765, reuse.rs:730 and string.rs:1425; `drop_token` reaches it with rc 1.
  3. `token_build`, states.rs:526, which writes owner := worker on an rc-1 token box: if the old owner ≠ worker, move one count from the old owner to the worker.
  4. `share`, in `Objects::set_rc` and `Objects::set_owner`: a mark (rc > 0 → < 0) does −1 for the owner; a move or own rewrite on an rc > 0 node moves one count from old to new.
  5. `transfer_back`: a flip does +1 for the worker (`set_rc` from < 0 to > 0 counts the stale prefix owner, then `set_owner` moves it to the worker); an own rewrite moves one count.
  6. These need no change:
     - `count_immortal` (check.rs:271): touches only rc 0.
     - `freeze`'s `UNCOUNTED` (mod.rs:1643): it accepts only rc-0 objects.
     - `into_token` (states.rs:236): rc stays 1, so the count stays.
     - The state-to-Dying writes (states.rs:86/366): still counted until `free_box`.
     - `set_lin`, `alloc_immortal` (rc 0).
  7. `recover`: the new heap starts with an empty table and `next_worker = 1`, and counts its copies through site 1.
- Proof impact: counts live in `Heap` and `Objects` exec fields that no spec mentions, with wrapping arithmetic and a verified grow loop. `Graph`'s specs, `share_walk`, and `kept` do not change because of them [I; the shape follows `Stats`].
- `HeapError::TransferLeak { worker: u32, live: u32 }`, using S1's field name. Display: "{live} objects of worker {worker} are left after its join". Also `HeapError::OwnedMiscount { worker: u32 }`. Both keep `HeapError` at 12 bytes [V, size probe].
- Surfacing: like `WrongOwner`. `From<HeapError> for VmFault` gives `Fault::Heap`, then `RuntimeError::Fault`. Neither `try_io` nor `try_panic` catches it [V].
- **Limit (C-6).** `TransferLeak` detects a forgotten `transfer_back` only when the right side holds a local object owned by the child. A result made only of fork-shared objects stays shared, which loses reuse but is not unsafe, and goes unreported. The negative test must return a value the child allocated (U4).

**Q4: interactions, and S1 coordination**
- Shared objects are never reused: `is_unique` is false for rc < 0 [V, test]. Flipped objects become reusable in both modes.
- rc == 0 objects (immortal, frozen, dummy) stop both walks [V share.rs:108,124]. S1's frozen fork-evv list is untouched by `share` and `transfer_back`.
- A `Foreign` lin object moves to the child; the child can only drop it or return it.
- **`evv_swap` is accepted.**
  - Signature: `pub unsafe fn evv_swap(&mut self, l: Word) -> (r: Word)`, infallible. It differs from S1's `Result`: `evv_set` does no runtime shape check either, so the swap has nothing that can fail.
  - It requires `wf`, `word_in(l)`, `evv_wf(l)` and `holds(l)` in both modes.
  - It ensures that `evv_spec() == l`, that the old evv is returned, that the heap is unchanged (no rc work) and that `held_ok` is preserved, with the reference to the returned word now held by the caller.
  - The reason for adding a primitive: `evv_get` + `evv_set` cost a dup + decref per switch, which makes `RunStats` depend on the schedule. That supersedes object-model.md 「タスクの切り替え」's "基本操作は足さない".
- **`RootSet` share (S1).** It works: receiver == worker, and roots are Unr, so nothing moves. Every REPL `let` value becomes shared and is never reused in place.
  - After U1, recovery copies shared objects as local. S1 must re-run `share(root, base_id)` over the roots after `recover` if it keeps this design. I suggest S1 reconsider sharing at `RootSet` against sharing only at a fork that reaches a root; this is S1's call.
- **Recovery (B-3)** is required by S1's design. Every root becomes rc < 0, and `recover` rejects rc < 0 [V recover.rs:644]. Without U1, the first runtime error after a `let` would break every REPL session.
  - Copy semantics: an rc < 0 object is copied like a local one. The copy is a fresh local object in the new heap, owned by the heap's worker, which S1 resets to the base ID before `recover`.
  - The copy's rc equals the number of its uses in the copied graph. DAG sharing is preserved by the existing `done` map keyed by the old address: the first user takes the new reference and later users `dup` it. A shared object at rc -2 reached from two roots becomes one local copy at rc 2 [V, the existing algorithm in object-model.md 「回復」 step 4].
  - Pinned shared objects (`MIN`) become ordinary counts.
  - lin-bit objects stay `Unrecoverable`, whatever their rc sign; roots are Unr, so a lin child under a shared parent cannot be reached from a root.
  - Verified: deleting the rc < 0 rejection verifies unchanged (1043, 0 errors) [V].
- The `leaks()` enumeration and `SharedReachesLocal` are unchanged. Moved non-lin local objects are reachable only from local or lin objects [V walk.rs:560-570, which checks direct children only].
- S1 ordering requirements, all met by S1 r1:
  - take the evv out (`evv_swap`) before `set_worker`;
  - at join, call `transfer_back` on every right-side word, then `retire_worker`, then any decref;
  - reset to the base ID before `recover`.

**Q5: verification**
- Move `GView`, `Graph`, `Objects`, `graph_of` and the path lemmas into `heap/graph.rs`, and parametrize `is_path`/`reached` by the expansion predicate.
- New `Graph` methods:
  - `fn owner(&self, n) -> (r: u32)`: requires a checked graph, rc[n] > 0 and live[n]; ensures r == gv.owner[n]. For `Objects` this follows from the existing `owners_ok` (prefix == model owner for live rc > 0) [V share.rs:1466-1477].
  - `fn see(&mut self, n) -> (fresh: bool)`: backed by `HashSetWithView<usize>` keyed by address [V, vstd hash_set.rs has `insert`/`contains`/`clear`; recover already uses `HashMapWithView<usize, _>`]; ghost `seen: Set<Node>` in the graph; ensures fresh == !old.seen.contains(n), seen' = seen ∪ {n}.
  - `seen` replaces `visited_lin: Vec` and its linear `contains_node`.
- Postconditions:
  - Exact, for rc, in both modes. For share: mark → mark_spec; move, own and stop → unchanged. For transfer_back: flip-reachable → 1, all else unchanged.
  - For owners: frame posts only. share: changes only to receiver. transfer_back: changes only to worker, and only from `from` or on flipped nodes.
  - In fast mode `node_post`'s owner part stays exact (moved ⇒ receiver).
- **B-2 spike, the go/no-go gate opening U3.** On a branch, add `Graph::owner` (rc > 0) and `Graph::see` with the `Objects` impl. Replace `visited_lin` in the current `share_walk` with `see`, with no semantic change.
  - Go: `cargo verus verify -p eml_rt` → 0 errors; ≤ 250 changed proof lines; no change to inv.rs; verify wall time ≤ 60 s.
  - No-go: keep `visited_lin` for lin nodes. Move detection for non-lin rc-1 nodes then needs its own set, and the rest of the design is unaffected. Report back before U3.
  - The `perm_wf` strengthening from r1 is dropped and no longer needed.
- No new trust rows except the `too_many_workers` T5 panic helper. Walks use only T2 cell functions [V]. `HashSetWithView` is vstd-specified std (T9), the same class as recover's map.
- Proof size estimate [I]:
  - graph refactor ±0;
  - spike ≈ 150–250;
  - share changes ≈ +500–800 (third stack, widened `sound`);
  - transfer_back ≈ +700–1000. Termination here is a pure measure, with no uniqueness invariant.
  - evv_swap ≈ 60–120;
  - counts ≈ 50.
  - Total ≈ +1.5–2.2k lines.
- Ways to keep it tractable:
  - `#[verifier::spinoff_prover]` on `share`, `transfer_back` and `evv_swap`;
  - owner-only stacks carry frame posts only;
  - counts are outside the specs.
- **Miri (B-5).** Each walk family and `evv_swap` gets one trusted test.
  - Baseline: 108 trusted tests; user 19.8 s (SB) and 24.4 s (TB) [V].
  - Budget per unit: TB `user` grows by ≤ 5 s per added test; totals stay ≤ 60 s (SB) and ≤ 120 s (TB).
  - `move_stack` and `seen` are std collections in safe code, so there is no new FFI.
  - The counts add no unsafe code. The existing trusted tests run checked alloc and free through them, so they need no test of their own.

## 2. Rationale

- **A visited set instead of an owner marker.** The r1 marker needed fresh receivers (C-4), fought `RootSet` (receiver == worker), forbade ID recycling (C-5), and needed the prefix owner of shared objects tied to the model (B-2). A checked-only hash set costs one insert per moved or owner-only node, and only in checked mode.
- **Separate owner-only stacks.** Mode agreement becomes structural: rc writers never receive pushes from checked-only work. A per-node argument is no longer needed.
- **`from` on `transfer_back`.** The join knows the child's ID, and channels or tasks will know their sender or intermediate ID. With `from`, a third-ID object becomes a `WrongOwner` report instead of a silent rewrite.
- **Runtime recount.** An exact checked invariant run after every checked UI run is stronger evidence than a Verus proof of a diagnostic count, and costs about 50 lines instead of a model extension.

## 3. Alternatives considered and rejected

- Owner-as-marker (r1): see Rationale.
- Dropping the own stack and accepting false `WrongOwner` on resends of poly-Lin containers: correct programs would fail checked runs. The own stack is cheap with `seen`.
- Running the own pass in fast mode too: it costs fast-mode time on every fork, for a race that only matters in worker-threads.
- Proving I-own in Verus: needs a per-owner count in the model and edits to every allocation and free lemma. The runtime recount covers the same property.
- `fork_share` as a separate op: the same walk under two names.
- `evv_swap -> Result`: there is nothing to check at runtime; `evv_set` has no check either.
- Recovery that preserves sharing (copy as rc -1): a new rc transition in recover's proof. Re-sharing roots is S1's one-line loop.
- Unbounded monotonic IDs: the table grows with every fork.

## 4. Assumptions made (impact if false)

- A1: S1 passes the right child's ID as `from` and calls `retire_worker` once per join. Otherwise: false `WrongOwner`/`TransferLeak`, or a leaked ID.
- A2: S1 resets to the base ID before `recover` and re-shares roots afterwards. Otherwise children get `WrongOwner` on `root_get` after an error, or the copies are owned by a dead ID.
- A3: The spike succeeds. If not, see the no-go branch; the units stay in the same order.
- A4: The owner-only stacks stay cheap for UI tests [I, estimated]. If not, measure `ui::run` for `par/` in checked mode and cap via R7.
- A5: `bench.rs` `RunStats` snapshots are unchanged: no bench uses `par`, and counts and IDs are checked-only events that `RunStats` does not include. The gate measures this (`cargo nextest run`). `bench/rt.sh` instruction counts may move with the `Heap` layout; they are re-measured at stage close, per CLAUDE.md, and are not a per-unit gate.

## 5. Implications for implementation

### API

```rust
impl<M: Mode> Heap<M> {
    pub fn fork_worker(&mut self) -> (r: u32);                                        // checked: allocate or recycle; fast: 0
    pub fn retire_worker(&mut self, id: u32) -> (r: Result<(), HeapError>);           // checked: TransferLeak or recycle; fast: Ok
    pub fn set_worker(&mut self, worker: u32);                                        // existing; checked: bumps next_worker
    pub unsafe fn share(&mut self, w: Word, receiver: u32) -> (r: Result<(), HeapError>);              // extended
    pub unsafe fn transfer_back(&mut self, w: Word, from: u32) -> (r: Result<(), HeapError>);          // new
}
impl Heap<Checked> { pub fn owned_by(&self, id: u32) -> u64; }                        // tests and diagnostics
impl<M: Mode> Ctx<M> { pub unsafe fn evv_swap(&mut self, l: Word) -> (r: Word); }     // new
// HeapError::TransferLeak { worker: u32, live: u32 }, HeapError::OwnedMiscount { worker: u32 }
```

- New `Heap` fields: `next_worker: u32`, `free_ids: Vec<u32>`, `owned: Vec<u64>`, `move_stack: Vec<*mut Header>`, `own_stack: Vec<*mut Header>`, `seen: HashSetWithView<usize>`. `visited_lin` is removed.
- `fresh()` resets the allocator: `next_worker = 1`, empty `free_ids` and `owned`.
- Errors after a state change: `transfer_back` joins the list in 「検査モードの約束」, except for the `from == worker` check, which comes first.
- `transfer_back` safety contract: `w` is immediate, or a live object of this heap or an attached chunk. Every reachable object is live. Every rc == -1 object on the flip path is referenced only from inside the value the caller holds.

### Gate G, run after every unit

1. `cargo nextest run -p eml_rt`: all pass.
2. `cargo nextest run`: the whole workspace passes, and the `crates/eml_vm/tests/bench.rs` snapshots are unchanged.
3. `cargo verus verify -p eml_rt`: `verification results:: N verified, 0 errors`, where N is greater than the previous unit's N, or equal to it for U1.
4. `git add -N` any new files, then `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` exits 0.
5. `cargo clippy --all-targets -- -D warnings && cargo fmt --check`: clean.
6. `/usr/bin/time -p -o t.txt nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::`: pass, with user ≤ 60 s. Baseline 19.8 s.
7. The same with `MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows"`: pass, with user ≤ 120 s and ≤ baseline 24.4 s + 5 s × (trusted tests added so far).

### Units, in this order

**U1: recovery copies shared objects.**
- Code: delete recover.rs:644-646.
- Tests:
  - Kind 1: the shared case (4th) of `recovery_refuses_what_it_cannot_copy` changes from refused to copied; delete it from that test's list.
  - New `recovery_copies_shared_objects_as_local` (`both_modes!`): a holder → a shared cons x, used twice, plus a shared list. After `recover`: the copies are rc > 0; x's copy is one object at rc 2, appearing in both fields; the single-use copies are `is_unique`; the contents are equal; the mortal count is as expected.
  - New: a lin-bit object at rc -1 is still `Unrecoverable`.
- Docs (object-model.md):
  - 「種類」 copy table: remove "rc が負の物体 (共有)" from the 写せない row. Add a row: "rc が負の物体: 局所の物体として写す。2つ目からの使い手は `dup` する".
  - 「回復」: step 2 or a bullet giving the reason: all tasks are discarded, so the copy's references are only roots and copied parents.
- Done when G passes with N = 1043.

**U2: IDs, counts, `TransferLeak`, recount.**
- Code: the Q3 fields and functions, the transition sites 1–5, `owned_entries`, `OwnedMiscount` in `check_invariants`, and `too_many_workers` (T5) with its trust.rs entry.
- Tests (`both_modes!` where meaningful, else `Heap<Checked>`):
  - `owned_by` follows alloc, free, `string_push`, the token round trip, `set_worker`, and share marking and moving;
  - `check_invariants` is Ok after each of these;
  - a raw `set_rc` that breaks I-own → `OwnedMiscount`;
  - `retire_worker` is Ok after the child frees everything, `TransferLeak { live: 1 }` when one child object survives, and always Ok in fast mode;
  - a retired ID is recycled; a leaked ID is not; `fork_worker` skips IDs that `set_worker` raised past.
- Existing tests that write rc raw and then call `check_invariants` may hit `OwnedMiscount`. The implementer lists each one as kind 2. Expected: none [I], because reach errors are reported first.
- Docs (object-model.md):
  - 「論理的な worker ID」: `fork_worker`, `retire_worker`, recycling, I-own, `TransferLeak`.
  - 「報告する誤り」: `TransferLeak` and `OwnedMiscount`.
  - 「検査モード」 counting sentence.
  - T5 row text: add `too_many_workers`.
  - Also the error.rs module comment.
- Done when G passes and a unit test asserts `size_of::<HeapError>() == 12`.

**U3: `share` transfer form** (opens with the B-2 spike).
- Code: graph.rs refactor; `Graph::owner` and `see`; three stacks; `seen`; the widened `sound`.
- Tests:
  - Kind 2: `a_lin_object_moves_to_the_receiver_and_its_children_are_shared` (share.rs:77). Its rc-1 child now stays rc 1 with owner 7. Rename it to `..._and_its_unique_children_move_with_it`; the expected rc changes from -1 to 1.
  - No other existing test changes: receiver == worker stays legal [I]. The implementer reports any surprise as kind 2.
  - New (`both_modes!` unless checked-only):
    - unique descendants of a lin root move;
    - an rc-2 node under a lin root is marked, and so is its rc-1 child;
    - a lin object under a marked non-lin parent moves its rc-1 descendants;
    - resending a -1 poly tuple: in checked mode, the FileIn and its rc-1 children change owner to the second receiver and can be released under that ID; fast-mode rc values match checked-mode rc values;
    - an rc > 1 object under the resent lin object keeps the sender's owner, and `WrongOwner` follows on the receiver's `dup` (checked);
    - reaching the same lin object twice → `LinReachedTwice` (unchanged test);
    - counts move with the walk;
    - a `LONG` unique list under a lin root, without recursion;
    - `share(w, worker)` on an Unr value only marks.
  - Miri: `trusted::share_moves_and_passes_owners`, both modes.
- Docs:
  - object-model.md 「`share`」 bullets 2–3: the three stacks, the move rule with the soundness argument, the owner-only stack (checked) and its reason, the R7 limit, `seen`.
  - The core-operation precondition row (object-model.md:297).
  - `LinReachedTwice` wording (:613): "所有を移す物体に2回届くこと".
  - multicore.md 「`Lin` の物体の所有者の移し替え」: the "今の `share` は…" sentence and "根" now mean any lin object reached.
  - multicore.md 「論理的な worker ID」: counts per heap, recycled at retire, until worker-threads.
- Done when G passes.

**U4: `transfer_back`.**
- Code: `heap/transfer.rs`.
- Tests (new heap/tests/transfer.rs):
  - a -1 list flips to rc 1 and `is_unique`, in both modes;
  - rc -2 and rc 0 stop;
  - an rc > 0 root: in fast mode the children are untouched; in checked mode the child-owned descendants are rewritten;
  - a -1 grandchild under a child-allocated node stays -1 in both modes;
  - the poly `(a, FileIn)` returned inside a child cons: in checked mode, `retire_worker` is Ok and the parent can release it;
  - a third-ID object below the root → `WrongOwner`;
  - `from == worker` → `WrongOwner`, with the heap unchanged;
  - skipping `transfer_back` on a child-allocated list → `TransferLeak` (the negative test for C-6);
  - a `LONG` list without recursion;
  - a freed reachable object → `UseAfterFree` (checked);
  - Miri: `trusted::transfer_back_flips_and_rewrites`.
- Docs:
  - New object-model.md section 「`transfer_back`」 after 「`share`」.
  - Core-operation rows (object-model.md:297 table, :333 list).
  - 「検査モードの約束」 list.
  - 「`unsafe` の核」: "`share` と `transfer_back` のたどり".
  - testing.md 「Verus」 item 4 and the unit-test list (line 133).
- Done when G passes.

**U5: `Ctx::evv_swap`.** It is independent and can be done before U3 if S1 needs it earlier.
- Code: ctx.rs next to `evv_set`.
- Tests:
  - swap then swap back restores the evv;
  - the rc of both words is unchanged;
  - a checked `RunStats`-style counter shows no increments or decrements;
  - add one `evv_swap` call to the trusted test that runs every Ctx basic operation (T8).
- Docs: object-model.md 「タスクの切り替え」 (`evv_swap` and why it replaces `evv_get`/`evv_set`), and the 「基本操作の契約」 table.
- Done when G passes.

## 6. Unresolved items (with defaults)

- R1. Is the move rule applied at every lin object reached, which refines multicore.md? Default: yes. It needs plan approval.
- R2. Fast-mode `fork_worker` returns 0. Default: yes.
- R3. When the per-heap tables move to a run table. Default: the worker-threads stage; the API stays the same.
- R4. `RootSet` sharing, and re-sharing roots after `recover`, belong to S1. Default: as A2.
- R5. `transfer_back` does not report `LinShared` on a lin-bit rc -1 object; it flips it. Default: flip.
- R6. `OwnedMiscount` is a `HeapError` variant rather than a panic. Default: a variant, so the VM reports it as a fault.
- R7. Resending a poly-Lin container whose lin part holds rc > 1 locals. Fast mode leaves those locals local: harmless at N = 1, a race in worker-threads. Checked mode reports `WrongOwner` on use. Default: document it in object-model.md 「`share`」 and in status.md; worker-threads must decide whether fast mode passes through rc -1 nodes too.

## 7. Additional checks

- [V] `HeapError` is 12 bytes; a `{u32,u32}` variant stays 12 bytes; `{u32,u64}` makes it 16 (temporary size probe test, reverted).
- [V] Deleting recover's rc < 0 rejection verifies unchanged; only the shared case fails, in both modes.
- [V] vstd `HashSetWithView` has `insert`/`contains`/`clear`; `wrapping_add`/`wrapping_sub` are specified (std_specs/num.rs:98,112).
- [V] Miri `trusted::` baseline: 108 passed; user 19.8 s (SB) and 24.4 s (TB) via `/usr/bin/time -p -o`.
- [V] The owner-write and free sites listed in Q3 come from grep. `prefix_first_put` appears only at check.rs:271 (immortal count), mod.rs:1643 (freeze), states.rs:526 (`token_build`) and share.rs `set_owner`; `init_prefix` only at mod.rs:896.
- [I] The checked-mode own-pass cost is estimated, not measured; no `par` exists yet.
- The scratch clone is clean, and no background tasks are running.
