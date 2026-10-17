# S13d Minimal Row Instantiation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop a call's row variable from absorbing the ambient row: every call whose callee row ends in a row variable becomes a deferred inclusion, solved per top-level body (and per REPL input) with a least fixpoint, so a generic handler given a pure callback under `State.run` no longer yields.

**Architecture:** `BodyCheck` pairs the callee's explicit labels at the call as today, then records an `Inclusion` (full callee row, ambient row, ambient source, call key, name) instead of unifying the tail. After `check_function`, `solve_inclusions` runs four phases (labels fixpoint, rigid-tail fixpoint, close, check) in a new `check/inclusion.rs`, reusing `Table::missing_labels` / `add_labels` / `closable_tail` / `include_row` and `include_call_row`'s reporting. The local-function group machinery (`LocalFrame`, `DeferredRow`, `add_missing_labels`, `close_group`, `closed_locals`) is deleted; local functions are ordinary deferred inclusions.

**Tech Stack:** Rust (`crates/eml_types`), insta, the UI and REPL test harnesses, `bench/` with `crates/eml_vm/tests/bench.rs`, `bench/run.sh` (macOS).

**Spec:** `docs/superpowers/specs/2026-10-18-s13d-minimal-rows-design.md`

## Global Constraints

- Code comments and docs are Japanese, written with the `yomiyasu:yomiyasu` skill; comments explain why and cite `docs/` paths (`docs/spec/types.md` 「推論」/「ローカル関数の推論」). CLAUDE.md stays English.
- Solver order (spec 「解き方」): run right after `check_function`, before `solve_constraints`, `usage::constrain`, `carry::constrain` and `take_problem`; in `check_input` also before `undecided_binding` and before the query render.
- Phase 3 never closes a free callee tail that (a) is the shared open tail of the callee and ambient of *any* inclusion, (b) occurs in that inclusion's ambient (label arguments included), or (c) occurs in the queried type of a `:type` input.
- Phase 4 checks shared-tail inclusions first, then the rest, each group in recording order.
- Masks are computed from the full callee row (explicit labels included), so `keep` = the callee's count of that effect.
- One E2002 per call expression, across the eager pairing at the call and the solver (`a_call_reports_a_missing_effect_once` in `crates/eml_types/tests/rows.rs` must stay green).
- Row bindings made by the solver carry the call's range as Kind origin (`with_kind_origin(range, KindReason::Unified, …)`).
- Test changes: kind 1 (pass/fail) only as listed in the spec's 「種類1」, plus any new one the implementer adds to that section of the spec with its reason *and gets the user's confirmation for before committing*; kind 2 within the spec's 「種類2」 scope; never bend the design to keep a test.
- Gate per task: the commands in the task's last step, plus `cargo clippy --all-targets` and `cargo fmt --check` on the touched crates. Use `git diff --no-ext-diff`.
- Commit messages end with the attribution lines the controller gives.

## Review Focus

