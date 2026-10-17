# S13c-3 Implicit cleanup at non-returning calls Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a path that ends in a non-returning call (a `never` operation, `exit`, or a user function like `die : String -> <IO> a`) leave live `Lin` values unconsumed: the linearity check accepts it, records which variables it implicitly drops, and translate emits `drop` for them right before the call.

**Architecture:** `eml_types` decides per call expression whether it is non-returning (from the callee declaration's HIR signature) and the usage pass carries a "this path diverges at call c" mark through `sequence` / `join`: a diverging branch matches any consumption set, and the variables a diverging path leaves unconsumed go into a new side table `BodyTypes::implicit_drops`. Translate reads that table and emits `drop v` (non-`unr` marks only) after the call's arguments and before the call. Values held in temporaries or caller frames are already released by S13c-1's unwinding.

**Tech Stack:** Rust workspace (`eml_types`, `eml_core_ir`), insta, the UI harness, `bench/run.sh`, `bench/scripts.sh` (macOS).

**Spec:** `docs/superpowers/specs/2026-10-17-s13c-unwinding-design.md` (section 「S13c-3 戻らない呼び出しの暗黙の後始末」, its tests under 「テスト」, the S13c-3 line of 「テストの変更」, 「段の終わりの文書」)

## Global Constraints

- A call is non-returning iff: the callee is a reference to a declaration with a type scheme (top-level function, extern function, effect operation, class method — not a local function, lambda parameter or `let` value); the call supplies at least all arrows of the declared type; the final result type is a quantified type variable that occurs nowhere else in the declared type (no parameter type, no row of any arrow) and carries no class constraint.
- On a path that ends in a non-returning call, unconsumed live variables are accepted; double use (E3002) stays an error. "Unconsumed" ignores evaluation order: a variable consumed anywhere on the path, even after the call, is not implicitly dropped. With several non-returning calls on one path, only the first in evaluation order is recorded.
- A clause's `k` follows the same rule (no `drop k` needed before `fail` / `exit`). Lambda and clause bodies follow the same rule for their captured variables; translate puts the `drop` inside the lifted function.
- Translate emits `drop v` for the recorded variables whose Core IR mark is not `unr`, in variable order, after the call's arguments are evaluated and before the call.
- Destructors stay runtime object release only (a `File` closes), with no effects and no failure; no user-defined destructors.
- Test changes allowed (spec 「テストの変更」): kind 3 for removing `drop k` from `tests/ui/run/scripts/csv2jsonl/main.em` (output byte-identical). Any kind-1 or kind-2 change found while working must be added to the spec first.
- Code comments and docs are Japanese, written with the `yomiyasu:yomiyasu` skill; comments explain why and cite `docs/` paths (the `citations` test checks headings exist). CLAUDE.md stays English.
- Gate per task: `cargo test --workspace`, `cargo clippy --all-targets`, `cargo fmt --check`. Use `git diff --no-ext-diff`.
- Commit messages end with the attribution lines the controller gives.

## Review Focus

- A non-returning call inside a `match` arm whose other arms consume a `File` bound by an outer `let` (the join's diverging branch): accepted, the `File` closes on the diverging arm, no leak (Task 2 test `linearity/diverging_match_arm`).
- A non-returning call used as an argument to a constructor or tuple next to a `Lin` value (`Pair f (fail "x")`): `f` is consumed by the constructor syntactically, so nothing is recorded and the unwinding releases the temporary (Task 2 test `linearity/diverging_argument_beside_a_file`).
- A divergent call whose row contains a `multi` operation while a `File` is unconsumed (the carry pass must not report E3006): accepted (Task 1 test `a_diverging_call_with_a_multi_row_drops_before_carrying`).
- `die` partially applied or passed as a value (`let d = die` then `d "x"`): not non-returning, so the unconsumed `File` is E3003 as before (Task 1 check-fail test).
- A REPL input that opens a `File` and calls `exit 1` without closing it: accepted, the session goes on, `close` reports no leak (Task 2 transcript `unwind/implicit_drop_exit.repl`).

---

### Task 1: Non-returning calls in the linearity check

**Files:**
- Create: `crates/eml_types/src/diverge.rs` (the non-returning test)
- Modify: `crates/eml_types/src/lib.rs` (`mod diverge;`, `BodyTypes::implicit_drops`), `crates/eml_types/src/usage.rs` (the divergence mark in `Uses`, `sequence`, `join`, `count` / `check_pat`, `check_continuation`; recording), the call sites of `usage::constrain` in `crates/eml_types/src/check/mod.rs` and `check/input.rs` (store the recorded table into `BodyTypes`)
- Test: `crates/eml_types/tests/linearity.rs`; new `tests/ui/check-fail/linearity/{diverging_path_double_use,not_diverging_local_function,not_diverging_partial_application,not_diverging_constrained_result,not_diverging_operation_result_in_row}.em` (+ snapshots)

**Interfaces:**
- Produces: `pub(crate) fn diverging_call(program: &Program, body: &Body, call: ExprId) -> bool` (rule in Global Constraints; reads the callee declaration's HIR signature / context, counts its outermost arrows). `pub implicit_drops: HashMap<ExprId, Vec<LocalId>>` on `BodyTypes` — key: the recorded non-returning call expression; value: the locals that path leaves unconsumed, ascending `LocalId`; empty for unreliable bodies.

- [ ] **Step 1: Write failing unit tests** in `linearity.rs` (each checks the program has no linearity error and asserts `implicit_drops`, rendered as `call-position: [local names]`):
  - `a_file_left_open_before_exit_is_dropped_at_exit`: `let f = Fs.open "a"` then `exit 1` → one entry `[f]`.
  - `a_diverging_branch_matches_a_consuming_branch`: `if c then Fs.close f else fail "x"` (local `Fail`) → entry at `fail` with `[f]`.
  - `a_variable_consumed_after_the_call_is_not_dropped`: `(fail "x", Fs.close f)` → no entry.
  - `only_the_first_diverging_call_is_recorded`: `g (fail "a") (fail "b")` with `f` open → one entry at the first `fail`.
  - `a_user_die_is_non_returning`: `die : String -> <IO> a` (eprintln + exit) called with `f` open → entry at the `die` call.
  - `a_clause_continuation_need_not_be_dropped_before_fail`: `| ctl ask () k -> fail "x"` → entry with `[k]`.
  - `a_captured_file_in_a_clause_is_dropped_on_the_diverging_branch`: a `ctl` clause capturing `f`, one branch closes it, the other `fail`s → entry with `[f]`.
  - `a_diverging_call_with_a_multi_row_drops_before_carrying`: `die2 : String -> <Choose, IO> a` with `f` open → no E3006.
- [ ] **Step 2: Write the check-fail UI tests**: `diverging_path_double_use` (a diverging branch uses a `File` twice → E3002); `not_diverging_local_function` (a local function returning `a` by calling `exit`, `f` open → E3003); `not_diverging_partial_application` (`let d = die` then `d "x"` → E3003); `not_diverging_constrained_result` (a class method `m : Unit -> a` constrained by the class variable, `f` open → E3003); `not_diverging_operation_result_in_row` (`State.get ()` result, `f` open → E3003).
- [ ] **Step 3: Run** `cargo test -p eml_types --test integration linearity::` and `cargo test -p eml_cli --test integration ui::` — the unit tests fail; the check-fail tests already fail as programs (snapshot them; their diagnostics must not change in Step 5).
- [ ] **Step 4: Implement** `diverging_call` and the usage changes: a `Uses` that carries the recorded calls its paths all end in; `sequence` keeps the first divergence and still merges later counts; `join` leaves diverging branches out of the "unused" side (`min`) but keeps their `max` / second-use evidence, records `(call, local)` for locals a diverging branch leaves unused while another branch uses them, and diverges iff all branches do; at a binding's scope end (`count` / `check_pat`, `check_continuation`) a local with `min == 0` under a diverging `Uses` is recorded for each of its calls instead of reported. Record only when the body is reliable.
- [ ] **Step 5: Run** `cargo test --workspace` — pass; the check-fail snapshots unchanged since Step 3; no other snapshot changes.
- [ ] **Step 6: Commit** `Accept unconsumed linear values on paths that end in a non-returning call`.

### Task 2: Translate drops before the non-returning call

**Files:**
- Modify: `crates/eml_core_ir/src/translate/expr.rs` (the call translation reads `implicit_drops` and emits `Rhs::Drop` for non-`unr` locals before the call instruction, inside lifted functions too)
- Modify: `tests/ui/run/scripts/csv2jsonl/main.em` (remove `drop k` before `fail`; kind 3)
- Test: new `tests/ui/run/linearity/{exit_with_open_file,die_with_open_file,fail_without_drop_k,diverging_match_arm,diverging_argument_beside_a_file,two_diverging_calls,consumed_after_the_call,captured_file_in_a_clause}.em` (+ snapshots), `crates/eml_cli/tests/repl/unwind/implicit_drop_exit.repl` (+ snapshot), a Core IR snapshot test in `crates/eml_core_ir/tests/translate.rs`

**Interfaces:**
- Consumes: Task 1 `BodyTypes::implicit_drops: HashMap<ExprId, Vec<LocalId>>`.

- [ ] **Step 1: Write failing tests**:
  - Core IR `translate.rs`: `a_non_returning_call_drops_the_open_file_first` — `let f = Fs.open "a"` then `exit 1`: the Core IR shows `let u: unit = drop f.N` immediately before `extern Prelude.exit(…)` (after the argument is built).
  - UI run tests (checked mode no leak; fast mode same output): `exit_with_open_file` (snapshot ends `--- exit: 1 ---`), `die_with_open_file` (`die` prints to stderr and exits 2), `fail_without_drop_k` (the spec's `| ctl yield row k st -> … fail …` shape under `Fail.to_result`), `diverging_match_arm`, `diverging_argument_beside_a_file` (`(f, fail "x")` → `Err "x"`), `two_diverging_calls` (`g (fail "a") (fail "b")` → `Err "a"`), `consumed_after_the_call` (`(fail "x", Fs.close f)` → `Err "x"`), `captured_file_in_a_clause` (a `ctl` clause capturing a `File`, one branch closes it, the other fails).
  - REPL `unwind/implicit_drop_exit.repl`: an input opens a `File` and calls `exit 1`; the next input runs; `close` shows no leak.
- [ ] **Step 2: Run** them — the Core IR test fails; the UI tests fail in the verifier (a `lin` variable left unconsumed) or by leaking.
- [ ] **Step 3: Implement** the drops in translate; remove `drop k` from `csv2jsonl`.
- [ ] **Step 4: Run** `cargo test --workspace`; the `csv2jsonl` snapshot must be unchanged; `cargo build && bench/scripts/compare.sh --fixture csv2jsonl` prints `same`. Run `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::` — pass.
- [ ] **Step 5: Commit** `Drop unconsumed linear values before a non-returning call` (kind 3 for csv2jsonl in the body).

### Task 3: Close S13c in the docs and benchmarks

**Files:**
- Modify: `docs/spec/linearity.md` (「暗黙の後始末」: the two exceptions — unwinding and drops before non-returning calls — the non-returning rule, destructors without effects or failure), `docs/spec/effects.md` (the abort / cleanup bullet), `docs/overview.md` (暗黙の後始末 row, 中断 glossary), `docs/spec/types.md` or `linearity.md` (「線形性の検査パス」: the side table), `docs/spec/core-ir.md` (translate's explicit `drop` before a non-returning call in 「決定木の葉の `drop`」's neighbourhood / 「Kind の印」 drops), `docs/implementation/benchmarks.md` (S13c: `bench/run.sh` and `bench/scripts.sh` results with one line per changed row), `docs/implementation/status.md`, `docs/future/roadmap.md` (mark S13c 済み in the 段の列 and S13c tables, delete the S13c section's 「決めたこと」 / 「論点」 keeping a pointer to the docs like S13b; rewrite the 「evidence passing の最適化」 and 「ネイティブ化」 items about aborts), `CLAUDE.md`
- Modify (parked from S13c-1/2): `crates/eml_core_ir/src/evidence/handlers.rs` (name under's second variable `"o"`; the affected snapshots are an expected-value change, so add a kind-2 line to the spec first), `docs/future/multicore.md:199` (aborts unwind to the fork marker, not yield), `docs/spec/core-ir.md:590` (masked calls listed conservatively), the `assertion_line:` metadata in `integration__ui__run@unwind__io_extern_as_value.em.snap` and `…mask_in_non_yielding_function.em.snap`

- [ ] **Step 1: Write the docs** per the list with the `yomiyasu:yomiyasu` skill.
- [ ] **Step 2: Measure** `bench/run.sh` and `bench/scripts.sh` on a release build (macOS) and record both in `benchmarks.md` as S13c, explaining each change (aborts unwind; IO no longer yields; printing callbacks run in place).
- [ ] **Step 3: Apply the parked items**; for the `"o"` rename add one kind-2 line to the spec's 「テストの変更」 first and review the snapshot diffs (only `w.N` → `o.N` in under entries).
- [ ] **Step 4: Run** `cargo test --workspace` (citations), clippy, fmt — pass.
- [ ] **Step 5: Commit** `Record S13c in the specs, the benchmarks and the roadmap`.
