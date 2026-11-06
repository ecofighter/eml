# F1 (r2): Rust work on the blocking pool that owns a `Lin` `Foreign` value

Revised deliverable of the pool-job guideline. G-4, G-5, G-9 and the §5 bullet on take errors are the amended texts the user adopted. The response table to the review is not kept.

---

# Part A: revised deliverable

## 1. Conclusion

One generic mechanism, "pool job rows", with these parts:

1. **Record** (`crates/eml_vm/src/cells.rs`): `OpKind::Ffi { call, ret: Option<u64> }` becomes
   ```rust
   OpKind::Job { from: JobSource, answer: Option<Box<dyn Any + Send>> }
   #[derive(Clone, Copy, PartialEq, Eq, Debug)]
   pub(crate) enum JobSource { C(u32) /* CodeBase C-function index */, Std(eml_extern::Extern) }
   ```
   The record holds **no heap words**. The completion arm stores the `Done::Job` box untouched (no downcast there). `OpKind::prefix()` returns `"cannot run a job"` for `Job` (reachable only through `finish`/`op_prefix`, which no job row uses). `cells::submit_job(.., from: JobSource, job: Op)` and `cells::take_job(..) -> JobTaken { from, result: io::Result<Box<dyn Any + Send>>, cancelled: bool }` replace `submit_job(.., call: u32, ..)` / `take_ffi`. `JobTaken::answer::<T: 'static>(self) -> Result<T, CellFault>` downcasts; a mismatch is `CellFault::Internal("an op completes with another kind of result")`. `Shared` is `Mutex<Registry>` ([V] cells.rs:802-805), so `Box<dyn Any + Send>` in the record needs no `Sync`.

2. **VM helper** (`crates/eml_vm/src/jobs.rs`, new; C calls share the record through `cells::take_job` but keep their own `Op` construction in `ffi.rs`):
   ```rust
   pub(crate) fn submit<R: Exec, T: Send + 'static>(
       ctx: &mut Ctx<R::M>, task: &mut Task, sched: &mut Scheduler<..>, dst: Slot, from: JobSource,
       job: impl FnOnce() -> T + Send + 'static,
   ) -> Result<(), VmFault>;
   // builds Op::Job(Box::new(move || Box::new(catch_unwind(AssertUnwindSafe(job)).map_err(panic_text)) as Box<dyn Any + Send>)),
   // sched.own_driver()?, cells::submit_job(.., from, op), task.set_raw(dst, token)

   pub(crate) enum Outcome<T> {
       Woke(T),
       Failed(io::Error),   // the driver could not run the job (submit on a broken driver) or the job panicked (kind Other, message = panic payload text)
       Cancelled,           // the T, if any, is already dropped
   }
   pub(crate) fn take<R: Exec, T: Send + 'static>(
       ctx: &mut Ctx<R::M>, task: &mut Task, shared: &cells::Shared, token: i64, wake: i64, from: JobSource,
   ) -> Result<Outcome<T>, VmFault>;
   // cells::take_job; from mismatch -> VmFault::Internal("an op is taken by another function");
   // wake not WOKE/CANCELLED -> VmFault::Internal("an op is taken with another wake");
   // downcast::<Result<T, String>>; Cancelled -> drop the T here, Outcome::Cancelled
   ```
   The job's box always holds `Result<T, String>`: `Ok(T)` from the closure, `Err(text)` from a caught panic. `jobs::submit` is the only place that wraps `catch_unwind`; the eml_rt `Op::Job` contract ("panic しない") stays true for the box the driver sees.