- A recursive local function whose body handles its own effect around a self-call (`let go n = handle go (n - 1) with …`) while also being called from outside: the inner self-call must still pass and the effect must not leak to the outer caller (Task 1 test `a_handler_inside_a_group_keeps_its_effect_inside` already exists in `effects.rs`; it must stay green unchanged).
- A lambda passed to two generic functions in sequence (`let f = fn x -> x + 1` then `List.map f xs` and `List.filter (fn y -> f y > 0) xs` under `<State Int>`): both instances must classify `Never` (Task 1 test `a_lambda_shared_by_two_generic_calls_stays_pure`).
- A pure callback whose generic callee also receives a producer that does carry the outer effect (`Yield.fold f 0 p` where `p : Unit -> <Yield Int | e> Unit` with the signature's rigid `e`): the `fold` row must stay `Var(0)`, not `Never` (Task 1 test `a_callee_row_shared_with_a_rigid_producer_stays_var`).
- Partial application of a local function (`let add x y = x + y` then `List.map (add 1) xs`): must still type-check after `closed_locals` is gone (Task 1 test `a_partially_applied_local_function_is_passed_to_map`).
- An error inside a postponed lambda argument and a missing effect at the same call: still one E2002 for the call plus the lambda's own error (existing `a_callee_that_is_not_a_name_is_described_without_quotes` in `check.rs` stays green).

---

### Task 1: Deferred inclusions and the per-body solver in `eml_types`

**Files:**
- Create: `crates/eml_types/src/check/inclusion.rs`
- Modify: `crates/eml_types/src/check/mod.rs` (declare the module; call the solver after `check_function` in `check_body`; drop `local_frames` / `closed_locals` from the `BodyCheck` initializer), `crates/eml_types/src/check/input.rs` (same, before `undecided_binding`; pass the query's result type to keep open), `crates/eml_types/src/check/body.rs` (delete `LocalFrame`, `DeferredRow`, `local_frames`, `closed_locals`, `add_missing_labels`, `label_counts` if unused, `close_group`, `self_call_frame`; simplify `local_functions`; `include_arg_row` records; `path` no longer opens locals; `call`'s `opened_later` is true only for `Res::Item`), `crates/eml_types/src/check/report.rs` (doc comment of `include_call_row` only, if its wording mentions the old flow), `crates/eml_types/src/table/row.rs` (add `Table::extend_to_rigid`; update the doc comments of `closable_tail`, `missing_labels`, `add_labels` that cite the old group rules)
- Test: `crates/eml_types/tests/rows.rs`, `check.rs`, `yields.rs`, `masks.rs`, `linearity.rs`, `effects.rs`, `crates/eml_types/src/table/tests.rs`

**Interfaces:**
- Produces (crate-private, used only inside `check/`):
  - `pub(super) struct Inclusion { callee: Row, ambient: Row, source: AmbientSource, key: (ExprId, usize), name: String }` — `callee` is the resolved full callee row at record time.
  - `BodyCheck.inclusions: Vec<Inclusion>` (new field, initialized empty in both `BodyCheck` constructors).
  - `impl BodyCheck { pub(super) fn solve_inclusions(&mut self, keep_open: &[Ty]) }` — `keep_open` is `&[]` for bodies and `&[result]` for `InputKind::Query`.
  - `impl Table { pub fn extend_to_rigid(&mut self, ambient: &Row, rigid: RowVar) -> bool }` — binds the resolved ambient's tail to `<| rigid>` when that tail is a free non-rigid variable; otherwise returns `false` and leaves the table unchanged.

- [ ] **Step 1: Write the failing tests.** Each is a new `#[test]` unless noted; programs may import the std (`import Yield (Yield(..))`, `import State (State(..))`, `import List`).
  - `yields.rs` `a_pure_callback_under_state_does_not_make_fold_yield`: `main : Unit -> <IO> Unit` whose body is `let (t, _) = State.run 0 (fn () -> Yield.fold (fn acc x -> acc + x) 0 (count_to 10))` then `println (show t)`, with the spec's `count_to`. `rows(text, "main")` contains a line ending `fold [Never]`.
  - `yields.rs` `a_callee_row_shared_with_a_rigid_producer_stays_var`: `sum : (Unit -> <Yield Int | e> Unit) -> <e> Int` with `sum p = Yield.fold (fn acc x -> acc + x) 0 p`; `rows(text, "sum")` contains `fold [Var(0)]`.
  - `yields.rs` `a_lambda_shared_by_two_generic_calls_stays_pure`: under `f : List Int -> <State Int> Int`, `let g = fn x -> x + 1` then `List.length (List.map g xs) + List.length (List.filter (fn y -> g y > 0) xs)`; both the `map` and the `filter` lines end `[Never]`.
  - `rows.rs`: replace `a_rigid_tail_in_a_local_function_is_order_dependent` by `a_rigid_tail_in_a_local_function_does_not_depend_on_call_order` (kind 1, spec 「種類1」): both `h () + ask () + k` and `ask () + h () + k` produce no `E2` diagnostic.
  - `rows.rs` `a_let_bound_lambda_is_called_inside_and_outside_a_handler_in_any_order`: in `f : Unit -> <IO> Int`, `let g = fn () -> 1`, then `(handle g () + ask () with | fn ask () -> 2) + g ()` and the same with the two calls swapped (`g ()` first): no diagnostics for either (`Ask` declared locally).
  - `check.rs` `a_lambda_called_before_it_is_passed_as_a_pure_function`: the spec's `passed_after_call` (`let g = fn x -> x + 1`, `let a = g 1`, `apply_pure g a` with `apply_pure : (Int -> Int) -> Int -> Int`) under `<IO>`: no diagnostics (today E2001).
  - `check.rs` `projecting_the_result_of_a_higher_order_local_function_needs_an_annotation`: `data Person = | Person { name : String }`; `projected : Unit -> <State Person> String` with `let run h = h ()` then `(run get).name` → exactly one `E2013`; the same with `(run get : Person).name` → no diagnostics.
  - `check.rs` `a_label_arriving_through_an_argument_unifies_with_the_ambient`: `f : Unit -> <State Int> Int` with `let run h = h ()` then `run get + 1` → no diagnostics (the `State s` that reaches `run`'s row only in phase 1 must unify `s` with `Int` in phase 4, before `solve_constraints` resolves `Num s`).
  - `check.rs` `a_partially_applied_local_function_is_passed_to_map`: `let add x y = x + y` then `List.map (add 1) xs` in a pure function → no diagnostics.
  - `masks.rs` `a_rigid_callee_with_a_label_of_the_masked_effect_keeps_it`: `f : (Unit -> <Log | e> Int) -> <Log, Log | e> Int` with `f h = h ()` → `masks(text, "f")` is the one line `<line:col>#0 [Log@1]` (fill the position the snapshot prints).
  - `masks.rs` `resuming_under_a_handler_of_the_same_effect`: the two functions below (with `effect L where op : Unit -> Int`). `in_signature` → `masks` shows `[L@1]` on the `k` call. `in_lambda` → exactly one `E2002` whose message is ``"`k` performs `L`, which this lambda does not allow"`` (today's diagnostic, unchanged):
    ```haskell
    in_lambda : Unit -> <L> Int
    in_lambda () =
      let f = fn () ->
        op () + (handle op () with
          | ctl op () k -> handle k 1 with
              | fn op () -> 2)
      f ()

    in_signature : (Unit -> <e> Int) -> <L | e> Int
    in_signature h =
      op () + (handle op () with
        | ctl op () k -> handle k (h ()) with
            | fn op () -> 2)
    ```
  - `linearity.rs`: rewrite `a_local_lambda_takes_the_row_of_its_caller` (kind 1, spec 「種類1」) into `a_pure_local_lambda_call_keeps_a_file_across_it`: same program, `carried(rest)` is empty.
  - `linearity.rs` `a_pure_function_passed_to_an_effect_polymorphic_function_keeps_a_file`: in `<Choice, IO>`, `let f = Fs.open "a.txt"`, `let g = keep f (fn () -> ())` with `keep : a -> (Unit -> <e> Unit) -> <e> a`, then `Fs.close g` → `carried` is empty.
  - `table/tests.rs` `extend_to_rigid_binds_only_a_free_tail`: a free-tail row is bound and then resolves to tail `Tail::Var(rigid)`; a closed row and a rigid-tail row return `false`.
- [ ] **Step 2: Run** `cargo test -p eml_types` — the new tests fail (the `in_lambda` / `in_signature` and `a_rigid_callee_with_a_label…` tests may already pass; they guard against regressions).
- [ ] **Step 3: Record inclusions.** In `include_arg_row`: resolve the row; if its tail is `Tail::Closed` or `Tail::Error`, call `include_call_row` as today. If the tail is a `Tail::Var`, first call `include_call_row` with the same labels and a closed tail (pairs the explicit labels now, reports today's errors), then, if that succeeded, push an `Inclusion` with the resolved full row and `self.ambient.clone()` / `self.ambient_source.clone()`. Keep one-E2002-per-call across both phases (a `HashSet<ExprId>` of reported calls on `BodyCheck`, or an equivalent).
- [ ] **Step 4: Implement `solve_inclusions` in `check/inclusion.rs`** with the spec's four phases. Phase 1 is today's `add_missing_labels` loop over `(callee, ambient)` with the same per-effect limit and `shares_tail` / growth skips (move the code; `label_counts` moves with it). Phase 2 loops `extend_to_rigid` for inclusions whose callee tail is rigid until nothing binds. Phase 3 builds the shared-tail set first, then closes each free callee tail not in that set via `closable_tail(&callee, keep_open, &[ambient])` and `close_row`. Phase 4 calls `with_ambient(ambient, source, |this| this.include_call_row(callee, key, range, &name, report))` over shared-tail inclusions first, then the others. Wrap every table mutation in `with_kind_origin(range, KindReason::Unified, …)`.
- [ ] **Step 5: Delete the group machinery** in `body.rs` and wire the solver into `check_body` (`mod.rs`) and `check_input` (`input.rs`, before `undecided_binding`, `keep_open = &[result]` only for `InputKind::Query`).
- [ ] **Step 6: Run** `cargo test -p eml_types`. New tests pass. Update the expected values of existing tests within the spec's 「種類2」 scope (`cargo insta review` for snapshots). Any other test that changes pass/fail: stop, add it to the spec's 「種類1」 with its reason, and ask the user before going on.
- [ ] **Step 7: Run** `cargo test -p eml_types && cargo clippy -p eml_types --all-targets && cargo fmt --check` — all pass.
- [ ] **Step 8: Commit** `Defer call row inclusions and solve them per body` (body lists the kind-1 changes and the reason).

### Task 2: Downstream expectations and the REPL query

**Files:**
- Test: `crates/eml_cli/tests/api.rs`; snapshots under `crates/eml_cli/tests/snapshots/`, `crates/eml_core_ir/tests/`, `crates/eml_lir/tests/`, `crates/eml_vm/tests/` (including `bench.rs`), UI tests `tests/ui/**`, REPL transcripts `crates/eml_cli/tests/repl/**`
- Modify (only if a kind-1 change is found): the spec's 「種類1」 section

**Interfaces:**
- Consumes: Task 1's behavior; no new code interfaces.

- [ ] **Step 1: Write the failing-or-guarding test** `api.rs` `type_of_keeps_the_row_variables_of_the_queried_type`: `console.repl.type_of("fn f -> f ()").ty.as_deref() == Some("(Unit -> <e> a) -> <e> a")` and `type_of("fn f x -> List.map f x")` gives `Some("(a -> <e> b) -> List a -> <e> List b")` (today's output; it guards phase 3's keep-open rule).
- [ ] **Step 2: Run** `cargo test --workspace`. Collect every failure.
- [ ] **Step 3: Update expected values** within the spec's 「種類2」 scope: instance names losing `<e>`, `yields` marks and yield checks disappearing in Core IR / LIR dumps, diagnostic positions or wording in UI tests and transcripts, `RunStats` in `crates/eml_vm/tests/bench.rs`. A UI test that moves between `run`, `run-fail` and `check-fail`, or any other pass/fail change: add it to the spec's 「種類1」 with its reason and ask the user before committing.
- [ ] **Step 4: Run** `cargo test --workspace && cargo clippy --all-targets && cargo fmt --check` — all pass.
- [ ] **Step 5: Commit** `Update the expectations for minimal row instantiation`.

### Task 3: The completion condition and its measurement

**Files:**
- Create: `bench/fold_state.em`; `tests/ui/run/effects/pure_callback_under_state.em` (+ snapshot)
- Modify: `crates/eml_vm/tests/bench.rs` (`PROGRAMS` and a test function `fold_state`), `docs/implementation/benchmarks.md` (new 「### S13d」 section after 「### S13c」)

**Interfaces:**
- Consumes: Tasks 1-2.

- [ ] **Step 1: Write the bench program and its test.** `bench/fold_state.em`: the spec's `count_to` and `main` printing `show t` where `let (t, _) = State.run 0 (fn () -> Yield.fold (fn acc x -> acc + x) 0 (count_to 100000))`. In `bench.rs`, add `"fold_state"` to `PROGRAMS` (sorted position) and `fn fold_state()` snapshotting `stats("fold_state", "5000050000\n")`; the snapshot must show `yields: 0`.
- [ ] **Step 2: Write the UI test** `tests/ui/run/effects/pure_callback_under_state.em`: three prints, (1) the `fold` sum under `State.run` over `count_to 1000`, (2) `Yield.for_each` with a callback that maps a list with a lambda under `State.run` and `put`s the count, (3) the spec's transformer (`map_lists`) used inside `State.run`. Accept the snapshot.
- [ ] **Step 3: Run** `cargo test -p eml_vm --test integration bench:: && cargo test -p eml_cli --test integration ui::` — pass, `fold_state` shows `yields: 0`.
- [ ] **Step 4: Measure and record.** Release-build `eml run` of the 1,000,000-element `fold` under `State.run` (same program as `bench/fold_state.em` with `count_to 1000000`) with `/usr/bin/time -l`, three runs; and `bench/run.sh`. In `benchmarks.md` 「### S13d」: machine block like S13c; a table of the spec's five shapes with yields before (spec 「計測」) and after (from `RunStats`: write a throwaway probe or reuse the UI programs through `eml_test_support::run_stats`; do not commit the probe); the 1M `fold` wall time and instructions before (0.09 s / 2.18e9 under `<IO>`, 0.28 s / 8.00e9 under `State.run`) and after; the `bench/run.sh` table.
- [ ] **Step 5: Commit** `Pin the S13d completion condition and record its measurements`.

### Task 4: Specs, status, roadmap and CLAUDE.md

**Files:**
- Modify: `docs/spec/types.md`, `docs/spec/effects.md`, `docs/spec/expressions.md`, `docs/implementation/architecture.md`, `docs/implementation/status.md`, `docs/future/roadmap.md`, `CLAUDE.md`

- [ ] **Step 1: Rewrite the specs** exactly as the spec's 「文書の更新」 lists: `types.md` 「推論」 inclusion item and examples, 「ローカル関数の推論」 (drop closing, non-closing exceptions, opening on value reference and steps 1-5; point to the body-wide solving; keep the `let f n = handle g n with …` example and the reason a group does not share one row), 「row の具体化の記録」 `Never`; `effects.md` 「健全性」 call item; `expressions.md` local-function item; `architecture.md` `local_functions` paragraph → `check/inclusion.rs`; `status.md` 「型と row の推論」 (remove the closing item and the first-call item, keep only the handle part of the carry item, add the two new limits); `roadmap.md` S13d row → 「済み」 with where decisions went, and 「S13a〜S13c は済み」 → 「S13a〜S13d は済み」.
- [ ] **Step 2: Update `CLAUDE.md`** (English): the Architecture sentence starting "A local function is monomorphic: its row tails are closed after the body is checked and opened at each value reference…" and the group fixpoint description → calls with a row-variable tail are deferred inclusions solved per body (labels fixpoint, rigid tails, close, check; shared tails stay open); value references of local functions share the row.
- [ ] **Step 3: Check** `git grep -n "closed_locals\|close_group\|閉じない例外\|最初の呼び出しで決まる" -- '*.md' '*.rs'` prints nothing outside `docs/superpowers/`; run `python3 ~/.claude/plugins/cache/yomiyasu/yomiyasu/1.1.0/skills/yomiyasu/scripts/yomiyasu_lint.py` on each edited Japanese doc and fix real findings.
- [ ] **Step 4: Commit** `Record S13d in the specs, the status and the roadmap`.
