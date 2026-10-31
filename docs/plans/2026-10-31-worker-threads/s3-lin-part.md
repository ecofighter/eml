# S3 response r2: rc > 1 local objects below the `lin` part of a re-sent shared `poly` container

Revised after review r1. The response table is at the end.

## 1. Conclusion

- **The documented shape cannot be published by a program that passes the compiler's checks.** The "limitation" (OM:259) is really a runtime invariant that nobody has written down yet. Decision:
  - write the invariant down,
  - enforce it in checked mode at the point where it is used (`share`) and in the global check (`check_invariants`),
  - delete the limitation text.
- **No marking, walking or rc change is added in fast mode.**
- **Invariant (the `lin` part rule).** Call a local object a *`lin` part object* if it is reachable from a shared non-`lin` object through local objects only.
  - **(a)** Only the holder of the unique chain (the one reference to the topmost shared object of the chain) can reach a `lin` part object.
  - **(b)** At every `share`, every `lin` part object reachable from the shared value has rc 1. That value includes the containers it reaches that are already shared.
  - Inside the holder's own frame, a `lin` part element may briefly have rc 2 (Perceus may read a borrowed field before it releases the rc -1 parent). Such a container is consumed in that frame, so no `share` and no other thread can reach it in that window. See section 2.
- **Checked-mode rules.**
  - **R1 (`share`).** When the owner-only stack pops a live local object that is neither `lin` nor rc 1 (local, non-`lin`, rc > 1), return `Err(ShareError::SharedLocal)`. `share` maps it to the existing `HeapError::SharedReachesLocal`.
  - **R2 (`check_invariants`).** For each shared non-`lin` object, walk its local descendants through local objects only. Return `SharedReachesLocal` if any of them has rc ≠ 1.
  - No new `HeapError` variant.
