# Appendix 02: lending large arguments to `blocking` C calls (design, reviewed and revised)

English design deliverable for the plan `docs/plans/2026-11-11-borrowed-params.md`. Where it disagrees with the plan's section 5, the plan wins. Tags: [V] verified, [I] inferred. Base f918f5fb.


## 1. Conclusion

**Adopt, shrunk to large objects.** A `blocking` call lends an `UnboxedArray`, `MutUnboxedArray`, `String` or `Bytes` argument when the object sits on a large-object page (block above 128 words, i.e. above about 1 KiB; the object has the page to itself). Smaller arguments are copied exactly as today, and a small `MutUnboxedArray` is written back as today. Copies are therefore bounded at 128 words per argument instead of unbounded.

Changes from r1:
- Small objects are no longer lent (r1 pinned 64 KiB pages for them).
- Every path that returns a page to the OS honours pins, including the in-heap free of a large object (`release_large` through `dealloc_page`). r1 covered only `Drop for Pages`.
- With those two, **no use-after-free can follow from a violated liveness assumption**: a pinned page is never deallocated and holds no other object, so nothing is ever reused under C. R1/R2 are still required, but only for data-race freedom (nobody writes what C reads), not for memory staying mapped.
- The take-time re-validation (r1 G-F6) is dropped.

Answers:
- Q1: read-only kinds yes, `MutUnboxedArray` yes, both only for large objects.
- Q2: `eml_rt::PagePin` on the large page, owned by the job closure, dropped after C returns; a process-wide pin table consulted by the one function through which pages reach `dealloc`.
- Q3: worth it only for large arguments, which is exactly what is lent. Measured: copies cost under 0.02 us per call at 1 KiB per argument and 15-25 us per MiB above (section 2). No `RunStats` counter.
- Q4: docs say "large arguments are lent under a page pin, small ones are copied"; locations in section 5.
- Nothing goes to docs/future/roadmap.md 「その後の項目」.

Dependencies: R1 is discharged by S1's G-6 as written (section 4). R2 holds today [V]. Unit 1 changes `crates/eml_rt`; S1's G-14 must be scoped to the borrow work (the orchestrator has said it will be).

## 2. Rationale

### Why shrink to large objects

