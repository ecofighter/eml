# NC-4b: the owner table, the crate graph, Verus, the roadmap

> Note (NC-4b as committed): not current where it differs from the plan. Section 4.1's silent `alone()` fallback in `IdCounts::of` and `eml_nrt::run`'s `for_worker(.., &Owners::new(), ..)` are superseded by S-NC-17 of `docs/plans/2026-11-17-native-checked.md` (fixes 4 and 5); sections 9-12 (Parts A-C) are superseded by the roadmap work and are not current.

Base: `plan/native-checked` @ 33993502 (the branch head when I finished; it is the commit the coordinator named 4fce297b, amended). Experiments ran on 4f6d9ea7; 33993502 changes no `eml_rt` file and none of the lines this design touches in `eml_nrt` / `eml_native` except the wording of the limit sentence, quoted below as of 33993502. Tags: [V] ran or measured by me in the scratch clone; [R] read in the source; [I] inferred; [P] reported by the previous designer, not re-run.

Scratch artifacts (all under `/private/tmp/claude-501/-Users-arakaki-Projects-eml/7165a97b-ee85-4236-86ab-b62e82dd973b/scratchpad/`):
- `nc4b-owners-proto.patch`: the prototype of the recommended change (non-test code of `eml_rt`, `eml_nrt`, `eml_vm`; `git diff` against 4f6d9ea7). It differs from the final shape in three named points (section 4.1).
- `nc4b-owners-proto-tests.patch`: the test-side edits and my probes.
- `nc4b-adv-tests.rs`: the four adversarial probes, as appended to `heap/tests/trusted.rs`.
- `nc4b-ledger-proto.patch`: the Verus ledger prototype of Part B.
- `nc4b-rt-gate-opt2.txt`: the rt gate output.

## 0. Verdict

