# Stage row-bound-retire: the row-variable bound `<never e>` leaves the language

- 2026-11-21. Base: main @ 6ff1f664. Supersedes NB-response-r2.md; the practical-program study is Appendix A (programs in Appendix B).
- Design-level. `Start row-bound-retire` commits it as `docs/plans/2026-11-21-row-bound-retire.md` with §8 made executable. No appendix. Outside the runtime group (00-decisions D-14 (c)).
- User decisions of 2026-11-21: the stage is scheduled; it is first in 「段の列」, before `native-checked`, 前提: なし; "No need for compatibility or user consideration. Remove it entirely" (so no parser recovery, no diagnostic, test or help for the old syntax).
- Tags: [V] verified, [I] inferred. Three prototypes, each a scratch export of HEAD, deleted afterwards; the repository was not touched; no `.em` program was run outside the project's own test suite.
  - P1 (r1): bounds ignored, `cargo test -p eml_types -p eml_syntax -p eml_hir`.
  - P2 (r2): a type-check harness over the study programs; `<once e>` as a declared carry.
  - P3 (r3): the "after removal" type checker (written bounds ignored; `assert!(mult_violated.is_empty())` in `solve_scc`; `assert!(scheme.mult.is_empty())` at the E2011 check; the multiplicity comparison and the clause `mult.upper(carry.mult) <= Once` removed from `unentailed`), then **`cargo test --workspace --no-fail-fast -q` once**; then the plain parser of D-2 with `cargo test -p eml_syntax -p eml_hir`.

## 0. Rulings (from r2, as amended)

- R-1: remove `<never e>`. No practical program needs the level `Never`; where a bound is needed (a class method) `never` is the wrong level. Appendix A.1.2.
- R-2: `<once e>` is not added in this stage. It is a later item whose design is fixed: the bound is the existing constraint `carry(Lin, <e>)` written in the declaration (P2: about ten lines, none of the deleted machinery). Appendix A.1.3.
- R-3: E2011 stops comparing multiplicity bounds, and no program changes. Argument in §3 D-5; evidence P3.
- Name: `row-bound-retire`. Parser: no recovery (user ruling).

## 1. Goal and completion conditions

Goal: no surface syntax, HIR field, Kind constraint, diagnostic, doc section or test knows a multiplicity upper bound of a row variable. The lattice, operation and effect multiplicities and the carry rule stay.

- C-1: the grammar of a row has no multiplicity keyword: `row_var ::= LIDENT`; `grammar/types.rs` names `NEVER_KW`, `ONCE_KW`, `MULTI_KW` only where `once` arrows are read. No test, diagnostic or doc sentence speaks of a multiplicity in a row.
- C-2 (holds at `Close`): `git grep -nE 'E1057|E2020|ROW_BOUND|RowBound|BoundSource|BoundCause|Exceeding|bound_sources|own_bounds|mult_at_most|RowUpper|Provenance::Bound|KindReason::Performs|Unentailed::Multiplicity|inferred_bounds' -- crates docs ':!docs/reports' ':!docs/plans'` prints only the vacancy sentence of docs/spec/diagnostics.md. Before `Close` it also prints the 「段の列」 entry.
- C-3 (holds at `Close`): `git grep -nE '<never|never e>' -- crates tests std bench docs ':!docs/reports' ':!docs/plans'` prints nothing.
- C-4: no function of `eml_types` can add `σ ≤ constant` on a multiplicity variable (`Table::mult_at_most` is gone); `solve_scc` debug-asserts that no multiplicity constraint is violated; the E2011 check debug-asserts `scheme.mult.is_empty()`. Both assertions are the ones P3 ran [V].
- C-5: the new tests of §4 pass.
- C-6: `cargo test --workspace -q`, `cargo nextest run`, `cargo clippy --all-targets -- -D warnings`, the `eml_cli` feature-combo clippy lines (`none`, `types`, `core`) and `cargo fmt --check` pass. No `eml_rt` gate, Verus run or bench measurement applies.

## 2. Scope

