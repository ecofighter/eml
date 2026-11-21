# Stage row-bound-retire: the row-variable bound `<never e>` leaves the language

- Created: 2026-11-21 (design-level plan, committed as 6fd5e7ea); refreshed into this executable plan 2026-11-21.
- Base: main @ 6177715b (`Plan remote-reclaim`; under it 6fd5e7ea `Plan row-bound-retire`, 6ff1f664 `Merge std-par`). Code facts are those of 6177715b; `git diff --stat 6ff1f664 6177715b` touches `docs/plans/` only [V: read]. Line numbers are hints; docs edits are addressed by quoted anchors (section 8), code edits by item names.
- Status: executable plan. User decisions of 2026-11-21 it implements: the stage is scheduled and run now; it is first in 「段の列」, before `native-checked`, 前提: なし; "No need for compatibility or user consideration. Remove it entirely" (no parser recovery and no diagnostic, test or help for the old syntax); `<once e>` is not added (D-9). Approving this plan is the agreement for the test changes of 7.4 and for 00 §D D-17.
- This file stands alone, except that RB-1 keeps Appendix A (the practical-program study and the E2011 argument) and Appendix B (the study programs) of the committed plan unchanged below section 10.
- Language: this plan is English. `docs/`, code comments and CLAUDE.md are Japanese. Every text marked "EN" here, and every comment line in the patches that carries `EN:`, is an English draft: translate it with PLaMo (`form: plain`, identifiers and section names in `glossary`, earlier parts of the same document as `history`), review it, and write no space between Japanese and Latin characters or inline code in new text. Texts given here in Japanese are final (they came through PLaMo and a review) and are pasted as they are.
- Tags: [V: proto] checked on a prototype of RB-2 and RB-3 in a scratch worktree at 6177715b (removed); [V: read] read in the source; [V: grep] the command is shown; [I] inferred.
- Appendix directory `docs/plans/2026-11-21-row-bound-retire/` (committed by RB-1 with this file, deleted at RB-4). Two patches, plain `git diff --no-ext-diff`, which apply in this order to 6177715b (`git apply --check` then `git apply`, each on the result of the one before) and rebuild the prototype tree exactly [V: proto, applied in order in a fresh worktree; `diff -rq` against the prototype tree is empty]: `rb-2-language.patch` (22 files, 73 insertions, 722 deletions, 5 `EN:` lines), `rb-3-machinery.patch` (18 files, 86 insertions, 788 deletions, 7 `EN:` lines). The patches hold code, tests and snapshots only; the docs edits are section 8.

## 1. Goal and success criteria

- Goal: no surface syntax, HIR field, Kind constraint, diagnostic, doc section or test knows a multiplicity upper bound of a row variable. The three-element lattice, operation and effect multiplicities and the carry rule stay.
- Success criteria:
  - S-1: `row_var ::= LIDENT`. `grep -cE 'NEVER_KW|ONCE_KW|MULTI_KW' crates/eml_syntax/src/grammar/types.rs` → `1` from RB-2 on (4 at the base; the one left reads the `once` arrow) [V: proto].
  - S-2 (from RB-3 on, in `crates`; at RB-4 everywhere): `git grep -nE 'E1057|E2020|ROW_BOUND|RowBound|BoundSource|BoundCause|Exceeding|bound_sources|own_bounds|mult_at_most|RowUpper|Provenance::Bound|KindReason::Performs|Unentailed::Multiplicity|inferred_bounds' -- crates docs ':!docs/reports' ':!docs/plans'` prints only the vacancy sentence of docs/spec/diagnostics.md (167 lines at the base; empty over `crates` on the prototype) [V: proto].
  - S-3: `git grep -nE '<never|never e>' -- crates tests std bench` is empty from RB-2 on (44 lines at the base) [V: proto]; the same over `docs ':!docs/reports' ':!docs/plans'` is empty at RB-4 (15 lines at the base).
  - S-4: no function of `eml_types` can add `σ ≤ constant` on a multiplicity variable (`Table::mult_at_most` is gone); `solve_scc` has `debug_assert!(mult_violated.is_empty())`; the E2011 check has `debug_assert!(scheme.mult.is_empty())`. Both held over the whole workspace on the prototype [V: proto].
  - S-5: the six new tests of 7.4 pass [V: proto].
  - S-6: `cargo nextest run` → 5205 tests, 22 skipped, from RB-2 on (5240 and 23 at the base) [V: proto, section 9].
  - S-7: after RB-4, `git grep -n 'row-bound-retire' -- docs ':!docs/plans'` is empty and `ls docs/plans | grep -c row-bound-retire` → `0`.
  - S-8: CI `success` on every job for the Close commit.

## 2. Scope

- In: `eml_syntax` (grammar/types.rs, ast.rs), `eml_hir` (hir.rs, lib.rs, lower/types.rs, lower/data.rs, lower/effect.rs, pretty.rs), `eml_types` (the twelve files of RB-3), their tests, six UI tests (five deleted, one edited) with five snapshots, the native tally snapshot, the docs of section 8, the roadmap.
- Out: the lattice `Never ≤ Once ≤ Multi`; `OpMultiplicity`; `EffectMultiplicity` in `eml_extern`; `yields`; the handler clause rules; `multiplicity_at_least_multi`; the carry rule and E3006; `KindScheme.mult` (lower bounds from in-SCC references remain); the generic solver (`solve_with_causes<T>`, `Lattice<T>`, `Graph<T>`, `Shortcut<T>`, `residual`) and its unit tests; the keywords `never` / `once` / `multi`; `<once e>` (later item, section 8.4); any aid for programs that wrote the old syntax.
- No change outside `crates/eml_syntax`, `crates/eml_hir`, `crates/eml_types`, `crates/eml_cli/tests/snapshots`, `tests/ui`, `docs`. No new crate edge, feature, re-export, `unsafe`, or public item. No `eml_rt` gate, Verus run or bench measurement applies (roadmap 「進め方」 step 2: none of `eml_vm`, `eml_rt`, `eml_lir`, `eml_core_ir`, `std/`, `bench/` changes).

## 3. Background context (code at 6177715b = base)