1. Root cause: the per-ID counts live in the `Control`, which outlives the heaps they count; a heap that is dropped with live objects (leak, fatal path, broken error) leaves their counts behind, and only recovery ever cleared them. Both engines have the bug.
2. Recommended fix (differs from the previous proposal): **the table belongs to the heaps that count in it, not to the `Control`.** A heap made alone gets a table of its own; the heaps of one run or session are made on one table through a handle (`Owners`); recovery gives the new heap a new table. Nothing is cleaned up anywhere: `reset_ids` is deleted, `Heap::drop` is unchanged, `Control` loses a field. A new run counts from zero by construction.
3. The previous proposal (subtract in `finalize_all`, keep the table in `Control`) fixes the reported symptom, but I broke it three ways: a destructor that panics in the drop walk brings the symptom back; the same inside recovery turns a case that is exact today into `OwnedMiscount`; an ID issued before the other heaps of an abandoned run are dropped gets a false `TransferLeak`. It also leaks issued IDs across failed runs. None of the four exists under the recommended design [V, section 3].
4. Crate graph: the end state of 01 §13.1 stands. What should change is when it is reached: pull `vm-retire` forward to directly after `native-checked`.
5. Verus: with the table tied to the heaps, "drop subtracts exactly once" and "recovery leaves counts exact" need no proof because no code does either. Per-operation exactness is provable only for one heap's own pages and costs 3-4 units; I recommend it as a later item, not now. A prototype of its ledger half verifies [V].
6. Roadmap: `native-checked` -> `vm-retire` -> `sched-core` -> `jit-tasks` -> `jit-repl` -> `ir-origins` -> `native-prof` -> `sched-proofs` -> `par-direct` -> `native-aot`. Two user decisions are needed: G-20 / D-VR-1 (the precondition gate of `vm-retire`) and D-20 (b) (the profiler's place).

Belongs in NC-4b: sections 4-8. Separate stages: Part A's moves and Part C. Later item: Part B's proof.

## 1. The bug

### 1.1 Reproduction [V]

`crates/eml_native/tests/rerun.rs` (scratch), one `RunConfig::default().with_debug_heap(true)` per case, at 4f6d9ea7:

```
native leak run1:   Leak([("Data", 1)])
native leak run2 (same machine):                 Fault Extern(Heap(OwnedMiscount { worker: 0 }))
native call3 of a function that allocates nothing: Fault Extern(Heap(OwnedMiscount { worker: 0 }))
native execute, clean program, same config:      Fault Extern(Heap(OwnedMiscount { worker: 0 }))
native fatal run1:  Fault Extern(Heap(UseAfterFree));  clean after it, same config: OwnedMiscount { worker: 0 } (twice)
vm leak run1:       Leak([("Data", 1)]);               clean after it, same config: OwnedMiscount { worker: 0 }
vm fatal run1:      Fault Extern(Heap(UseAfterFree));  clean after it, same config: OwnedMiscount { worker: 0 }
baseline: two clean runs on one config: ok, ok
```

Instrumented (a fresh checked heap on the same `Control` reads `owned_by(0..4)`): before `[0, 0, 0, 0]`; after the leak `[1, 0, 0, 0]` on both engines; after the fatal run `[1, 0, 0, 0]` on both; after a clean run `[0, 0, 0, 0]`. The count left is exactly the positive-rc objects the dropped heap still held, under ID 0.

### 1.2 Root cause [R, V]

- `Control.workers: WorkerIds` holds `next`, `free` and the per-ID counts (`control.rs:66, 74-85`). Every checked heap of the `Control` counts in it (`IdCounts::for_control`).
- `Heap::drop` (`heap/walk.rs:986-996`) reclaims, runs the `Foreign` destructors and returns the pages. It never touches the table.
- The only code that clears counts is `IdCounts::reset_ids`, called by `Heap::fresh`, i.e. by `Ctx::recover` (`heap/mod.rs:874-876`).
- `eml_nrt::run::on_worker` makes a `Ctx` per run and drops it on every exit (`run.rs:99-132`); `M::end` calls `check_owned(&[ctx.heap()])` before `leaks()` (`mode.rs:48-52`). A later run recounts its own heap and finds the table ahead by the dead heap's objects.
- VM: `execute` makes a `Machine` per call on the config's `Control`. The leak check of worker 0 is in `Machine::close`, which consumes the machine; a broken error (`is_broken`: heap fault, internal, driver) skips recovery (`lib.rs:421-426`). Both drop the heap with counts left.

### 1.3 Every exit path at the base

| Exit | Table after | How |
|---|---|---|
| success | exact | [V] baseline |
| leak, native | stale | [V] |
| leak, VM (`close`) | stale | [V] |
| heap fault (`FATAL_FAULT`), native; broken error, VM | stale | [V] |
| other fatal codes (`FATAL_RECORDED`: uncaught failure, limits, interrupt; `FATAL_PANICKED`), native | stale: same drop of the same `Ctx` | [R] `run.rs:120-131` |
| non-broken error or abandoned `exit`, VM | exact: `recover` zeroes the table, then copies | [R]; `workers::recovery_resets_the_table` passes |
| other workers' heaps, VM | success: dropped empty (`check_others`); error: dropped after recovery's reset; broken error: kept until `Machine::drop`, then stale | [R] `lib.rs:453-455, 671-673, 776-784` |
| `par` / `compute` / channel IDs of an abandoned run | forgotten by recovery; without recovery they stay issued for the life of the `Control` | [V] probe A11: three abandoned heaps fork IDs `[1, 2, 3]` |
| recovery refused (`Unrecoverable`, `UnknownPage`) | zeroed plus the half-built heap's copies (2 where the heap holds 4) | [P] |
| `Foreign` destructor panics while a heap is dropped | stale (6 of 6 left) | [V] probe A1 |
| `Foreign` destructor panics in the old heap's drop inside `recover` | exact (2, check `Ok`) | [V] probe A1b |
| heap dropped on another thread, heaps kept after a worker panic | as a drop on the owning thread | [R] |

Latent instance in the tree: `trusted::checked_words_cross_heaps_and_unknown_pages_are_refused` drops a heap `c` with one live object on a shared `Control` and never checks the table again [R].

## 2. What the table is for, and the alternatives

Readers of the counts [R]: `retire_worker` (non-zero is `TransferLeak`; zero lets the ID be reused) and `check_owned` (`OwnedMiscount`), which exists only to verify the unproved counts. The count of ID 0 is never read by anything but that self-check. The counts are diagnostic; memory safety never depends on them (`heap/mod.rs:181`).

What the count needs: an object of ID x can lie on any heap of a run and is recounted, moved and freed from any of those threads, so the count is shared by the heaps that exchange objects. That set of heaps is a run (`eml run`, `NativeMachine::run`) or a session (REPL: heap 0 across inputs, plus a run's workers). It is not "every heap ever made on this `Control`".

| Option | Verdict |
|---|---|
| A. Subtract a dropped heap's objects (the previous proposal) | Works for the reported symptom. Adds cleanup on a path that can unwind and an ordering rule; breaks as in section 3; issued IDs still leak |
| B. The table belongs to its heaps (`Arc`); a handle joins heaps to it; recovery makes a new one | **Chosen.** Removes state (`Control.workers`, `reset_ids`) and adds none; no cleanup; no ordering or happens-before rule at drop; a dead, leaked or abandoned heap can only affect its own table |
| C. Reset in `begin_run` | Wrong: the REPL's heap 0 and its counts live across runs |
| D. Derive the count on demand at `retire_worker` | Impossible: needs a walk of other threads' heaps at every join |
| E. Per-heap ledgers or per-page tables summed at retire | More state (a registry of heaps, lookups through page headers) for the same cross-thread sum |
| F. Remove `check_owned` | The stale counts of forked IDs and the leaked IDs remain; and the counts are unproved, so the recount is their only check (Part B) |
| G. A fresh `Control` per run | The `Control` carries the stop words the signal handler holds, the limits and the byte count |

Why B is right and not only tidy: a heap may die before its table only when no live object on its pages is still referenced (else memory safety is gone) and none is unreferenced (else it is a leak, an error after which the engine recovers or drops everything). So in a correct engine a table never has to survive the death of a heap that still holds counted objects. Option A's subtraction only ever serves reusing a table whose heaps are dead; B does not reuse it.

Fit with the future: `jit-tasks` makes the `Owners` handle a field of the run (`eml_sched::Run`) and each worker's base makes its `Ctx` on it; `jit-repl`'s session keeps heap 0 and its table, and recovery (`Ctx::recover`) swaps both; with the VM gone nothing else names the table. The sentence "one `Machine` per `Control`, because the ID table is in the `Control`" loses its reason.

## 3. The previous proposal, tested [V]

Applied `nc4b-rt.patch` and `nc4b-tests.patch` on 4f6d9ea7 (branch `opt1`): `cargo test -p eml_rt --lib` -> `946 passed; 0 failed; 1 ignored` (938 + its 4 + my 4 probes, which only print). Its own claims hold. Probes, three trees:

| Probe | Base | Previous proposal | Recommended (B) |
|---|---|---|---|
| A1: heap = one `Foreign` whose destructor panics + a list of 5; drop under `catch_unwind`; then a new heap on the same `Control` | left 6, `OwnedMiscount` | **left 5, `OwnedMiscount`** | left 0, `Ok` |
| A1b: the same heap with a rooted list of 2; `recover()` unwinds in the old heap's drop | `owned_by(0) = 2`, check `Ok` | **`owned_by(0) = 9`, `OwnedMiscount`** (a regression) | 2, `Ok` |
| A3: a second heap holds 3 objects of forked ID 1; `recover()`; `fork_worker()` before that heap is dropped; `retire_worker` | reissued 1, `Ok` | **reissued 1, `TransferLeak { worker: 1, live: 3 }`** (the proposal's stated "engine rule") | reissued 1, `Ok` |
| A11: three heaps in turn fork an ID, allocate under it, are dropped | IDs `[1, 2, 3]` | IDs `[1, 2, 3]` (never reissued) | IDs `[1, 1, 1]` |

Further points against it [R]: the subtraction is in the same walk as the destructors, so any fix for A1 is a second walk; `share` / `transfer_back` hold the heap's `IdCounts` in a local while they walk (its risk 3); "exactly once per block" rests on `Pages::blocks_in` listing no block twice, which its contract does not state (`alloc.rs:3053-3066` promises completeness only), so the property the proposal adds is not provable as the code stands (Part B).

What I keep from it: its two `eml_native` tests (renamed) and its refused-recovery test.

## 4. The design (NC-4b)

### 4.1 The change

`crates/eml_rt/src/control.rs`:
- `Control` loses the field `workers`. `WorkerIds` (unchanged inside) is held by `Arc`.
- `IdCounts { table: Option<Arc<WorkerIds>>, chunks }` (was `control: Option<Arc<Control>>`). `enter_id`, `count_id`, `fork_id`, `retire_id`, `owned_by` read `self.table`.
- New public handle, exported from `lib.rs`:

```rust
/// 検査モードの所有者の表。1つのrunかセッションのヒープが分け合う。
pub struct Owners { table: Option<Arc<WorkerIds>> }   // Clone, Default; Send + Sync (asserted in lib.rs)
impl Owners {
    pub fn new() -> Owners;                            // a new, empty table
    /// # Panics: when given a heap of another table (checked mode).
    pub fn check<M: Mode>(&self, heaps: &[&Heap<M>]) -> Result<(), HeapError>;   // the body of Control::check_owned
}
```
- `IdCounts::alone()` (a copy of a new table; enters ID 0), `IdCounts::of(&Owners)` (a copy of the handle's table; a handle without a table, which only a fast heap gives, yields `alone()`), both `external_body` inside `verus!`; `IdCounts::owners(&self) -> Owners` and `WorkerIds::empty_table` outside `verus!`.
- Deleted: `IdCounts::for_control`, `IdCounts::reset_ids`, `Control::check_owned`, the `external_body` on `empty_table`.

`crates/eml_rt/src/heap/mod.rs`, `heap/ctx.rs`, `heap/walk.rs`:

```rust
impl<M: Mode> Heap<M> {
    pub fn with_control(control: Arc<Control>) -> Heap<M>;                   // signature unchanged: a table of its own
    pub fn with_owners(control: Arc<Control>, owners: &Owners) -> Heap<M>;   // same ensures as with_control
    pub fn owners(&self) -> Owners;                                          // outside verus!, beside remote_pending
}
impl<M: Mode> Ctx<M> {
    pub fn new(control: Arc<Control>, host: HostRef) -> Ctx<M>;              // worker 0 on with_control
    pub fn for_worker(control: Arc<Control>, owners: &Owners, host: HostRef, k: usize) -> Ctx<M>;   // signature change
}
```
- Both heap constructors call one private `fn counting(control, owned: IdCounts)` that is today's body; both `Ctx` constructors call one private `fn around(heap, control, host, k)` that is today's body of `for_worker` after the heap is made (its `requires` are the `ensures` of the heap constructors).
- `Heap::fresh` loses the `reset_ids` call: `Heap::with_control` already gives the new heap a new table, and `set_worker` enters the inherited ID in it.
- `Heap::drop` and `finalize_all`: unchanged.

Engines:
- `crates/eml_nrt/src/run.rs:107`: `Ctx::<M>::for_worker(Arc::clone(control), &Owners::new(), host, WORKER)`. `mode.rs:48-52`: `ctx.heap().owners().check(&[ctx.heap()])`.
- `crates/eml_vm` (until it is deleted): `CodeBase::worker_ctx` takes `owners: &Owners`; `spawn_workers` and `run_worker` pass a clone; the two callers in `lib.rs` (the main worker's `Ctx`, `spawn_workers`) pass `&ctx.heap().owners()` of worker 0; `check_heaps` calls `ctx.heap().owners().check(&heaps)`; the two unit-test fixtures of `sched.rs` (`worker_1` users' `home`, `forwarding`'s `owner_ctx`) make their second `Ctx` on the first one's table.
- `eml_sched`, `eml_rows`, `eml_native` code, `begin_run`: untouched.

Prototype vs final shape: the prototype (a) keeps `for_worker` and adds `for_worker_of` (final: one `for_worker` that takes the handle, so a multi-worker engine cannot forget it); (b) has `IdCounts::owners` as `external_body` (final: outside `verus!`); (c) `eml_nrt::run` still calls the old `for_worker`.

### 4.2 Rules the engines keep

- R1: heaps that exchange objects (`share`, `transfer_back`, a free of another heap's block) are made on one table. A violation is loud: with the two `sched.rs` fixtures left on separate tables, `cargo test -p eml_vm --lib` gave `85 passed; 2 failed`: `an_op_record_left_on_worker_1_is_swept` with `TransferLeak { worker: 1, live: 4294967295 }`, and `worker_1_frees_the_value_of_a_finished_task_of_worker_0` at an `unwrap` of a `cells` call (`sched.rs:1260`; message not captured) [V].
- R2: `Owners::check` is given every live heap of the table (as today for the `Control`).
- R3: a heap is dropped while its table stays in use only when it holds no counted object (the run's end check found no leak on it). Otherwise the engine recovers or drops every heap of the table. The VM already does this [R].
- No rule about the order of drops and ID issue, none about threads at a drop, nothing in `begin_run`.

### 4.3 Edge cases

| Case | Behaviour |
|---|---|
| second `run` / `call` on a checked `NativeMachine`, `execute` with a reused `RunConfig`, either engine | each run's first heap has a new table: counts from zero [V] |
| recovery accepted | copies counted in the new table; IDs of the abandoned run do not exist in it; `recovery_resets_the_table` passes unchanged [V] |
| recovery refused | the half-built heap and its table are dropped; the old heap's table is as it was (today: zeroed) |
| `NotQuiescent` | returns before the new heap is made; nothing changes |
| other heaps of an abandoned run | dropped whenever; they write nothing |
| destructor panic in a drop, in recovery's drop | no count involved [V A1, A1b] |
| heap dropped on another thread, leaked, abandoned | affects only its own table |
| constants work heap (`constants::build`), `Frozen` | own table; its objects are immortal and uncounted |
| fast mode | no table; `Owners::new()` is one small allocation per run in `eml_nrt::run` |
| page map, byte count | stay in `Control`; both are already released by the heap's drop |

### 4.4 Results on the prototype [V]

- `cargo verus verify -p eml_rt` -> `1278 verified, 0 errors` (base 1273 + `with_owners`, `owners`, `counting`, `for_worker_of`, `around`). Expected in the final shape: 1276 (`owners` outside `verus!`, no `for_worker_of`); the implementer records the number.
- `cargo test -p eml_rt --lib` -> `942 passed; 0 failed; 1 ignored` (938 + 4 probes). Before the test-side edits exactly two tests failed: `trust::the_trust_list_is_exactly_the_trusted_items` and `trusted::checked_words_cross_heaps_and_unknown_pages_are_refused`.
- `cargo test -p eml_native --test integration` -> `223 passed; 1 ignored` (218 + my 5); the repro prints: leak run1 `Leak`, run2 `Leak`, `call` `Ok([I64(3)])`, clean `execute` on the same config `ok`; after the fatal run `ok`, `ok`; VM: `ok` after the leak and after the fatal run.
- `cargo test -p eml_nrt` -> `76 passed`, `28 passed`. `cargo test -p eml_vm --lib` -> `87 passed`. `cargo test -p eml_vm --test integration` -> `580 passed`. `cargo test -p eml_sched` -> `81`, `1`. `cargo test -p eml_rows` -> `18`, `6`. `cargo test -p eml_cli --test integration` -> `264 passed; 0 failed; 2 ignored` (UI at 1, 2 and 8 workers, transcripts with recovery).
- rt gate: `bench/rt-gate.sh 4f6d9ea7 5` on the prototype commit -> exit 0. Every gated row is within +0.022 % (largest: `yield` +0.022 %, `string-eq-early` +0.010 %, `find-byte-1mib` +0.009 %, `compare` +0.007 %; `dup` -0.032 %, `free` +0.000 %, `map-reuse` -0.025 %, `find-bytes-1mib` -0.065 %); the sum over the gated rows is -2,988,598 instructions. Ungated rows moved as they do (`empty` +0.532 %, `read-all-bytes` +0.178 %, `read-all-bytes-std` -0.286 %) [V].
- Miri: on the prototype, `-Zmiri-strict-provenance`: `cargo miri test -p eml_rt --lib -- trusted::adv_ trusted::checked_words_cross_heaps` -> `5 passed`; with `-Zmiri-tree-borrows` added and `trusted::checked::a_panicking_finalizer trusted::fast::a_panicking_finalizer` -> `9 passed` (the four probes include the drop that unwinds, the recovery that unwinds, and two heaps on one table across a recovery) [V].
- Not run: `cargo nextest run` as a whole, clippy, `cargo fmt`, verusfmt, TSan, ASan, the full Miri `trusted::` budget.

### 4.5 Trust, gate

- T15 keeps its row. Its pinned items change from {`empty_table`, `for_control`, `reset_ids`} to {`Owners` (opaque struct), `alone`, `of`}: ten items before, ten after. No new row, no `assume(`, no `unsafe impl`. `trust.rs` changes by those six lines, so NC-G4 / SC-6 get the exception of section 8.
- rt gate (crates/eml_rt/CLAUDE.md): required. Fast code changes in two places: `Heap::with_control` and `Ctx::for_worker` each call one more private function, and `IdCounts` keeps its size (`Option<Arc<_>>` + `Vec`). If a gated row moves over +0.1 %: mark `counting` and `around` `#[inline(always)]`, re-run Verus and the gate; if it still moves, write the two heap constructors out (no `counting`).

## 5. Tests (NC-4b): N = 5271 + 6 = 5277; NC-5 on: 5283

New, written first, red at the base:

| # | Test | Crate / file | Asserts |
|---|---|---|---|
| 1 | `a_heap_made_after_a_dropped_one_counts_from_zero` | `eml_rt` `heap/tests/workers.rs` | a heap allocates a list of 3 and is dropped; a second `with_control` heap on the same `Control`: `owned_by(0) == 0` and `check_all` is `Ok` empty, with a list of 2, and after releasing it. Base: `OwnedMiscount { worker: 0 }` |
| 2 | `heaps_of_one_control_made_apart_count_apart` | same | `a`, `b` both `with_control` on one `Control`: `a.fork_worker() == 1` and `b.fork_worker() == 1`; `b` allocates 2: `a.owned_by(0) == 0`, `b.owned_by(0) == 2`, each passes `check_all`. Base: IDs 1 and 2, `a.owned_by(0) == 2` |
| 3 | `a_refused_recovery_leaves_the_table_as_it_was` (`Checked`) | `eml_rt` `heap/tests/recover.rs` | a root holding a list of 2 and a `lin` object: `owned_by(0) == 4`; `recover()` is `Err(Unrecoverable)`; `owned_by(0) == 4`, `check_all` `Ok`. Base: 2 [P] |
| 4 | `a_finalizer_that_panics_in_a_drop_leaves_no_count` (`Checked`) | `eml_rt` `heap/tests/trusted.rs` (crosses `Drop` and an unwind; Miri) | probe A1 with assertions: the drop unwinds; a new heap on the same `Control` has `owned_by(0) == 0` and passes `check_all`. Base: 6 |
| 5 | `checked::a_run_after_a_leak_reports_its_own_leak` | `eml_native` `tests/checked.rs`, helper `run_twice` of `nc4b-tests.patch` | both runs of the leaking program on one machine return `RuntimeError::Leak(vec![("Data".to_string(), 1)])`; a clean program through `eml_native::execute` with the same `RunConfig` is `Ok`. Base: run 2 is `OwnedMiscount { worker: 0 }` |
| 6 | `checked::fatal::a_run_after_a_fatal_error_counts_from_zero` | same | a program that keeps one object and decrefs another twice: both runs `HeapError::UseAfterFree`; the clean program is `Ok`. Base: `OwnedMiscount { worker: 0 }` |

Changed existing tests (7.4):
- Kind 2: `workers::check_owned_refuses_a_heap_of_another_control` -> `a_check_refuses_a_heap_of_another_table`, `should_panic(expected = "an owner check over a heap of another table")`, body `Owners::new().check(&[&heap])`. `trust::the_trust_list_is_exactly_the_trusted_items`: the six list lines.
- Kind 3 (expectations byte-identical): `workers::two_heaps`, `trusted::two_heaps` and `remote.rs`'s pair make the second heap `with_owners(.., &a.owners())`; `check_all` and every `control.check_owned(..)` call become `x.owners().check(..)`; `trusted::checked_words_cross_heaps_and_unknown_pages_are_refused` makes `b` on `a`'s table; `workers::heaps_of_one_control_hand_out_ids_from_one_table` -> `heaps_made_on_one_handle_hand_out_ids_from_one_table`; `workers::check_owned_counts_every_heap_of_the_control` -> `a_check_counts_every_heap_of_the_table`; `workers::check_owned_does_nothing_in_fast_mode` -> `a_check_does_nothing_in_fast_mode`; `workers::recovery_resets_the_table` -> `recovery_counts_in_a_new_table` (doc: "Recovery makes the new heap on a new table: the IDs and counts of the abandoned heap are not in it, and only the copies are counted."); the two `eml_vm` `sched.rs` fixtures.
- No VM test is added: the VM is covered by its 580 integration tests at 2 and 8 workers in checked mode, which fail if a worker's heap is on another table.

## 6. Docs (EN drafts; Japanese through PLaMo, `form: plain`, no space between Japanese and Latin)

docs/spec/object-model.md:
- 「論理的な worker ID」, anchor 「誤りと `exit` で捨てた run の ID は、回復が表を消すときに一緒に消える (下の「回復」)」 -> "The IDs of a run abandoned by an error or `exit` go with its table: recovery makes the surviving heap on a new table (「回復」 below)."
- same section, the bullet anchored 「数と ID の表 (`WorkerIds`) は `Control` に置き」, replaced whole: "The counts and the IDs are in an owner table. A table belongs to the heaps that count in it, not to the `Control`. `Heap::with_control` makes a heap with a table of its own; `Heap::with_owners(control, &owners)` makes a heap that counts in the table of the handle `owners` (`Owners`); `Heap::owners()` returns the handle of a heap's table. The engine makes every heap of one run or session on one table (`Ctx::for_worker(control, &owners, host, k)`), because objects move between those heaps: an object on another heap's pages is counted under the owner ID in its prefix. The smallest ID not yet issued is an `AtomicU32` and the retired IDs are under a `Mutex`. The counts are Relaxed `AtomicU64`s in chunks of 64 IDs; the chunks are shared by `Arc` and each heap keeps a copy of the list, so that a count changes without a lock, and the copy is refreshed under the lock only when an ID outside it comes. A table lives as long as a heap or a handle holds it and is never cleared. Dropping a heap writes no count; a table goes when its last heap and handle go. So a run that ended with a leak or through the fatal path leaves nothing for a later run on the same `Control`, which counts in its own table. In the REPL, heap 0 and its objects stay across inputs and so does their table; recovery replaces it (「回復」 below). A heap is dropped while its table stays in use only when it holds no counted object, that is, when the run's end check found no leak on it; otherwise the engine recovers or drops every heap of the table. A fast heap has no table." Held by: tests 1, 2, 4; `heaps_made_on_one_handle_hand_out_ids_from_one_table`.
- same section, anchor 「数え直しは `Control::check_owned(&[&Heap])` である」: `Owners::check(&[&Heap])`; 「同じ `Control` のすべてのヒープ」 -> "every heap of the table"; 「ほかの `Control` のヒープを渡すと panic する」 -> "it panics when given a heap of another table".
- 「報告する誤り」 (anchor 「`Control::check_owned` が数え直した」) and 「別のスレッドからの解放」 (two mentions of `Control::check_owned`): the name only.
- 「欄」, anchor 「今の論理的な worker ID はヒープに置き、ID の表は `Control` に置く」 -> "The current logical worker ID is in the heap, and the heap holds its owner table (「論理的な worker ID」 above)."
- 「回復」 step 1, from 「なければ `Control` の ID の表を消す。」 to 「VM はセッションを使えなくする」 -> "otherwise it makes an empty heap with the old heap's `Control`, worker ID and head of the frozen region, on a new owner table. The new table has the base ID 0 and the inherited worker ID and nothing else: the copies are counted in it as they are allocated, and the IDs and counts of the abandoned run stay in the old table, which goes with the old heap and the other heaps of that run. On `Unrecoverable` the half-built heap and its table are dropped and the old table is as it was; the engine makes the session unusable."
- 「回復」, the bullet anchored 「回復の誤りは `Unrecoverable` と」: 「`Control` の ID の表は手順1で消したままである」 -> "the old heap's table is unchanged"; 「`NotQuiescent` は表を消して新しいヒープを作る前に返るので、表は残る」 -> "`NotQuiescent` returns before the new heap is made". Held by: test 3.
- 「実行の制御」: delete the table row anchored 「| ID の表 |」; in the bullet anchored 「確保器に`Control`を渡すため」: "`Ctx::for_worker(control, &owners, host, k)` makes its heap with `Heap::with_owners(control, &owners)` and copies worker `k`'s stop word; `Ctx::new(control, host)` is worker 0 on a heap with a table of its own"; the bullet anchored 「1つの `Control` を同時に使う `Machine` は1つだけである」 -> "Only one `Machine` uses a `Control` at a time: the stop words, the exit field and `begin_run` are the `Control`'s, so two machines would stop and end each other. A copied `RunConfig` shares the `Control`. The owner tables are not involved: each machine's heaps count in their own table."
- 「`unsafe` の核」 T15: 「`WorkerIds::empty_table`、`IdCounts::detached`、`for_control`、`enter_id`、`count_id`、`fork_id`、`retire_id`、`reset_ids` は」 -> "`Owners`, the handle of a table, is opaque as well. `IdCounts::detached`, `alone`, `of`, `enter_id`, `count_id`, `fork_id` and `retire_id`".

docs/spec/native.md:
- 「検査モード」: delete the bullet anchored 「検査モードで、リークか失敗の手順で終わったrunは、その物体の数を`RunConfig`の`Control`のworkerごとの表に残す。」.
- 「実行の始めと終わり」: 「runのヒープについての`Control::check_owned`」 -> "`Owners::check` over the run's heap"; add after the item on the worker's `Ctx`: "The run makes its `Ctx` on a new owner table (`Owners::new`), so it counts from zero whatever earlier runs on the same `Control` left ([オブジェクトモデル](object-model.md)の「論理的な worker ID」). `NativeMachine::run` and `call` may be called again after a run that leaked or took the fatal path, and so may a run with a copy of the `RunConfig`." Held by: tests 5, 6.

docs/spec/vm.md: 「追記」, anchor 「`Unrecoverable` と `UnknownPage` のときは `Control` の ID の表はもう消してあり、`NotQuiescent` のときは消す前に返るので表は残る」 -> "in each of the three the old heap keeps its owner table as it was"; 「誤り、止めること、終わり」 item 4: `Control::check_owned` -> "`Owners::check` on the table of worker 0's heap; every worker's `Ctx` is made on that table". docs/spec/lir.md R-16: the name only.

docs/implementation/testing.md: 「単体テスト」 `heap/tests/workers.rs` bullet: `Owners::check` for `Control::check_owned`, and its last sentence -> "that two heaps made on one handle issue IDs and count in one table, that two heaps of one `Control` made apart count apart, that a heap made after a dropped one counts from zero, that a check refuses a heap of another table, and that recovery counts its copies in a new table; a refused recovery is in `heap/tests/recover.rs`"; the `heap/tests/remote.rs` bullet and the Miri list: the name; the Miri list gains "a destructor that panics while a heap is dropped leaves no count for a later heap of the same `Control`"; 「`eml_native` の検証」 `tests/checked.rs` bullet gains tests 5 and 6 in one sentence. docs/future/multicore.md, anchor 「表は`Control`に置き、同じ`Control`のどのヒープもRelaxedのatomicで数える」 -> "the table belongs to the heaps of a run or session, each of which counts with Relaxed atomics".

Comments (same texts, shortened): `control.rs` module doc (drop the table from what `Control` holds), docs of `WorkerIds`, `IdCounts`; `heap/mod.rs` the `owned` field and `fresh`; `heap/walk.rs:932`; `heap/share.rs:2324`; `lib.rs:73`; `eml_vm/src/lib.rs:121` (「止める語と終わりの欄が`Control`にあるためである」); `eml_native/src/jit.rs`, doc of `run`: delete from 「検査モードでは、リークか失敗の手順で終わったrunは」 to 「報告しうる (docs/spec/native.mdの「検査モード」)。」 and put "`run` may be called again whatever the last run's outcome: each run counts in a table of its own (docs/spec/native.mdの「実行の始めと終わり」)."

CLAUDE.md, crates/eml_rt/CLAUDE.md: no change.

## 7. Completion criteria of NC-4b

1. Red step: the six tests written, the helper edits not yet applied: `cargo test -p eml_rt --lib` -> tests 1-4 fail; `cargo test -p eml_native --test integration -- checked::a_run_after checked::fatal::a_run_after` -> 2 failed with `OwnedMiscount { worker: 0 }`.
2. After the change, one binary at a time: `cargo test -p eml_rt --lib` -> `942 passed; 0 failed; 1 ignored`; `-p eml_native --test integration` -> the count at 33993502 + 2 (218 at 4f6d9ea7 [V]; 33993502 adds tests to `checked.rs` and `clif.rs`); `-p eml_vm --lib` -> 87; `-p eml_vm --test integration` -> 580; `-p eml_nrt` -> the counts at 33993502 (76 and 28 at 4f6d9ea7 [V]); `-p eml_cli --test integration` passes.
3. `cargo clean -p eml_rt`, `cargo verus verify -p eml_rt` -> `1276 verified, 0 errors` (or the recorded number with the reason); `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` exit 0.
4. Miri twice (`-Zmiri-strict-provenance`, then with `-Zmiri-tree-borrows`): `cargo miri test -p eml_rt --lib -- trusted::a_finalizer_that_panics_in_a_drop trusted::checked_words_cross_heaps trusted::checked::a_panicking_finalizer trusted::fast::a_panicking_finalizer` pass.
5. `git grep -n 'check_owned\|reset_ids\|for_control' -- crates docs ':!docs/plans'` -> empty; `git grep -n 'OwnedMiscount' -- docs/spec/native.md crates/eml_native/src` -> empty; `git grep -n '表を消\|表は消' -- docs crates ':!docs/plans'` -> empty.
6. `git diff --no-ext-diff HEAD~1 HEAD -- crates/eml_rt/src/trust.rs`: three `-` lines (`empty_table`, `for_control`, `reset_ids`) and three `+` lines (`Owners`, `alone`, `of`).
7. G with N = 5277; clippy; `cargo fmt`; then on the clean tree `bench/rt-gate.sh <the commit before NC-4b>` exit 0 before anything is pushed or NC-5 starts.

## 8. Plan amendment (docs/plans/2026-11-17-native-checked.md) and D-19

- Header, new line (the date and the approval are filled in when the user approves the unit): "Amendment of <date> (<approval>): unit NC-4b (7.1, 7.2; S-NC-17; D-19 (l)) removes the limit NC-4 recorded. The per-ID counts move from the `Control` to an owner table that belongs to the heaps counting in it (`eml_rt::Owners`); recovery makes a new table; `reset_ids` and `Control::check_owned` are deleted. N: NC-4b 5277, NC-5 on 5283."
- 5.1 NC-G4: "Applies" becomes `NC-2, NC-4b`; add: "NC-4b replaces three items of T15 (`empty_table`, `for_control`, `reset_ids`) by three (`Owners`, `alone`, `of`); no row is added and no `assume(` or `unsafe impl`." 7.3 SC-6 gains: "NC-4b: the grep counts 3 added lines (`Owners`, `alone`, `of`) and the `trust.rs` diff is the six lines of 7.2 NC-4b."
- 5.4 new entry S-NC-17: section 4.1 of this design, verbatim.
- 7.1 new row after NC-4; NC-5's "After" becomes NC-4b: `| NC-4b | \`Give the heaps of a run their own owner table\` | NC-4 | S-NC-17; D-19 (l); NC-G4; 10.2 | Opus from the appendix patch and this section; Fable reviews the staged diff and 7.5 before the commit (the trusted core) |`
- 7.2 new unit NC-4b: Work = section 4.1; New tests = section 5; Docs = section 6; Completion = section 7. Left to the implementer: the Japanese texts, the final test bodies.
- 7.2 later units: NC-5 "N = 5271 - 1 + 2 + 5 = 5277" -> "N = 5277 - 1 + 2 + 5 = 5283"; "G (N = 5277)" -> "G (N = 5283)" in NC-5 and NC-6; 7.3.1 item 1: "... 5271 (NC-4), 5277 (NC-4b), 5283 (NC-5 on)"; NC-7 step 6: the message names NC-4b.
- 7.4: kinds as in section 5; "New tests (cumulative N)": after the NC-4 entry (5271) insert "NC-4b +6 -> 5277", and NC-5 becomes "+7 -1 -> 5283".
- 7.5 new row: "The owner table and its heaps (NC-4b) | The table is reached only through a heap's `IdCounts` or an `Owners` handle, both `Arc`s of one `WorkerIds`; no heap operation changed. `Heap::drop` reads and writes what it did. A wrong count is a wrong diagnostic (`heap/mod.rs` on `owned`), and a heap made on the wrong table shows as `OwnedMiscount` or `TransferLeak` at the first checked end check. T15 keeps its row with three items replaced; `Owners` is `Send + Sync` because `WorkerIds` is (asserted in `lib.rs`)."
- 10.2 new paragraph: "NC-4b: after its commit, on the clean tree, `bench/rt-gate.sh <the commit before NC-4b>`, default 5 pairs, no `--allow`. On the prototype: `bench/rt-gate.sh 4f6d9ea7 5` exit 0, every gated row within +0.022 % (`yield`), sum -2,988,598 instructions. Remedy if a gated row moves: `#[inline(always)]` on `counting` and `around`; then the constructors written out."
- Appendix: copy `nc4b-owners-proto.patch` as `nc-4b-rt.patch` and `nc4b-owners-proto-tests.patch` as `nc-4b-tests.patch` (deleted at NC-7), with the three differences of 4.1 stated in the unit.
- 00 §D D-19: in (g) "Units: nine (NC-3b and NC-4b added)". Append: "(l) (<date>, NC-4b) The per-ID counts and the IDs of checked mode are not in the `Control`. They are in an owner table that belongs to the heaps counting in it: a heap made alone has its own, the heaps of one run or session are made on one handle (`eml_rt::Owners`), and recovery makes the new heap on a new table. No code clears or subtracts counts; `reset_ids` and `Control::check_owned` are deleted (`Owners::check` recounts). A `Control` can be used again after any run, on both engines. Engines keep three rules: heaps that exchange objects share a table; the check gets every heap of the table; a heap leaves a table that stays in use only without counted objects. `jit-tasks` puts the handle in the run and `jit-repl` takes it from the session's heap; neither adds a reset."

## 9. Part A: the crate hierarchy

### 9.1 Target graph

The end state of 01 §13.1, unchanged (19 directories): runtime closure `eml_extern`, `eml_rt`, `eml_steps`, `eml_io`, `eml_cabi`, `eml_sched`, `eml_nrt` (+ `eml_tls`, `eml_gui` behind features); tools: the front end, `eml_lir`, `eml_core_ir`, `eml_native`, `eml_clink`; artifact `eml_cli`; dev `eml_test_support`. I questioned each crate that two engines share:

| Crate | Verdict | Reason |
|---|---|---|
| `eml_vm` (16,072 + 21,622 test lines) | delete | one engine |
| `eml_rows` (2,831) | merge into `eml_nrt` by 01 §13.3's file map | its only reason was two consumers; `rows::call`, `SHARED` and the two-list test go |
| `eml_sched` (11,101; `cells.rs` is 10,083) | keep | three cargo-enforced properties that do not depend on the VM: it cannot name `eml_fatal` or an entry, it has no `extern "C"` or asm (all of it runs under Miri and the sanitizers), `tls` / `gui` never reach it. With the VM gone, `sched-core` may split and narrow `cells.rs` instead of adding beside it |
| `eml_cabi` (73) | keep | merging into `eml_clink` is impossible (`eml_nrt` consumes `CCall` / `Dtor`, and a runtime crate cannot depend on a tool); merging into `eml_extern` puts `unsafe` and code pointers into the leaf the front end reads |
| `eml_clink`, `eml_extern`, `eml_io` | keep | reasons independent of the VM (01 §13.1) |
| `eml_steps` | new at `sched-core` | as 01 |

Where run state lives:

| State | Crate | Why |
|---|---|---|
| `Control`, `Limits`, `Stop`, `Owners`, `Ctx::recover` | `eml_rt` | `Ctx` and `Heap` read them at every safepoint, page and count; verified code |
| `RunConfig`, `Finished`, `RunStats`, `RuntimeError`, `RunFault`, the run (`Run`, which holds the run's `Owners` from `jit-tasks`), engine-level recovery (`recover.rs`) | `eml_sched` | the run interface and the scheduler |
| the session (root `Ctx`, roots, the worker struct), `Image`, entries, rows | `eml_nrt` | what generated code calls |
| code, the JIT region | `eml_native` | tool |

No crate rename: `eml_nrt` and `eml_native` keep their names until `native-aot` decides the split of `eml_native` (its 論点).

### 9.2 When each move happens

| Move | Stage | Cost | Risk |
|---|---|---|---|
| `Owners` in `eml_rt`; `Control` loses the table | NC-4b | 11 files, +198 / -85 on the prototype | rt gate |
| delete `eml_vm`; `eml_rows` -> `eml_nrt`; features to `eml_nrt`; `eml_sched`'s `testing` and `test_ctx` / `stream_on` deleted; `rows!`; `Finished` loses `preemptions`, `placed`, `placed_jobs`; `max_depth` -> `Options.stack`; `nix/crate-graph.sh` tables; `surface.rs` `SAFE` moved | `vm-retire`, pulled forward (Part C) | the stage's 9 units, minus what needs native tasks | the outage of Part C |
| `GateHost` deleted: `rows/gui.rs` calls the scheduler directly | `jit-tasks` (the unit that binds the GUI rows) | replaces 01's "second implementation, then delete" | none |
| `cells.rs` narrowed to what `eml_nrt` names (D-VR-5a) and split | `sched-core` SC-1 lists it; finished at `jit-tasks`' Close | replaces D-SC-3's "additions only" | review of 10k lines |
| `eml_steps`; edge `eml_sched -> eml_steps` | `sched-core` | as 01 | as 01 |

Interaction with the Control fix: none beyond NC-4b. The table needs no session object in another crate; the handle travels with the first heap.

## 10. Part B: Verus

### 10.1 Properties

| # | Property | Under the design | Verus today |
|---|---|---|---|
| P1 | a count is written only through a heap's copy of its table | by construction (`IdCounts` is the only writer; a copy is made by `alone` or `of`) | structural |
| P2 | dropping a heap changes no count; no double subtraction | no code subtracts | nothing to prove |
| P3 | after recovery the new heap's table holds exactly its copies | new table + P4 on the allocations | the "new table" half is the call of `with_control` in `fresh` |
| P4 | per operation, own pages: for every ID, (counts this heap has added) = (number of own blocks with state not freed, rc > 0 and that owner) | maintained at 8 call sites of `count_id` / `move_owned` | not proved; provable sequentially (10.2) |
| P5 | the same across heaps (frees and owner rewrites of another heap's blocks) | T14 operations | not provable in the sequential model: each heap's model has only its own pages and learns of foreign frees at `reclaim`. Needs `rt-concurrent-proofs` (deferred, 01 §14.4) |
| P6 | the recount lists every counted block exactly once | `owned_entries` over `Pages::blocks` | completeness is in the contract; "no block twice" is not (`alloc.rs:3053-3066`) |

### 10.2 Sketch for P4 and P6

```rust
// control.rs, inside verus!  (T15: one new pinned item, "uninterp net")
impl IdCounts { pub(crate) uninterp spec fn net(&self, id: u32) -> int; }   // counts this copy has added for `id`
// count_id: ensures final.net(id) == old.net(id) + (if up { 1 } else { -1 }), forall j != id: unchanged   (trusted: it defines the ledger)
// alone / of / detached: ensures forall id: net(id) == 0;   enter_id / fork_id: ensures net unchanged
// heap side
spec fn counted(core, h) -> bool { core.dom().contains(h) && core.state(h) != Freed && core.rc_of(h) > 0 }
spec fn census(core, id) -> nat { Set::new(|h| counted(core, h) && core.model@.owner[h] == id).len() }
spec fn counts_ok(&self) -> bool { M::CHECKED ==> forall|id| self.owned.net(id) == self.census(id) + self.pending(id) }
```
- `pending`: `alloc_header` counts before `publish` puts the block in the model, so the invariant carries the block under construction.
- Lemmas: finiteness of `dom` (precedent: `active()` with `vstd::set::lemma_set_remove_len` in `release`), `census` under insert, free, owner change and rc sign change (about 6 lemmas).
- Frame: every `&mut self` helper between a public operation and a `count_id` call needs `final(self).owned == old(self).owned`. The prototype needed it on `free_remote` and `free_box_slow` for `free_box` alone.
- `share` / `transfer_back` move `owned` into `Objects` and count inside the `Graph` operations `set_rc` and `flip`; the ledger has to enter the abstract graph's invariant.
- P6: add "no address twice" to the `ensures` of `blocks_in` and state `owned_entries`' result as a multiset.
- The claim holds under `full()` only (the model agrees), like every model fact of checked mode.

Prototype [V] (`nc4b-ledger-proto.patch`, on the recommended design): the `uninterp` ledger, the trusted `ensures` of `count_id`, and proved ledger `ensures` on `move_owned`, `alloc_header` (+1 for the current ID when checked and rc > 0) and `free_box` (-1 for the owner when checked and rc > 0, on the local, slow and remote exits) -> `cargo verus verify -p eml_rt`: `1278 verified, 0 errors`. Before the two frame clauses the same run failed with `postcondition not satisfied` at the exit after `free_box_slow`.

Effort [I]: the ledger half is cheap (4 of the 8 sites done in about 40 lines). The census half and the traversals are the cost: about 6 set lemmas, the `Graph` invariant in `share.rs` (2,400 lines) and `transfer.rs` (1,000 lines), `counts_ok` in the `ensures` of every public operation, each under the `eml_rt` gate and Verus time: 3-4 units.

### 10.3 Recommendation

- NC-4b: no proof work. P1-P3 hold by construction, which is the point of the design.
- Do not start P4 now. Its value is bounded: the counts are diagnostic, `Owners::check` verifies them at the end of every checked run, and P5 keeps the check necessary for any run with more than one heap. New later item `rt-owned-proofs` (text in 11.3), with P6 as its first, cheap unit.
- Stays trusted: T15 (the atomics, the ID allocation, `net`'s definition), T14 (foreign blocks), until `rt-concurrent-proofs`.

## 11. Part C: the roadmap

### 11.1 The order

| # | Stage | Needs | Goal | Deletes |
|---|---|---|---|---|
| 1 | `native-checked` (running) | - | as planned, plus NC-4b. NC-5's comparison with the checked VM is the last use of the VM as oracle and is `vm-retire`'s record for the files native code accepts | - |
| 2 | `vm-retire` | 1 | native code is the only engine; the crate graph of 01 §13.1 except `eml_steps`; one engine in every document | `eml_vm`, `eml_rows`, `--vm` / `--native`, `Engine`, the parity tests, `native: skip`, `rows::call`, `SHARED`, `eml_sched`'s `testing`, the VM's Miri, ASan and TSan lines, `max_depth` in words, vm.md's instruction and frame sections |
| 3 | `sched-core` | 2 | as 01 §10; `cells.rs` is reshaped, not frozen; docs/spec/runtime.md is the one spec of the task layer | D-SC-3's adapter rule, D-SC-10, G-12 |
| 4 | `jit-tasks` | 3 | as 01 §11; each unit turns its ignored tests and refused files back on; no second engine to compare with | `eml_native::unsupported`, `GateHost`, 「VMとの違い」 (never written), the both-engines steps of JT-5..JT-11, the parity half of JT-13 |
| 5 | `jit-repl` | 4 | as 01 §12.1 without the default switch; its Close records full coverage (the old D-VR-1 (a)-(j) without the VM comparisons): no refused file, no ignored outage test | the VM reference record, the outage count |
| 6 | `ir-origins` | - (placed after 5) | as planned; no VM adaptation (D-IO-9 goes); it also gives roles to the lowerings `jit-tasks` added and re-keys the REPL's same-name comparison | - |
| 7 | `native-prof` | 4, 6 | as planned; "`--prof` with the VM" goes | - |
| 8 | `sched-proofs` | 3 | as planned | `docs/plans/2026-11-17-runtime/` |
| 9 | `par-direct`, `native-aot`, `native-inline` | as today (`vm-retire` is already closed) | unchanged | - |

Why `vm-retire` goes first: after `native-checked` nothing needs the VM as an engine, and every later stage carries a cost for it. `sched-core` must leave `cells.rs` (10,083 lines shaped by the VM's 62 `unsafe` call sites) as it is and write runtime.md beside vm.md; `jit-tasks` runs five groups of test files on both engines, builds a parity harness, a difference list and a second `GateHost` implementation, all deleted two stages later; `Finished` could not gain two fields in `sched-core` because of the VM's struct literals (00 §D D-6); every `eml_rt` API change is adapted in a crate that is going away (NC-4b: 4 files).

What is red, and until when [V counts from the acceptance snapshots at 4f6d9ea7]:

| What | From | Until |
|---|---|---|
| 123 `run` + 34 `run-fail` UI files with an `Async` row; `bench/scripts.sh`, `bench/net.sh`, `bench/async` | `vm-retire` VR-4 | `jit-tasks` JT-7 (task rows lowered) |
| 25 `run` + 1 `run-fail` UI files with `par`; the `par` rows of `bench/run.sh` | VR-4 | JT-9 |
| the 2- and 8-worker and seed runs of `par/` and `async/` | VR-4 | JT-11, JT-13 |
| `eml repl`, the 20 transcripts, `terminal::` | VR-4 | `jit-repl` |
| the VM's task tests that move to `crates/eml_native/tests` ignored | VR-2 | the `jit-tasks` unit named in each `#[ignore]` reason |
| whole-run Miri (15 small runs), the VM's ASan and TSan lines | VR-4 | JT-6 (Miri row scripts), JT-14 (sanitizer lines); the whole-run Miri is a stated loss of 01 |

During the outage the 492 + 47 accepted files run checked native then fast native, and every refused file is still compiled to LIR with the verifiers on and its refusal reason pinned, so the front end and the IR passes keep full coverage. A program with tasks gets E0004; `eml repl` is a usage error.

Three mechanisms keep the outage honest (all in `vm-retire`):
- The VM reference record: before the deletion, one checked VM run of every `run` / `run-fail` file native code refuses writes its error class and its schedule-free `RunStats` fields to an appendix file; `jit-tasks`' harness compares native runs with the record instead of with a live VM. Deleted at `jit-repl`'s Close.
- The outage count: every test that waits for native tasks or the native REPL carries `#[ignore = "jit-tasks: <form>"]` or `#[ignore = "jit-repl"]`, and one test pins the count per reason, next to the existing `accepted / rejected` snapshot. Both numbers reach 0 at `jit-repl`'s Close.
- The fates of 01 §11.10 restated: files that drive programs through the common helpers (`tasks`, `chan`, `par`, `roots`, `run`, `spawns`, `evidence`, `reuse`, most of `ffi`, `io`, `pinned`, `signal`, `workers`) move now with their helpers pointed at native code; files about the VM's `Machine` (`machine`, `seat`, parts of `api`) move ignored for `jit-repl` / `jit-tasks`; the bytecode files are deleted as 01 says.

The one judgment call: `ir-origins` and `native-prof` after `jit-repl` reverses D-20 (b). `native-prof` needs `jit-tasks` either way, so the profiler arrives later only by `vm-retire` and `jit-repl`; in exchange the outage is `sched-core` + `jit-tasks` (+ `jit-repl` for the REPL) and nothing else. If the user keeps D-20 (b) (`ir-origins` right after `vm-retire`, `native-prof` right after `jit-tasks`), nothing else in this design changes; the outage grows by the length of `ir-origins` for programs and of `native-prof` for the REPL.

Risks: no second implementation while the scheduler is rebuilt (the UI outputs, the reference record and the VM's sources in git remain); a slip of `jit-tasks` lengthens a visible outage; sanitizer coverage of the real driver through whole programs is missing until JT-14 (`eml_io`'s own ASan, TSan and Miri lines stay).

### 11.2 What each plan file becomes

| File | Becomes |
|---|---|
| `2026-11-17-native-checked.md` | section 8; nothing else. NC-7's roadmap step also applies 11.3 |
| `2026-11-17-vm-retire.md` | rewritten as the next executable plan. Base: `Close native-checked`. D-VR-1 becomes: (a) NC-5's parity result on the accepted files, (b) the VM reference record, (c) the inventory of `unverified` tests from NC-5. Units: VR-1 plan, record, the list of every ignored test with its reason; VR-2 tests; VR-3 harness and CLI (`ui::run` checked then fast native; refused files compiled and pinned; `eml repl` refuses; the outage count; `-- stack:` marks); VR-4 delete `eml_vm`; VR-5 `eml_rows` -> `eml_nrt`; VR-6 `rows!` and the deletions (without the `cells.rs` narrowing); VR-7 docs (vm.md's scheduler, limits, I/O and run sections become docs/spec/runtime.md as the normative text of the task layer; status.md says tasks and the REPL are not implemented natively yet); VR-8 bench tables on native code for the programs it runs; VR-9 Close |
| `2026-11-17-sched-core.md` | base `Close vm-retire`; D-SC-3 becomes "SC-1 lists what `cells.rs` loses, keeps and splits"; D-SC-10 and G-12 deleted; the VM's files are read at the commit before VR-4, named in the plan; runtime.md already exists |
| `2026-11-17-jit-tasks.md` | base `Close sched-core`; each unit lists the ignored tests and refused forms it turns on; JT-13's parity is against the record; `GateHost` deleted at the GUI rows' unit; `Finished.claims` / `join_waits` / `reorders` may be added where first needed (D-6 loses its reason); D-ET-2 re-check stays; its Close narrows `cells.rs` (D-VR-5a) |
| `2026-11-17-jit-repl.md` | no `--vm`, no default switch; the Close condition is the full-coverage record |
| `2026-11-22-ir-origins.md` | base `Close jit-repl`; D-IO-9 deleted; scope gains the roles of the task lowerings and the REPL comparison's re-keying (D-20 (f)) |
| `2026-11-22-native-prof.md` | base `Close ir-origins`; the VM clause of its usage errors deleted |
| `2026-11-17-sched-proofs.md` | unchanged |
| `2026-11-17-runtime/01-design.md` | not edited (00 wins); 00 §D gets D-21 |

### 11.3 Roadmap text (EN drafts for docs/future/roadmap.md 「段の列」, in this order)

- `native-checked`: unchanged.
- `vm-retire`: "Delete `eml_vm` and make native code the only engine, before native code runs tasks. `eml_rows` is merged into `eml_nrt`; `rows::call`, `SHARED` and the feature `testing` go; the depth limit becomes a stack size in bytes; `eml run` runs native code and has no `--native` or `--vm`. Until `jit-tasks`, a program whose `main` has `Async` in its row or that uses `par` is refused with E0004, and until `jit-repl`, `eml repl` is a usage error. Before the deletion the stage records, for every UI file native code refuses, the checked VM's error class and schedule-free `RunStats`; `jit-tasks` compares with that record. Every test that waits for native tasks or the native REPL is ignored with the stage that turns it on, and one test pins their number. UI tests run each accepted file in checked and then fast native mode and compile every refused file to LIR. [VM](../spec/vm.md)'s task-layer sections become docs/spec/runtime.md. 前提: `native-checked`. 設計: [ネイティブ化の設計](native.md)の「確定した設計判断」"
- `sched-core`: today's text with "VMは自分のスケジューラを変えずに持ち、" deleted and, after the list of what the scheduler holds, "The task-cell core (`cells.rs`) is narrowed to what the new scheduler and the rows use and is split along the new files." 前提: `vm-retire`.
- `jit-tasks`: today's text with the sentence 「`eml_vm`を消すまで、…「VMとの違い」は2つの項を持つ。…」 replaced by "Each unit turns on the UI files and tests its forms were refused for; at the Close no program is refused and only the REPL's tests are ignored. Runs are compared with the record `vm-retire` took." and `--native` dropped from the flag sentence. 前提: `sched-core`.
- `jit-repl`: "Run the REPL on native code: code appended by name, session roots, recovery after a fatal error, `Async` inputs as root tasks, `--debug-heap` as checked native code. The Close records that every UI file, seed and worker count passes in checked and fast mode, that every negative test of checked mode fires, that the transcripts, the Miri row scripts, both sanitizers, the four CI targets and the census pass, and that no test is ignored for a missing engine feature; the record of the VM and the count of ignored tests are deleted. 前提: `jit-tasks`."
- `ir-origins`: today's text with 「VMは文の新しい型に合わせるだけである。」 deleted and "The lowerings of the task rows, `par` and the blocking C calls get their roles here." added. 前提: なし (placed after `jit-repl`).
- `native-prof`: unchanged but 前提: `ir-origins`, `jit-tasks` in the new order.
- `par-direct`, `native-aot`: the 前提 `vm-retire` is satisfied; in `par-direct` "VMのworker 1つで19,886,982,836命令" stays as a recorded number with "measured on the VM before it was deleted".
- 「その後の項目」, new item under 「処理系」: "`rt-owned-proofs`: prove in `eml_rt` that, on one heap's own pages, the counts a checked heap has added for an ID equal the number of its live positive-rc objects with that owner, and that the recount lists each such object once. 後に回した理由: the counts are diagnostic, `Owners::check` recounts them at the end of every checked run, and the cross-heap half needs `rt-concurrent-proofs`. 見直す条件: an `OwnedMiscount` with no engine cause; or the recount's cost shows in a checked run's profile; or `rt-concurrent-proofs` starts." Items that say "after `vm-retire`" (`native-backtrace`, `--stack-size`, 「JIT (投機的)」's deletion) keep their text; their condition is met earlier.

### 11.4 Decision-log entries (00 §D; §A needs the user)

- UJ-19 (proposed, for §A): "The VM is deleted directly after `native-checked`, before native code runs tasks and the REPL. Accepted: programs with tasks or `par` are refused, and the REPL is unavailable, until `jit-tasks` and `jit-repl`. This replaces UJ-15's 'behind a precondition gate' and UJ-17 (1)'s premise that the VM keeps its scheduler until deleted; G-20 and D-VR-1 become the Close condition of `jit-repl`."
- D-21 (proposed): "(a) Stage order: `native-checked`, `vm-retire`, `sched-core`, `jit-tasks`, `jit-repl`, `ir-origins`, `native-prof`, `sched-proofs`; 01 §1's table and §11.7, §11.10's 'both engines' columns, D-SC-3's 'additions only', D-SC-10, G-12, D-JR-5, D-VR-1, D-VR-3's parity clauses and D-VR-5a's place are restated by the rewritten plans. (b) `vm-retire` takes a record of the checked VM on the files native code refuses and pins the number of ignored tests; both are deleted at `jit-repl`'s Close. (c) D-6 is withdrawn: `Finished` gains its fields where first needed. (d) D-20 (b) is replaced: `ir-origins` and `native-prof` follow `jit-repl`; D-IO-9 is deleted; D-20 (f)'s `jit-tasks` and `jit-repl` clauses move into `ir-origins`. (e) D-14 (d)'s owners of docs/future text follow the new order. (f) New later item `rt-owned-proofs`."

## 12. Open decisions for the user

| # | Decision | Recommended |
|---|---|---|
| 1 | NC-4b: owner table per heap family (this design) or subtraction at drop (the previous one) | this design |
| 2 | Delete the VM directly after `native-checked` (UJ-19) | yes |
| 3 | The profiler stages after `jit-repl` (D-21 (d)) or as D-20 (b) | after `jit-repl` |
| 4 | `rt-owned-proofs` as a later item, not a stage | later item |

## Additional checks

- The scratch copy is a clone (`git clone` of the repository into the scratchpad, detached at 4f6d9ea7), not a `git worktree`: a worktree would have written into the main repository's `.git`. Branches `opt1` (previous proposal + probes), `opt2` (recommended prototype), `base-probes`.
- Some of my early `cargo test` runs overlapped the NC-4 implementer's runs in the main tree (the coordinator's note); every count above was read from a completed run.
- `Verus 1273 at the base` is the plan's number (D-19 (a)); I ran Verus only on the two prototypes.
- The effort figures of Part B and the unit counts of Part C are [I].
- 33993502 against 4f6d9ea7 touches `eml_native` (abi, jit, lower, `tests/checked.rs`, `tests/clif.rs`), `eml_nrt` (`constants.rs`, `image.rs`, `run.rs` docs), native.md and the plan; `run.rs:107` (`for_worker`) and `mode.rs:50` (`check_owned`) are as quoted; the plan's N at NC-4 is 5271 and NC-5 on 5277 [R, `git show` in the main repository, read-only]. The per-binary counts of `eml_native` and `eml_nrt` at 33993502 were not run.