3. **Ownership of a `Lin` `Foreign`**: a consumed (non-borrowed) extern argument hands its reference to the row ([V] lir.md:63: extern の借用でない引数は「消費」で、参照を1つ受け取る); the row therefore owns the word and must release it, as `cells::take_op` releases the record's stream ([V] cells.rs:3817-3822). The submit row does, in this order, with no fallible step between (a) and (b):
   (a) `let value = unsafe { ctx.heap().foreign_take::<T>(w) }?;`
   (b) `unsafe { ctx.heap_mut().decref(w) }?;` (box freed, finalizer is a no-op)
   (c) copy the `Unr` inputs (`Bytes` -> `Vec<u8>` via `ctx.heap().bytes(w)`), build the closure capturing `value` and the copies,
   (d) `jobs::submit(..)` (cancellation point inside `cells::submit`: if it fires the closure is dropped and `value` with it, [V] cells.rs:4291-4296).
   A `VmFault` from (a) or (b) ends the run; the heap's drop runs the no-op finalizer, nothing double-frees. The take row re-boxes with `unsafe { ctx.heap_mut() }.alloc_foreign(value, true)` on the current task's heap.

   New `eml_rt` op, **implemented and gated in the scratch clone** (section 7):
   ```rust
   pub unsafe fn foreign_take<T: Send + 'static>(&self, w: Word) -> (r: Result<T, HeapError>)
       requires  self.wf(), w.is_ptr_spec(), self.word_in(w),
                 !M::CHECKED ==> self.settleable() && self.readable(w) && self.owned(w) && self.rc_value(w) == 1,
       ensures   r is Err ==> M::CHECKED || r == Err::<T, HeapError>(HeapError::WrongKind),
   ```
   Body = `foreign_unique`'s checked branch (`UseAfterFree`, `NotUnique`, `WrongOwner`), the `Kind::Foreign` check, then `foreign::take::<T>(h, Tracked(p))` with the shared `&BlockPerm` from `tracked_perm`, exactly as `foreign` calls `foreign::value`. `foreign::take` is `#[verifier::external_body]`, T3: reads the vtable, compares `TypeId`, `ptr::read`s the value, writes `&Table::<Taken>::VTABLE` (a private unit struct; `drop_in_place::<Taken>` is a no-op) into slot 0. The ghost model never reads slots 0/1 of a `Foreign` block (foreign.rs module doc), so a shared permission suffices and no heap-state `ensures` is needed; `&self` (not `&mut self`) as for `foreign_unique`.

4. **eml-side protocol**: the `submit_*` / `Async.io_wait` / `take_*` triple, hand-written in std (as `Std.Net`), not lowered by translate:
   ```haskell
   -- std, not pub
   extern submit_x : Bytes -> Conn -> <IO> Async.OpId         -- Unr object args borrowed (copied), Lin Foreign args consumed (moved into the job)
   extern take_x   : Async.OpId -> Async.Wake -> <IO> Conn    -- consumes the token, returns the re-boxed value
   x : Bytes -> Conn -> <IO, Async> Conn
   x b c =
     let (w, op) = Async.io_wait (submit_x b c)
     take_x op w
   ```
   Rows: submit = `borrowing(&[true, false], kinded(name, &[Obj, Obj], &[Unr, Lin], Int, Unr, Purity::Effectful))`; take = `kinded(name, &[Int, Enum], &[Unr, Unr], ret, ret_mark, Purity::Effectful)` (`faults` is true for `Effectful` via `kinded`, matching the C `take_row`, [V] c.rs:283-305). No new row helper.

5. **Payload typing**: untyped `Box<dyn Any + Send>` in the record; typed by the take of the row that submitted it. Any `T: Send + 'static` is allowed (not only `Foreign` values): the take row turns `T` into eml values itself (`alloc_foreign(_, true)`, `alloc_bytes`, `fresh_string`, scalars via `task.set_raw`), or returns a tuple through the existing 「組を返す extern」 rule. Several `Lin` `Foreign` inputs: one `foreign_take` + `decref` each, captured as a tuple. The C path is `jobs::take::<u64>` with `JobSource::C(index)`.

6. **Cancellation and teardown**: the moved value is a plain Rust value; every exit path ends in a Rust `drop` of `T` on whichever thread holds it then (table in section 5). After `Cancelled` the take raises and the value is never returned to eml. Linear soundness: the submit row consumed `c`, so the binding is dead before `io_wait`; unwinding frees only values the frames still hold ([V] linearity.md:36), so nothing is freed twice and nothing leaks.

7. **Virtual clock**: unchanged test driver; the job runs inline on worker 0 at settle ([V] test.rs:168), completion order is seed-shuffled ([V] test.rs:126-132). Results are seed-independent because the closure depends only on its captures (G-4).