In: `eml_syntax` (grammar/types.rs, ast.rs), `eml_hir` (hir.rs, lower/types.rs, lower/data.rs, pretty.rs, `codes`), `eml_types` (shape.rs, table/, kind/, check/, dump.rs, ty.rs, `codes`), their tests, six UI tests (five deleted, one edited), one native tally snapshot, the docs of §5, the roadmap.

Out: the three-element lattice; `OpMultiplicity`; `EffectMultiplicity` in `eml_extern`; `yields`; the handler clause rules; `multiplicity_at_least_multi`; the carry rule and E3006; `KindScheme.mult` (lower bounds from in-SCC references remain); the generic solver and its unit tests; the keywords themselves; `<once e>`; any aid for programs that wrote the old syntax.

## 3. Decisions

- **D-1. Remove.** Removed with it: E1057, E2020, `Provenance::Bound`, `KindReason::RowBound` / `Performs`, `BoundSource`, `BoundCause`, `Exceeding`, `SourceIndex`, `bound_sources`, `own_bounds`, `broken_bounds`, `with_bound_cause`, the rule that drops a carry a bound entails, the E2020/E3006 de-duplication, the SCC caveat of status.md, one scaling test, four deferred roadmap items. Invariant gained: a multiplicity constraint is a lower bound, so only carries can be violated. [V] the only producers of `Var ≤ Const` on multiplicity are `Table::mult_at_most` (one caller, shape.rs:781) and its two copies in solve.rs. [V P3] over the whole workspace no multiplicity constraint was violated.
- **D-2. The parser has no case for a multiplicity in a row.** `row_bound` and the keyword branch of `row_param` are deleted; `effect_row` reads a row variable, or labels and an optional `| LIDENT`. `EffectRow::bound` is deleted. A keyword there is whatever the grammar says of any other token ([V P3] `f : Unit -> <never e> Unit` gives `E0011 1:14 expected an effect`; `data T <never e> = …` gives `E0011 1:9 expected a row variable`; the parser terminates on every old test input). Nothing pins these outcomes.
- **D-3. HIR.** `RowVarDecl` loses `never`; `row_var` loses `bound`; `bound_not_declared` and `codes::ROW_BOUND_NOT_DECLARED` go; pretty.rs prints the tail name.
- **D-4. Type checker: delete.** `Table::mult_at_most`; the bound block of `Shape::instantiate_rigid` and its `file` parameter if then unused; `Provenance::Bound`; `KindReason::RowBound` and `Performs` (row.rs:303-316 uses the current origin for label lower bounds); `BoundSource`, `BoundCause`, `Exceeding`; in solve.rs `broken_bounds`, `with_bound_cause`, `bound_origin`, `SourceIndex`, `bound_sources`, `own_bounds`, `Merged.own_bounds`, the bound copies in `equate` and `copy_scheme`, the `entailed` filter of `carry_residual`, `Graph::closed_upper` if no reader remains; `KindScheme.bound_sources`; `KindConstraint::RowUpper` and its loop in dump.rs; `report::row_bound_exceeded`, `ROW_BOUND_NOTE`, the multiplicity-name helper if unused; the `bounded` de-duplication in `report_violations`; `codes::ROW_BOUND_EXCEEDED`. `solve_scc` solves multiplicity with `solve` and debug-asserts no violation; `carry_violated` loses the `broken` filter.
- **D-5. E2011 compares linearity bounds and carries only.** `Unentailed::Multiplicity`, the multiplicity `Lattice` in entail.rs and the clause `mult.upper(carry.mult) <= Once` go; the "multiplicity only" note and help in check/mod.rs go; the check debug-asserts `scheme.mult.is_empty()`, citing the spec sentence of §5.
  - Why nothing changes: (1) a scheme's `mult` is the residual over the function's own multiplicity variables, the σ of its rigid row variables; (2) a rigid row variable is never bound ([V] table/row.rs), so in a body its σ is only the lower side of `σ_e ≤ σ_v` (row.rs:319-322), and label lower bounds and copied constraints land on inference variables above it; (3) with no constant upper bound left, the residual walk from `σ_e` ends in nothing unless an inference variable equals an own variable, which only `Merged::equate` does, for a function referenced from its own SCC; (4) the function of an instance or default method is never referenced: a method name is `ValueItem::Method`, whose scheme comes from the class, and call edges exist only for `ValueItem::Function` and `ValueItem::Binding` ([V] scc.rs:39-40; the reviewer re-read it and found no counterexample). The clause `mult.upper(carry.mult) <= Once` is dead because no assumed scheme has a multiplicity upper bound, and a carry with a constant `Once` row never reaches a scheme ([V] kinds.rs:81; reviewer: `lowers()`).
  - Evidence [V P3]: with exactly `assert!(scheme.mult.is_empty())` installed and the comparison removed, `cargo test --workspace --no-fail-fast -q` gave 5215 passed, 26 failed, no assertion fired. The 26 are: the 23 `row_bounds` tests and `classes::a_method_that_needs_a_bound_the_class_lacks` (all deleted in §4), the entail.rs unit test whose bounded-row case §4 removes, and `ui::check_fail` on one file, `check-fail/types/row_bound_exceeded.em` (deleted in §4). `ui::run`, `ui::run_fail`, both `ui::*_native`, the REPL transcripts, `eml_core_ir`, `eml_vm` and every other crate passed unchanged.
