# S13b-2 `fn` / `ctl` clauses and local mutual recursion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Handler operation clauses start with `fn` (no `k`, resumes with the body's value) or `ctl` (today's clauses), and `let f x = … and g y = …` defines a mutually recursive group of local functions, checked as one unit with one row per function solved by a fixpoint.

**Architecture:** The parser accepts an optional `fn` / `ctl` after `|`; HIR reports E1051 and desugars a `fn` clause into an ordinary resuming clause with a hidden `$k`, so types, translate and EP keep their current shapes (EP's `runs_in_place` already makes such clauses in-place). The parser reads `and` bindings after a function-form `let` (skipping one `SEP` before a line-initial `and`, like `then` / `else`); HIR binds all names of the group first and records the groups (`Body::local_groups`); `eml_types` checks a group as today's group-shaped `local_functions` does, adding a label fixpoint before closing, and Core IR lifts each group with one shared capture list.

**Tech Stack:** Rust workspace (edition 2024), insta snapshots, the eml UI test harness.

**Spec:** `docs/superpowers/specs/2026-10-17-s13b2-clauses-mutual-recursion-design.md`

## Global Constraints

- Code comments and docs are Japanese, written with the `yomiyasu:yomiyasu` skill; comments explain why, not what, and cite `docs/` paths for rules. CLAUDE.md stays English.
- No compatibility shims: every existing operation clause is rewritten with `ctl` (spec 「テストの変更」, kind 2).
- Test changes follow the kinds in `docs/implementation/testing.md`; every kind-2 change named in the spec is the agreement, anything else found on the way is recorded in the commit message (and, if it is not mechanical, reported to the controller).
- New keywords: `ctl`, `and`. New diagnostic code: E1051 `CLAUSE_KEYWORD` (HIR). Duplicate names in an `and` group reuse E1017 `DUPLICATE_BINDING`; a value binding after `and` (or `and` after a value `let`) is E0011.
- Function-form `let`s without `and` keep today's sequential scoping exactly; no existing local-function test changes.
- E3001's recursive-local message becomes `a recursive local function cannot capture a linear value`.
- `fn` clause carry diagnostic wording: `the continuation of this \`fn\` clause`.
- Hidden names: `$k` (the `fn` clause continuation), `$r` (stateless body value), `$v` / `$st` (stateful body pair). They cannot be resolved from user code.
- `bench/state.em` only gains `ctl`; `crates/eml_vm/tests/bench.rs` `RunStats` snapshots must not change. A change is a finding to report, not to accept.
- Gate per task: `cargo test --workspace`, `cargo clippy --all-targets`, `cargo fmt --check`. Use `git diff --no-ext-diff` in scripts.
- Commit messages end with the attribution lines the controller gives.

## Review Focus

- A `fn` clause inside a handler nested in a local function, or a handler inside another `fn` clause body: the hidden `$k` must refer to its own clause (Task 3 run test `fn_clauses.em` covers nesting).
- A mutually recursive function used as a value (`List.map even xs`, partial application): the lifted closure must carry the group's shared captures (Task 6 run test).
- A group nested inside another recursive local function's body, calling the outer function: the outer frame's pending deferred calls and the inner fixpoint must not close the outer row too early (Task 5 types test).
- `and` after a definition whose body is an indented block, with comments or blank lines before the `and` line: the `and` must close the body block and continue the group (Task 1 syntax tests).
- A `fn` clause whose body returns a `Lin` value (a `File`) or, in a stateful handler, a `Lin` state: no double use or leak through the hidden `$r` / `$v` / `$st` (Task 3 run test `fn_clauses_state.em`, using a `data Token : Lin` type declared in the test).

---

### Task 1: Parse `fn` / `ctl` clause keywords and `let … and …`

**Files:**
- Modify: `crates/eml_syntax/src/lexer/mod.rs` (keyword table near `"fn" => FN_KW`), `crates/eml_syntax/src/syntax_kind.rs` (add `CTL_KW`, `AND_KW`, and the node kind `AND_BINDING`), `crates/eml_syntax/src/grammar/expressions.rs` (`handler_clause`, `let_stmt` / `let_head_and_body` and the one-line `let … in` path), `crates/eml_syntax/src/ast.rs` (`OpClause`, `ReturnClause`, `LetStmt`, `LetExpr`, new `AndBinding`)
- Test: `crates/eml_syntax/tests/handlers.rs`, `crates/eml_syntax/tests/expressions.rs`, `crates/eml_syntax/tests/lexer.rs`
- Create: `tests/ui/check-fail/syntax/and_value_binding.em` (+ snapshot)

**Interfaces:**
- Produces:
  - `SyntaxKind::CTL_KW`, `SyntaxKind::AND_KW`, `SyntaxKind::AND_BINDING`.
  - `ast::OpClause::keyword(&self) -> Option<SyntaxToken>` and `ast::ReturnClause::keyword(&self) -> Option<SyntaxToken>`: the `FN_KW` or `CTL_KW` token right after `|`, if any.
  - `ast::AndBinding { pat() -> Option<Pat>, params() -> Option<ParamList>, body() -> Option<Expr> }` (same accessors as a function-form `LetStmt`), and `LetStmt::and_bindings(&self) -> AstChildren<AndBinding>` / `LetExpr::and_bindings(&self) -> AstChildren<AndBinding>`. An `AND_BINDING` node is a child of the `LET_STMT` / `LET_EXPR` it continues and holds `AND_KW`, the name pattern, a `PARAM_LIST` and the body.

- [ ] **Step 1: Write failing tests.**
  - `handlers.rs`: CST snapshots (the file's existing style) for `handle f () with | fn ask () -> 1`, `| ctl fail e -> 0`, `| ctl get () k st -> k st st`; error-free parses of `| ask () k -> k 1` and `| fn return x -> x` (HIR reports them); an AST test that `keyword()` returns `FN_KW` / `CTL_KW` / `None`.
  - `expressions.rs`: CST snapshots for a block with `let even k = …` and a line-initial `and odd k = …` at the same column; the same where `even`'s body is an indented block and a comment and a blank line come before the `and` line (Review Focus 4); three bindings (`let … and … and …`); the one-line `let f x = a and g y = b in f 1`. Errors: `and x = 1` after a function-form `let` → E0011; `let x = 1` followed by `and g y = y` → E0011; `and g y : Int = y` → E0011 (result type, same wording as the existing local-function result-type error).
  - `lexer.rs`: `ctl` lexes as `CTL_KW`, `and` as `AND_KW`.
  - UI `check-fail/syntax/and_value_binding.em`.
- [ ] **Step 2: Run** `cargo test -p eml_syntax` — the new tests fail.
- [ ] **Step 3: Implement.**
  - `handler_clause`: after `p.bump(PIPE)`, eat one `FN_KW` or `CTL_KW` if present, then continue as today.
  - `let`: after a function-form definition's body, while the next tokens are `AND_KW` or `LAYOUT_SEP AND_KW`, skip that one `SEP` (exactly as the `if` parser skips one before `then` / `else`, `docs/spec/layout.md` 「`then` / `else` の前の `SEP`」), and parse an `AND_BINDING` (name, `apat+` as a `PARAM_LIST`, `=`, body). A missing `PARAM_LIST` after `and` is E0011 (`` `and` defines a local function, so it needs parameters ``); an `and` after a value `let` is E0011 and is parsed as an `AND_BINDING` for recovery. For the one-line form, the `and` bindings come before `in`.
- [ ] **Step 4: Run** `cargo test -p eml_syntax` and `cargo test -p eml_cli --test integration ui::` — pass; no existing snapshot changes (no existing source writes `fn` / `ctl` after a clause `|`, and `ctl` / `and` are not used as names anywhere in the repository).
- [ ] **Step 5: Commit** `Parse fn and ctl clauses and let-and groups`.

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

### Task 4: Lower `let … and …` groups in HIR

**Files:**
- Modify: `crates/eml_hir/src/lower/expr.rs` (`lower_stmts`, the `LetExpr` arm, `lower_local_function`), `crates/eml_hir/src/hir.rs` (`Body::local_groups`, `LocalGroup`), the body-finishing code that builds `Body`
- Test: `crates/eml_hir/tests/lower.rs`
- Create: `tests/ui/check-fail/names/duplicate_local_function.em` (+ snapshot)

**Interfaces:**
- Consumes: Task 1 `AndBinding`, `LetStmt::and_bindings`, `LetExpr::and_bindings`.
- Produces:
  - `Stmt::Functions(Vec<LocalFunction>)` holds the `let` definition and its `and` definitions in definition order (one element for a `let` without `and`, as today).
  - `pub struct LocalGroup { pub members: Vec<LocalId>, pub recursive: bool }` and `Body::local_groups: Vec<LocalGroup>`: one per `Stmt::Functions`, members in definition order; `recursive` is true when some member's lambda refers to some member (itself included), read from `Body::closure_captures` of each member's lambda. Order: by the group's first member's `LocalId` (outer groups before groups nested in their bodies, since a group binds its names before lowering its bodies).
  - `Body::group_of(&self, local: LocalId) -> Option<&LocalGroup>`.

- [ ] **Step 1: Write failing HIR tests** in `lower.rs`:
  - `an_and_binding_sees_the_later_function` (`let f x = g x` / `and g y = y` / `f 1`: `g` in `f`'s body is the local) and the one-line form `let f x = g x and g y = y in f 1`.
  - `a_let_without_and_keeps_sequential_scope` (`let f x = g x` / `let g y = y`: the first `g` is E1001 as today; `let f x = 1` / `let f x = f x` keeps shadowing).
  - `a_duplicate_name_in_an_and_group_is_e1017` (second definition labelled; references go to the first).
  - `local_groups_record_recursion` (`let f x = g x and g y = f y` → recursive; `let f x = 1 and g y = 2` → not recursive; `let go n = go n` → recursive; `let h x = x` → not recursive).
- [ ] **Step 2: Run** `cargo test -p eml_hir --test integration lower::` — fail.
- [ ] **Step 3: Implement.** For a function-form `LetStmt` / `LetExpr`, first `lower_pat` the `let` name and every `and` name (a name already bound in this group: E1017 on the later one and do not bind it again, so references keep the first), then lower each definition's params and body as `lower_local_function` does today; emit one `Stmt::Functions`. Record each group's members while lowering and compute `local_groups` when the `Body` is finished.
- [ ] **Step 4: Add** `check-fail/names/duplicate_local_function.em`.
- [ ] **Step 5: Run** `cargo test --workspace` — pass. (Mutual recursion past HIR is not exercised yet; Tasks 5-6 add it.)
- [ ] **Step 6: Commit** `Lower let-and groups of local functions`.

### Task 5: Check local function groups with a row fixpoint in `eml_types`

**Files:**
- Modify: `crates/eml_types/src/check/body.rs` (`local_functions`, `LocalFrame` use), `crates/eml_types/src/table/row.rs` (two new `Table` operations), `crates/eml_types/src/usage.rs` (the `Lambda` arm's own-reference handling and its `debug_assert_eq!`, `scope_end`), `crates/eml_types/src/carry.rs` (the `Lambda` arm's `own` filter), `crates/eml_types/src/kind/mod.rs` and `check/report.rs` (`KindReason::RecursiveLocal`)
- Test: `crates/eml_types/tests/check.rs`, `crates/eml_types/tests/effects.rs`, `crates/eml_types/tests/linearity.rs`, `crates/eml_types/src/table/tests.rs`
- Create: `tests/ui/check-fail/linearity/mutual_recursion_captures_file.em` (+ snapshot); update the snapshots of `recursive_local_captures_file.em` and `recursive_local_captures_once_continuation.em` (kind 2, new E3001 wording)

**Interfaces:**
- Consumes: Task 4 `Body::local_groups`, `Body::group_of`.
- Produces:
  - `Table::missing_labels(&self, callee: &Row, ambient: &Row) -> Vec<Label>`: the labels of `callee` (resolved; its tail ignored) not matched in `ambient` when same-effect labels pair in row order (scoped labels); does not modify the table.
  - `Table::add_labels(&mut self, row: &Row, labels: Vec<Label>) -> bool`: if `row`'s resolved tail is a free non-rigid variable, extends it with `labels` (fresh tail) and returns `true`; otherwise returns `false` and changes nothing.
  - `KindReason::RecursiveLocal { name: String, callee: String }`.

- [ ] **Step 1: Write failing tests.**
  - `table/tests.rs`: `missing_labels_pairs_same_effect_labels_in_order` (`<Ask, Ask>` vs `<Ask | r>` misses one `Ask`), `add_labels_extends_only_a_free_tail` (rigid tail and closed row return `false`).
  - `check.rs`: `mutually_recursive_local_functions_check` (even/odd with `and` → `Int -> Bool` each), `a_one_way_and_group_types_like_sequential_lets` (`let f x = g x and g y = y + 1` gives the same types as `let g y = y + 1` / `let f x = g x`).
  - `effects.rs`:
    - `a_group_effect_reaches_every_member` (`f` calls `g`, `g` performs `ask`: both rows contain `Ask`).
    - `a_handler_inside_a_group_keeps_its_effect_inside` (the spec's `f` / `g` example: `f : Int -> <> Int` usable from a `<>` signature; `g` row `<Ask>`).
    - `a_three_member_fixpoint` (`f`→`g`→`h`→`f`, only `h` performs `Ask`: all three rows contain `Ask`).
    - `a_rigid_ambient_reports_e2002_in_a_group` (a call site whose row tail is rigid cannot receive the label → E2002).
    - `a_nested_group_calls_the_enclosing_recursive_function` (Review Focus 3: a group inside a self-recursive local function's body calls the outer function; checks with the expected rows).
  - `linearity.rs`: `a_mutually_recursive_function_cannot_capture_a_file` (E3001, new wording, label names the callee), `a_non_recursive_and_group_member_can_capture_a_file` (`let f () = use_file file and g x = x`: no reference inside the group, `f` may capture the `File`).
  - UI: `check-fail/linearity/mutual_recursion_captures_file.em`.
- [ ] **Step 2: Run** `cargo test -p eml_types` — new tests fail.
- [ ] **Step 3: Implement** `local_functions` per the spec's 4 steps. Today's function already binds every member's type, pushes a frame per member, checks the lambdas in text order, closes each tail and then includes the deferred rows; add the fixpoint between checking and closing: run it over the collected `DeferredRow`s (`missing_labels` on `deferred.row` vs `deferred.ambient`, `add_labels` on `deferred.ambient`, until no call adds anything), keep closing each member's tail with `keep` = its parts plus captured types excluding the group's members (and `pending` as today), then include the deferred rows as today. In `usage.rs` and `carry.rs`, references to members of the same group are not captures; when the group is `recursive`, every member's function type gets `kind_at_most(.., Unr)` with `RecursiveLocal { name, callee }` at the first reference to a group member found in any member's body (for a member with no such reference of its own, use the first such reference in the group). New message: `a recursive local function cannot capture a linear value`; label: `` `{name}` refers to `{callee}` here, so it may be called any number of times `` (`{callee}` equals `{name}` for a self-reference); keep the existing note.
- [ ] **Step 4: Run** `cargo test --workspace` — pass (the two existing E3001 snapshots change wording only; accept them).
- [ ] **Step 5: Commit** `Check local function groups with a row fixpoint`.

### Task 6: Lift local function groups in Core IR

**Files:**
- Modify: `crates/eml_core_ir/src/translate/mod.rs` (`lifted_functions`, its doc comment)
- Test: `crates/eml_core_ir/tests/translate.rs` (or `tests/local_functions.rs`, following where the S12c lifting tests live)
- Create: `tests/ui/run/functions/mutual_recursion.em`, `tests/ui/run/functions/mutual_recursion_csv.em` (+ snapshots)

**Interfaces:**
- Consumes: Task 4 `Body::local_groups`; Task 5 types.
- Produces: every member of a group has the same `Lifted::captured`, the sorted union of the members' `closure_captures` minus the members, with captured local functions of earlier groups replaced by their captures (`lifted_captures`), sorted after replacement.

- [ ] **Step 1: Write failing tests.**
  - Core IR: `mutually_recursive_functions_share_their_captures` (`even` captures `a`, `odd` captures `b`: both lifted functions take `a`, `b` first, and in-group calls pass them), `a_group_member_as_a_value_carries_the_shared_captures` (`List.map even xs` builds a closure over the shared captures; Review Focus 2).
  - UI `run/functions/mutual_recursion.em`: even/odd with `and`; mutual recursion with captured variables; mutual recursion that performs an effect handled outside; the spec's `f`/`g` handler-inside-group example called from a pure context; a three-member group; the one-line `let … and … in` form; a block mixing an `and` group with plain function lets; a member passed to `List.map`.
  - UI `run/functions/mutual_recursion_csv.em`: a CSV field/row state machine written as one `let … and …` group (functions for an unquoted field, a quoted field, a quote inside a quoted field, and the next row) over a string with `String.byte_at` / `String.slice`, printing the parsed rows, including quoted fields with commas and doubled quotes.
- [ ] **Step 2: Run** them — fail (translate panics or mis-lifts).
- [ ] **Step 3: Implement** `lifted_functions` over `body.local_groups` in order: compute the shared captures once per group, then `program.reserve` and insert each member with the same list. Update the comment that says only earlier functions and the function itself can be referenced.
- [ ] **Step 4: Run** `cargo test --workspace` — pass.
- [ ] **Step 5: Commit** `Lift local function groups with shared captures`.

### Task 7: Record S13b-2 in the docs

**Files:**
- Modify: `docs/spec/lexical.md`, `docs/spec/grammar.md`, `docs/spec/expressions.md`, `docs/spec/effects.md`, `docs/spec/types.md`, `docs/spec/linearity.md`, `docs/spec/core-ir.md`, `docs/spec/diagnostics.md`, `docs/spec/examples.md` and the other `docs/` files with handler examples (`rg -l '\| [a-z_.]+ .*->' docs` plus `rg -l 'handle' docs`), `docs/implementation/status.md`, `docs/implementation/architecture.md`, `docs/implementation/diagnostics.md`, `docs/future/roadmap.md`, `CLAUDE.md`

- [ ] **Step 1:** Apply the spec's 「段の終わりの文書」 list. In docs examples, clauses that only resume immediately become `fn`; every other operation clause gets `ctl`. `types.md` 「ローカル関数の推論」 describes group checking, the fixpoint and how the closing exceptions apply per function; `expressions.md` drops 「相互再帰は書けない」 and states the `and` group rules (the consecutive-`let` shadowing sentence stays true for lets without `and`); `layout.md` adds `and` to the 「`then` / `else` の前の `SEP`」 section; `lexical.md` adds `ctl` and `and`; `diagnostics.md` adds E1051 and updates E1017, E3001 and E3006. `roadmap.md`: mark S13b-2 済み in the S13b table (like S13b-1) and delete the 「決めたこと: `k` を書かない節と相互再帰 (S13b-2)」 subsection. `CLAUDE.md`: mention S13b-2's grammar (`fn` / `ctl` clauses, `let … and …` groups, the `ctl` and `and` keywords) in the `eml_syntax` paragraph and the group handling (fixpoint, shared captures) in the type-checker / Core IR paragraphs, in English.
- [ ] **Step 2:** Check: `rg -n '\| [a-z_]+ .* k ->' docs` finds no clause without `fn` / `ctl`; `cargo test --workspace` still passes (docs-only).
- [ ] **Step 3: Commit** `Record S13b-2 in the specs, the status and the roadmap`.