8. **Blocking C calls move onto the generic record in this stage** (`JobSource::C`; the payload downcast is `JobTaken::answer` in `cells.rs`, and `ffi.rs` takes its `u64` through `take_job(..).answer::<u64>()` on its own path, not through `jobs::take`); translate's lowering and `Op::FfiSubmit`/`Op::FfiTake` stay. A `blocking` attribute on std rows is deferred (U1).

## 2. Rationale

- Returning by value is a premise; moving the `T` out at submit is the only way for the job to own it without a lent pointer. The value is outside the heap while in flight: the record holds no heap word, the pool reads no heap memory (T16 stays C-only), per-worker heaps are untouched (box freed on the submitter's heap, new box on the taker's heap, same worker by A1), and checked mode needs no new rule.
- `foreign_take` via a vtable swap is the smallest `eml_rt` change: one `external_body` function and one `Heap` method with `foreign_unique`'s contract; free paths, `has_finalizer`, Verus invariants and `Heap` drop are untouched (the no-op finalizer still runs once). Verified: `cargo verus verify -p eml_rt`, `verusfmt --check`, and the new `trusted` test pass on the scratch clone (section 7).
- Re-boxing in the take (not at the collection point) avoids `with_worker` and a record word and keeps `cells` type-agnostic; one `alloc_foreign` per job is negligible next to a pool round trip.
- `Box<dyn Any + Send>` in the record keeps `cells` free of per-extern kinds; the C `u64` is one more `T`.
- `catch_unwind` in `jobs::submit`: third-party code (rustls, OS verifiers) can panic; a panic should fail the one eml operation (IO failure) rather than kill the process, and this is native-friendly (the wrapper is Rust around the closure, no VM involvement). C jobs do not go through `jobs::submit` (they build the `Op` in `ffi::job`), so their path is unchanged.
- Hand-written triple in std matches `Std.Net`/`Std.Stream`/`Std.Exec`, needs no new `eml_extern` derivation (C-blocking borrows Lin args and returns them in a tuple; Rust jobs consume and re-box: two different derivations), and works for native code unchanged (rows are `Ctx` externs by name; the protocol is eml code).
- Dropping on `Cancelled` mirrors the C handle rule and is forced by the protocol: the take raises, no binding can receive the value. A row that must offer "cancel then continue" (none known; TLS's wrapper already raises `Cancelled` and loses the `Conn`, [V] std/Tls.em:184 via dossier) would have to be designed as a non-raising take returning `(Wake, T)`; that is a per-row choice outside this guideline and must be stated in the row's spec section.

## 3. Alternatives considered and rejected

- **Lent `*mut T` into the box kept alive by the continuation** (as C slots): contradicts the by-value premise, needs an `unsafe impl Send` wrapper and a T16 extension, and leaves a detached pool thread writing into a heap page after abandonment.
- **Move out and write back into the same box** (`foreign_take` + `foreign_put`): two eml_rt ops and a hole-box state held as a record word; no benefit over a fresh box.
- **Per-kind `OpKind` variant with a typed field**: one variant per Rust job type in `cells`; the TLS one-off the user named.
- **Re-box at the collection point under `with_worker(record.worker)`** (Net's `made`): the host would need to know `T`.
- **A `blocking` attribute on std rows lowered by translate**: needs `ExternRef::Submit/Take(Extern)`, a second derivation in `eml_extern`, split VM arms and translate changes; deferred (U1).
- **Let panics propagate to the driver thread** (status quo): kills the process on a verifier panic; rejected (C-2).
- **Return the value on `Cancelled`**: forces every wrapper to handle a half-done value; the C rule drops.

## 4. Assumptions made

- A1: a task parked on an op record and its `resume_io` resumption are never rescued or stolen. [V] vm.md 「救済」 bullet 1. If a later stage changes this, G-13 applies (re-box under `with_worker(record.worker)` and mark the object per 「救済」 bullet 4).
- A2: a consumed `Lin` `Foreign` argument has local rc 1 and is owned by the current worker. [V] for the ownership rule (lir.md:63) and the existing `foreign_unique` use on the same objects (`tls_conn`, tls.rs:140-142); [I] that no `Lin` object ever has rc > 1 (linearity forbids `dup`). Checked mode turns a violation into `NotUnique`/`WrongOwner` → internal error; fast mode would read a value another reference still sees, so G-9 requires checked-mode tests for each row.
- A3: `cells::submit` drops the `Op` when the cancellation point fires. [V] cells.rs:4291-4296.
- A4: the only pinned message is `a C call op is taken by another function` at ffi.rs:216 (no test references it). [V] `grep -rn "a C call op is taken" crates` prints ffi.rs:216 only. Renaming is kind 3.
- A5: Verus accepts `foreign_take` with `foreign_unique`'s contract. [V] `cargo verus verify -p eml_rt` on the scratch clone (section 7).

## 5. Implications for implementation

### Constraints

- Pool jobs never touch `Ctx`, the heap, RC, or any VM lock; they own every input (copies of `Bytes`/`String`, scalars by value, `Lin` `Foreign` values moved out) and return every output by value inside `Result<T, String>`. Rust jobs lend no heap pointers; T16 stays reserved for C calls.
- The record holds no heap word for a job.
- `submit_x` and `Async.io_wait` are adjacent in the std wrapper (rescue precondition, vm.md 「救済」 bullet 6). Jobs use `io_wait`, never `io_wait_until` (the driver cannot cancel a job; a deadline belongs to the caller's scope and lands after the job ends).
- `T: Send + 'static`; `T`'s `Drop` must be correct on any thread at any time, including after the `Machine` is poisoned.
- The job's result depends only on its captures and the outside world it is allowed to touch; it waits on nothing of the same run (24-slot deadlock, declarations.md:234).
- Expected failures are values in `T` (as `Conn`'s failed flag); panics are caught by `jobs::submit` and become `Outcome::Failed` with kind `Other`.
- Every job row's take raises `Cancelled` (kind `IO_CANCELLED`, message `task was cancelled`) and `Failed(e)` (kind `ffi::io_kind(&e)`, message `{row prefix}: {e}`, where the plan fixes `{row prefix}` as a literal string per row) through `failed::<R>` with `Purity::Effectful`, either directly as `ffi::take` does or through `raise_again::<R>` with `Raise::Io`, which is the same call for an `Effectful` row. A failure the job returns inside `T` (its `Err` value) is the row's own expected failure and carries the row's own kind and message, not the prefix.

### Lifecycle of the value

| Point | Where the `T` is | What happens |
|---|---|---|
| submit row (a)-(c) | moved out by `foreign_take`, then captured | box released by the row's `decref` (b); no finalizer |
| cancelled at submit (`Registry::point`) | closure dropped in `cells::submit` | `T` dropped on the submitting worker; record `Done(Err(cancelled), true)`; take raises `Cancelled` |
| running, then task cancelled / scope failed | pool thread | job runs to the end; answer stored; resume `Cancelled`; take drops `T` on the owner's worker, raises `Cancelled` |
| `Woke`, `Ok` | record `answer` | take downcasts, re-boxes on the owner's heap |
| `Woke`, `Err` (driver could not run it, or panic caught) | none / dropped by unwinding on the pool thread | take raises IO failure |
| run ends (`exit`/error) with a completed record | record | `sweep_all` drops the record; `T` dropped on worker 0 |
| quiesce: queued job past the budget | closure, `pool.forget` | dropped unrun on the quiescing worker's thread |
| quiesce: running job past the budget (abandoned) | pool thread | answer discarded by the driver thread when it arrives, or with the inbox at driver drop, or never (process exit); `Machine` poisoned; the leaked heap holds no part of `T` |
| virtual clock, run ends with queued jobs | test driver `quiesce` | job runs inline, result dropped on worker 0 |

### Units of work (one commit each)

1. **eml_rt `foreign_take`** (the diff is in the scratch clone, `git -C <scratch-F1> diff`): `foreign::take::<T>` in `foreign.rs` plus `struct Taken` outside `verus!`; `Heap::foreign_take` next to `foreign_unique`; `trust.rs` row `("foreign.rs: external_body take", "T3")`; `trusted.rs` test `a_taken_foreign_value_is_not_finalized` registered in `both_modes!`; object-model.md: 「核の操作」 table row for `foreign_take` (next to `foreign_unique`), checked-mode table row (rc 1 → `NotUnique`, `WrongOwner`, `WrongKind` in both modes), 「報告する誤り」 (add `foreign_take` to the `NotUnique`, `WrongOwner` and `WrongKind` lists), T3 row text gains "`foreign_take` が値を移した後の箱の破棄処理が何もしないこと". No `expected.txt` change (no extern result changes).
2. **Generic job record in `cells.rs`** (C path still the only client): `OpKind::Job { from, answer }`, `JobSource`, completion arm stores the box, `prefix()` arm, `submit_job(.., from, op)`, `take_job -> JobTaken`, `JobTaken::answer::<T>`; `ffi.rs` does `take_job(..).answer::<u64>()` in place of `take_ffi`; `ffi_submit` passes `JobSource::C(c)`. Test changes: `a_job_answer_of_another_type_is_an_internal_error` (cells.rs:6348) changes expected behaviour (the completion now succeeds and the error moves to `take_job(..).answer::<u64>()`): kind 2, list in the plan; `a_job_record_takes_the_boxed_answer` (6292) and `a_cancelled_task_submits_no_job` (6322) are kind 3 renames. vm.md 「操作の記録」 kind list says "プールの仕事 (`OpKind::Job`。仕事の出どころとプールが返した箱を持つ)"; 「`blocking` の呼び出し」 bullet "完了は `Done::Job` で、記録は箱の `u64` を答えに受ける。`u64` でない値は内部の誤り..." becomes "完了は `Done::Job` で、記録は箱をそのまま持つ。取り出しが `u64` に戻し、違う型は内部の誤り `an op completes with another kind of result` である".
3. **`jobs.rs`** (`submit`, `take`, `Outcome`, `catch_unwind`); `ffi::take` keeps its own path (C jobs do not go through `jobs::submit`); unit test in `cells.rs` or `jobs.rs`: a job returning `()` taken as `u64` yields the internal error; a panicking job yields `Outcome::Failed` with the panic text; vm.md new subsection 「プールの仕事」 under 「`Async` のタスク」 after 「操作の記録」 (protocol, lifecycle table, constraints, panic rule), 「`blocking` の呼び出し」 cites it for the shared parts; stdlib.md 「extern の境界」 paragraph: std rows may run Rust work on the pool with the `submit_*`/`io_wait`/`take_*` triple, consumed `Lin` `Foreign` arguments are moved into the job and returned as new objects by the take, cites vm.md 「プールの仕事」.
4. **(S1) TLS rows** on units 1-3.

### Completion criteria (exact commands, expected results)

- Unit 1: `cargo verus verify -p eml_rt` → last line `verification results:: N verified, 0 errors`; `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` → exit 0, no output; `cargo test -p eml_rt --lib taken_foreign` → `test result: ok. 2 passed`; `cargo test -p eml_rt --lib trust` → `test result: ok.` with 0 failed; `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::a_taken_foreign_value_is_not_finalized` → `2 passed`, and again with `MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows"`.
- Unit 2: `grep -c "OpKind::Ffi\|take_ffi\|downcast" crates/eml_vm/src/cells.rs` → `0` for the first two patterns, `1` for `downcast` (inside `JobTaken::answer`); `cargo nextest run -p eml_vm` → all pass; `cargo nextest run -p eml_vm --test integration -E 'test(ffi::) | test(ffi_abandon::) | test(bench::)'` → all pass with `RunStats` snapshots unchanged (`git status --porcelain crates/eml_vm/tests` empty after the run).
- Unit 3: `grep -c "catch_unwind" crates/eml_vm/src/jobs.rs` → `1`; `grep -c "FfiSubmit\|FfiTake" crates/eml_vm/src/jobs.rs` → `0`; `cargo nextest run -p eml_vm --lib -E 'test(jobs::)'` → all pass; `grep -c "プールの仕事" docs/spec/vm.md docs/spec/stdlib.md` → each ≥ 1.
- All units: `cargo nextest run`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` → exit 0; `cargo nextest run -p eml_cli --test integration -E 'test(ui::)'` → all pass with no snapshot change (`git status --porcelain crates/eml_cli/tests` empty).

### Docs to write

- vm.md: 「操作の記録」 (kind list), new 「プールの仕事」, 「`blocking` の呼び出し」 (completion bullet, cites 「プールの仕事」), 「救済」 bullet 6 adds "プールの仕事の行".
- object-model.md: 「核の操作」 table, checked-mode table, 「報告する誤り」, T3 row.
- stdlib.md 「extern の境界」 paragraph. declarations.md: no change (held args are unrelated: the moved value is consumed, not held).
- docs/future/ffi.md 「長く走る呼び出し」: the "完了は `Done::Job` で..." bullet cites vm.md 「プールの仕事」.

## 6. Unresolved items (with defaults)

- U1: `blocking` attribute on std `extern` rows lowered by translate. Default: not this stage; revisit when a third job-row family (plugins/GUI) appears.
- U2 (resolved): mismatch message `an op is taken by another function` for both C and std; rename of ffi.rs:216 is kind 3 (A4).
- U3: `foreign_take` on an `Unr` `Foreign`. Default: the op does not inspect the lin bit; rc 1 is the guard; no `Unr` `Foreign` exists today.
- U4: panic text in `Outcome::Failed`: default `io::Error::other(format!("a job panicked: {text}"))` with `text` from `&str`/`String` payloads, else `"a job panicked"`.

## 7. Additional checks

- [V] `foreign_take` feasibility: implemented in `patches/01-foreign-take.patch` (foreign.rs, heap/mod.rs, trust.rs, trusted.rs). `cargo test -p eml_rt --lib taken_foreign` → 2 passed (fast and checked); `cargo test -p eml_rt --lib trust` → 162 passed; `verusfmt --verus-only --check` → exit 0; `cargo verus verify -p eml_rt` → `verification results:: 1201 verified, 0 errors` (41.8 s).
- [V] Consumed extern args hand the reference to the row: lir.md:63; precedent of a row releasing a word it was handed: `cells::take_op` cells.rs:3817-3822; precedent of a row storing it: `submit_read` cells.rs:4016.
- [V] `Shared { cells: Mutex<Registry>, mail }` cells.rs:802-805.
- [V] Pool catches panics (`pool.rs:185`) and the driver re-raises (`mod.rs:652`); so without `catch_unwind` in `jobs::submit` a job panic ends the process.
- [V] `ffi.rs:216` is the only `a C call op is taken` occurrence; no test pins it.
- [V] Unwinding frees only values held by the frames: linearity.md:36.
- [V] cells job tests: 5863, 5912, 5940, 6292, 6322, 6348; `submit_ffi_job` helper at 6268.
- [V] Existing checks from r1 (rescue, test driver, C `take_row`, `Wake` tags, `foreign` module API) unchanged.

## 8. Conformance criteria

- **G-1 (one record kind)** A std row running Rust work on the pool MUST use `OpKind::Job` through `jobs::submit`/`jobs::take`; MUST NOT add an `OpKind` variant; the completion arm MUST NOT inspect the payload. Why: `cells` stays type-agnostic. Applies to: `cells.rs`, `jobs.rs`, VM arms of job rows. Check: `awk '/^enum OpKind/,/^}/' crates/eml_vm/src/cells.rs | grep -c "^    [A-Z]"` equals the count on `main` (12) minus 1 (`Ffi`) plus 1 (`Job`) = 12; `grep -c "Done::Job(value)" crates/eml_vm/src/cells.rs` prints `1` and the line following that match contains no `downcast`; `grep -rn "jobs::submit" crates/eml_vm/src` lists every job row's VM arm.
- **G-2 (by value, moved out)** A `Lin` `Foreign` input of a job row MUST be consumed by the submit row, moved out with `Heap::foreign_take::<T>` and released with `decref` before the closure is built; the job MUST return it (or its successor) by value; the take row MUST re-box it with `alloc_foreign(_, true)`. The record MUST hold no heap word. Why: no lent pointers, no per-worker crossing. Applies to: submit/take VM arms. Check: in the submit row's `FunctionRow`, the `Lin` position has `borrowed` false; in the VM arm, the lines `foreign_take::<` and `decref(` appear in that order before `jobs::submit(`; `grep -n "foreign_unique\|foreign::<" <row module>` has no match inside the closure passed to `jobs::submit`; `OpKind::Job` is constructed with `net_record(.., None)` only.
- **G-3 (typed take)** The payload MUST be `Box<dyn Any + Send>` holding `Result<T, String>` and downcast only in `jobs::take`/`JobTaken::answer`; a mismatch MUST be `CellFault::Internal("an op completes with another kind of result")`; a `JobSource` mismatch MUST be `VmFault::Internal("an op is taken by another function")`. Why: wrong types are VM bugs. Applies to: `cells.rs`, `jobs.rs`, `ffi.rs`. Check: `grep -rn "downcast" crates/eml_vm/src | grep -v "downcast_ref::<&str>\|downcast_ref::<String>"` prints exactly one line, in `cells.rs` inside `fn answer` (the excluded lines read a panic payload's text in `jobs.rs`, not a job's answer); a unit test named `a_job_answer_of_another_type_is_an_internal_error` asserts that `CellFault`.
- **G-4 (closure purity)** The closure passed to `jobs::submit` MUST be a `move ||` closure that captures only owned `Send + 'static` values and MUST NOT name `heap` or any `Word`; `jobs::submit`'s job parameter MUST keep the bound `impl FnOnce() -> T + Send + 'static`, which rejects `ctx`, `task`, `sched` and `shared` at compile time. Why: pool threads; abandonment; deterministic test driver. Applies to: `jobs.rs`, every `jobs::submit` call site. Check: `grep -c "FnOnce() -> T + Send + 'static" crates/eml_vm/src/jobs.rs` prints `1`; the closure text is the text from `move ||` to the `)?;` that ends the `jobs::submit` call, and a closure body MUST NOT itself contain `)?;` (use `match` or `map_err` inside the closure) so that this cut is exact; over every file that calls `jobs::submit`, `for f in $(grep -rl "jobs::submit" crates/eml_vm/src); do perl -0ne 'while (/jobs::submit(?:::<[^>]*>)?\((.*?)\)\?;/sg) { my $c = $1; $c =~ s/^.*?move \|\|//s; print "$c\n" }' "$f"; done | grep -c "heap\|Word"` prints `0`, and the number of closures printed equals `grep -rc "jobs::submit" crates/eml_vm/src | awk -F: '{s+=$2} END {print s}'` minus the definition line in `jobs.rs` (every call site passes a `move ||` closure); `cargo nextest run -p eml_cli --test integration -E 'test(ui::)'` passes (the `async/` seed runs cover determinism only for rows with a UI test under `tests/ui/run/async/`; a row without one, such as the TLS rows, is not seed-covered and the plan says so).
- **G-5 (protocol shape)** In std, every `submit_x` call MUST appear as the argument of `Async.io_wait` and the next statement MUST call the take row that the submit's `JobSource` names with the returned `(w, op)` (one take row MAY serve several submit rows that share its payload type and `JobSource`); `io_wait_until` MUST NOT be used on a job token. Why: rescue precondition; jobs are uncancellable. Applies to: `std/*.em`. Check: `grep -n "submit_x" std/X.em | grep -v "^[0-9]*:extern "` prints only lines matching `Async.io_wait (submit_x`, and the line after each is the take call `take_y op w`; `grep -c "io_wait_until (submit_" std/X.em` prints `0`.
- **G-6 (cancel and failure outcomes)** `jobs::take` MUST drop the `T` for `Outcome::Cancelled` before returning; each take row MUST raise kind `Cancelled` / `task was cancelled` on `Outcome::Cancelled` and an IO failure with kind `ffi::io_kind(&e)` on `Outcome::Failed(e)`; it MUST NOT return the value in either case. Why: C handle rule; no binding can receive it. Applies to: `jobs.rs`, take rows. Check: `grep -n "Outcome::Cancelled" <row module>` is followed within 6 lines by `IO_CANCELLED` and `"task was cancelled"`; a `run-fail` UI test under `tests/ui/run-fail/` for the row ends with stderr containing `task was cancelled`.
- **G-7 (row typing)** Submit rows: `ret` `Int`, `ret_mark` `Unr`, `Purity::Effectful`, `borrowed` true at every `Unr` `Obj` position; take rows: `params` `[Int, Enum]`, `param_marks` `[Unr, Unr]`, `Purity::Effectful`; neither is `pub` in std. Why: matches the C `take_row` and Net; `OpId` must not escape std. Applies to: `eml_extern/src/lib.rs`, `std/*.em`. Check: read the two `FunctionRow` literals against the five values; `grep -n "^pub extern submit_x\|^pub extern take_x" std/X.em` prints nothing; `cargo nextest run -p eml_core_ir --test integration -E 'test(externs::)'` passes.
- **G-8 (eml_rt gate)** `foreign_take` MUST have `foreign_unique`'s `requires`/`ensures`, MUST return `WrongKind` in both modes on a type mismatch and on a taken box, MUST leave the box releasable by `decref` with no drop, MUST be listed in `trust.rs` as T3 with a `trusted` test. Why: `crates/eml_rt/CLAUDE.md`. Applies to: `foreign.rs`, `heap/mod.rs`, `trust.rs`, `trusted.rs`, object-model.md. Check: the unit-1 commands in section 5; `grep -c "foreign_take" crates/eml_rt/src/trust.rs docs/spec/object-model.md` prints ≥ 1 for each file.
- **G-9 (checked-mode coverage)** Every job row MUST be reached on the success path of a UI test under `tests/ui/run/` (runs in checked then fast mode). Why: `foreign_take`'s rc/owner guards fire only in checked mode. Applies to: `tests/ui/run/<category>/`. Check: `grep -rl "<public entry that reaches the row>" tests/ui/run` prints ≥ 1 file, and the plan names one such file; in a scratch copy, replacing the row's submit arm body with `return Err(VmFault::Internal("probe"))` makes `cargo nextest run -p eml_cli --test integration -E 'test(ui::run)'` fail on the named file, and the unmodified tree passes it.
- **G-10 (native-ready)** `jobs.rs` MUST NOT reference `Op::FfiSubmit`/`Op::FfiTake`, `CodeBase`, or translate; the protocol MUST be ordinary extern rows. Why: native.md 「extern のエントリ」. Check: `grep -c "FfiSubmit\|FfiTake\|CodeBase" crates/eml_vm/src/jobs.rs` prints `0`.
- **G-11 (C calls on the same record)** `ffi.rs` MUST obtain its `u64` through `cells::take_job(..).answer::<u64>()` (or `jobs::take::<u64>`) with `JobSource::C(index)`; `cells.rs` MUST NOT mention C calls in the job record. Check: `grep -c "JobSource::C" crates/eml_vm/src/ffi.rs` ≥ 2 (submit and take); `grep -c "C call\|CCall" crates/eml_vm/src/cells.rs` prints `0` outside `#[cfg(test)]` (`awk '/#\[cfg\(test\)\]/{exit} {print}' crates/eml_vm/src/cells.rs | grep -c "C call\|CCall"` → `0`).
- **G-12 (spec placement)** The rule MUST be in vm.md 「プールの仕事」; 「`blocking` の呼び出し」 and stdlib.md 「extern の境界」 MUST cite it; object-model.md MUST carry `foreign_take`; code comments MUST cite those sections and never `docs/plans/`. Check: `grep -c "^### プールの仕事" docs/spec/vm.md` → `1`; `grep -c "「プールの仕事」" docs/spec/vm.md docs/spec/stdlib.md crates/eml_vm/src/jobs.rs` → ≥ 1 each; `grep -rn "docs/plans" crates docs/spec std` → no output.
- **G-13 (movement guard)** vm.md 「プールの仕事」 MUST state that the take re-boxes on the current worker because a task waiting on an op is never rescued or stolen, and 「救済」 bullet 1 MUST keep that exclusion; any stage that lets an op-parked task move MUST change the take to `with_worker(record.worker)` + marking (「救済」 bullet 4) in the same stage. Why: `par-steal`/`main-pinning`. Check: `grep -c "操作か仕事を待っているタスク" docs/spec/vm.md` ≥ 1 and the 「プールの仕事」 section contains the string "救済" (`awk '/^### プールの仕事/,/^### /' docs/spec/vm.md | grep -c "救済"` ≥ 1).

---

