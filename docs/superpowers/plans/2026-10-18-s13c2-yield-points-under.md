# S13c-2 Yield points and under Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop counting `IO` and effects whose operations are all `never` as labels that may yield, drop the yield checks EP placed after `Effectful` externs and `never` clause calls, and run immediately-resuming clauses that call such effects in place under the handler's outer `evv`, so a generic handler whose callback prints no longer yields.

**Architecture:** `eml_types`' row classification (`Table::row_yield`) skips labels of extern effects and of effects whose multiplicity is `Never`; everything downstream (`yields` marks, row bits, instance names) follows. EP stops treating `Effectful` externs and `never` clause calls as yield points and stops marking `never` entries and `$externN` wrappers `yields`. The in-place clause test no longer counts `IO` externs and non-tail `never` performs; an in-place entry whose body calls anything gets the under form (`evv_get` / `ev_evv` / `evv_set` at entry, `evv_set` back before each return). Aborts in non-`yields` functions already work through S13c-1's unwinding.

**Tech Stack:** Rust workspace (`eml_types`, `eml_core_ir`, `eml_vm`), insta, the UI and REPL harnesses.

**Spec:** `docs/superpowers/specs/2026-10-17-s13c-unwinding-design.md` (section 「S13c-2 yield の地点と under」, its tests under 「テスト」, and the S13c-2 lines of 「テストの変更」)

## Global Constraints