- The bound today [V: read]: the parser reads `never` before a row variable (`row_bound` in grammar/types.rs; `EffectRow::bound` in ast.rs); HIR stores it in `RowVarDecl.never` and reports E1057 where it may not be written (`bound_not_declared`); `Shape::instantiate_rigid` turns it into `σ ≤ Never` with `Provenance::Bound` through `Table::mult_at_most`, the only API that puts a constant above a multiplicity variable; the solver copies it to references (`equate`, `copy_scheme`), records where an inferred bound came from (`SourceIndex`, `bound_sources`, `KindScheme.bound_sources`) and reports E2020 (`KindReason::RowBound`, `report::row_bound_exceeded`); `KindReason::Performs` exists only to name the effect in E2020; E2011 has a branch for "only multiplicity bounds are unentailed".
- No std file, bench program or UI test other than the six of 7.4 writes the bound [V: grep, S-3's command at the base].
- `Vars::Operation` in `eml_hir::lower::types` differs from `Vars::Define` only in refusing the bound, so RB-3 folds it into `Define` [V: proto].
- `signature_file` and the `file` parameter of `Shape::instantiate_rigid` and `declaration_problem` exist only to place the `never` keyword; RB-3 removes them [V: proto, clippy clean].
- `Graph::closed_upper`, the second result of `Shortcut::new`, `upward_closure` and `report::multiplicity_word` have no reader left after the deletion; RB-3 removes them [V: proto].
- Citations: `crates/eml_cli/tests/citations.rs` fails when a comment or a document cites a heading that does not exist. Code cites 「row 変数の多重度の上限」 in 7 lines at the base (`git grep -n '多重度の上限' 6177715b -- crates` → 7) and in none after RB-3 [V: proto]. Therefore the spec section and every document line that cites it are deleted in RB-3, not RB-2; RB-2 edits docs/spec/grammar.md and docs/implementation/testing.md only. With the deletions of 8.2 applied to the prototype tree, `citations::` passes (4 tests) [V: proto].
- E2011 after the stage [V: proto, Appendix A]: the scheme of an instance or default method has an empty `mult`, because nothing references its function (a method name is `ValueItem::Method`; call edges exist only for `ValueItem::Function` and `ValueItem::Binding`, scc.rs). The assertion ran in every test of the workspace.
- What the plain parser says of the old syntax [V: proto; nothing pins it]: `f : Unit -> <never e> Unit` → `E0011 1:14 expected an effect`; `data T <never e> = …` → `E0011 1:9 expected a row variable`.

What the prototype showed:
- Test counts by `cargo nextest list --workspace` [V: proto]: base 5263 listed, 23 ignored; after RB-2 5227 listed, 22 ignored; after RB-3 the same (one unit test renamed). RB-2 removes 43 names and adds 7 (7.4).
- `cargo test --workspace -q --no-fail-fast` on the final tree, once: 5205 passed, 0 failed, 22 ignored over 31 result lines [V: proto].
- At the RB-2 state: `cargo test -p eml_syntax -p eml_hir -p eml_types` and `cargo test -p eml_cli` pass, except that `terminal::interrupts::interrupts_while_waiting_do_not_end_the_repl` failed once while another build ran on the machine; it passed alone right after and in the final full run [V: proto]. It is unrelated to the stage; a failure of it alone is re-run once before stopping.
- Snapshots: besides the five deleted files, the only snapshot that changes is `integration__ui__run_native.snap`, `accepted 493` → `accepted 492` [V: proto]. `integration__ui__run_fail_native.snap` does not change. The six new tests have their expected values in the patch.
- `cargo clippy --all-targets -- -D warnings`, `cargo clippy -p eml_cli --no-default-features [--features types | --features core] -- -D warnings`, `cargo fmt --check`, `bash nix/crate-graph.sh --check` pass on the final tree; `cargo clippy -p eml_syntax -p eml_hir -p eml_types --all-targets -- -D warnings` and `cargo fmt --check` pass at the RB-2 state [V: proto].

## 4. Prerequisites

| Needed | For | How to check | If missing |
|---|---|---|---|
| The dev shell (Rust 1.99.1, nextest, clippy, insta) | every unit | `cargo --version` → `cargo 1.99.1`; `cargo nextest --version`, `cargo insta --version` exit 0 | stop |
| `plamo-translate` (MCP tool or CLI) | Japanese text | tool listed or `plamo-translate --help` exit 0 | the `natural-japanese` skill (CLAUDE.md) |
| `gh` authenticated with push | the checkpoint | `gh auth status` exit 0 | ask the user |
| The base | RB-1 | `git merge-base --is-ancestor 6177715b HEAD` exit 0; `git diff --no-ext-diff --stat 6177715b HEAD -- crates std tests bench` empty; `grep -c '^- D-16 ' docs/plans/2026-11-17-runtime/00-decisions.md` → `1`; `grep -c '^- D-17 ' …` → `0` | stop; if a commit after 6177715b touched the files of section 2, re-check the patches with `git apply --check` and stop on a failure |

Memory rule: the stage adds no `.em` file and runs none ad hoc. Never run two test binaries at once; stop any Bash call moved to the background before finishing a unit.

A unit that finds it must change a file outside section 2, or an expected value outside 7.4, stops for the user.

## 5. Decisions

| ID | Decision | Why |
|---|---|---|
| D-1 | Remove the bound and everything that exists only for it | No program needs the level `Never`; where a bound is needed (a class method) `never` is the wrong level (Appendix A). A multiplicity constraint becomes a lower bound only, so only a carry can be violated |
| D-2 | The parser has no case for a multiplicity keyword in a row; nothing tests or documents the old syntax | user ruling |
| D-3 | HIR: `RowVarDecl` is `{ name }`; `row_var` takes the name only; `bound_not_declared`, `ROW_BOUND_NOT_DECLARED` and `Vars::Operation` go | D-1 |
| D-4 | Type checker: delete, do not disable (the list of RB-3) | D-1 |
| D-5 | E2011 compares linearity bounds and carries only; the check debug-asserts `scheme.mult.is_empty()` | vacuous after D-4 (section 3, Appendix A); no program changes [V: proto] |
| D-6 | E1057 and E2020 are vacant and not reused | as E2007 and E2008 |
| D-7 | The lattice keeps three elements | `Never` is an operation and effect multiplicity |
| D-8 | Two code units: the language (RB-2), then the unreachable machinery with the spec section and its citers (RB-3) | every commit is green and no citation dangles (section 3) |
| D-9 | `<once e>` is a later item with its design recorded (8.4) | no class writes a row variable in a method signature |

Conformance (every unit's review):

| ID | Rule | How to check |
|---|---|---|
| RB-G1 | Expected values change only as 7.4 lists | G-5 |
| RB-G2 | No draft marker left in a commit | SC-2 |
| RB-G3 | Nothing outside section 2's paths changes | SC-3 |
| RB-G4 | No test, diagnostic or doc sentence about a multiplicity in a row is added | review of the diff; S-3 |

## 6. Implementation constraints and conventions

- One unit = one commit; each passes G (7.3.1) and the SC lines in force (7.3.2). `cargo fmt` before every commit.
- Take each unit's patch, then replace every `EN:` line by the PLaMo translation of its draft. A draft written `/// EN:` is a doc comment and stays `///`; a `// EN:` draft directly above a `#[test]` or a `const` in a test file becomes a `///` comment like its neighbours; a `// EN:` draft inside a function body stays `//`. Wrap Japanese comments at about 110 columns like the neighbouring ones; the patches hold each draft on one line.
- Japanese text: no space between Japanese and Latin characters or inline code, removed from every paragraph, table row, list item and comment the stage edits or moves (CLAUDE.md), including old text in them and comment lines the patches add or keep.
- Docs edits are addressed by anchors: an anchor is a quoted substring that `git grep -cF -- '<anchor>' 6177715b -- <file>` counts once [V: grep, every anchor of section 8 counted 1]. The edit applies to the bullet, sentence, table row or section that contains the anchor. Code comments cite `docs/spec/` and `docs/implementation/` only; docs name no stage.
- Snapshots: the patches already carry every expected value. If a test still writes a `.snap.new`, read it against 7.4 before `cargo insta review`; never accept over the whole tree.
- Quote every glob (zsh). `git diff` in checks takes `--no-ext-diff`. Pathspecs in checks name directories (`crates`, `docs`), never a bare wildcard form.

## 7. Implementation tasks

### 7.1 Task table

| ID | Commit title | After | Decisions | Executor |
|---|---|---|---|---|
| RB-1 | `Start row-bound-retire` | - | this plan, 00 §D D-17, roadmap 8.1, baseline | orchestrator |
| RB-2 | `Remove the row-variable bound from the grammar` | RB-1 | D-2, D-8 | Sonnet from `rb-2-language.patch` |
| RB-3 | `Remove the machinery of the row-variable bound` | RB-2 | D-3..D-6 | Sonnet from `rb-3-machinery.patch`; Opus reviews the diff against section 3 and 8.2 |
| CP-A | push, CI of RB-3 | RB-3 | S-6 | orchestrator |
| RB-4 | `Close row-bound-retire` | CP-A | roadmap 「進め方」 | Sonnet: steps listed |
| CP-Z | CI of RB-4, then merge | RB-4 | S-8 | orchestrator |

### 7.2 Units

"N tests" is the expected `cargo nextest run` total, all passing.

#### RB-1: `Start row-bound-retire`

1. On main at the base: `git switch -c plan/row-bound-retire`.
2. Replace sections 0-9 of `docs/plans/2026-11-21-row-bound-retire.md` with sections 1-10 of this plan, keeping its Appendix A and Appendix B; add `docs/plans/2026-11-21-row-bound-retire/` with the two patches.
3. Append D-17 to `docs/plans/2026-11-17-runtime/00-decisions.md` §D, after D-16: "D-17 (2026-11-21; user decision): the later item of D-14 (c) is the stage `row-bound-retire`, outside the group, first in 「段の列」 before `native-checked`, 前提: なし. No decision of 01 changes. `native-checked`'s base is `Close row-bound-retire`; its refresh re-reads `integration__ui__run_native.snap` (`accepted 492`)."
4. Roadmap edits of 8.1.
5. Baseline: section 9's table on the clean tree; fill "Recorded". A count that differs is corrected in the plan before the commit if a commit after 6177715b explains it; otherwise stop.
6. Commit; message: D-17, the user decisions of the header, the baseline.
- Completion: `git diff --no-ext-diff --name-only HEAD~1 HEAD` → the plan, the two patches, `00-decisions.md`, `docs/future/roadmap.md`; N = 5240 tests, 23 skipped.

#### RB-2: `Remove the row-variable bound from the grammar`

Work (`rb-2-language.patch`, 22 files; five `EN:` lines to replace):
- `crates/eml_syntax/src/grammar/types.rs`: `effect_row` reads `LIDENT`, or, when not at `>`, `effect (',' effect)* ('|' LIDENT)?`; `row_bound` is deleted; `row_param` loses its keyword branch.
- `crates/eml_syntax/src/ast.rs`: `impl EffectRow` (its only method, `bound`) is deleted.
- `crates/eml_hir/src/lower/types.rs`: the one call becomes `self.row_var(&tail, None)` (the parameter goes in RB-3).
- Tests and snapshots: exactly 7.4's RB-2 rows.
- `EN:` lines: `crates/eml_types/tests/classes.rs` (3), `crates/eml_types/tests/linearity.rs` (2).
- Docs (8.2, part RB-2): docs/spec/grammar.md (two edits), docs/implementation/testing.md (one).
- Completion: G; N = 5205 tests, 22 skipped; S-1; S-3's first command empty; `cargo nextest run -p eml_syntax -p eml_hir -p eml_types` passes; `git diff --no-ext-diff --stat HEAD~1 -- crates tests` → 22 files.

#### RB-3: `Remove the machinery of the row-variable bound`

Work (`rb-3-machinery.patch`, 18 files; seven `EN:` lines to replace):
- `eml_hir`: `RowVarDecl` loses `never` (hir.rs); `row_var(&mut self, name)` and no `bound_not_declared` (lower/types.rs); `Vars::Operation` is deleted and `lower/effect.rs` uses `Vars::Define`; `RowVarDecl { name }` in lower/data.rs; pretty.rs prints the tail name; `codes::ROW_BOUND_NOT_DECLARED` is deleted (lib.rs).
- `eml_types`:
  - lib.rs: `codes::ROW_BOUND_EXCEEDED` deleted.
  - shape.rs: `instantiate_rigid(&self, table, generics)`; no bound block; imports trimmed; the two unit tests call it without a file.
  - table/mod.rs: `mult_at_most` deleted. table/row.rs: the label lower bounds use `require_mult` (the current origin).
  - kind/mod.rs: `Provenance::Bound`, `KindReason::RowBound`, `KindReason::Performs`, `BoundSource`, `BoundCause`, `Exceeding` and their `order_key` arms deleted; the `order_key` unit test loses its `row_bound` and `Performs` entries and the helpers only they used.
  - kind/problem.rs: `KindScheme.bound_sources` deleted.
  - kind/solve.rs: `broken_bounds`, `with_bound_cause`, `bound_origin`, `SourceIndex`, `bound_sources`, `own_bounds`, `Merged.own_bounds`, `upward_closure`, `Graph::closed_upper` deleted; `equate(lin, mult, callee)` copies no bound; `copy_scheme` copies multiplicity constraints with the reference's origin; `carry_residual` has no `entailed` filter; `Shortcut::new` returns the `Shortcut`; `solve_scc` debug-asserts `mult_violated.is_empty()` and reports no multiplicity violation.
  - kind/entail.rs: `Unentailed::Multiplicity`, the multiplicity lattice and the `Once` clause deleted; the unit test is `a_carry_is_entailed_by_an_unrestricted_value_or_the_same_pair`.
  - check/mod.rs: `signature_file` and the `file` parameter of `declaration_problem` deleted; the E2011 check debug-asserts `scheme.mult.is_empty()` and always attaches the general note; `report_violations` has no `bounded` set.
  - check/report.rs: `row_bound_exceeded`, `ROW_BOUND_NOTE`, `multiplicity_word` deleted.
  - ty.rs, dump.rs: `KindConstraint::RowUpper` and its loop deleted.
- `EN:` lines: check/mod.rs (1), dump.rs (1), kind/entail.rs (1), kind/solve.rs (3), ty.rs (1).
- Docs (8.2, part RB-3), the roadmap edit of 8.3.
- Completion: G; N = 5205 tests, 22 skipped; S-2 over `crates` empty and over `docs` only the vacancy sentence and the 「段の列」 entry; S-3's second command prints only the 「段の列」 entry and one line of docs/overview.md; S-4 by reading the three places; `git grep -n '多重度の上限' -- crates docs ':!docs/plans' ':!docs/reports'` → the 「段の列」 entry and one line of docs/overview.md (RB-4 removes both); `git diff --no-ext-diff --stat HEAD~1 -- crates tests` → 18 files.

#### RB-4: `Close row-bound-retire` (roadmap 「進め方」 steps 1-9)

1. `spec/`: `git grep -n 'row-bound-retire' -- docs/spec` empty; no "now … later" notice was added.
2. No measurement (section 2).
3. docs/implementation/status.md: done in RB-3 (the two lines cite the deleted section). Re-read 「今の言語の範囲」 for any other sentence about the bound: none expected.
4. docs/implementation/architecture.md: done in RB-3; no crate or path changed.
5. docs/future/roadmap.md: delete the 「段の列」 entry `row-bound-retire`. The later item of 8.3 is already in place.
6. docs/overview.md: the two cells of 8.2 (part RB-4). docs/README.md: no cell changes.
7. CLAUDE.md: no rule or pointer changes [V: grep, no mention of the bound].
8. `git rm -r docs/plans/2026-11-21-row-bound-retire.md docs/plans/2026-11-21-row-bound-retire/`, after checking that D-1..D-9 are in `docs/` (types.md 「Kind」「推論」「instance と既定のメソッドの検査」, grammar.md, diagnostics.md, the later item).
9. Commit message: what moved where; the test changes by kind with the reason (7.4); no measurement and why; the later item replaced four items; the dates of the user decisions.
- Completion: G; S-2, S-3, S-7; `git grep -n '上限' -- docs/spec/types.md` shows no sentence about a row variable's bound (the lines about `Row<s>`, the label count of the inclusion fixpoint and `Unr` stay).

### 7.3 Gates and standing checks

#### 7.3.1 G (every unit, repository root, after `git add -A`)

1. `cargo nextest run` → exit 0; `N tests run: N passed[ (k slow)], S skipped` with N = 5240, S = 23 (RB-1) and N = 5205, S = 22 (RB-2 on). If N differs, list with `cargo nextest list` and compare with 7.4's names; any difference 7.4 does not explain stops the unit.
2. `cargo clippy --all-targets -- -D warnings` → exit 0.
3. `cargo fmt --check` → exit 0.
4. `bash nix/crate-graph.sh --check` → exit 0.
5. Snapshot and expectation changes are only those of 7.4 for the unit (`git diff --no-ext-diff --stat HEAD~1 -- crates tests` read against 7.4).
6. `cargo nextest run -p eml_cli --test integration -E 'test(/^citations::/)'` passes.

#### 7.3.2 SC (standing checks)

```sh
# SC-1 (always): no docs/plans citation in code or docs
grep -rn 'docs/plans/20' crates docs std --include='*.rs' --include='*.md' --include='*.em' | grep -v '^docs/plans/'   # empty
# SC-2 (RB-2 on; RB-G2): no draft marker left (the patches bring 5 and 7; every one is replaced before the unit's commit)
git grep -nE '(//|--|#) ?EN: ' -- std crates tests bench                # empty
# SC-3 (always; RB-G3): only section 2's paths change
git diff --no-ext-diff --name-only <RB-1> HEAD -- crates std tests bench | grep -vE '^(crates/eml_(syntax|hir|types)/|crates/eml_cli/tests/snapshots/|tests/ui/)'   # empty
# SC-4 (RB-2 on; S-3)
git grep -nE '<never|never e>' -- crates tests std bench               # empty
# SC-5 (RB-3 on; S-2 in code)
git grep -nE 'E1057|E2020|ROW_BOUND|RowBound|BoundSource|BoundCause|Exceeding|bound_sources|own_bounds|mult_at_most|RowUpper|Provenance::Bound|KindReason::Performs|Unentailed::Multiplicity|inferred_bounds' -- crates   # empty
# SC-6 (always): the feature-combo clippy lines of eml_cli
cargo clippy -p eml_cli --no-default-features -- -D warnings
cargo clippy -p eml_cli --no-default-features --features types -- -D warnings
cargo clippy -p eml_cli --no-default-features --features core -- -D warnings
```

Non-vacuity at the base [V: grep at 6177715b]: SC-2 prints 0 lines; SC-4 prints 44; SC-5's pattern prints 167 over `crates docs`; SC-3's three directories hold 622 UI `.em` files and 145 tracked files of the three crates, so its `grep -v` has input from RB-2 on (22 paths) and at RB-1 the `git diff` is empty by construction.

### 7.4 Test changes by kind

Reason for all: the feature is removed, and nothing about the old syntax is tested (user ruling).

Kind 1 (deletions), all in RB-2 unless marked:

| Where | Tests |
|---|---|
| UI, each with its snapshot | `run/kinds/never_row_bound.em`; `check-fail/names/row_bound_in_operation.em`; `check-fail/names/row_bound_not_declared.em`; `check-fail/types/row_bound_exceeded.em`; `check-fail/syntax/row_bound_syntax.em` |
| `crates/eml_syntax/tests/types.rs` (5) | `a_row_variable_can_be_bounded_by_never`, `bounded_row_variables_in_every_row_position`, `only_never_can_bound_a_row_variable`, `never_must_be_followed_by_a_row_variable`, `a_row_parameter_cannot_be_bounded` |
| `crates/eml_hir/tests/lower.rs` (7) | `a_bound_belongs_to_the_row_variable`, `a_misplaced_never_bounds_no_row_variable`, `an_annotation_may_repeat_a_declared_bound`, `a_bound_must_be_declared_in_the_signature`, `a_data_field_cannot_bound_a_row_variable`, `an_operation_cannot_bound_a_row_variable`, `a_bounded_row_parameter_stays_a_parameter` |
| `crates/eml_types/tests/row_bounds.rs` (the file and its `mod` line; 27) | `a_written_bound_stays_in_the_scheme`, `a_call_infers_the_bound`, `a_recursive_group_shares_an_inferred_bound`, `an_unused_row_variable_has_no_bound`, `panic_fits_a_never_bound`, `an_effect_of_never_operations_fits_a_never_bound`, `labels_beside_a_bounded_variable_are_not_bounded`, `a_call_that_exceeds_a_written_bound`, `a_multi_operation_exceeds_a_never_bound`, `an_operation_argument_row_exceeds_a_never_bound`, `unwritten_occurrences_carry_the_bound`, `a_self_call_exceeds_its_own_bound`, `a_mutual_call_exceeds_the_bound`, `a_call_from_a_local_function_exceeds_the_bound`, `a_method_call_exceeds_the_class_bound`, `one_call_reports_once_for_two_bounds`, `an_inferred_bound_names_the_inner_call`, `a_chain_names_only_the_nearest_call`, `a_recursive_group_names_the_call_in_the_group`, `a_ring_reports_its_calls_to_the_bounded_function`, `an_inferred_bound_broken_inside_its_group`, `a_bound_drops_the_carry_it_entails`, `a_bound_drops_the_carry_inside_its_group`, `an_effect_with_a_once_operation_exceeds_a_never_bound_even_through_its_never_operation`, `a_value_use_of_a_bounded_function_keeps_the_bound`, `a_function_that_uses_a_var_exceeds_the_bound`, `a_once_operation_exceeds_a_never_bound` |
| `crates/eml_types/tests/classes.rs` (2, and the const `HELPER`) | `a_method_can_rely_on_a_bound_in_the_class_signature`, `a_method_that_needs_a_bound_the_class_lacks` |
| `crates/eml_types/tests/scaling.rs` (1, `#[ignore]`, and its generator `ring_with_inferred_bounds`) | `a_ring_of_functions_with_inferred_bounds` |
| RB-3, `crates/eml_types/src/kind/mod.rs`, the `order_key` unit test | its eight `row_bound(…)` and two `KindReason::Performs` entries |
| RB-3, `crates/eml_types/src/kind/entail.rs` | `a_carry_is_entailed_by_an_unrestricted_value_a_bounded_row_or_the_same_pair` loses the bounded-row case and is renamed `a_carry_is_entailed_by_an_unrestricted_value_or_the_same_pair` |

Kind 2 (expected values): `crates/eml_cli/tests/snapshots/integration__ui__run_native.snap`, `accepted 493` → `accepted 492` (RB-2; one `run` file fewer) [V: proto].

Kind 3 (values byte-identical): `tests/ui/run/par/panic_reaches_nearest_try_panic.em` line 8 writes `foo : (Unit -> <e> Int) -> <Panic | e> Int` (RB-2; its snapshot does not change [V: proto]); `a_par_child_may_return_a_lin_value` moves from `row_bounds.rs` to the end of `linearity.rs` (RB-2); `bound_sources: Vec::new()` leaves the `KindScheme` literals of entail.rs and solve.rs, and the two `instantiate_rigid` calls of shape.rs's unit tests lose the file argument (RB-3).

New tests (RB-2; six; expected values in the patch) [V: proto]:

| File | Test | Expectation |
|---|---|---|
| `eml_types/tests/linearity.rs` | `a_function_that_keeps_a_file_across_its_callback_takes_once_effects` | `each_line` over `FileIn.read_line` called with a callback that assigns a `var` and prints: no diagnostic |
| | `a_function_that_keeps_a_file_across_its_callback_refuses_a_multi_callback` | `E3006 16:24 \`each_line\` keeps a linear value alive across a call that may resume more than once`, secondary `11:7 \`file\` is kept alive across this call` |
| | `a_function_that_keeps_a_file_across_its_callback_runs_under_a_once_handler` | `locked` under `State.run`: no diagnostic |
| `eml_types/tests/classes.rs` | `an_instance_that_keeps_a_file_across_its_callback_needs_more_than_the_class` | `E2011 18:3 \`each\` in the instance for \`Path\` needs more than the signature of \`each\` allows`, the general note |
| | `an_instance_inherits_the_carry_of_a_recursive_group` | `E2011 19:3 \`run\` in the instance for \`Int\` needs more than the signature of \`run\` allows` |
| | `an_instance_method_may_call_its_method_with_a_handled_effect` | no diagnostic |

Count: 5 + 7 + 27 + 2 + 1 = 42 tests deleted, 6 added, 1 moved: 5263 − 42 + 6 = 5227 listed; the deleted `#[ignore]` test takes the ignored from 23 to 22; 5227 − 22 = 5205.

## 8. Docs: the file-by-file list

Each row: file, anchor (counted once at 6177715b), the edit. "Delete" removes the whole bullet, table row or sentence that contains the anchor.

### 8.1 RB-1 (docs/future/roadmap.md)

- Anchor `- row変数の上限\`<never e>\`を言語からなくすこと`: delete the item and its sub-bullet.
- Anchor `- \`native-checked\`: ネイティブのコードが受け入れるプログラムに`: insert before it, as the first entry of 「段の列」 (final text):

> - `row-bound-retire`: row変数の多重度の上限`<never e>`を言語からなくす。rowに多重度のキーワードを書く構文も、E1057とE2020もなくなり、2つの番号は欠番にする。多重度の制約は下限だけになり、rowの多重度を読むのは持ち越し規則だけになる。型検査は、上限の出どころの記録と、SCC全体の解の上での上限の確かめを持たなくなる。受け取った関数の呼び出しをまたいで線形な値を持つ関数は、今と同じく推論で持ち越しの制約をスキームに持ち、`multi`の操作を含むrowを渡す呼び出しはE3006になる。クラスのメソッドのシグネチャには上限を書けなくなり、row `e`の呼び出しをまたいで線形な値を持つinstanceはいつもE2011になる。E2011は多重度の境界を比べなくなる。instanceのメソッドのスキームは多重度の境界を持たないためである。変わるのは`eml_syntax`、`eml_hir`、`eml_types`とそのテスト、UIテストだけである。前提: なし。設計: [型と Kind](../spec/types.md)の「推論」

### 8.2 Edits by unit

Part RB-2:

| File | Anchor | Edit |
|---|---|---|
| docs/spec/grammar.md | `row_var     ::= 'never'? LIDENT` | the line becomes `row_var     ::= LIDENT` |
| docs/spec/grammar.md | `row 変数の前には、多重度の上限 \`never\` を書ける` | delete the bullet |
| docs/implementation/testing.md | `推論で上限が付く関数の輪 (\`a_ring_of_functions_with_inferred_bounds\`)、` | delete exactly the anchor text from the sentence |

Part RB-3:

| File | Anchor | Edit |
|---|---|---|
| docs/spec/types.md | `row変数の上限\`<never e>\`で、受け取った関数のrowを制限するために使う` | replace the bullet. EN: "`Never` is the Kind of a row that contains only `never` operations. No surface syntax names it. The carry rule asks only whether a row is `Once` or below, so no constraint tells `Never` from `Once`. The lattice has the three elements `Never ≤ Once ≤ Multi`." |
| docs/spec/types.md | `row 変数の多重度の上限 \`never\`、\`data\` の row の型引数` | remove 「row 変数の多重度の上限 `never`、」 from the list and change 「の4つである」 to 「の3つである」 |
| docs/spec/types.md | `並列のタスクの row (\`Row<Never>\`) に入らない` | the sentence 「`IO` がこれに当たり、並列のタスクの row (`Row<Never>`) に入らない。」 becomes 「`IO`がこれに当たる。」 |
| docs/spec/types.md | `### row 変数の多重度の上限` | delete the section: the heading through the bullet with the anchor `標準ライブラリの関数に、この上限を使うものはない`, up to the next heading `### row の型引数` |
| docs/spec/types.md | `シグネチャに書いた row 変数の多重度の上限 (上の「row 変数の多重度の上限」) は` | delete the sub-bullet |
| docs/spec/types.md | `row の側の変数の上限から導ける持ち越しの制約は、多相化するときに残さない` | replace the sub-bullet. EN: "A multiplicity constraint is a lower bound: no constraint puts a constant above a multiplicity variable. So the multiplicity lattice alone is never violated, and only carry constraints read the multiplicity of a row." |
| docs/spec/types.md | `書いたものと推論したもののどちらも書き出さず` | replace the paragraph. EN: "Multiplicities are not written out. Later stages do not read them, and the display of Kind constraints is limited to the `once` arrow." |
| docs/spec/types.md | `導けない制約 (線形性と多重度の境界、持ち越しの制約) があれば E2011 にする` | 「(線形性と多重度の境界、持ち越しの制約)」 becomes 「(線形性の境界と持ち越しの制約)」; append to the same bullet. EN: "The scheme of an instance method or a default method has no multiplicity bound. A method name refers to the method of the class, so nothing refers to the function of the instance, and none of its own multiplicity variables is made equal to another variable." |
| docs/spec/types.md | `シグネチャの row 変数に多重度の上限 (\`<never e>\`) を書けば` | delete the sub-bullet |
| docs/spec/declarations.md | `操作のシグネチャの row 変数に、多重度の上限 \`never\` は書けない` | delete the bullet |
| docs/spec/diagnostics.md | `\| E1057 \| \`ROW_BOUND_NOT_DECLARED\` \|` | delete the row |
| docs/spec/diagnostics.md | `\| E2020 \| \`ROW_BOUND_EXCEEDED\` \|` | delete the row |
| docs/spec/diagnostics.md | `E2007 と E2008 は欠番で、ほかの診断に使わない。` | the sentence becomes 「E1057、E2007、E2008、E2020は欠番で、ほかの診断に使わない。」 |
| docs/spec/effects.md | `\`never\`の操作だけを含むrow (\`Row<Never>\`) に入れられる` | 「多重度は`Never`で、`never`の操作だけを含むrow (`Row<Never>`) に入れられる。」 becomes 「多重度は`Never`である。」 |
| docs/spec/stdlib.md | `row に上限がないので、\`IO\` を起こす計算も渡せる` | 「row に上限がないので」 becomes 「rowを閉じていないので」 |
| docs/future/ffi.md | `(\`par\`の引数のような、エフェクトの行の制限)` | replace the sentence that contains it (「そのため設計は、…のどちらかを選ぶ。」). EN: "So the design either gives the callback the closed row `<>`, so that no `never` operation leaves it, or ends the process as it does for a fatal error." |
| docs/implementation/diagnostics.md | `\| E1057 \| \`ROW_BOUND_NOT_DECLARED\` \|` | delete the row |
| docs/implementation/diagnostics.md | `\| E2020 \| \`ROW_BOUND_EXCEEDED\` \|` | delete the row |
| docs/implementation/diagnostics.md | `導けない制約がすべて多重度の境界のときは` | delete that sentence (the special note and help) from the E2011 row |
| docs/spec/diagnostics.md | `線形性と多重度の境界、持ち越しの制約` | in the E2011 row, 「線形性と多重度の境界、持ち越しの制約」 becomes 「線形性の境界と持ち越しの制約」 (RB-3) |
| docs/implementation/architecture.md | `シグネチャに書いた row 変数の上限 (\`<never e>\`) は、本体の検査 (\`check_body\`)` | delete the bullet |
| docs/implementation/architecture.md | `\`never\` を書いたシグネチャの宣言を指す参照はどれも` | delete the bullet |
| docs/implementation/architecture.md | `E2011 は、instance のメソッドと既定のメソッドの Kind のスキームが` | append to the bullet. EN: "It compares linearity bounds and carry constraints; the scheme of such a method has no multiplicity bound ([型と Kind](../spec/types.md) の「instance と既定のメソッドの検査」)." |
| docs/implementation/status.md | `row 変数の多重度の上限 \`<never e>\`: シグネチャで宣言し` | delete the bullet |
| docs/implementation/status.md | `同じ SCC の中では、上限を SCC 全体の解の上で確かめるので` | delete the bullet |

After part RB-3, re-read docs/implementation/architecture.md 「`eml_types` の内部」 as one block for a sentence that exists only for the bound; none other was found [V: read].

Part RB-4:

| File | Anchor | Edit |
|---|---|---|
| docs/overview.md | `row 変数の多重度の上限 \`never\`、\`data\` の row の型引数と \`Lin\` の宣言に限り` | remove 「row 変数の多重度の上限 `never`、」 from the cell |
| docs/overview.md | `\`Never\` は \`never\` の操作だけの row の Kind で、シグネチャに \`<never e>\` と書ける` | the sentence becomes 「`Never`は`never`の操作だけのrowのKindである」 |

Checked, no edit: docs/future/multicore.md, CLAUDE.md, README.md, docs/spec/examples.md [V: grep].

### 8.3 RB-3 (docs/future/roadmap.md, 「その後の項目」「型システム」)

Delete the four items with their sub-bullets (anchors `- \`<once e>\`: row 変数の多重度の上限に \`Once\` を書けるようにする`, `- \`data\` の row の型引数の上限 (\`data Task <never e> a\`)`, `- 操作の row 変数の上限`, `- \`:type\` と診断での上限の表示`). The first cites the deleted section, the others presuppose the bound. Add after the item with the anchor `- Kind の宣言化: シグネチャに Kind の境界を書き` and its sub-bullets (final text):

> - メソッドのシグネチャのrow変数の上限`<once e>`: クラスのメソッドのシグネチャに、instanceがrow `e`の呼び出しをまたいで線形な値を持ってよいことを書けるようにする。トップレベルの関数には要らない。推論が持ち越しの制約をスキームに残すためである。要るのはクラスのメソッドだけである。メソッドのスキームは宣言だけから作るので、ファイルを1行ずつ読んで受け取った関数を行ごとに呼ぶinstanceや、受け取った関数の結果で`MutArray`を埋めるinstanceはE2011になる ([型と Kind](../spec/types.md)の「instance と既定のメソッドの検査」)。この上限は、宣言に書いた持ち越しの制約`carry(Lin, <e>)`である。そのため、多重度の変数の定数の上限も新しい診断も要らない。文法がrow変数の前の`once`を読み、`Shape::instantiate_rigid`が持ち越しの制約を1つ足し、前提の`carry(Lin, s)`からどの`carry(l, s)`も導けるとし、`multi`の操作を含むrowを渡す呼び出しはE3006になる。トップレベルのシグネチャに書けるか、ほかのどこに書けないか (`data`のrowの型引数、操作のrow変数。操作ごとのrow変数は節の中で`Multi`以上になる) は、そのとき決める。`Never`の上限を使う関数は今はない。受け取った関数が中断するかどうかに関わるのは実行系の境目を越えるexternで、今のexternはどれも閉じたrowを取る。
>   - 後に回した理由: stdにもUIテストにも、メソッドのシグネチャにrow変数を書くクラスがない。
>   - 見直す条件: 受け取った関数の呼び出しをまたいで線形な値 (ファイル、埋めている途中の`MutArray`) を持つinstanceのあるクラスが、stdかプログラムに要るとき。

### 8.4 What the later item records

The design of `<once e>` as a declared carry was prototyped (Appendix A, A.3): about ten lines in `eml_types`, none of the machinery RB-3 deletes.

## 9. Verification plan

Baseline (RB-1 fills "Recorded" on the clean tree at its base):

| Check | Command | Expected at 6177715b | Recorded |
|---|---|---|---|
| Tests | `cargo nextest run` | `5240 tests run: 5240 passed, 23 skipped` [V: proto, `cargo nextest list`: 5263 listed, 23 ignored] || `cargo nextest list`: 5240 (the full run of the same code passed at `Close std-par` + its fix commit: 5239 + 1, and CI on d023005b) |
| Clippy | `cargo clippy --all-targets -- -D warnings` | exit 0 || exit 0 |
| Format | `cargo fmt --check` | exit 0 || exit 0 |
| Crate graph | `bash nix/crate-graph.sh --check` | exit 0 || exit 0 |
| Native tally | `sed -n 5p crates/eml_cli/tests/snapshots/integration__ui__run_native.snap` | `accepted 493` [V: read] || `accepted 493` |
| The bound in code and tests | `git grep -nE '<never\|never e>' -- crates tests std bench \| wc -l` | 44 [V: grep] || 44 |
| The machinery | S-2's command `\| wc -l` | 167 [V: grep] || 168 (167 + the 「段の列」 entry RB-1 adds, which names E1057 and E2020) |
| Code citations of the section | `git grep -n '多重度の上限' -- crates \| wc -l` | 7 [V: grep] || 7 |
| Keywords in the row grammar | `grep -cE 'NEVER_KW\|ONCE_KW\|MULTI_KW' crates/eml_syntax/src/grammar/types.rs` (the `\|` is the table's escape for `|`) | 4 [V: grep] || 4 |
| Draft markers | SC-2 `\| wc -l` | 0 [V: grep] || 0 |

After each unit: G and the SC lines in force, and the unit's completion line. After RB-3 also `git grep -n '多重度の境界' -- docs ':!docs/plans' ':!docs/reports'` → empty. Expected totals: RB-1 5240 / 23 skipped; RB-2, RB-3, RB-4 5205 / 22 skipped [V: proto].

Checkpoint CP-A (after RB-3): push the branch; CI green on every job. The cross-target clippy lines and the sanitizer and Miri runs are CI's; the stage changes no `unsafe`, no OS-specific code and no runtime crate, so no local checkpoint extra is required.

## 10. Risks and unresolved items

- R-1: a rule relies on a multiplicity upper bound that grep missed. Held by the two debug assertions, which ran over the whole workspace on the prototype.
- R-2: the E2011 argument misses a way to reference an instance method's function. The assertion fails loudly in any test; the comparison can be restored from git.
- R-3: the class-method case has no notation until the later item is taken up (accepted; the ways to write it are a top-level function, reading the resource whole before calling the function, or returning an `Iter`).
- R-4: `terminal::interrupts::interrupts_while_waiting_do_not_end_the_repl` failed once on a loaded machine (section 3). Not caused by the stage.
- R-5: `native-checked`'s plan reads the native tally; D-17 records the new number.
- R-6: "no function written in eml needs `Never`" is an argument, not a test [I]. The removal does not rest on it alone: no extern, std function, test or bench uses the bound [V: grep].
- Unresolved: none.

## Additional checks

- The prototype applied the code and test changes only. The docs edits were simulated as deletions (the section, every line that cites it, the diagnostics rows, the architecture and status bullets, the roadmap items) to run `citations::`; the replacement sentences of section 8 cite only 「instance と既定のメソッドの検査」 and 「推論」, which exist.
- Not run on the prototype: `cargo nextest run` itself (the totals come from `cargo nextest list` and from `cargo test --workspace`), the cross-target clippy lines, CI.

- Finding 4 of the plan review: `docs/plans/2026-11-21-remote-reclaim.md` cites this plan by path; that plan is refreshed at its own Start, which also places its 「段の列」 entry after this stage's.

## Appendix A: the practical-program study and the E2011 argument (from r2)

### A.1 Study

#### A.1.1 Where a bound can matter at all

A Kind scheme is inferred for every function with a body. A scheme is made from the declaration alone only for class methods, extern functions (std only), operations and constructors. So a written bound can change the outcome only there; elsewhere it can only reject more.

#### A.1.2 Programs

Results are from the harness [V] unless marked.

| # | Program | Needs | Today | After removal | With `<once e>` (prototype) | Otherwise written |
|---|---|---|---|---|---|---|
| A1 | generic combinator: `Array.tabulate 3 f` (std; fills a `MutArray` across `f`) with an `IO` callback and with a `multi` callback | none | `IO`: accepted. `multi`: `E3006 14:32 \`tabulate\` keeps a linear value alive across a call that may resume more than once` | same | same | - |
| A2 | streaming: `each_line : FileIn.FileIn -> (String -> <e> Unit) -> <IO, Panic \| e> FileIn.FileIn`, recursion over `FileIn.read_line`; callback uses a `var` and `println`; second call with a `multi` callback | none | scheme `carry(Lin, <e>)`; `var`+`IO`: accepted; `multi`: `E3006 20:24 \`each_line\` keeps …`, secondary `11:7 \`file\` is kept alive across this call` | same | same | - |
| A2' | the same with `<never e>` on `each_line` | - | the `var`+`IO` callback is E2020 [I: same shape as `row_bounds::a_function_that_uses_a_var_exceeds_the_bound`] | cannot be written | - | drop the bound |
| B1 | class: `class Source s where each : s -> (String -> <e> Unit) -> <IO, Panic \| e> Unit`; `instance Source Path` opens the file and calls `each_line` | `once` | `E2011 18:3 \`each\` in the instance for \`Path\` needs more than the signature of \`each\` allows` (instance scheme `carry(Lin, <e>)`) | same | - | B4, or a top-level function, or return an `Iter` |
| B2 | B1 with `<never e>` in the class | - | instance accepted, but `each (Path "a.txt") (fn line -> println line)` is `E2020 21:11 the row passed to \`each\` may perform \`IO\`, but \`each\` allows only \`never\` operations` | cannot be written | - | - |
| B3 | B1 with `<once e>` in the class | `once` | E0011 (only `never`) | E0011 (D-2) | accepted; class scheme `s <= Unr, carry(Lin, <e>)`; `println` callback accepted | - |
| B4 | B1 rewritten: the instance does `FileIn.read_all`, closes, then `List.each f (String.lines text)` | none | accepted | accepted | accepted | (this is the rewrite; it gives up streaming) |
| B5 | B3 plus `class Keep a where keep : a -> b -> (Unit -> <once e> Unit) -> <e> b` whose instance keeps `x : b` across `action ()`, a generic `Source s =>` user, and a `multi` callback | `once` | - | E2011 twice | both instances accepted; generic use accepted; `multi`: `E3006 37:10 \`each\` keeps a linear value alive across a call that may resume more than once` | constrain `b` (makes it `Unr`) for `Keep` |
| C1 | `Fail.to_result (fn () -> Par.par (fn () -> check 1) (fn () -> check 0))`, children raise `Fail` | (`never`, on an extern) | `E2002 9:54 \`check\` performs \`Fail\`, which this lambda does not allow` (the children's row is the closed `<Panic>`) | same | same | C2 |
| C2 | C1 with `Fail.to_result` inside each child and the results combined outside | none | accepted | accepted | accepted | - |
| D1 | handler that resumes once + a resource holder: `locked : (Unit -> <e> a) -> <IO \| e> a` keeps a `FileIn` across `action ()`; called under `State.run` with `get`/`put` | none | accepted; scheme `carry(Lin, <e>)` | same | same | with `<never e>` on `locked` this would be E2020, since `State` has `once` operations |
| E | `Std.Async` callbacks (`start`, `spawn`, `shield`, `timeout`, `race`, `compute`) | none | every one takes a closed row (`<IO, Panic, Async>` or `<Panic>`) [V std/Async.em] | same | same | - |
| F | `Std.Gui` (`window`, `grid`, `cell`) | none | `(… -> <Gui msg \| e> a) -> <Gui msg \| e> a`; nothing linear is kept [V std/Gui.em signatures] | same | same | - |
| G | C callbacks (not implemented) | closed row | docs/future/ffi.md line 209: the callback's row is `<>` | - | - | - |

Reading:

- Functions with bodies (A1, A2, D1) need no bound: the carry is inferred and the misuse is E3006, which names the kept value. A `never` bound on them only breaks ordinary callbacks (A2', D1).
- The level `Never` has one use, C1: a function received by an extern that crosses a runtime boundary must not suspend, yet could be allowed to abort. `par` had it and gave it up for total children (00-decisions UJ-14, SC-5); C2 is the way to write it. No function written in eml can need `Never`: a continuation resumed exactly once is not observable to code that keeps linear values, and only a second resumption is.
- A bound is needed in B1/B5 only, and the level is `Once`.

#### A.1.3 `<once e>` as a declared carry (prototype)

`carry(l, s)` means "if `l` is `Lin` then `s ≤ Once`", so `carry(Lin, <e>)` is the bound `σ_e ≤ Once`. The prototype [V]: the parser reads `once` before a row variable; HIR marks the row variable; `Shape::instantiate_rigid` pushes one `Carry { lin: Const(Lin), mult: Var(σ_e) }`; `unentailed` treats an assumed `carry(Lin, s)` as entailing every `carry(l, s)`. That is all: about ten lines in `eml_types`. The carry stays in the scheme by the existing residual rule, is copied to callers by the existing rule, and a violating call is the existing E3006 (B5). No `Var ≤ Const` multiplicity constraint, no E2020, no bound sources. Not designed in the prototype: where the keyword may be written (the present E1057 rules), the origin of a violation inside the declaring SCC, and the wording when the carry is declared rather than inferred.

### A.2 Why the multiplicity comparison in E2011 is vacuous after removal

Claim: after removal, the scheme of the function of an instance method or default method has an empty `mult`.

1. A scheme's `mult` holds the residual constraints over the function's own multiplicity variables, which are the σ of its signature's row variables (rigid in the body).
2. A rigid row variable is never bound ([V] table/row.rs: every binding path tests `is_rigid_row`). Inside a body its σ therefore occurs only as the lower side of `σ_e ≤ σ_v`, from binding an inference variable `v` to a row whose tail is `e` (row.rs:319-322). Label lower bounds (`Const ≤ σ_v`) and copied scheme constraints land on inference variables, above `e`, never below it.
3. With no constant upper bound left (D-4), walking up from `σ_e` reaches no constant, and reaches another own variable only if an inference variable equals one. An inference variable equals an own variable only through `Merged::equate`, that is, when the function is referenced from its own SCC.
4. The function of an instance or default method is never referenced: a method name resolves to `ValueItem::Method`, whose scheme comes from the class declaration, and the call graph has edges only for `ValueItem::Function` and `ValueItem::Binding` paths ([V] scc.rs:39-40). So no `equate` touches its own variables.

Hence nothing is left to compare. Evidence [V]: with written bounds ignored, an assertion "the scheme compared by E2011 has no multiplicity bound other than a constant upper bound" held over all 544 `eml_types` integration tests (every one checks the Prelude's instances); the two candidate witnesses (an instance method that calls its own method, or a self-recursive helper, with a lambda performing a handled `once` effect) are accepted today and leave the instance scheme's `mult` empty, while the helper `go` itself gets `Once ≤ σ_e` from its self-reference.

Side observation, out of scope: by the same reasoning the lower bounds that remain in ordinary schemes come only from in-SCC references and make callers stricter than needed. Dropping `KindScheme.mult` would accept more programs and wants its own soundness review; this stage does not do it.

## Appendix B: the study programs

### a1_combinator.em

```
import Array

effect Choice where
  multi choose : Unit -> Bool

pick : Int -> <Choice> Int
pick i = if choose () then i else 0

main : Unit -> <IO, Panic> Unit
main () =
  let a = Array.tabulate 3 (fn i ->
    println (show i)
    i * 2)
  let n = handle Array.length (Array.tabulate 3 pick) with
    | ctl choose () k -> k True + k False
  println (show (Array.length a + n))
```

### a2_stream.em

```
import FileIn

effect Choice where
  multi choose : Unit -> Bool

each_line : FileIn.FileIn -> (String -> <e> Unit) -> <IO, Panic | e> FileIn.FileIn
each_line file f =
  match FileIn.read_line file with
    | (None, file) -> file
    | (Some line, file) ->
      f line
      each_line file f

main : Unit -> <IO, Panic> Unit
main () =
  var count = 0
  FileIn.close (each_line (FileIn.open "a.txt") (fn line ->
    count <- count + 1
    println line))
  handle FileIn.close (each_line (FileIn.open "a.txt") (fn line -> if choose () then println line else ())) with
    | ctl choose () k ->
      k True
      k False
```

### b1_class_nobound.em

```
import FileIn

data Path =
  | Path String

each_line : FileIn.FileIn -> (String -> <e> Unit) -> <IO, Panic | e> FileIn.FileIn
each_line file f =
  match FileIn.read_line file with
    | (None, file) -> file
    | (Some line, file) ->
      f line
      each_line file f

class Source s where
  each : s -> (String -> <e> Unit) -> <IO, Panic | e> Unit

instance Source Path where
  each (Path p) f = FileIn.close (each_line (FileIn.open p) f)

main : Unit -> <IO, Panic> Unit
main () = each (Path "a.txt") (fn line -> println line)
```

### b2_class_never.em

```
import FileIn

data Path =
  | Path String

each_line : FileIn.FileIn -> (String -> <e> Unit) -> <IO, Panic | e> FileIn.FileIn
each_line file f =
  match FileIn.read_line file with
    | (None, file) -> file
    | (Some line, file) ->
      f line
      each_line file f

class Source s where
  each : s -> (String -> <never e> Unit) -> <IO, Panic | e> Unit

instance Source Path where
  each (Path p) f = FileIn.close (each_line (FileIn.open p) f)

main : Unit -> <IO, Panic> Unit
main () = each (Path "a.txt") (fn line -> println line)
```

### b3_class_once.em

```
import FileIn

data Path =
  | Path String

each_line : FileIn.FileIn -> (String -> <e> Unit) -> <IO, Panic | e> FileIn.FileIn
each_line file f =
  match FileIn.read_line file with
    | (None, file) -> file
    | (Some line, file) ->
      f line
      each_line file f

class Source s where
  each : s -> (String -> <once e> Unit) -> <IO, Panic | e> Unit

instance Source Path where
  each (Path p) f = FileIn.close (each_line (FileIn.open p) f)

main : Unit -> <IO, Panic> Unit
main () = each (Path "a.txt") (fn line -> println line)
```

### b4_class_rewrite.em

```
import FileIn
import List
import String

data Path =
  | Path String

class Source s where
  each : s -> (String -> <e> Unit) -> <IO, Panic | e> Unit

instance Source Path where
  each (Path p) f =
    let (text, file) = FileIn.read_all (FileIn.open p)
    FileIn.close file
    List.each f (String.lines text)

main : Unit -> <IO, Panic> Unit
main () = each (Path "a.txt") (fn line -> println line)
```

### b5_class_once_multi.em

```
import FileIn

effect Choice where
  multi choose : Unit -> Bool

data Path =
  | Path String

each_line : FileIn.FileIn -> (String -> <e> Unit) -> <IO, Panic | e> FileIn.FileIn
each_line file f =
  match FileIn.read_line file with
    | (None, file) -> file
    | (Some line, file) ->
      f line
      each_line file f

class Source s where
  each : s -> (String -> <once e> Unit) -> <IO, Panic | e> Unit

instance Source Path where
  each (Path p) f = FileIn.close (each_line (FileIn.open p) f)

class Keep a where
  keep : a -> b -> (Unit -> <once e> Unit) -> <e> b

instance Keep Int where
  keep _ x action =
    action ()
    x

generic : Source s => s -> <IO, Panic> Unit
generic s = each s (fn line -> println line)

main : Unit -> <IO, Panic> Unit
main () =
  generic (Path "a.txt")
  handle each (Path "a.txt") (fn line -> if choose () then println line else ()) with
    | ctl choose () k ->
      k True
      k False
```

### c1_par_fail.em

```
import Std.Par
import Fail (Fail(..))

check : Int -> <Fail String> Int
check n = if n > 0 then n else fail "not positive"

main : Unit -> <IO, Panic> Unit
main () =
  let r = Fail.to_result (fn () -> Par.par (fn () -> check 1) (fn () -> check 0))
  println (show r)
```

### c2_par_fail_rewrite.em

```
import Std.Par
import Fail (Fail(..))

check : Int -> <Fail String> Int
check n = if n > 0 then n else fail "not positive"

both : Result String Int -> Result String Int -> Result String (Int, Int)
both a b =
  match (a, b) with
    | (Ok x, Ok y) -> Ok (x, y)
    | (Err e, _) -> Err e
    | (_, Err e) -> Err e

main : Unit -> <IO, Panic> Unit
main () =
  let (a, b) = Par.par (fn () -> Fail.to_result (fn () -> check 1)) (fn () -> Fail.to_result (fn () -> check 0))
  println (show (both a b))
```

### d1_gui_handler.em

```
import State (State(..))
import FileIn

locked : (Unit -> <e> a) -> <IO | e> a
locked action =
  let lock = FileIn.open "lock"
  let r = action ()
  FileIn.close lock
  r

main : Unit -> <IO, Panic> Unit
main () =
  let (n, s) = State.run 0 (fn () -> locked (fn () ->
    put (get () + 1)
    get ()))
  println (show (n + s))
```

### f2_scc_carry.em

```
import FileIn

ping : Int -> (Unit -> <e> Unit) -> <IO, Panic | e> Unit
ping n action =
  if n == 0 then ()
  else
    let lock = FileIn.open "lock"
    action ()
    FileIn.close lock
    pong (n - 1) action

pong : Int -> (Unit -> <e> Unit) -> <IO, Panic | e> Unit
pong n action = if n == 0 then () else ping (n - 1) action

class Runs a where
  run : a -> (Unit -> <e> Unit) -> <IO, Panic | e> Unit

instance Runs Int where
  run n action = pong n action

main : Unit -> <IO, Panic> Unit
main () = run 2 (fn () -> println "x")
```

### q3a.em

```
effect Ask where
  ask : Unit -> Int

go : Int -> (Unit -> <e> Int) -> <Panic | e> Int
go n f =
  if n == 0 then f ()
  else
    handle go (n - 1) (fn () -> ask ()) with
      | fn ask () -> 1

class Runs a where
  run : a -> (Unit -> <e> Int) -> <Panic | e> Int

instance Runs Int where
  run n f = go n f

main : Unit -> <IO, Panic> Unit
main () = println (show (run 2 (fn () -> 7)))
```