- **G-S2-5 holds in fast mode with S1's threads, given the invariant.** This is argued [I] and exercised by the tests in section 5.
  - At fork, the `lin` parts reachable from the right child's closure are rc 1 and reachable only through a chain that the parent has given away.
  - So the rc > 0 objects reachable from the child (its `lin` parts plus its own allocations) and from the parent (its other locals) are disjoint.
  - Re-sends pass whole chains on. Join brings everything back after the child's Release and the parent's Acquire.
  - rc < 0 objects are written only at rc (`share`'s `mark_step`). Owner prefix writes are checked-mode only and touch only rc > 0 objects that the sender exclusively reaches.
- **No changes to rc, reuse decisions, `RunStats` or bench snapshots.** Marking stays schedule-independent.
- **Contingency, if the premise ever fails** (checked mode reports `SharedReachesLocal` from a program that passes the compiler's checks): run the owner-only walk in both modes below rc -1 containers and mark the rc > 1 objects it meets. It is the only fix that keeps rc identical across modes. It is not adopted now.

## 2. Rationale

- **The first share already marks rc > 1 objects below a `lin` object.** OM:252; share.rs `to_move = lin || (from_move && rc == 1)`, otherwise `mark_step`. [V] Read the source.
- **No VM path writes into an existing `lin` part object.** [V] Audited the code:
  - Every `Heap::set_field` / `set_raw` site in eml_vm writes an object built in the same instruction:
    - apply.rs:301-307 writes the new closure from `alloc_data`.
    - data.rs:104 writes the con from `alloc` / `con_at` / `reuse_in_place`.
    - vm.rs:1193-1196 writes the fork evidence and node from `alloc_data`.
    - vm.rs:1572 writes the join pair from `alloc_data`.
    - compile/forks.rs:88-108 writes frozen constants.
  - `reuse_in_place` needs a token. `into_token` needs rc 1, held by the caller (`NotUnique`).
  - `ev_set_state`, `ev_take_*` and `ev_set_clause` (vm.rs:739-767, 1017) need rc > 0 and an evidence held by `evv`. OM:167 says `share` never reaches a running evidence.
  - Externs that write in place (`MutArray.write`, string and bytes pushes) act on the word in a frame slot.
- **A frame slot can own a `lin` field only by releasing its parent.**
  - V1, the LIR verifier, rejects `dup`/`decref` of `lin` and `poly` variables. [V] lir.md:132 and :138; test `crates/eml_lir/tests/verify.rs` `v1_rejects_a_dup_or_decref_of_a_lin_or_poly_variable`.
  - So an owned `lin` field comes from `release` of its parent. At rc -1, `is_last` is true and the kept fields are taken without `dup`; the parent is freed (OM:263; `apply` on a shared closure: vm.md:146, OM:154). [V] Doc.
  - Once taken out, the object is no longer in a `lin` part.
  - The parent is `Lin`, so it is used exactly once (types plus V1). No `share` can reach it while its holder is consuming it. This makes the borrowed-read window harmless. [I]
- **`transfer_back` creates no new shared→local edge.** It flips only rc -1 objects reached from the root through rc -1 objects (OM:271). [V] Doc.
- **Empirical check.** I added an abort probe in the owner-only branch of `share_walk` (scratch clone; section 7). It fires only in the hand-built unit test. It never fires in:
  - 623 UI programs,
  - 435 seeded or sequential runs of `par/`, `async/` and `run-fail/`,
  - 406 `eml_vm` integration tests,
  - 750 `eml_rt` tests,
  - 4 adversarial programs. Two re-send pairs or closures through nested `par`. Two send a poly closure that captures a `MutArray String` through tasks and channels; there, the task start and receive `transfer_back` flip the closure back to local, so the next send is a first share again.

  This is evidence, not proof. [V]
- **Why R1 sits in the owner-only walk.**
  - The owner rewrite there assumes the invariant.
  - When an owner-only pop happens, the mark and move stacks are empty and stay empty. `lemma_finish` needs exactly `inv ∧ mark empty ∧ moves empty` (share.rs:1488-1504 [V]). So any object that is also reachable through the local walk is already marked or moved.
  - R1 is therefore deterministic and fires only for objects reached solely through a `lin` part.
- **Mode equality is kept.** R1 and R2 write nothing. `share` may return `Err` after changing state (OM:690). [V]

## 3. Alternatives considered and rejected

- **Mark where the owner-only walk meets rc > 1, in both modes.**
  - It costs a fast-mode walk of every `lin` part on every re-send.
  - The shape it would handle is not producible.
  - Kept as the contingency (section 1).
- **First share marks the whole `lin` part.** It loses in-place writes of moved `Lin` values (multicore.md 「`Lin` の物体の所有者の移し替え」).
- **New compile-time rule.** V1 plus shared-immutability already forbids the shape.
- **Accept and document as is.** Checked mode would report a delayed `WrongOwner`, or nothing. G-S2-5 would rest on an unchecked claim.
- **New `HeapError` variant.** `SharedReachesLocal` already names the class of violation.
- **A `transfer_back` check that mirrors R1.** It would need a fourth stack to tell objects reached through a `lin` part from those on the child's own path. The invariant already rules the hazard out (finding C-3).

## 4. Assumptions made

- **A1. Shared objects are never written in place,** apart from their rc word. Checked mode enforces this with `NotWritable` (OM:872), and the VM audit above confirms it. S1 and S2 must not add a write to the fields of an rc < 0 object. If false: a `lin` part could change after `share`. R1 and R2 would catch it in checked mode.
- **A2. Every path that publishes a value goes through `share`** (S1's fork and join, future rescue, pool jobs), or keeps the invariant itself. If false: a shared→local edge appears without first-share marking, which is a fast-mode race.
- **A3. The compiler's linearity holds:** V1 in debug builds, plus the type system for `multi` continuations (linearity.md:21, :90). A compiler bug that breaks it shows up as `SharedReachesLocal` in checked mode.
- **A4. S1's model (coordination fact):** the `par` right child may run on another worker; the parent releases its worker and waits at join; the outcome is published with Release; `transfer_back` runs after an Acquire. Async tasks and channels stay on one worker in this stage. If tasks or cells later cross threads (`async-threads`), the same argument applies, because cell and task IDs hold nothing but the item. Re-check it then.
- **A5. S2 provides remote free and an owner check on foreign free that compares the prefix owner with the current logical worker.** `lin` part objects carry the receiver's owner after `share` (moved) or after the owner-only rewrite, so a foreign free by the right child passes S2's check.

## 5. Implications for implementation

### Constraints

- Change `eml_rt` plus the docs only. `share` errors already propagate through vm.rs:1066-1077 and cells.rs:961-963 and 3258. [V]
- Run the full `eml_rt` gate (crates/eml_rt/CLAUDE.md).
- Write the spec text in the same commit as the code, applying the natural-japanese skill. The Japanese below is a draft.

### Commit 1: R1 in `share`, spec text (TDD order)

1. **Unit test (expected-value change, kind 2).** In `crates/eml_rt/src/heap/tests/share.rs`, rename `an_rc2_object_under_a_resent_lin_object_keeps_the_senders_owner` to `an_rc2_object_under_a_resent_lin_object_is_an_error`.
   - It stays checked-only (`H = Heap<Checked>`, a `#[test]` fn).
   - Keep the setup up to the second share. Then assert `heap.share(pair, 2) == Err(HeapError::SharedReachesLocal)`, then `drop(heap)`.
   - Comment draft: 「共有の物体から局所の物体だけを通って届く rc > 1 の物体は `lin` の部分の不変条件を破る。送り直しの所有者だけのたどりがそこに着くと `SharedReachesLocal` になる (docs/spec/object-model.md の「`share`」)」.
2. **New `both_modes!` test `resending_after_marking_an_rc2_node_under_the_lin_part_is_ok`.**
   - Build `kept = list(1)`, then `dup(kept)`; `lin.field0 = kept`; `pair = (a, lin)`.
   - `share(pair, 1)`, then assert `rc(kept) == -2`.
   - `set_worker(1)`; `share(pair, 2).unwrap()`. Assert `(rc(pair), rc(lin), rc(kept)) == (-1, 1, -2)` and `assert_owner::<M>(lin, 2)`.
   - `set_worker(2)`; `decref(pair)`; `decref(kept)`. Assert `mortal(&heap) == 0`.
3. **Code in share.rs.**
   - Add `ShareError::SharedLocal`.
   - In `share_walk`'s final `else` branch, add `if check_lin && on_own && rc > 1 { return Err(ShareError::SharedLocal); }`. At that point `lin && rc > 0` and `rc == 1` have already taken the visit branch.
   - Map it in `share` to `HeapError::SharedReachesLocal`.
4. **Verus (concrete).**
   - New spec:
     ```
     pub open spec fn reaches_local_past_shared<N>(g: GView<N>, root: N) -> bool {
         exists|n: N| #[trigger] reached(g, passes(g), root, n) && g.nodes.contains(n)
             && g.rc[n] > 1 && !g.lin[n] && !reached(g, local(g), root, n)
     }
     ```
   - `share_walk` ensures:
     - (E1) `sound(g0, root) && r == Err(ShareError::SharedLocal) ==> check_lin && reaches_local_past_shared(g0, root)`.
     - (E2) Replace the Ok clause with `well_formed(g0, root) && !(check_lin && reaches_shared_lin(g0, root)) && !(check_lin && reaches_local_past_shared(g0, root)) ==> r is Ok`.
   - Proof at the new branch:
     - `base_inv` already gives `reached(g0, passes(g0), root, own[i])` for every own-stack entry (share.rs:303-309 [V]).
     - At an own pop, mark and moves are empty, so `lemma_finish` (or its body) yields `node_post` for every node.
     - The popped node has current rc > 1 and is non-`lin`. `node_post` then rules out both cases where it was local-reached with g0.rc > 0: moved means rc 1 or `lin`; marked means rc < 0. So it is not local-reached, and its rc is unchanged (g0.rc == current rc > 1). That is the witness for E1.
     - E2 is the contrapositive at the same point.
   - Heap level:
     - Add `reaches_local_past_shared_from(w)`.
     - Add the ensures `w.is_ptr_spec() && old(self).share_sound(w) && r == Err(HeapError::SharedReachesLocal) ==> old(self).reaches_local_past_shared_from(w)`.
     - Add `M::CHECKED && w.is_ptr_spec() ==> !self.reaches_local_past_shared_from(w)` to `share_all_ok`. Only share.rs uses it. [V]
   - In a valid heap, `reaches_local_past_shared` is false: such a path must pass a shared non-`lin` node and then only local nodes to n, which violates (b). Document this in the predicate's comment; it is not proved.
   - **Fallback, if E2 does not close within the commit:**
     - Keep E1.
     - Weaken the Ok clause to `!check_lin && well_formed ==> r is Ok`, plus the existing checked clause conjoined with `!reaches_local_past_shared`, stated as an `assume`-free precondition only on `share_all_ok`.
     - If that still fails, drop the checked-mode Ok guarantee for this error.
     - List `SharedReachesLocal` in OM:688 among the errors that "モデルが合っていても返りうる", as `LinShared` and `WrongOwner` are.
     - Record which variant landed in the commit message.
5. **Docs (same commit).** See "Spec/doc sections" below.

### Commit 2: R2 in `check_invariants`

- In `walk.rs`, inside `verus!`, add `check_lin_parts(&self) -> Result<(), HeapError>`, modelled on `check_reach`'s block loop:
  - `requires self.wf(), self.safe_settleable(), M::CHECKED`.
  - No functional ensures (diagnostic, like the `OwnedMiscount` recount).
  - For each live block with rc < 0 and non-`lin`, push its children with rc > 0 onto an explicit stack.
  - Pop: if the node is not live, return `UseAfterFree`. If rc ≠ 1, return `SharedReachesLocal`. Otherwise push its children with rc > 0.
  - Count total pops. If the count exceeds `blocks.len()`, return `SharedReachesLocal`; a valid `lin` part is a tree with each node reached once. The counter also serves as the `decreases` measure.
- Call it from `check_invariants` after `check_reach`. vm.md:342 already runs it at `Finish` in checked mode, and it is available to S2's threaded `trusted` tests.
- **Test (eml_rt, checked):** `check_invariants_finds_an_rc2_object_in_a_lin_part`.
  - Same setup as the commit 1 test up to the `set_field` of `kept` into `lin` (no second share).
  - Assert `check_invariants() == Err(SharedReachesLocal)`.
  - Undo with `raw::set_slot(lin, 0, Word::UNIT)`, then clean up.
  - Also assert `check_invariants() == Ok(())` right after the first share in `resending_after_marking_an_rc2_node_under_the_lin_part_is_ok`.
- **Docs:** OM:668 bullet, add 「共有の `lin` でない物体から局所の物体だけを通って届く局所の物体の rc が 1 でないことも、この誤りにする」.

### Tests that exercise G-S2-5 under threads

- **Checked mode, N = 2, through S1's harness.**
  - `tests/ui/run/par/poly_lin_resend.em` (existing).
  - New `tests/ui/run/par/poly_lin_resend_shared_elements.em`: scratch `p1.em` (path in section 7) with a header comment. Expected stdout `(0, 0, "ab12", "ab12", "ab12ab12", "ab12")` [V].
  - Run both with right children on a second worker thread. If an rc > 0 object is reached from two workers, it surfaces as one of:
    - `WrongOwner` (prefix owner ≠ current logical worker),
    - `SharedReachesLocal` (R1/R2 at `Finish`),
    - S2's foreign-free owner mismatch.
  - Pin the N = 2 runs in whichever harness S1 defines.
- **Fast mode, real threads, Miri:** `eml_rt` trusted test `a_lin_part_moves_to_another_thread_and_back`, placed after S2's per-worker heap and remote-free units land.
  - Thread A: build pair `C = (a, L)`, where `L` is `lin` with child `X` (rc 1) and element `S` (rc 2, A keeps one reference).
  - Thread A: `share(C, 1)`, then send the word to thread B over a std channel (Release/Acquire).
  - Thread B: `is_last` / `free_block` `C` (remote free), write `L`'s field in place, `decref S` (shared path), free `L` and `X` (remote free).
  - Concurrently, thread A: `dup` / `decref` its `S` reference and allocate and free its own locals.
  - Finish: A drains its remote list, then `check_invariants() == Ok(())` and `mortal == 0`.
  - Miri would flag any non-atomic access to the same rc or field word from both threads. The test must stay within the per-test Miri budget (+5 s under Tree Borrows).
  - If S2's API is not in place within the stage, the default is to keep only the N = 2 checked UI runs and record the Miri test as S2's follow-up.

### Completion criteria (exact commands, exact expected results)

- `cargo nextest run -p eml_rt -E 'test(/an_rc2_object_under_a_resent_lin_object_is_an_error$/) | test(/resending_after_marking_an_rc2_node_under_the_lin_part_is_ok$/) | test(/check_invariants_finds_an_rc2_object_in_a_lin_part$/)'`: 4 tests run, 4 passed. The `both_modes!` test runs as `fast::…` and `checked::…`; the other two are checked-only `#[test]`s.
- `cargo nextest run`: 0 failed. `git status --porcelain` lists exactly one new snapshot, `crates/eml_cli/tests/snapshots/integration__ui__run@par__poly_lin_resend_shared_elements.em.snap`, and no modified snapshot.
- `git diff --stat crates/eml_vm/tests/bench.rs`: empty.
- `cargo verus verify -p eml_rt`: last line reports `0 errors`.
- `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')`: exit 0.
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check`: exit 0.
- `grep -rn "「\`share\`」の制限\|前の持ち主のまま残る\|前の持ち主に残る" docs crates`: no output. Today it matches testing.md:136, status.md:142, OM:259, roadmap.md:31 and tests/share.rs:507. [V]
- `grep -c "SharedReachesLocal" docs/spec/object-model.md`: ≥ 4 (the `share` bullet, the `share` row, the `share` entry in 「報告する誤り」, the `check_invariants` entry).
- Miri `trusted::` (both settings): unchanged until the threaded test lands. Then within the budget in crates/eml_rt/CLAUDE.md.

### Test changes by kind

- **Pass/fail (kind 1):** none.
- **Expected-value (kind 2):** `an_rc2_object_under_a_resent_lin_object_keeps_the_senders_owner` becomes `…_is_an_error`. The expected result changes from "`share` Ok, then `dup` gives `WrongOwner{1,2}`" to "`share` returns `SharedReachesLocal`". Reason: the shape violates the `lin` part invariant, which the spec now states and checked mode enforces.
- **Mechanical (kind 3):** none.
- **New tests:**
  - `resending_after_marking_an_rc2_node_under_the_lin_part_is_ok` (both modes),
  - `check_invariants_finds_an_rc2_object_in_a_lin_part` (checked),
  - UI `par/poly_lin_resend_shared_elements.em`,
  - Miri `a_lin_part_moves_to_another_thread_and_back` (after S2).

### Spec/doc sections to write or revise

- **docs/spec/object-model.md 「`share」`, the bullet at line 259.** Replace it with (draft):
  「`lin` の部分の不変条件: 共有の `lin` でない物体から局所の物体だけを通って届く局所の物体を `lin` の部分と呼ぶ。`lin` の部分の物体に届くのは、その上の共有の物体のただ1つの参照を持つ者だけである。`share` が届く `lin` の部分の物体は、どれも rc 1 である。初めの `share` は `lin` の物体の下の rc > 1 の物体に印を付けるので、この形を作る (上の項目)。共有の物体の欄は書き換えない (`set_field` の `NotWritable`)。`lin` の値と `poly` の値は `dup` できない ([LIR](lir.md) の「Kind の印の検査」)。そのため、プログラムが `lin` の部分の物体を持つには、道の上の rc -1 の物体を最後の参照として手放して取り出すしかなく、取り出した物体はもう `lin` の部分にない。手放す前に借りて読んだ値の rc が一時 2 になっても、その入れ物は同じフレームで消費されるので、`share` もほかのスレッドも届かない。`transfer_back` が裏返すのは、根から rc -1 の物体だけを通って届く物体なので、新しい `lin` の部分はできない。これで、送り直しの所有者だけのたどりは `lin` の部分のすべての物体の所有者を受け手に書き換え、`lin` の部分にほかの worker から届く道は残らない。検査モードは、所有者だけのスタックから `lin` でも rc 1 でもない局所の物体を取り出したら `SharedReachesLocal` を返す。速いモードは、正しいプログラムに頼る」
- **OM:331 (`share` row).** Append 「共有の物体から局所の物体だけを通って届く局所の物体は `lin` か rc 1 である (`SharedReachesLocal`)」.
- **OM 「報告する誤り」, after line 652.** Add 「`share` の所有者だけのたどりで、`lin` でも rc 1 でもない局所の物体に着くこと (`SharedReachesLocal`)。上の「`share`」の `lin` の部分の不変条件の違反である」.
- **OM:668.** Commit 2 text (above).
- **OM:688.** Change it only if the Verus fallback lands (commit 1, step 4).
- **docs/implementation/status.md:142.** Delete the bullet.
- **docs/future/roadmap.md:31.** Delete the sentence 「共有にした `poly` の入れ物を送り直すとき、…扱いを決める (…の制限)。」.
- **docs/implementation/testing.md:104.** 「`Dead` と `LinReachedTwice`」 becomes 「`Dead`、`LinReachedTwice`、`SharedReachesLocal`」.
- **docs/implementation/testing.md:136.** 「…その下の rc > 1 の物体が前の持ち主に残ること」 becomes 「…その下の rc > 1 の局所の物体が `SharedReachesLocal` になることと、印を付けた rc > 1 の物体が送り直しで誤りにならないこと」. Also add `check_invariants`'s `lin` part check to the heap test list.

## 6. Unresolved items

- **Whether S2's remote-free API lands early enough in the stage for the Miri threaded test.** Default: if it does not, keep the N = 2 checked UI runs as the evidence and hand the Miri test to S2's follow-up.
- **Which Verus variant lands.** Default: E1 + E2 as specified. Fall back as described in commit 1, step 4.

## 7. Additional checks

- [V] Probe in scratch clone `/tmp/claude-0/-home-user-eml/e8305f47-7239-534e-8be7-5a0da86d7258/scratchpad/wt/scratch-S3` (0f9cd9b). The probe (abort when `check_lin && on_own && rc > 1` in the final `else` of `share_walk`):
  - fired in `an_rc2_object_under_a_resent…` (SIGABRT), so it works;
  - did not fire in `eml_rt --lib` (750 passed), `eml_vm --test integration` (406 passed), 623 UI entry files under `--debug-heap`, or 435 runs of `par/`, `async/` and `run-fail/` under `sequential`, `seeded:1` and `seeded:7`.
- [V] Adversarial programs, all under `--debug-heap` with no probe hit:
  - `/tmp/claude-0/-home-user-eml/e8305f47-7239-534e-8be7-5a0da86d7258/scratchpad/wt/s3probe/p1.em`: 3-level re-send of a pair with shared string elements.
  - `/tmp/claude-0/-home-user-eml/e8305f47-7239-534e-8be7-5a0da86d7258/scratchpad/wt/s3probe/p2.em`: child rebuilds the pair, then re-sends it twice.
  - `/tmp/claude-0/-home-user-eml/e8305f47-7239-534e-8be7-5a0da86d7258/scratchpad/wt/s3probe/p3.em` and `/tmp/claude-0/-home-user-eml/e8305f47-7239-534e-8be7-5a0da86d7258/scratchpad/wt/s3probe/p4.em`: a poly closure that captures a `MutArray String` with shared elements, relayed through 2 tasks and 3 channels. In p4 the closure is captured by a spawned task's closure. Run under `normal`, `seeded:1` and `seeded:5`.
- [V] VM in-place write audit (grep over eml_vm/src for `set_field|set_raw|reuse_in_place|ev_set_state|ev_take_state|ev_take_evv|ev_set_clause`). Every heap write is to an object built in the same instruction or to a held local evidence. The `task.set_raw` hits are register writes, not heap writes.
- [V] V1 rule: lir.md:138; `crates/eml_lir/tests/verify.rs:2760` (`v1_rejects_a_dup_or_decref_of_a_lin_or_poly_variable`).
- [V] Verus anchors:
  - `base_inv` own-stack entries are `reached(g0, passes(g0), root, _)` (share.rs:303-309).
  - `lemma_finish` requires only `inv ∧ mark empty ∧ moves empty` and ensures `node_post` for all nodes (share.rs:1488-1504).
  - `share_all_ok` and `ShareError` are used only in share.rs.
- [V] `check_invariants` runs at `Finish` in checked mode (vm.md:342; eml_vm/src/lib.rs:286). `check_reach` is inside `verus!` (walk.rs:491).
- [V] `both_modes!` expands to `fast::name` and `checked::name` (heap/tests/mod.rs:10-19).
- No background tasks left running.

## 8. Conformance criteria

N/A (not a Fable subtask). For consolidation, against S2's G-S2-5:
- Checked mode MUST return `SharedReachesLocal` from `share` (R1) and from `check_invariants` (R2) on a `lin` part object with rc ≠ 1.
- Fast mode MUST NOT walk, mark or check anything extra.
- G-S2-5 for `lin` parts rests on the invariant, and is exercised by the N = 2 checked UI runs and the Miri threaded test above.

## Response to review r1

| Finding | Verdict | Response |
|---|---|---|
| A-1 | Accepted | Audited the VM's write sites in code (section 2, section 7): every heap write is to an object built in the same instruction or to a held local evidence. Retagged: the audit is [V]; the overall impossibility is an argument [I] backed by the probe. Added p3/p4 (tasks and channels with a poly closure). Probe still never fires. |
| A-2 | Accepted | The enforcing rule is V1 (lir.md:138), which rejects `dup`/`decref` of `lin` and `poly` variables after every LIR pass in debug builds. Its negative test is `v1_rejects_a_dup_or_decref_of_a_lin_or_poly_variable`. A3 now cites it. |
| B-1 | Accepted | The predicate is now concrete (`reaches_local_past_shared`, defined over g0). The proof reuses `base_inv`'s own-stack reachability and `lemma_finish`'s `node_post`. A node marked during the walk has rc < 0, so it cannot trip the check. A staged fallback (OM:688 list) is given. |
| B-2 | Accepted | The grep now targets the exact anchors and lists today's matches. |
| B-3 | Accepted | Modes are stated (checked-only `#[test]`; `both_modes!` gives 2) and the filter is exact. Count is 4 with the commit 2 test. |
| C-1 | Partially accepted | Walked through the threaded cases under S1's model (section 1, G-S2-5 bullet; A4, A5). (i) The child freeing or consuming a `lin` part: the owner prefix is the child's ID, so S2's foreign-free check passes. (ii) The parent's stale words: they point only to marked objects or to moved words that Perceus has dead-ended (the `Lin` value was moved into the closure) [I]. (iii) Partial `transfer_back`: covered under C-3. "No race" is now worded as conditional on the invariant. Evidence added: N = 2 checked UI runs and a Miri two-thread test (after S2). Not accepted: the claim that the deleted test was evidence for G-S2-5. It pinned a hand-built violation, and its replacement pins the detector. |
| C-2 | Accepted | Added R2 (commit 2): a `lin` part check in `check_invariants`, as a global detector at `Finish` and for S2's threaded tests. |
| C-3 | Rejected | `transfer_back`'s owner-only stack never descends below rc ≤ -2 objects. It pushes children only of rc -1 non-`lin` objects and of rewritten rc > 0 objects (OM:274-276). Below rc -1 non-`lin` objects only `lin` locals occur (`shared_points_to_no_local`), and below those the invariant allows only rc 1. So the rc > 1 objects owned by `from` that it rewrites are reached through local paths of the returned value. Under S1, the child thread has published and stopped, and any child object not returned is a `TransferLeak` in checked mode. No other holder exists. |
| C-4 | Accepted | Contingency stated (section 1): owner-only walk in both modes with marking. |
| C-5 | Partially accepted | rc -1 means exactly one reference, whatever the type. Only its holder reads the parent's fields or calls `is_last` (Relaxed load, then an Acquire reread at -1). This is the existing premise behind `transfer_back` (multicore.md 「裏返しが正しい理由」), not a new claim. It is S2's to restate for remote free. Noted under A1. |