- A label may yield iff its effect is not an extern effect (`Context::is_extern_effect`) and its `effect_multiplicities` entry is not `Multiplicity::Never`. Rows with no remaining label classify by their tail exactly as today.
- Under is added to an in-place entry iff the clause body (before the entry rewrite) contains a `Rhs::Call` other than the final tail `resume k`, or an `Effectful` `Rhs::Extern`. Clauses that call nothing (`State`'s `get` / `put`) get no under.
- Under form: at entry start `w = evv_get()`, `o = ev_evv(ev)`, `evv_set(o)`; before each `return v` (after `ev_set_state(ev, s)`) `evv_set(w)`.
- `$externN` wrappers are never `yields`; `never` entries are never `yields`.
- Test changes allowed (spec 「テストの変更」): kind 2 for EP Core IR / LIR snapshots of programs whose rows hold only `IO` / `never`-only effects, `RunStats` snapshots, and the two named tests rewritten (`every_label_always_yields`, `an_io_extern_is_a_yield_point`). Any kind-1 change found while working must be added to the spec first.
- Code comments and docs are Japanese, written with the `yomiyasu:yomiyasu` skill; comments explain why and cite `docs/` paths (the `citations` test checks headings exist). CLAUDE.md stays English.
- Gate per task: `cargo test --workspace`, `cargo clippy --all-targets`, `cargo fmt --check`. Use `git diff --no-ext-diff`.
- Commit messages end with the attribution lines the controller gives.

## Review Focus

- A `mask`ed call inside a function whose row holds only `IO` / `Fail` labels (two nested `Fail` handlers, the inner skipped): the abort reaches the right handler and nothing leaks (Task 2 test `unwind/mask_in_non_yielding_function`).
- An extern wrapper used as a value under `try_io` (`List.map Fs.open paths` with a missing file): the failure is caught, earlier `File`s close (Task 2 test `unwind/io_extern_as_value`).
- An in-place clause whose body has its own `try_io` / `to_result`: its own failure is caught inside the clause, not by the outer handler (Task 3 test `effects/in_place_clause_catches_its_own_failure`).
- An in-place clause under an abort (the S13c-1 note): the abort passes the in-place entry frame, whose saved `evv` is released, and the handler state the clause took out is released (Task 3 test `unwind/abort_through_in_place_clause`).
- `exit` deep inside an `IO`-only function called under a `State` handler (a `yields` caller around a non-`yields` callee): exit code kept, no leak (Task 1 test `unwind/exit_under_state_handler`).

---

### Task 1: Classify `IO` and `never`-only labels as not yielding

**Files:**
- Modify: `crates/eml_types/src/context.rs` (add `Context::may_yield`), `crates/eml_types/src/table/yields.rs` (`row_yield`)
- Modify: `crates/eml_core_ir/src/evidence/mod.rs` (remove the debug check `calls_io_extern` that rejects `Effectful` externs in functions EP does not split)
- Test: `crates/eml_types/tests/yields.rs`; new `tests/ui/run/unwind/{exit_in_io_only_function,io_failure_in_io_only_function,exit_under_state_handler}.em` (+ snapshots); Core IR / LIR / `RunStats` snapshots (kind 2)

**Interfaces:**
- Produces: `pub(crate) fn may_yield(&self, effect: EffectId) -> bool` on `Context` (rule in Global Constraints). `Table::row_yield` returns `Always` iff some label `may_yield`, else classifies the tail as today.

- [ ] **Step 1: Rewrite and add tests** in `yields.rs`: rename `every_label_always_yields` to `io_and_never_only_labels_never_yield` — `<IO>` body → `body Never`; `<Fail>` (a local `effect Fail where never fail : Unit -> a`) → `Never`; `<Ask, IO>` → `Always`. Add `a_row_variable_beside_io_is_var`: `f : (Unit -> <e> Int) -> <IO | e> Int` → `body Var(0)`.
- [ ] **Step 2: Add the UI run tests**: `exit_in_io_only_function` (`exit 4` three calls deep in `<IO>` functions holding a `File`; snapshot ends `--- exit: 4 ---`), `io_failure_in_io_only_function` (`try_io` around `<IO>` functions that hold a `File` and open a missing file; prints the `Err`), `exit_under_state_handler` (`State.run` around an `<IO>` helper that calls `exit 5`).
- [ ] **Step 3: Run** `cargo test -p eml_types --test integration yields::` — fails.
- [ ] **Step 4: Implement** `may_yield` and use it in `row_yield`; remove `calls_io_extern` and its assertion in `evidence/mod.rs`.
- [ ] **Step 5: Run** `cargo test --workspace`; `cargo insta review` — accept only `yields` marks and yield checks disappearing from `IO`-only / `never`-only functions, renamed instances (`twice<e>` bits), and `RunStats` changes; no stdout/stderr change in existing UI tests. If a masked call in a function that is no longer `yields` breaks EP here (a forced yield point in a function EP no longer splits), bring Task 2's `masked_call` change into this task and say so in the report.
- [ ] **Step 6: Commit** `Do not count IO and never-only effects as yielding` (kind 2 in the body).

### Task 2: EP yield points without `IO` externs and `never` clauses

**Files:**
- Modify: `crates/eml_core_ir/src/evidence/split.rs` (yield points), `crates/eml_core_ir/src/evidence/handlers.rs` (`yielding_entry`: `never` entry not `yields`; `masked_call`: a masked call in a non-`yields` function gets no yield check), `crates/eml_core_ir/src/translate/program.rs` (`$externN` wrapper `yields = false`)
- Test: `crates/eml_core_ir/tests/evidence.rs`; new `tests/ui/run/unwind/{mask_in_non_yielding_function,io_extern_as_value}.em` (+ snapshots); snapshots (kind 2)

**Interfaces:**
- Consumes: Task 1's classification (functions whose rows hold only `IO` / `never`-only labels are not `yields`).

- [ ] **Step 1: Rewrite and add tests** in `evidence.rs`: rename `an_io_extern_is_a_yield_point` to `an_io_extern_is_not_a_yield_point` — in a `yields` function (row `<Ask, IO>`), `extern Prelude.println` is followed by the next statement, no `prim yielding()`. Add `a_never_clause_call_is_not_a_yield_point` (a non-tail `perform` of a `never` op in a `yields` function: no `prim yielding()` after the `clause`), `a_never_entry_does_not_yield` (the `never` entry is `internal clause …` without `yields`), `an_extern_wrapper_does_not_yield` (`$externN` for `Fs.open` used as a value has no `yields`), `a_masked_call_in_a_non_yielding_function_has_no_slow_path` (two nested `Fail` handlers with a skipped inner one: `evv_mask`, `evv_set`, the call, `evv_set(w0)`, nothing else).
- [ ] **Step 2: Add the UI run tests**: `mask_in_non_yielding_function` (the abort reaches the outer `Fail` handler past the masked inner one; prints which handler caught it), `io_extern_as_value` (`try_io` around `List.map Fs.open [existing, missing]`; prints `Err …`; checked mode no leak).
- [ ] **Step 3: Run** `cargo test -p eml_core_ir --test integration evidence::` — fails.
- [ ] **Step 4: Implement**: in `split.rs` an `Effectful` extern is never a yield point and a `Call::Clause` of a non-resumable operation (`OperationInfo::resumable == false`, from `program.effects`) is not one; `yielding_entry` passes `yields = !never`; `masked_call` inserts into `forced` only when the enclosing function is `yields`; `$externN` uses `yields = false` (update its comment).
- [ ] **Step 5: Run** `cargo test --workspace`; `cargo insta review` — accept only removed yield checks / slow paths after `Effectful` externs and `never` clauses, `never` entries and `$externN` without `yields`, and `RunStats` changes.
- [ ] **Step 6: Commit** `Drop yield checks after IO externs and never clauses` (kind 2 in the body).

### Task 3: In-place clauses under the handler's outer `evv`

**Files:**
- Modify: `crates/eml_core_ir/src/evidence/handlers.rs` (`runs_in_place`: an `Effectful` extern and a non-tail `perform` of a non-resumable operation are not yield points; `into_entry`: the under form when the body calls anything, per Global Constraints)
- Test: `crates/eml_core_ir/tests/evidence.rs`; `crates/eml_vm/tests/scaling.rs`; new `tests/ui/run/effects/{in_place_clause_failure_reaches_outer_handler,in_place_clause_io_failure_reaches_outer_try_io,in_place_clause_catches_its_own_failure}.em` and `tests/ui/run/unwind/abort_through_in_place_clause.em` (+ snapshots); snapshots (kind 2)

**Interfaces:**
- Consumes: Task 2 (no yield checks after `Effectful` externs / `never` clauses).

- [ ] **Step 1: Write failing tests** in `evidence.rs`: `a_fn_clause_that_prints_runs_in_place_under_the_outer_evv` (`| fn ask () -> println "x"; 1` → `…$entry` is an in-place entry with `prim evv_get()`, `prim ev_evv(e.N)`, `prim evv_set(…)` at the start and `prim evv_set(w)` before `return`, and no `yield_to`); `a_fn_clause_that_calls_nothing_has_no_under` (`State`-like `fn get () st -> (st, st)`: no `evv_get` in the entry); `a_fn_clause_that_fails_runs_in_place` (body performs a `never` op of an outer effect: in place, with under).
- [ ] **Step 2: Write failing tests** in `scaling.rs`: `a_generic_handler_with_a_printing_callback_does_not_yield` — `Yield.for_each` over a producer of 1000 elements whose callback calls `println`, run from `main : Unit -> <IO> Unit`; `yields == 0` and `yield_frames == 0` (the spec's first completion condition).
- [ ] **Step 3: Write the UI run tests**: `in_place_clause_failure_reaches_outer_handler` (a `fn` clause calls `fail`; the handle body wraps the `perform` in an inner `Fail.to_result`; the outer `to_result` catches it — print both results), `in_place_clause_io_failure_reaches_outer_try_io` (same with `Fs.open "missing"` and `try_io`), `in_place_clause_catches_its_own_failure` (the clause body has its own `try_io` around a failing open and resumes with a fallback), `abort_through_in_place_clause` (a `fn` clause of a handler whose state is a `File` calls a helper that `exit 6`s; snapshot ends `--- exit: 6 ---`; checked mode no leak).
- [ ] **Step 4: Run** the new tests — the Core IR tests and the scaling test fail; the UI tests may already pass by output (clauses still yield).
- [ ] **Step 5: Implement** `runs_in_place` and the under form in `into_entry`.
- [ ] **Step 6: Run** `cargo test --workspace`; `cargo insta review` — accept only entries turning in-place (with under where they call), removed `yield_to` closures, and `RunStats` changes; no stdout/stderr change in existing UI tests. Run `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::` — pass.
- [ ] **Step 7: Commit** `Run clauses that call IO or never operations in place under the outer evv` (kind 2 in the body).

### Task 4: Record S13c-2 in the specs

**Files:**
- Modify: `docs/spec/types.md` (「row の具体化の記録」), `docs/spec/core-ir.md` (「`yields` の印」 wording where it mentions `IO`, 「yield の地点」, 「`perform` と節の3つの形」 with under, 「IO の extern」, 「`mask` の包み」 non-`yields` case, 「EP が作る補助の関数」 if affected), `docs/spec/vm.md` (「IO の失敗と `exit`」 no yield check after the extern), `docs/spec/effects.md` if it says `IO` yields, `docs/implementation/status.md`, `CLAUDE.md` (the `RowYield` sentence, "IO extern calls are EP yield points", the in-place clause description)
- Modify (parked from S13c-1): `tests/ui/run/unwind/inner_state_file.em:1` comment 外側 → 内側; design doc line about 「状態が evidence と一緒に閉じること」 in 「テスト」; `docs/spec/vm.md` 「巻き戻し」 now-dead clause 「このフレームが中断を始めたフレームでなければ」

- [ ] **Step 1: Write the docs** per the list with the `yomiyasu:yomiyasu` skill; no roadmap change (S13c ends with S13c-3).
- [ ] **Step 2: Run** `cargo test --workspace` (citations) — pass.
- [ ] **Step 3: Commit** `Record S13c-2 in the specs`.