- **D-6. Codes.** E1057 and E2020 are vacant and not reused, as E2007 and E2008 are.
- **D-7. Three lattice elements stay.** No row constraint tells `Never` from `Once` after the stage; `Never` stays as an operation and effect multiplicity.
- **D-8. Top-down order.** The language first (parser, every test that writes the syntax, `spec/`, the roadmap items that cite the deleted section), then the unreachable machinery.
- **D-9. `<once e>` is not in the stage.** Its design is in the later item of §6.

## 4. Test changes

Reason for all: the feature is removed, and by the user's ruling nothing about the old syntax is tested.

Kind 1 (deletions), one by one:

- UI, with snapshots: `run/kinds/never_row_bound.em`; `check-fail/names/row_bound_in_operation.em`; `check-fail/names/row_bound_not_declared.em`; `check-fail/types/row_bound_exceeded.em`; `check-fail/syntax/row_bound_syntax.em`.
- `crates/eml_syntax/tests/types.rs`: `a_row_variable_can_be_bounded_by_never`, `bounded_row_variables_in_every_row_position`, `only_never_can_bound_a_row_variable`, `never_must_be_followed_by_a_row_variable`, `a_row_parameter_cannot_be_bounded`.
- `crates/eml_hir/tests/lower.rs`: `a_bound_belongs_to_the_row_variable`, `a_misplaced_never_bounds_no_row_variable`, `an_annotation_may_repeat_a_declared_bound`, `a_bound_must_be_declared_in_the_signature`, `a_data_field_cannot_bound_a_row_variable`, `an_operation_cannot_bound_a_row_variable`, `a_bounded_row_parameter_stays_a_parameter`.
- `crates/eml_types/tests/row_bounds.rs`: the file and its `mod` line; 27 tests: `a_written_bound_stays_in_the_scheme`, `a_call_infers_the_bound`, `a_recursive_group_shares_an_inferred_bound`, `an_unused_row_variable_has_no_bound`, `panic_fits_a_never_bound`, `an_effect_of_never_operations_fits_a_never_bound`, `labels_beside_a_bounded_variable_are_not_bounded`, `a_call_that_exceeds_a_written_bound`, `a_multi_operation_exceeds_a_never_bound`, `an_operation_argument_row_exceeds_a_never_bound`, `unwritten_occurrences_carry_the_bound`, `a_self_call_exceeds_its_own_bound`, `a_mutual_call_exceeds_the_bound`, `a_call_from_a_local_function_exceeds_the_bound`, `a_method_call_exceeds_the_class_bound`, `one_call_reports_once_for_two_bounds`, `an_inferred_bound_names_the_inner_call`, `a_chain_names_only_the_nearest_call`, `a_recursive_group_names_the_call_in_the_group`, `a_ring_reports_its_calls_to_the_bounded_function`, `an_inferred_bound_broken_inside_its_group`, `a_bound_drops_the_carry_it_entails`, `a_bound_drops_the_carry_inside_its_group`, `an_effect_with_a_once_operation_exceeds_a_never_bound_even_through_its_never_operation`, `a_value_use_of_a_bounded_function_keeps_the_bound`, `a_function_that_uses_a_var_exceeds_the_bound`, `a_once_operation_exceeds_a_never_bound`. (The 28th, `a_par_child_may_return_a_lin_value`, moves: kind 3.)
- `crates/eml_types/tests/classes.rs`: `a_method_can_rely_on_a_bound_in_the_class_signature`, `a_method_that_needs_a_bound_the_class_lacks`, the const `HELPER`.
- `crates/eml_types/tests/scaling.rs`: `a_ring_of_functions_with_inferred_bounds` (`#[ignore]`) and `ring_with_inferred_bounds`.
- `crates/eml_types/src/kind/mod.rs`, the `order_key` unit test (list ending near line 930): the eight `row_bound(…)` and two `Performs` entries.
- `crates/eml_types/src/kind/entail.rs`: `a_carry_is_entailed_by_an_unrestricted_value_a_bounded_row_or_the_same_pair` loses the bounded-row case (`mult: [(var(0), ONCE)]`, `carry(var(1), var(0))`) and is renamed.

