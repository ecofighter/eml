# S13b-2 `fn` / `ctl` clauses and local mutual recursion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Handler operation clauses start with `fn` (no `k`, resumes with the body's value) or `ctl` (today's clauses), and consecutive function-form `let`s in a block form one mutually recursive group checked by strongly connected components with one row per function solved by a fixpoint.

**Architecture:** The parser accepts an optional `fn` / `ctl` after `|`; HIR reports E1051 and desugars a `fn` clause into an ordinary resuming clause with a hidden `$k`, so types, translate and EP keep their current shapes (EP's `runs_in_place` already makes such clauses in-place). HIR forms groups, binds all names first and records the components (`Body::local_components`); `eml_types` checks component by component (deferred in-component calls, label fixpoint, closing, inclusion), and Core IR lifts each component with one shared capture list.

**Tech Stack:** Rust workspace (edition 2024), insta snapshots, the eml UI test harness.

**Spec:** `docs/superpowers/specs/2026-10-17-s13b2-clauses-mutual-recursion-design.md`

## Global Constraints

- Code comments and docs are Japanese, written with the `yomiyasu:yomiyasu` skill; comments explain why, not what, and cite `docs/` paths for rules. CLAUDE.md stays English.
- No compatibility shims: every existing operation clause is rewritten with `ctl` (spec 「テストの変更」, kind 2).
- Test changes follow the kinds in `docs/implementation/testing.md`; every kind-2 change named in the spec is the agreement, anything else found on the way is recorded in the commit message (and, if it is not mechanical, reported to the controller).
- New diagnostic code: E1051 `CLAUSE_KEYWORD` (HIR). Duplicate names in a group reuse E1017 `DUPLICATE_BINDING`. E3001's recursive-local message becomes `a recursive local function cannot capture a linear value`.
- `fn` clause carry diagnostic wording: `the continuation of this \`fn\` clause`.
- Hidden names: `$k` (the `fn` clause continuation), `$r` (stateless body value), `$v` / `$st` (stateful body pair). They cannot be resolved from user code.
- `bench/state.em` only gains `ctl`; `crates/eml_vm/tests/bench.rs` `RunStats` snapshots must not change. A change is a finding to report, not to accept.
- Gate per task: `cargo test --workspace`, `cargo clippy --all-targets`, `cargo fmt --check`. Use `git diff --no-ext-diff` in scripts.
- Commit messages end with the attribution lines the controller gives.

## Review Focus

- A `fn` clause inside a handler nested in a local function, or a handler inside another `fn` clause body: the hidden `$k` must refer to its own clause (Task 3 run test `fn_clauses.em` covers nesting).
- A mutually recursive function used as a value (`List.map even xs`, partial application): the lifted closure must carry the component's shared captures (Task 6 run test).
- A component nested inside another recursive local function's body, calling the outer function: the outer frame's pending deferred calls and the inner fixpoint must not close the outer row too early (Task 5 types test).
- A group broken by a `use` statement or a one-line `let … in` between two function lets: the later function must not be visible from the earlier one (Task 4 HIR tests).
- A `fn` clause whose body returns a `Lin` value (a `File`) or, in a stateful handler, a `Lin` state: no double use or leak through the hidden `$r` / `$v` / `$st` (Task 3 run test `fn_clauses_state.em`, using a `data Token : Lin` type declared in the test).

---

### Task 1: Parse `fn` / `ctl` clause keywords

**Files:**
- Modify: `crates/eml_syntax/src/lexer/mod.rs` (keyword table near `"fn" => FN_KW`), `crates/eml_syntax/src/syntax_kind.rs` (add `CTL_KW` next to `FN_KW`), `crates/eml_syntax/src/grammar/expressions.rs` (`handler_clause`), `crates/eml_syntax/src/ast.rs` (`OpClause`, `ReturnClause`)
- Test: `crates/eml_syntax/tests/handlers.rs`, `crates/eml_syntax/tests/lexer.rs`

**Interfaces:**
- Produces: `SyntaxKind::CTL_KW`; `ast::OpClause::keyword(&self) -> Option<SyntaxToken>` and `ast::ReturnClause::keyword(&self) -> Option<SyntaxToken>`, each returning the `FN_KW` or `CTL_KW` token right after `|`, if any.

- [ ] **Step 1: Write failing tests** in `handlers.rs`: CST snapshots (existing style of the file) for `handle f () with | fn ask () -> 1`, `| ctl fail e -> 0`, `| ctl get () k st -> k st st`, and the error-free parse of `| ask () k -> k 1` and `| fn return x -> x` (no syntax diagnostics; HIR reports them). An AST test asserting `keyword()` returns `FN_KW` / `CTL_KW` / `None` for those clauses. In `lexer.rs`, `ctl` lexes as `CTL_KW`.
- [ ] **Step 2: Run** `cargo test -p eml_syntax --test integration handlers::` — expect the new tests to fail.
- [ ] **Step 3: Implement.** In `handler_clause`, after `p.bump(PIPE)`, eat one `FN_KW` or `CTL_KW` if present, then continue as today (`RETURN_KW` check, then the operation path). `fn` after `|` in a handler clause never starts a lambda.
- [ ] **Step 4: Run** `cargo test -p eml_syntax` — all pass (existing snapshots unchanged: no existing source writes `fn` / `ctl` after a clause `|`; `ctl` as an identifier does not occur in the repository).
- [ ] **Step 5: Commit** `Parse fn and ctl keywords on handler clauses`.

### Task 2: Lower clause keywords in HIR and rewrite every handler with `ctl`

**Files:**
- Modify: `crates/eml_hir/src/lower/handler.rs` (`op_clause`, `return_clause`, `clause_example`), `crates/eml_hir/src/lib.rs` (codes: `CLAUSE_KEYWORD = "E1051"`), `crates/eml_hir/src/hir.rs` (`Body::fn_clauses`)
- Modify (rewrite): every handler in `tests/**/*.em`, `crates/*/tests/**` source strings, `crates/eml_cli/tests/repl/**/*.repl`, `crates/eml_syntax/tests/corpus/*`, `std/Prelude.em` (`try_io`), `bench/state.em`; accept the resulting snapshot changes.
- Test: `crates/eml_hir/tests/effects.rs`

**Interfaces:**
- Consumes: Task 1 `keyword()`.
- Produces:
  - `pub struct FnClause { pub k: LocalId, pub body: ExprId, pub range: TextRange }` in `hir.rs`, and `Body::fn_clauses: Vec<FnClause>` (one per lowered `fn` clause; `k` is the hidden `$k`, `body` is the user's body expression, `range` the clause range).
  - A lowered `fn` clause is an ordinary `OpClause` with `resumes: true` whose closure params are `[args.., $k]` (stateless) or `[args.., $k, st]` (stateful), and whose closure body is a `Block` of one `Stmt::Let` and a tail call:
    - stateless: `let $r = <body>` then `$k $r`
    - stateful: `let ($v, $st) = <body>` then `$k $v $st`
  - `$k` is recorded through `record_continuation` like a written `k`.

- [ ] **Step 1: Write failing HIR tests** in `effects.rs` (use the file's existing helpers and snapshot style):
  - `a_clause_without_a_keyword_is_e1051`: `| ask () k -> k 1` → E1051 `this clause needs \`fn\` or \`ctl\`` (exact wording yours, but the message must name both keywords) with a fix inserting `ctl ` right after `| ` (assert the fix text and position with `fixes`).
  - `a_fn_clause_for_a_never_operation_is_e1051`: `| fn fail e -> 0` for `never fail` → E1051, note that a `never` operation cannot resume.
  - `a_return_clause_with_a_keyword_is_e1051`: `| fn return x -> x` and `| ctl return x -> x` → E1051.
  - `a_fn_clause_lowers_to_a_resuming_clause`: HIR pretty snapshot of a stateless and a stateful `fn` clause showing `$k`, `$r`, `$v`, `$st`.
  - `a_fn_clause_arity_does_not_count_k`: `| fn ask () st -> 1` without `from` → E1010 whose note does not mention `k` (`the \`fn\` clause takes the arguments of \`ask\``; stateful: `… and then the state`).
  - `missing_clause_help_writes_ctl`: E1013 help reads `| ctl ask _ k -> ...` / `| ctl fail _ -> ...`.
- [ ] **Step 2: Run** `cargo test -p eml_hir --test integration effects::` — the new tests fail.
- [ ] **Step 3: Implement** in `handler.rs`:
  - Missing keyword on an op clause: E1051 with the fix, then lower it as `ctl`. `fn` on a `never` op: E1051, then lower it as the `ctl` clause it can only be (its params are exactly the operation's arguments), so E1013 does not cascade. `fn` / `ctl` on `return`: E1051, keep lowering the clause.
  - `fn` clause expected param count: `arity + usize::from(stateful)`; E1010 notes for `fn` clauses as in Step 1.
  - Build the desugared closure with `hidden_param` (the helper the omitted `return` uses) for `$k`, `$r`, `$v`, `$st`; push `FnClause` to `Body::fn_clauses`.
  - `clause_example` writes `| ctl …`.
- [ ] **Step 4: Run the HIR tests** — pass.
- [ ] **Step 5: Rewrite every existing operation clause with `ctl`.** Write a throwaway program in the scratchpad (not committed) that parses each `.em` / `.repl` input with `eml_syntax` and inserts `ctl ` after the `|` of every `OP_CLAUSE` with no keyword (or use the E1051 fix positions from `eml check`). Apply it to `tests/`, `std/Prelude.em`, `bench/state.em`, `crates/eml_syntax/tests/corpus`, and the REPL transcripts. For `.rs` test sources, fix the source strings by hand, driven by the failing tests (`rg -n 'handle' crates/*/tests` lists candidates). Do not turn any clause into `fn`.
- [ ] **Step 6: Run** `cargo test --workspace`; review every snapshot diff with `cargo insta review` (or `cargo insta accept` after reading `git diff --no-ext-diff`): only `ctl ` insertions and the column shifts they cause in diagnostics may change. Confirm `crates/eml_vm/tests/bench.rs` has no diff.
- [ ] **Step 7: Commit** `Require fn or ctl on operation clauses` (body: kind-2 scope from the spec, list of rewritten areas).

### Task 3: Type `fn` clauses and test them end to end

**Files:**
- Modify: `crates/eml_types/src/check/handle.rs` (`op_clause`), `crates/eml_types/src/check/body.rs` (`stmts`, `Stmt::Let`), `crates/eml_types/src/check/report.rs` (`Origin`), `crates/eml_types/src/kind/mod.rs` (`CarriedValue`), `crates/eml_types/src/carry.rs` (`held`)
- Test: `crates/eml_types/tests/effects.rs`, `crates/eml_types/tests/linearity.rs`, `crates/eml_core_ir/tests/evidence.rs`
- Create: `tests/ui/run/effects/fn_clauses.em`, `tests/ui/run/effects/fn_clauses_state.em`, `tests/ui/check-fail/names/clause_without_keyword.em`, `tests/ui/check-fail/names/fn_clause_for_never.em`, `tests/ui/check-fail/linearity/fn_clause_carries_continuation.em` (+ snapshots)

**Interfaces:**
- Consumes: Task 2 `Body::fn_clauses`, the desugared shape.
- Produces: `Origin::FnClauseBody { op: String }`; `CarriedValue::FnClauseContinuation { clause: TextRange }` (its `key()` is `clause`).

- [ ] **Step 1: Write failing tests.**
  - `eml_types/tests/effects.rs`: `a_fn_clause_body_mismatch_names_the_operation_result` (`| fn ask () -> "x"` for `ask : Unit -> Int` → E2001 pointing at `"x"`, message/note saying the `fn` clause for `ask` must return `Int`; no mention of `$k` or `$r`); the stateful variant expects `(Int, s)` wording for a body that returns a non-tuple.
  - `eml_types/tests/linearity.rs`: `a_fn_clause_continuation_held_across_multi_is_e3006`: `once ask` handled with `| fn ask () -> choose ()` inside an outer handler of a `multi choose` → E3006 whose secondary says `the continuation of this \`fn\` clause` and points at the clause.
  - `eml_core_ir/tests/evidence.rs`: `a_fn_clause_runs_in_place` (the entry of `| fn ask () -> n` is the in-place form, same as `| ctl ask () k -> k n`; compare the two programs' evidence output) and `a_fn_clause_that_performs_yields` (body calls another effect's operation → yielding entry).
  - UI `run`:
    - `effects/fn_clauses.em`: stateless `fn` clause; a `fn` clause on a `multi` operation; a `fn` clause whose body performs another effect; a handler mixing `fn` and `ctl` clauses; a handler with a `fn` clause nested inside another `fn` clause's body (Review Focus 1). Print each result.
    - `effects/fn_clauses_state.em`: `run_state` from the spec (`| fn get () st -> (st, st)`, `| fn put st2 _ -> ((), st2)`); a stateful `fn` clause whose state is a test-declared `data Token : Lin` that the clause passes along and the program finally consumes (Review Focus 5).
  - UI `check-fail`: `names/clause_without_keyword.em` (E1051 + fix), `names/fn_clause_for_never.em` (E1051), `linearity/fn_clause_carries_continuation.em` (E3006 as above).
- [ ] **Step 2: Run** the new tests — they fail (wrong wording or `$k` in messages).
- [ ] **Step 3: Implement.** In `op_clause`, when `clause` is a `fn` clause (look it up in `Body::fn_clauses` by its `$k`), record the expected body type — the operation result `ty` after the arguments, or `Tuple([ty, sigma])` when stateful — in a checker map `fn_clause_bodies: HashMap<ExprId, (Ty, String)>` keyed by the user body. In `stmts`, a `Stmt::Let` without annotation whose `init` is in that map is checked with `check_expr(init, expected, Origin::FnClauseBody { op })` instead of inferred. In `carry.rs::held`, a `Held::Local` that is some `FnClause::k` becomes `CarriedValue::FnClauseContinuation { clause }`; render it in `report.rs` next to the other `CarriedValue` arms.
- [ ] **Step 4: Run** `cargo test --workspace` — pass.
- [ ] **Step 5: Commit** `Type fn clauses against the operation result`.

### Task 4: Form local function groups in HIR

**Files:**
- Modify: `crates/eml_hir/src/lower/expr.rs` (`lower_stmts`, `lower_local_function`), `crates/eml_hir/src/hir.rs` (`Body::local_components`, `LocalComponent`), the body-finishing code that builds `Body`
- Modify: `crates/eml_core_ir/tests/translate.rs` (`local_function_names_do_not_collide_with_generated_names`: put the expression statement `()` between the two `let k0` so the expected `main$let$k0#2` stays)
- Test: `crates/eml_hir/tests/lower.rs`
- Create: `tests/ui/check-fail/names/duplicate_local_function.em` (+ snapshot)

**Interfaces:**
- Produces:
  - `Stmt::Functions(Vec<LocalFunction>)` now holds the whole group in definition order.
  - `pub struct LocalComponent { pub members: Vec<LocalId>, pub recursive: bool }` and `Body::local_components: Vec<LocalComponent>`: every local function belongs to exactly one component; `recursive` is true when the component has two or more members or its member refers to itself. Order: groups by their first name's `LocalId` (outer groups before groups nested in their bodies), and within a group callees before callers (reverse topological, like `eml_types/src/scc.rs`). References are the members found in `Body::closure_captures` of each member's lambda.
  - `Body::component_of(&self, local: LocalId) -> Option<&LocalComponent>`.

- [ ] **Step 1: Write failing HIR tests** in `lower.rs`:
  - `an_earlier_local_function_sees_a_later_one` (`let f x = g x` / `let g y = y` / `f 1` resolves `g` to the local).
  - `a_later_group_name_hides_an_outer_name_in_earlier_bodies` (outer top-level `g`; inside the block the group's `g` wins in `f`'s body).
  - `an_expression_statement_breaks_the_group`, `a_value_let_breaks_the_group`, `a_use_statement_breaks_the_group`, `a_one_line_let_in_breaks_the_group` (each: the earlier body's `g` is E1001 or resolves outward).
  - `comments_and_blank_lines_keep_the_group`.
  - `a_duplicate_name_in_a_group_is_e1017` (second definition labelled; references go to the first).
  - `components_are_in_dependency_order` (assert `local_components` for `even`/`odd` plus a non-recursive helper: `[helper] (recursive: false)`, `[even, odd] (recursive: true)` in the right order) and `a_self_calling_function_is_a_recursive_component`.
- [ ] **Step 2: Run** `cargo test -p eml_hir --test integration lower::` — fail.
- [ ] **Step 3: Implement.** In `lower_stmts`, collect a run of consecutive function-form `LetStmt`s; first `lower_pat` every name (skip binding a name already bound in this run: E1017 on the later one, references keep the first), then lower each function's params and body as `lower_local_function` does today; emit one `Stmt::Functions`. Record each group's members while lowering and compute `local_components` when the `Body` is finished (Tarjan, as `eml_types/src/scc.rs`).
- [ ] **Step 4: Sweep for silent meaning changes (spec 「テストの変更」).** Temporarily make lowering print (`eprintln!`) every reference in a group body that resolves to a later group member while an outer binding or item of the same name exists. Run `cargo test --workspace 2>&1 | rg <marker>`; for each hit, decide per the spec (keep the old meaning by inserting an expression statement or renaming, or change the expectation), note it for the commit message, and report any hit to the controller. Remove the print.
- [ ] **Step 5: Fix** `local_function_names_do_not_collide_with_generated_names` as listed in Files; add `check-fail/names/duplicate_local_function.em`.
- [ ] **Step 6: Run** `cargo test --workspace` — pass. (Mutual recursion past HIR is not exercised yet; Tasks 5-6 add it.)
- [ ] **Step 7: Commit** `Group consecutive local functions in HIR`.

### Task 5: Check local function components in `eml_types`

**Files:**
- Modify: `crates/eml_types/src/check/body.rs` (`local_functions`, `LocalFrame` use), `crates/eml_types/src/table/row.rs` (two new `Table` operations), `crates/eml_types/src/usage.rs` (the `Lambda` arm's own-reference handling and its `debug_assert_eq!`, `scope_end`), `crates/eml_types/src/carry.rs` (the `Lambda` arm's `own` filter), `crates/eml_types/src/kind/mod.rs` and `check/report.rs` (`KindReason::RecursiveLocal`)
- Test: `crates/eml_types/tests/check.rs`, `crates/eml_types/tests/effects.rs`, `crates/eml_types/tests/linearity.rs`, `crates/eml_types/src/table/tests.rs`
- Create: `tests/ui/check-fail/linearity/mutual_recursion_captures_file.em` (+ snapshot); update the snapshots of `recursive_local_captures_file.em` and `recursive_local_captures_once_continuation.em` (kind 2, new E3001 wording)

**Interfaces:**
- Consumes: Task 4 `Body::local_components`, `Body::component_of`.
- Produces:
  - `Table::missing_labels(&self, callee: &Row, ambient: &Row) -> Vec<Label>`: the labels of `callee` (resolved; its tail ignored) not matched in `ambient` when same-effect labels pair in row order (scoped labels); does not modify the table.
  - `Table::add_labels(&mut self, row: &Row, labels: Vec<Label>) -> bool`: if `row`'s resolved tail is a free non-rigid variable, extends it with `labels` (fresh tail) and returns `true`; otherwise returns `false` and changes nothing.
  - `KindReason::RecursiveLocal { name: String, callee: String }`.

- [ ] **Step 1: Write failing tests.**
  - `table/tests.rs`: `missing_labels_pairs_same_effect_labels_in_order` (`<Ask, Ask>` vs `<Ask | r>` misses one `Ask`), `add_labels_extends_only_a_free_tail` (rigid tail and closed row return `false`).
  - `check.rs`: `mutually_recursive_local_functions_check` (even/odd → `Int -> Bool` each), `a_group_of_independent_functions_types_as_before` (later calls earlier and earlier calls later, both non-recursive).
  - `effects.rs`:
    - `a_component_effect_reaches_every_member` (`f` calls `g`, `g` performs `ask`: both rows contain `Ask`).
    - `a_handler_inside_a_component_keeps_its_effect_inside` (the spec's `f` / `g` example: `f : Int -> <> Int` usable from a `<>` signature; `g` row `<Ask>`).
    - `a_three_member_fixpoint` (`f`→`g`→`h`→`f`, only `h` performs `Ask`: all three rows contain `Ask`).
    - `a_rigid_ambient_reports_e2002_in_a_component` (a call site whose row tail is rigid cannot receive the label → E2002).
    - `a_nested_component_calls_the_enclosing_recursive_function` (Review Focus 3: a component inside a self-recursive local function's body calls the outer function; checks with the expected rows).
  - `linearity.rs`: `a_mutually_recursive_function_cannot_capture_a_file` (E3001, new wording, label names the callee), `a_non_recursive_group_member_can_capture_a_file`.
  - UI: `check-fail/linearity/mutual_recursion_captures_file.em`.
- [ ] **Step 2: Run** `cargo test -p eml_types` — new tests fail.
- [ ] **Step 3: Implement** `local_functions` per the spec's 4 steps for each component of the statement's group, in `local_components` order: bind every group name to its `local_function_type` first; then per component push frames for its members only, check the members' lambdas in text order, run the fixpoint over the collected `DeferredRow`s (`missing_labels` on `deferred.row` vs `deferred.ambient`, `add_labels` on `deferred.ambient`, until no call adds anything), close each member's tail with `keep` = its parts plus captured types excluding the component's members (and `pending` as today), then include the deferred rows as today. In `usage.rs` and `carry.rs`, references to members of the same component are not captures; when the component is `recursive`, every member's function type gets `kind_at_most(.., Unr)` with `RecursiveLocal { name, callee }` at that member's first reference to a component member. New message: `a recursive local function cannot capture a linear value`; label: `` `{name}` refers to `{callee}` here, so it may be called any number of times `` (`{callee}` equals `{name}` for a self-reference); keep the existing note.
- [ ] **Step 4: Run** `cargo test --workspace` — pass (the two existing E3001 snapshots change wording only; accept them).
- [ ] **Step 5: Commit** `Check local function components with a row fixpoint`.

### Task 6: Lift local function components in Core IR

**Files:**
- Modify: `crates/eml_core_ir/src/translate/mod.rs` (`lifted_functions`, its doc comment)
- Test: `crates/eml_core_ir/tests/translate.rs` (or `tests/local_functions.rs`, following where the S12c lifting tests live)
- Create: `tests/ui/run/functions/mutual_recursion.em`, `tests/ui/run/functions/mutual_recursion_csv.em` (+ snapshots)

**Interfaces:**
- Consumes: Task 4 `Body::local_components`; Task 5 types.
- Produces: every member of a component has the same `Lifted::captured`, the sorted union of the members' `closure_captures` minus the members, with captured local functions of earlier components replaced by their captures (`lifted_captures`), sorted after replacement.

- [ ] **Step 1: Write failing tests.**
  - Core IR: `mutually_recursive_functions_share_their_captures` (`even` captures `a`, `odd` captures `b`: both lifted functions take `a`, `b` first, and in-component calls pass them), `a_component_member_as_a_value_carries_the_shared_captures` (`List.map even xs` builds a closure over the shared captures; Review Focus 2).
  - UI `run/functions/mutual_recursion.em`: even/odd; mutual recursion with captured variables; mutual recursion that performs an effect handled outside; the spec's `f`/`g` handler-inside-component example called from a pure context; a three-member component; a group mixing independent functions and a component; a member passed to `List.map`.
  - UI `run/functions/mutual_recursion_csv.em`: a CSV field/row state machine (functions for an unquoted field, a quoted field, a quote inside a quoted field, and the next row) over a string with `String.byte_at` / `String.slice`, printing the parsed rows, including quoted fields with commas and doubled quotes.
- [ ] **Step 2: Run** them — fail (translate panics or mis-lifts).
- [ ] **Step 3: Implement** `lifted_functions` over `body.local_components` in order: compute the shared captures once per component, then `program.reserve` and insert each member with the same list. Update the comment that says only earlier functions and the function itself can be referenced.
- [ ] **Step 4: Run** `cargo test --workspace` — pass.
- [ ] **Step 5: Commit** `Lift local function components with shared captures`.

### Task 7: Record S13b-2 in the docs

**Files:**
- Modify: `docs/spec/lexical.md`, `docs/spec/grammar.md`, `docs/spec/expressions.md`, `docs/spec/effects.md`, `docs/spec/types.md`, `docs/spec/linearity.md`, `docs/spec/core-ir.md`, `docs/spec/diagnostics.md`, `docs/spec/examples.md` and the other `docs/` files with handler examples (`rg -l '\| [a-z_.]+ .*->' docs` plus `rg -l 'handle' docs`), `docs/implementation/status.md`, `docs/implementation/architecture.md`, `docs/implementation/diagnostics.md`, `docs/future/roadmap.md`, `CLAUDE.md`

- [ ] **Step 1:** Apply the spec's 「段の終わりの文書」 list. In docs examples, clauses that only resume immediately become `fn`; every other operation clause gets `ctl`. `types.md` 「ローカル関数の推論」 describes the component order, the fixpoint and how the closing exceptions apply per function; `expressions.md` drops 「相互再帰は書けない」 and the consecutive-`let` shadowing sentence, and states the group rules; `diagnostics.md` adds E1051 and updates E1017, E3001 and E3006. `roadmap.md`: mark S13b-2 済み in the S13b table (like S13b-1) and delete the 「決めたこと: `k` を書かない節と相互再帰 (S13b-2)」 subsection; keep the `and` group proposal in 「言語機能と構文」. `CLAUDE.md`: mention S13b-2's grammar (`fn` / `ctl` clauses, `ctl` keyword, local function groups) in the `eml_syntax` paragraph and the component handling in the type-checker / Core IR paragraphs, in English.
- [ ] **Step 2:** Check: `rg -n '\| [a-z_]+ .* k ->' docs` finds no clause without `fn` / `ctl`; `cargo test --workspace` still passes (docs-only).
- [ ] **Step 3: Commit** `Record S13b-2 in the specs, the status and the roadmap`.