| | r1 (pin every object's page) | r2 (pin large pages only) |
|---|---|---|
| Retained by an abandoned, still-running call | 64 KiB per small argument (an 8-byte string keeps a 64 KiB page) | the large objects' own pages (object size, what the copy retains today) plus small copies (as today) |
| If the lent object is freed during the window (R1/R2 broken) | small block goes to the free list and is reused: C reads or corrupts another object | the page is parked, never reused: no memory-safety effect |
| `Drop for Pages` of heaps with no lends | lock per page | one atomic load per page |
| Benefit lost | none | copies up to 1 KiB per argument stay: under 0.02 us per call [V, table below] |
| Code | copy path deleted | copy path stays next to the lend path |

Safety beats concision here: the small-object lend is where the single-invariant weakness and the 64 KiB amplification both live, and it buys nothing measurable.

### Hazards and how each is closed

Lend window of a lent object `o`: from `FfiSubmit` on the owner thread until the C function returns on the pool thread (possibly after the run ended).

| Hazard | Closed by |
|---|---|
| (a) abandon: heap recovered or dropped while C reads/writes | `o`'s page is pinned for the whole window. Pages reach `dealloc` only in `dealloc_page` (called by `release_large`) and `Drop for Pages` [V: review r1 confirmed these are the only deallocs in `eml_rt`; `Frozen` holds a `Pages`]. Both go through one pin-aware function (unit 1). A pinned page is parked and returned by the last pin. |
| (a) abandon: `o` freed individually while C runs | Does not happen under R1/R2 (below). If it did, `release_large` parks the page: C keeps a mapped page that nothing else uses. |
| (a) other workers' heaps | Heaps 1..N-1 are only reclaimed (remote-freed blocks) and dropped whole: `others.clear()` at crates/eml_vm/src/lib.rs:584 (recovery), :801 (run end), :897 (`close`), :909 (`Drop`) [V: read]. Worker k quiesces before returning its `Ctx` (vm.md:587). Both paths end in `Drop for Pages`. |
| (b) cancellation / scope failure | Unchanged: the job is not stopped, the cancel is delivered after completion, the failing `run`/`scope` waits [V: vm.md 「`blocking` の呼び出し」; tests `a_cancelled_blocking_call_resumes_after_its_job`, `a_failing_scope_waits_for_its_cancelled_blocking_call`]. For a lent array the write is already in place, which is what write-back produced. |
| (c) another task writes a lent read-only object | Every in-place writer of `String`/`Bytes`/`UnboxedArray` requires `Heap::is_unique` (rc == 1) [V: object-model.md 「文字列の操作」「配列」, heap/mod.rs:3648]. The continuation holds a counted reference of its own (R1): if rc == 1 the only holder is the suspended continuation; otherwise rc != 1 and writers copy. Shared (rc < 0) and frozen (rc 0) objects are never written in place. |
| (c) owner thread reads a lent `MutUnboxedArray` C is writing | The array is lin, rc 1, reachable only from the suspended continuation. Heap walks (`live_objects`, `check_invariants`, `leaks`, `finalize_all`) follow only `scanned` words and `Raw` has scan 0 [V: every payload read in crates/eml_rt/src/heap/walk.rs is guarded by `self.core.scanned`; review r1 confirmed `Heap::drop` = `reclaim` + `finalize_all`, no payload access]. Recovery copies `Raw` only when root-reachable; a lin object never is. Walks are called only from `eml_vm` at run end, after recovery and in `close` [V: grep of `check_invariants|leaks|live_objects` in crates/eml_cli/src and crates/eml_test_support/src finds nothing]; a `Machine` is borrowed for the whole run, so no caller can walk mid-run. |
| (c) ordering of C's writes and the owner's later reads | Pool queue mutex (submit to C), inbox mutex + `wait`/`completions` + registry lock (C to take): the `RawBuf` edges (vm.md:562-563) [V: doc]. |
| (d) liveness | R1 = S1's G-6 (section 4). |
| (e) trust | New row T17; T16, T4 and T1 reworded (unit 1); `trust.rs` entries; Miri tests. |
| (f) virtual clock | Jobs run in place on worker 0's thread at `wait` or in the test driver's `quiesce`; no abandon exists. Same edges; pins are taken anyway (one path). |

R2 today [V]: `finish_run` (lib.rs:1505-1526) sweeps the registry only on a successful run, and a successful run cannot have an unfinished job cell (`left_at_end`, cells.rs:1706-1711); `run_recovering` (lib.rs:549-555) recovers after `Ran::Abandoned` and after every non-broken error; after a broken error only `close` (decrefs roots, then walks) and `Drop` remain; a panicked run's heaps are dropped whole (lib.rs:1441-1457, `Drop`).

### Cost in REPL sessions

No cost on runs that do not abandon. An abandoned call that is still running keeps, until C returns: its pool thread, its handle cells, its small copies, and the pages of its large arguments. That is the same amount of memory the copies hold today, and like the copies it is outside the `Control` budget (the page is refunded when its heap drops). It is not bounded across runs, as today: each run that abandons a never-returning call adds one such set. Recovery, `close`, the `exit` code and the continuing session are unchanged.

### Measurement [V]

Release build of the scratch clone, aarch64-darwin, `memcmp a a (8n)` on an `UnboxedArray Float` (two array arguments, so two copies per blocking call), min of 5 runs, startup subtracted. Probe source: section 5, unit 2 (`blk.em`; `syn.em` drops `blocking` and `Async` from the row).

| elements | bytes/arg | blocking us/call | sync us/call | blocking - sync - floor (= 2 copies) |
|---|---|---|---|---|
| 16 | 128 | 8.53 | 0.06 | 0.00 |
| 128 | 1 024 | 8.58 | 0.08 | 0.02 |
| 1 024 | 8 192 | 9.08 | 0.19 | 0.41 |
| 8 192 | 65 536 | 12.84 | 1.24 | 3.12 |
| 131 072 | 1 048 576 | 56.95 | 18.12 | 30.35 |
| 1 048 576 | 8 388 608 | 417.30 | 140.39 | 268.43 |
| 8 388 608 | 67 108 864 | 4461.24 | 1191.38 | 3261.38 |

The first two rows are what stays copied. From the third row on the argument is lent.

## 3. Alternatives considered and rejected

1. **Keep copying everything; record in roadmap.** Rejected: a safe design exists whose failure mode under a broken assumption is a leak, not corruption; the premise then says implement.
2. **Lend every object, pin its page (r1).** Rejected: 64 KiB retained per small argument of an abandoned call, and a freed small block is reused, so safety would rest on R1/R2 alone.
3. **Read-only kinds only.** Rejected: both kinds need the same pin; a large `MutUnboxedArray` is the case that copies twice. Its extra hazard (owner-thread reads) is closed [V].
4. **Refuse to abandon / poison the `Machine` / wait in driver drop (the `RawBuf` rule).** Rejected: a hung C call would hang `exit`/Ctrl-C or end the REPL session (vm.md:572, `ffi_abandon` tests).
5. **Retain whole heaps on abandon.** Rejected: unbounded per abandon.
6. **Pin table per `Control`.** Rejected: a frozen chunk's `Pages` has its own `Control` and `Heap::attach` does not compare it [V: frozen.rs:356-360, heap/mod.rs:1897]; a process-wide table keyed by page address needs no such condition, and a pinned page is never deallocated, so its address is unique while pinned.
7. **Pin only when a job is abandoned.** Rejected: needs a per-job state shared with the driver and an ordering argument across teardown.
8. **Record owns the arguments (submit consumes, take returns).** Rejected: changes lowering, rows, `OpKind::Job` and Core IR snapshots; G-6 already gives the reference.
9. **Count parked pages against `Control` until the real dealloc.** Rejected: a never-returning call would lower the session's limit for good; today's copies are not counted either.
10. **Re-validate lent arguments in `FfiTake` (r1 G-F6).** Rejected: checked mode already reports a freed argument at its next use after take (the returned lin array, or the decref of a read-only argument), and with pin-aware frees a violation is no longer a memory-safety event.
11. **Off-heap / immovable array type.** Already rejected in docs/future/ffi.md 「却下した案」.
12. **A `RunStats` counter for copied FFI bytes.** Rejected: what remains is bounded at 128 words per argument.

## 4. Assumptions made

- **R1 (liveness; discharged by S1 G-6).** Statement: *for every object argument of a `blocking` call, from `FfiSubmit` until `FfiTake` returns, the object's count is at least 1 through a counted reference that only the calling task's frame or its continuation can release; for `MutUnboxedArray` that reference is the only one.* Derivation from G-6, checkable line by line:
  1. The lowering puts the yield point `io_wait` between `submit` and `take`, and every object argument is an operand of `take` (vm.md 「`blocking` の呼び出し」). So each is an RC value live across that yield point. G-6 forbids S1 to change this lowering.
  2. G-6: such a value "MUST be held by the continuation closure through a counted reference of the closure's own, whether the variable was owned or borrowed in the yielding function", and the count stays at least 1 "from the capture until the resumed code's last use ... regardless of what frames below do". The last use is `take`.
  3. G-6: "between `submit` and the capture the value is valid in the frame (owned, or lent by a live owner)". This covers the wrapper case (a function whose own parameter is passed to the `blocking` row): S1's rule B1 makes a parameter that a continuation closure captures owned, and if it were borrowed the lender's frame is still on the stack until the yield has passed through it, by which time the closure holds its own count.
  4. `MutUnboxedArray` is lin; S1 allows only `unr` RC parameters to be borrowed, so it is moved into the continuation and stays unique.
  G-6 is sufficient; S1 needs to add nothing. G-6's "not guaranteed once the continuation is dropped (cancellation, unwind, quiesce abandon)" is covered here by R2 and the pin. Requested (not required) addition to S1's test list: a Perceus test that a function passing its parameter to a `blocking` row has that parameter owned.
- **R2 (teardown).** A continuation whose job was issued is released by decref only through `FfiTake` after the completion arrived; every other end drops it without touching the heap, and that heap is then recovered or dropped whole. True today [V, section 2].
- **R3 (declarer's contract, unchanged).** C uses a pointer only until it returns, only within the count or length, and writes only through a `MutUnboxedArray` pointer. New consequence to document: for a `blocking` call the return can be after the run ended.
- Masking the low 16 bits of an object word gives its page head for small, large and frozen blocks in both modes [V: review r1, alloc.rs:792, 861].
- A large-object page holds exactly one block [V: object-model.md 「ページ」 "その物体だけの揃えたページに置く"].
- Whether an object is on a large page is read from the page head (`class == LARGE`, alloc.rs:105, 1285), which is not written after the page is listed and is read by other threads by design [V: object-model.md 「ページ」].

## 5. Implications for implementation

Two commits, in this order. Japanese text goes through PLaMo as CLAUDE.md requires.

### Unit 1: `eml_rt` page pin (T17)

Files: `crates/eml_rt/src/alloc.rs`, `crates/eml_rt/src/heap/unboxed.rs` or `heap/mod.rs` (the `Heap` method), `crates/eml_rt/src/lib.rs`, `crates/eml_rt/src/trust.rs`, `crates/eml_rt/src/heap/tests/trusted.rs`, `docs/spec/object-model.md`.

Code:
- Table: `static PINS: Mutex<BTreeMap<usize, Pinned>>`, `Pinned { count: usize, parked: Option<Parked> }`, `Parked { page: *mut u8, bytes: usize }` (`unsafe impl Send`). `static PINNED: AtomicUsize` counts live pins and is changed only while holding the table lock. The lock is a leaf (no other lock, no allocation of heap pages, no `Control` call inside it); poison is ignored.
- `return_page(page: *mut u8, bytes: usize)`: the only caller of `std::alloc::dealloc` for pages. If `PINNED.load(SeqCst) == 0`, dealloc. Otherwise lock; if the table has `page.addr()`, set `parked` and return; else unlock and dealloc. Write it as a `#[verifier::external_body]` function inside `verus!` (so the `unsafe` is allowed by `trust.rs`).
- `dealloc_page` (existing `external_body`, T1): body becomes a call to `return_page`; its Verus contract does not change, so `release_large` and its proofs do not change.
- `impl Drop for Pages`: the per-page `dealloc` becomes `return_page(page, bytes)`; `forget_page` and `control.refund(bytes)` stay as they are (a parked page is refunded when its heap drops).
- `pub struct PagePin(usize)`: `Send`, not `Clone`. `impl Drop`: lock, `count -= 1`; at 0 remove the entry, `PINNED -= 1` per pin, take `parked`; after unlocking dealloc a parked page. Put the dealloc in an `external_body` helper or directly in the `Drop` body (both are accepted by `trust.rs`).
- `Heap::pin(&self, w: Word) -> Result<Option<PagePin>, HeapError>`: precondition and checked-mode checks as `Heap::meta(w)`; returns `Some` iff the block's page head has `class == LARGE`; then registers the page (lock, `count += 1`, `PINNED += 1`). Immediates and blocks on small pages give `None`. Must work for blocks of the calling heap, of another heap of the run, and of an attached frozen chunk.
- `#[doc(hidden)] pub fn PagePin::parked(addr: usize) -> bool`: whether the page containing `addr` is in the table with `parked` set. For tests (address-keyed, so parallel tests in one process do not disturb each other).
- Why the atomic fast path cannot miss a pin: a pin for page P is created by a thread that holds P's object valid (R1 step 3), so no `return_page(P)` can run at the same time; for every other page a miss is irrelevant.
- `trust.rs`: add `("alloc.rs: external_body return_page", "T17")`, `("alloc.rs: Drop for PagePin", "T17")` (plus an entry for a dealloc helper if one is added) and `"T17"` to the literal array (trust.rs:538-540).

Spec, same commit (docs/spec/object-model.md):
- 「確保器」: new subsection 「ページの留め」: `Heap::pin` and when it returns a pin; a pinned page is parked instead of returned, whether its block is freed or its `Pages` dropped; the last pin returns it on the thread that drops it; `Control` is refunded when the heap gives the page up, so a parked page is outside the budget; the table is process-wide (alternative 6); only large pages are pinned and why (section 2 table).
- 「確保と解放」 (the sentence that a freed large page goes to `std::alloc::dealloc` at once), 「別のスレッドからの解放」 (reclaim returns large pages), 「回復」, 「凍結した領域」の「寿命」: add "unless pinned, then parked (「ページの留め」)".
- 「`unsafe` の核」: list `PagePin` and the table among the items outside `verus!`. Reword **T4**: the `Pages` drop no longer calls `dealloc` itself, it hands each page to `return_page` (T17) and refunds `Control`. Reword **T1** for `dealloc_page` the same way. New row **T17** "ページの留め": `return_page`, `Drop for PagePin`, the table. Promise: a page with a live pin is never passed to `dealloc`; every page handed to `return_page` is passed to `dealloc` exactly once, by `return_page` or by the last pin, with the layout it was allocated with; the park-or-dealloc decision and the pin's removal are made under the table lock. Why trusted: `Drop` and the table are outside Verus. Miri: the tests below.

Tests (`trusted` module; every test drops all its pins, so Miri's leak check passes):
1. `a_pinned_large_page_outlives_its_heap`: array of 10 000 elements, `unboxed_ptr_mut`, `pin` is `Some`, drop the heap, assert `PagePin::parked(ptr)`, write the last element and read it back, drop the pin, assert `!PagePin::parked(ptr)`. **Fails in the default run if parking is missing** (the `parked` assertion).
2. `a_pinned_large_object_freed_by_its_heap_is_parked`: same object, pin, decref to free it while the heap lives, assert `parked`, read through the pointer, allocate other large objects, drop pin, drop heap. Covers `release_large`.
3. `a_small_object_is_not_pinned`: `pin` of an 8-element array, a short string and an immediate is `None`.
4. `two_pins_of_one_page_return_it_once`: two pins of one large object, heap drop, drop both.
5. `another_thread_writes_a_pinned_array_while_the_owner_allocates_and_walks`: a thread writes through the lent pointer while the owner allocates, frees, calls `check_invariants` and `leaks` (checked mode) and then drops the heap; join; drop pin. Under Miri a walk that read `Raw` elements is a reported race.
6. `a_pinned_frozen_page_outlives_its_chunk`: immortal string above 1 KiB, `freeze`, `text_payload`, pin, drop the last `Arc<Frozen>`, assert `parked`, read, drop pin.
7. `a_pinned_page_outlives_recovery`: root-reachable large `Bytes`, pin, `Ctx::recover`, assert `parked`, read the old position, compare with the copy, drop pin.

Verification (expected results):
- `cargo nextest run -p eml_rt` -> all pass, including `the_trust_list_is_exactly_the_trusted_items`.
- `cargo verus verify -p eml_rt` -> 0 errors.
- `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` -> exit 0.
- `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::` -> pass, no leak report; again with `-Zmiri-tree-borrows` -> pass; within the CPU budgets of crates/eml_rt/CLAUDE.md.
- `grep -n "dealloc(" crates/eml_rt/src/alloc.rs` -> hits only inside `return_page` and the pin's drop path.
- `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` -> clean.
- `bench/rt.sh` before and after: record in the commit message (one atomic load is added to each large free; not a gate).

### Unit 2: `eml_vm` lends large arguments in `FfiSubmit`; docs

Files: `crates/eml_vm/src/ffi.rs`, `crates/eml_rt/src/driver.rs` (doc of `Op::Job`), `crates/eml_vm/tests/{ffi.rs,ffi_abandon.rs,miri.rs}`, `crates/eml_vm/tests/miri/blocking.lir`, the tests' `rs_*` fixtures, docs below.

Code (`ffi.rs`):
- `enum Arg` gains `Lent(u64, PagePin)` (the exposed position from the same accessor the sync path uses, and the pin). `copy_args` (rename it, e.g. `job_args`) does, per `UnboxedArray`/`MutUnboxedArray`/`Text` argument: call the accessor; `heap.pin(w)?`; `Some(pin)` -> `Arg::Lent(position, pin)`; `None` -> the existing copy. Pins are taken on the owner thread before the job is handed to the driver. Range checks still run before any pin or copy.
- Closure: unchanged shape. All `Arg`s (so all pins) are dropped inside the closure after the bridge returns and before the answer is returned (G-F3).
- `CAnswer.back` becomes `Vec<Option<Box<[u64]>>>`, one entry per `MutUnboxedArray` argument in order, `None` for a lent one. `write_back` skips `None`.
- Rows, lowering, operands, disassembly and `OpKind::Job` (holds no heap words) do not change. `FfiTake` does nothing for lent arguments.

Spec and docs, same commit. Content:
- docs/spec/vm.md 「`blocking` の呼び出し」 (the bullets at lines 1100, 1117-1122): `FfiSubmit` lends an array/`String`/`Bytes` argument on a large-object page (the position `Op::Ffi` would pass, plus a `PagePin`) and copies the others; the job holds `CCall`, slots, copies, pins and handle cells and touches no `Ctx`, no RC and no heap memory except the lent ranges inside C; the window ends when C returns, even after the run; the rules R1 (citing [LIR] for the counted capture) and R2; `FfiTake` writes back only copied arrays. Also vm.md:563 (what a pool job may be lent: the file job's `RawBuf` and a `blocking` call's pinned large arguments), :572 (after abandon the heap is released; pinned pages are parked until C returns; an abandoned call keeps its pool thread, copies, pinned pages and handle cells), :776-779 (std pool jobs own their inputs; C jobs may also reach lent ranges), :1091.
- docs/spec/object-model.md T16: replace "`blocking`の呼び出しは何も貸さず…" with: a `blocking` call lends only objects on large pages; its window runs from `FfiSubmit` to C's return on the pool thread; during it only that C call reads or writes the range; the lender keeps a counted reference (R1/R2), nobody writes a lent read-only object, nobody else reads a lent `MutUnboxedArray`, and the page is pinned (T17). Also lines 438 and 559 (which calls lend).
- docs/spec/declarations.md 「`extern "C"`」 (lines 232-235): a pointer passed to a `blocking` call points to the argument itself or to a copy, the declarer cannot tell which, and is valid until C returns; C's writes to a `MutUnboxedArray` are in the array the caller receives; an abandoned call keeps its copies, the pages of its large arguments and its handles until it returns.
- docs/future/ffi.md 「長く走る呼び出し」 lines 89-97 (state the size rule and the pin; "every path that drops the continuation first quiesces the driver" becomes "quiesce may abandon; the pin covers that"), line 113 (「データの配置」: blocking calls lend large objects too); 「却下した案」: add alternatives 2 and 4 of this document.
- docs/future/async.md:243, 261, 274 and docs/future/multicore.md:84: the job receives `CCall`, slots, copies of small arguments and pins of large ones.
- docs/implementation/testing.md:227, 286 and docs/implementation/status.md:211: what the abandoned call uses and keeps; ASan checks that its writes land in a still-allocated page that is returned after C returns.
- `crates/eml_rt/src/driver.rs:64` (`Op::Job`): a job reaches memory through values it owns or ranges lent under T16 with a `PagePin` it owns.
- Completion criterion for the wording: the implementer reads every hit of `grep -rn "blocking" docs crates/eml_vm/src crates/eml_rt/src crates/eml_io/src` in the files above and confirms none still says that the job holds only copies or that no heap pointer is lent; the list above is the known set [V: grep at f918f5fb], not a guarantee that it is complete.

Test changes by kind (docs/implementation/testing.md 「テストの変更の運用」):
- Kind 1: none.
- Kind 2: none expected. Existing tests use small arguments and keep name, comment and assertions (they now state the small-object rule). No `RunStats`, LIR, Core IR, disassembly or UI snapshot changes are expected [I]; if one changes, stop and report.
- New tests (each under 1 s; `ffi_abandon` ones follow testing.md:286: 20 ms budget, open every gate before returning, no sleeps):
  - `ffi::a_blocking_call_receives_a_large_array_itself` (both modes; test driver and pool): a fixture returning its pointer argument as `Int`, called sync and `blocking` on the same 10 000-element array: equal. **Fails before unit 2.**
  - `ffi::a_blocking_call_receives_a_copy_of_a_small_array`: same with 3 elements: different.
  - `ffi::a_blocking_call_fills_a_large_array_on_the_pool`: C fills, eml reads first and last element.
  - `ffi::another_task_allocates_while_a_blocking_call_writes_a_large_array` (pool, checked and fast): gated fill; the other task allocates and drops strings and arrays, then opens the gate.
  - `ffi::a_blocking_call_reads_a_large_string_shared_with_another_worker` (2 workers) and `..._a_large_constant_string`.
  - `ffi_abandon::a_large_array_lent_to_an_abandoned_call_keeps_its_page`: gated fill that publishes its pointer argument through a static before waiting; after `run` and `close` return, assert `eml_rt::PagePin::parked(pointer)`; open the gate; wait for the fixture's done gate; assert `exit == Some(3)`. **Fails in the default run if the pin is missing**; ASan additionally checks the write.
  - `ffi_abandon::a_large_string_lent_to_an_abandoned_call_is_read_after_recovery`: interrupt instead of `exit`, a second run on the same `Machine` while the call still waits, `parked` asserted, then gate open; the fixture's byte sum arrives through a gate value.
  - `ffi_abandon::a_large_array_lent_on_worker_one_keeps_its_page` (2 workers; the lender is a task placed on worker 1).
  - `miri::a_blocking_call_lends_a_large_array_on_the_test_driver` (extend `tests/miri/blocking.lir`).

Verification (expected results):
- `cargo nextest run` -> all pass. `cargo nextest run -p eml_vm -E 'test(ffi::) | test(ffi_abandon::)'` -> all pass.
- `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::` -> pass; again with `-Zmiri-tree-borrows`.
- `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=address CARGO_TARGET_DIR=target/asan cargo test -Zbuild-std --target aarch64-apple-darwin -p eml_vm --test integration -- ffi:: ffi_abandon::` -> pass, no ASan and no leak report. Linux ASan and ThreadSanitizer (`ffi:: ffi_abandon::`, `eml_rt trusted::`) run in CI: expected no report.
- `cargo clippy --all-targets -- -D warnings`, the feature-combo clippy lines of CLAUDE.md, `cargo fmt --check` -> clean.
- Not a gate, record in the commit message: the probe table before and after (probe below). Expected direction only: rows of 8 KiB per argument and above lose most of the last column.

Probe (`blk.em`; `syn.em` is the same with `blocking` and `Async` removed from the `extern` row; replace `NNN`, `KKK`; release `eml run`, min of 5, subtract the `KKK = 0` time):

```
link on linux "libm.so.6"
link on macos "m"

import Async (Async)
import MutUnboxedArray
import UnboxedArray

extern "C" blocking memcmp : UnboxedArray Float -> UnboxedArray Float -> Int as size_t -> <IO, Async, Panic> Int as int

loop : UnboxedArray Float -> Int -> Int -> Int -> <IO, Async, Panic> Int
loop a n k acc = if k == 0 then acc else loop a n (k - 1) (acc + memcmp a a n)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let n = NNN
  let a = MutUnboxedArray.freeze (MutUnboxedArray.make n 1.5)
  println (show (loop a (n * 8) KKK 0))

main : Unit -> <IO, Panic> Unit
main () = Async.run body
```

Left to the implementer: names; whether `Drop for Pages` and `Drop for PagePin` call helpers; the fixture functions.

Stage close: benchmarks.md tables are regenerated as CLAUDE.md already requires.

## 6. Unresolved items (with defaults)

- **U1. Reading the page class for blocks outside the calling heap.** `Heap::pin` must read `class` for blocks of other heaps and frozen chunks. Default: reuse the verified page-head read that recovery uses for the `sink` of any reachable block (object-model.md 「回復」 step 2). If that read cannot give `class` without a new trusted item, return `None` (copy) for blocks whose page `sink` is not the calling heap's, drop tests "shared with another worker", "constant string" and trusted test 6, and say so in 「ページの留め」. Safe either way.
- **U2. S1 G-14.** Default: the orchestrator scopes it to the borrow work. If it stays as written, unit 1 cannot land and this task moves to roadmap 「その後の項目」 with 後に回した理由 "`eml_rt`を変えない段だった" and 見直す条件 "`eml_rt`の確保器を変える段 (`rt-layout`など) を進めるとき".
- **U3. Std.Tls job inputs.** Default: unchanged (not C, small inputs).
- **U4. Miri CPU budget.** If `trusted::` exceeds the budgets, merge tests 1, 3 and 4 before raising a budget.

## 7. Additional checks

r1: read `crates/eml_vm/src/ffi.rs` in full; `alloc.rs` 3290-3410; `frozen.rs` 356-364, 530-542; `control.rs` (`Control`); `trust.rs` 1-60, 225-262, 430-560; `eml_vm/src/lib.rs` 500-600, 1470-1530; `cells.rs` 1700-1730; the spec sections named in the briefing; built a release `eml` in the scratch clone and ran the probe.

r2:
- Read the review and S1's r1 (section 1, section 2 "Continuations", G-5, G-6, G-14).
- `grep -n others crates/eml_vm/src/lib.rs` and read lib.rs 780-800, 885-912: other workers' heaps are only reclaimed and cleared whole.
- `grep` of `scanned` in `heap/walk.rs`: walk reads are guarded by `scanned`. `grep` of walk entry points in `crates/eml_cli/src` and `crates/eml_test_support/src`: none.
- `grep` of `LARGE|block_words|page_of` in `alloc.rs`: `class == LARGE` exists as an exec check (alloc.rs:1285) for the heap's own blocks; other-heap reads not confirmed (U1).
- Read the T4 row of object-model.md.
- Not checked: sanitizer behaviour of the design (no implementation); Windows; whether the REPL's driver is kept across an abandoning run (review C-1 says it is dropped; vm.md:572 says abandon does not break the driver; the retention statement in section 2 does not depend on it).

## 8. Conformance criteria

**G-F1. Only pinned, large-page memory is lent past a suspension.** Code that hands a pointer into an eml heap object to code that can run after the lender's task is suspended (pool job, the native `blocking` lowering) MUST obtain `Heap::pin` for the object on the lending thread first, MUST lend only if it returns a pin, and MUST copy otherwise. The borrower owns the pin.
- Why: quiesce can abandon the borrower; a small block cannot be protected from reuse.
- Applies to: `eml_vm::ffi`, native `blocking` submit (`jit-core`, `jit-externs`), any new `Op::Job` that reaches heap memory. Not to `RawBuf` ops (never abandoned) or synchronous calls.
- How to check: `ffi_abandon::*keeps_its_page` (default run and ASan); `ffi::a_blocking_call_receives_a_copy_of_a_small_array`.

**G-F2. Pages reach `dealloc` only through `return_page`.** Any code that returns a page to the OS or reuses its memory (returning small pages during a run, a moving collector) MUST park a pinned page.
- Why: the pin is what keeps an abandoned C call's memory mapped and unshared.
- Applies to: `eml_rt::alloc`, future allocator work.
- How to check: `grep -n "dealloc(" crates/eml_rt/src`; trusted tests 1, 2, 6, 7.

**G-F3. Pins are dropped by the borrower after C returns and before its answer is delivered.**
- Why: no pin outlives the call, so pages are parked only for abandoned calls; and no pin is dropped while C still uses the pointer.
- Applies to: the job closure, native equivalents.
- How to check: review of the closure (the `Arg`s are dropped after the bridge call, inside the closure); trusted test 1 asserts `!PagePin::parked` after the pin is dropped.

**G-F4. The continuation holds the arguments and only take releases it.** Lowerings of a `blocking` call MUST keep every object argument as an operand of the take step with the yield point between submit and take (so S1 G-6 applies). Teardown MUST NOT decref the continuation of a task whose job was issued and not taken; it may drop it without touching the heap and then recover or drop the heap whole.
- Why: a counted reference held by a suspended task is what stops another holder from being unique and writing in place while C reads.
- Applies to: `eml_core_ir` translate, S1 (G-6), `cells.rs` teardown, `finish_run`, native unwinding and the fatal-error path of `jit-effects`.
- How to check: S1's G-6 checks; checked mode over `tests/ui/run/async/blocking_fixture_*` and `ffi::`; review of changes to `sweep_all`, `left_at_end`, `run_recovering`.

**G-F5. Nobody but C reads or writes a lent `MutUnboxedArray`'s elements, and nobody writes a lent read-only object, during the window.** Heap walks MUST read only scanned words. In-place writers of `String`/`Bytes`/`UnboxedArray` MUST require `Heap::is_unique`. No pass may place a use of an object argument between submit and take.
- Why: C runs without synchronisation with the owner thread.
- Applies to: `eml_rt` walks and checked-mode checks, new in-place rows, LIR passes, native inlined writes (`native-inline`).
- How to check: trusted test 5 under Miri; ThreadSanitizer on `ffi::`.

**G-F6. The pool thread touches no `Ctx`, no RC and allocates nothing in a heap.** A job reaches heap memory only through lent ranges inside the C call; the only runtime state it changes is the pin table.
- Why: per-worker heaps and checked mode have no rule for a second thread.
- Applies to: every `Op::Job` closure.
- How to check: review of the closure's captures.

**G-F7. A new lending boundary extends T16 and reuses `PagePin`.** It MUST state its window, reader and writer in the T16 row, MUST NOT add a second keep-alive mechanism, and MUST add a Miri test in `crates/eml_rt/src/heap/tests/trusted.rs`.
- Why: crates/eml_rt/CLAUDE.md gate; one mechanism keeps G-F2 checkable.
- Applies to: `jit-externs`, Std.Tls jobs if they ever lend, GUI.
- How to check: `the_trust_list_is_exactly_the_trusted_items`; review against 「`unsafe` の核」.