[V] P1 and P3 together: the failing tests are exactly those listed (P1: 5 in `eml_syntax`, 7 in `eml_hir`; P3: see D-5), and the whole workspace otherwise passes.

Kind 2 (expected values), by scope:

- `crates/eml_cli/tests/snapshots/integration__ui__run_native.snap`: `accepted 493` becomes 492: exactly one `run` file is deleted, `kinds/never_row_bound.em`, and it is not in `par/` or `async/` [I that it is counted as accepted; the unit reads the number].

Kind 3 (values byte-identical): `tests/ui/run/par/panic_reaches_nearest_try_panic.em` writes `foo : (Unit -> <e> Int) -> <Panic | e> Int`; `a_par_child_may_return_a_lin_value` moves to `crates/eml_types/tests/linearity.rs`; `bound_sources: Vec::new()` leaves the `KindScheme` literals in unit tests (entail.rs, solve.rs).

New tests (first), all in `eml_types` and all checked on P2/P3 [V]:

- `linearity.rs`: streaming `each_line : FileIn.FileIn -> (String -> <e> Unit) -> <IO, Panic | e> FileIn.FileIn` (r2 A2) is accepted with a callback that assigns a `var` and prints, and gives `E3006 20:24 \`each_line\` keeps a linear value alive across a call that may resume more than once` with the secondary on `file` for a `multi` callback; the resource holder `locked` under `State.run` (r2 D1) is accepted.
- `classes.rs`: (a) the file-backed `instance Source Path` (r2 B1) is E2011 with the general note; (b) reviewer's case: `ping` keeps a `FileIn` across `action ()` and is mutually recursive with `pong`; `instance Runs Int where run n action = pong n action` is `E2011 19:3 \`run\` in the instance for \`Int\` needs more than the signature of \`run\` allows` (the instance scheme is `carry(Lin, <e>)`, with an empty `mult`); (c) an instance method that calls its own method with a lambda performing a handled `once` effect is accepted.

## 5. Docs (Japanese through PLaMo; current state only)

In RB-2 (the rule leaves the language):

- docs/spec/types.md: line 18 (`Never` is the Kind of a row with only `never` operations; no surface syntax names it; the carry rule asks only `≤ Once`); line 19 (three writable Kinds); line 24 (drop the clause about `Row<Never>`); delete 「row 変数の多重度の上限」 (lines 59-76); 「推論」: delete the bullets at lines 157 and 161, add "multiplicity constraints are lower bounds only; only a carry can be violated"; line 284 (drop the bound sentences); 「instance と既定のメソッドの検査」: line 456 reads 「線形性の境界と持ち越しの制約」 and gains one sentence (the scheme of an instance or default method has no multiplicity bound, because nothing references its function), delete line 457.
- docs/spec/grammar.md: line 113 `row_var ::= LIDENT`; delete the bullet at line 163.
- docs/spec/declarations.md: delete line 97.
- docs/spec/diagnostics.md: delete the E1057 and E2020 rows; line 166: 「E1057、E2007、E2008、E2020は欠番で、ほかの診断に使わない。」
- docs/spec/effects.md line 96: keep 「多重度は`Never`」, drop the `Row<Never>` clause.
- docs/spec/stdlib.md line 1001 (`both`): 「row に上限がないので」 becomes 「rowを閉じていないので」.
- docs/future/roadmap.md: edit 4 of §6 (the items that cite the deleted section).
- docs/future/ffi.md line 209: the sentence 「そのため設計は、コールバックの外へ出る`never`の操作を禁じる (`par`の引数のような、エフェクトの行の制限) か、…」 becomes 「そのため設計は、コールバックのrowを閉じた`<>`にして`never`の操作が外へ出ないようにするか、致命的な誤りと同じくプロセスを終えるかのどちらかを選ぶ。」 (the same paragraph already gives the callback the row `<>`; `par`'s argument is no longer an example of a multiplicity restriction).

In RB-3 (the machinery):

- docs/implementation/diagnostics.md: delete the E1057 and E2020 rows; E2011 loses the multiplicity-only note and help.
- docs/implementation/architecture.md 「`eml_types` の内部」: delete the bullets at lines 287 and 294; the E2011 bullet (line 296) says it compares linearity bounds and carries; re-read lines 280-296 as one block for anything else that exists only for the bound.
- docs/implementation/testing.md line 711: drop 「推論で上限が付く関数の輪 (`a_ring_of_functions_with_inferred_bounds`)」 from the list of scaling shapes.

At Close: docs/implementation/status.md (delete lines 21 and 135); docs/overview.md lines 30 and 31; the roadmap (§6, edit 5). After the edits, `grep -n '上限'` over docs/spec/types.md and docs/implementation/architecture.md lines 280-296 must show no sentence about a row variable's bound (types.md lines 17, 187, 197 and 329 speak of other things and stay). Checked, no edit: docs/future/multicore.md line 346, CLAUDE.md, README.md.

## 6. Roadmap edits

At `Start`:

1. Delete the item 「row変数の上限`<never e>`を言語からなくすこと」 from 「その後の項目」.
2. First entry of 「段の列」, before `native-checked`:

> - `row-bound-retire`: row変数の多重度の上限`<never e>`を言語からなくす。rowに多重度のキーワードを書く構文も、E1057とE2020もなくなり、2つの番号は欠番にする。多重度の制約は下限だけになり、rowの多重度を読むのは持ち越し規則だけになる。型検査は、上限の出どころの記録と、SCC全体の解の上での上限の確かめを持たなくなる。受け取った関数の呼び出しをまたいで線形な値を持つ関数は、今と同じく推論で持ち越しの制約をスキームに持ち、`multi`の操作を含むrowを渡す呼び出しはE3006になる。クラスのメソッドのシグネチャには上限を書けなくなり、row `e`の呼び出しをまたいで線形な値を持つinstanceはいつもE2011になる。E2011は多重度の境界を比べなくなる。instanceのメソッドのスキームは多重度の境界を持たないためである。変わるのは`eml_syntax`、`eml_hir`、`eml_types`とそのテスト、UIテストだけである。前提: なし。設計: [型と Kind](../spec/types.md)の「推論」

3. 00-decisions.md §D: "D-17 (2026-11-21; user decision): the later item of D-14 (c) is the stage `row-bound-retire`, outside the group, first in 「段の列」 before `native-checked`, 前提: なし. No decision of 01 changes. `native-checked`'s base is `Close row-bound-retire`; its refresh re-reads `integration__ui__run_native.snap`."

In RB-2 (same commit as the deletion of 「row 変数の多重度の上限」, so that no roadmap item cites a missing section):

4. In 「型システム」 delete 「`<once e>`」, 「`data` の row の型引数の上限」, 「操作の row 変数の上限」, 「`:type` と診断での上限の表示」, and add after 「Kind の宣言化」:

> - メソッドのシグネチャのrow変数の上限`<once e>`: クラスのメソッドのシグネチャに、instanceがrow `e`の呼び出しをまたいで線形な値を持ってよいことを書けるようにする。トップレベルの関数には要らない。推論が持ち越しの制約をスキームに残すためである。要るのはクラスのメソッドだけである。メソッドのスキームは宣言だけから作るので、ファイルを1行ずつ読んで受け取った関数を行ごとに呼ぶinstanceや、受け取った関数の結果で`MutArray`を埋めるinstanceはE2011になる ([型と Kind](../spec/types.md)の「instance と既定のメソッドの検査」)。この上限は、宣言に書いた持ち越しの制約`carry(Lin, <e>)`である。そのため、多重度の変数の定数の上限も新しい診断も要らない。文法がrow変数の前の`once`を読み、`Shape::instantiate_rigid`が持ち越しの制約を1つ足し、前提の`carry(Lin, s)`からどの`carry(l, s)`も導けるとし、`multi`の操作を含むrowを渡す呼び出しはE3006になる。トップレベルのシグネチャに書けるか、ほかのどこに書けないか (`data`のrowの型引数、操作のrow変数。操作ごとのrow変数は節の中で`Multi`以上になる) は、そのとき決める。`Never`の上限を使う関数は今はない。受け取った関数が中断するかどうかに関わるのは実行系の境目を越えるexternで、今のexternはどれも閉じたrowを取る。
>   - 後に回した理由: stdにもUIテストにも、メソッドのシグネチャにrow変数を書くクラスがない。
>   - 見直す条件: 受け取った関数の呼び出しをまたいで線形な値 (ファイル、埋めている途中の`MutArray`) を持つinstanceのあるクラスが、stdかプログラムに要るとき。

At `Close row-bound-retire`:

5. Delete the 「段の列」 entry. 「row 要素ごとの捕捉 Kind」 stays as it is.

## 7. Risks

- R-1: a rule relies on a multiplicity upper bound that grep missed. Mitigation: C-4's assertions, which P3 ran over the whole workspace.
- R-2: D-5's argument misses a way to reference an instance method's function. Mitigation: the debug assertion runs in every test (it held on P3); the comparison can be restored from git.
- R-3: the class-method case has no notation until the later item is taken up. Accepted (R-2 ruling); the ways to write it are a top-level function, reading the resource whole before calling the function, or returning an `Iter`.
- R-4: one number in the native tally snapshot meets `native-checked`. Mitigation: D-17.
- R-5: "no function written in eml needs `Never`" is an argument, not a test [I]: a continuation resumed exactly once is not observable to code that keeps linear values. R-1 does not rest on it alone: no extern, std function, test or bench uses the bound [V], and the one boundary case (r2 C1, `par` children raising `Fail`) was given up by UJ-14.

## 8. Units (design level)

- RB-1 `Start row-bound-retire`: the plan, roadmap edits 1-3.
- RB-2 language (D-2, D-8): the parser, `EffectRow::bound`, the HIR call site passes no bound; every kind 1 and kind 2 change of §4 except the two unit tests inside `eml_types/src`; the new `linearity.rs` and `classes.rs` tests; the RB-2 docs of §5; roadmap edit 4. Done when C-1 and C-6 hold and C-3 prints only the 「段の列」 entry.
- RB-3 machinery (D-3 to D-6): code deletions, the two unit-test edits, the RB-3 docs of §5. Done when C-4, C-5 and C-6 hold, C-2 prints only the vacancy sentence and the 「段の列」 entry, and no expected value outside §4 changed.
- RB-4 `Close row-bound-retire`: status.md, overview.md, roadmap edit 5, delete the plan; C-2 and C-3 hold. No measurement step.

## 9. Open questions

None.

---

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

### A.2 R-3: why the multiplicity comparison in E2011 is vacuous after removal

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
