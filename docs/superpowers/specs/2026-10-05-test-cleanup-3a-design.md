# テスト本体の整理 3a: 重複したテストと古くなったテストの削除と統合の設計

位置づけ: 作業用の設計文書。この作業を終えたら、この文書は削除する。変更の理由は `docs/implementation/test-changes.md` に記録する。

## 目的と範囲

テストとテストの文書の整理の、サブプロジェクト3の最初のサイクルである。サブプロジェクト3は次の3つのサイクルに分け、この順に進める。

| サイクル | 内容 |
|---|---|
| 3a | 重複したテストと古くなったテストの削除と統合 (この文書) |
| 3b | 大きいテストファイルの話題ごとの分割と、`src/` の単体テストの `tests.rs` への移動 |
| 3c | UI テストの分類のサブディレクトリへの移動、`ui.rs` の走査のパターンの `**/*.em` への変更、`cli.rs` と文書のパスの更新 |

削除を先にするのは、後で消すテストを分割や分類で動かさないためである。

3a の判断の基準は、[testing.md](../../implementation/testing.md) の「テストの置き場所」の表と「重複させない」である。1つの事実は表で選んだ1か所だけで確かめ、UI テストは機能ごとの代表的なプログラムと代表的な診断の表示だけを持つ。

### ユーザーと合意済みの決定

- サブプロジェクト3を 3a、3b、3c の3つのサイクルに分ける
- 3a では、棚卸しで挙げた次の候補をすべて採る。グループ1 (A1、A2、A3、A14、A15、B1、B2、B3、B4)、グループ2 (A4、A5、A6、A7、A8、A10、A11、A12、A13、A16)、グループ3 (A9、B6、C1、C2)
- すべて種類1の変更で、`test-changes.md` に記録する

### 範囲の外

- テストファイルの分割と移動 (3b)、UI テストのディレクトリの移動 (3c)
- 棚卸しで重複でないと判断したもの。`missing_io_row.em` (E2002 の代表)、`type_mismatch.em` (E2001 の代表)、`handler_clauses.em`、`operation_declarations.em`、`effect_arguments.em`、`resume_and_drop_arity.em` (それぞれの番号の唯一の表示)、`multiple_errors.em` (独立した複数の誤りの報告)、`multi_over_once.em` (段階5まで置く)、UI にしかない `infinite_row_through_effect_argument.em`、`once_continuation_through_effect_argument.em`、`pipe_into_function_parameter.em` など

## 1. UI テストの削除

次の .em と、そのスナップショット (`crates/eml_cli/tests/snapshots/` の同じ名前の `.snap`) を削除する。

| # | ファイル | 残す側 |
|---|---|---|
| A1 | `check-fail/stray_tokens.em` | `eml_syntax/tests/parser.rs::stray_tokens_are_one_error_until_the_next_item`。E0003 の表示は `multiple_errors.em` にもある |
| A2 | `check-fail/unexpected_character.em` | `check-fail/multiple_errors.em` が `€` の E0001 を同じ形で表示する。細部は `lexer.rs::unexpected_characters_are_merged_into_one_error` |
| A3 | `check-fail/undefined_effect_in_higher_order.em` | `eml_types/tests/check.rs::an_undefined_effect_row_is_fresh_at_each_call` (同じプログラム) |
| A4 | `check-fail/effect_in_lambda.em` | `check.rs::a_lambda_cannot_perform_effects_its_expected_type_does_not_allow` (同じソース)。E2002 の表示は `missing_io_row.em` と `pipe_into_function_parameter.em` が残る |
| A5 | `check-fail/unhandled_effect.em` | `eml_types/tests/effects.rs::an_unhandled_operation_is_not_in_the_row` (同じソース) |
| A7 | `check-fail/multi_return_clause.em` | `effects.rs::the_return_clause_of_a_multi_handler_cannot_capture_a_linear_value` |
| A8 | `check-fail/continuation_through_operation.em` | `effects.rs::a_continuation_cannot_pass_through_a_polymorphic_operation_parameter` |
| A9 | `check-fail/later_stage_effects.em` | `eml_hir/tests/effects.rs::handlers_with_an_initial_state_come_in_stage_6` (同じ `from` の E0004) |
| A10 | `check-fail/rigid_type_variable.em` | `check.rs::rigid_type_variables_do_not_unify_with_other_types`。E2001 の表示は `type_mismatch.em` が残る |
| C1 | `check-fail/later_stage_lambda_syntax.em` | 3章の B3 + C1 で、同じ3件の E0004 を `eml_hir/tests/lower.rs` に移す。E0004 の UI は `not_yet_supported.em` だけになる |
| B1 | `run/empty.em` | `run/comments_only.em` (段階1以降、プログラムの部分が同じ `main () = ()`) |
| A11 | `run/jumpless_join_after_call.em` | `eml_core_ir/tests/lower.rs::join_points_left_without_jumps_are_removed` (同じソース。debug ビルドでは verifier も通る) |
| A12 | `run/short_circuit.em` | `run/operators.em` に統合する (2章) |
| A13 | `run/strings_and_let.em` | `run/strings_freed_in_branches.em` に統合する (2章) |

`crates/eml_cli/tests/cli.rs` が削除するファイルを使っているので、次のように置き換える。期待する終了コードと、`[E0001]` を含むことの確認は変えない。

- `run/empty.em` (2か所) → `run/comments_only.em`
- `check-fail/unexpected_character.em` (2か所) → `check-fail/multiple_errors.em`

## 2. UI テストの書き換え

| # | ファイル | 変更 | スナップショット |
|---|---|---|---|
| B2 | `check-fail/not_yet_supported.em` | 段階2で E0004 でなくなった `add`、`fn x -> x`、`add`、`add 1` の行を除き、`data` と `main` だけにする。先頭のコメントを「`data` declarations, which a later stage implements.」の趣旨に直す。`data` の行は2行目のまま置く | 変わらない見込み。変わる場合も、`data` の E0004 の1件だけであることを確かめる |
| A6 | `check-fail/continuation_misuse.em` | `twice` の場合だけを残し、`unused`、`discarded`、`captured` を除く。コメントを2回再開の場合だけの説明に直す | E3001 が1件になり、行番号が変わる |
| A12 | `run/operators.em` | 短絡評価の行の後ろに `println (show_bool (True && noisy False))` を足す。`short_circuit.em` にだけあった場合である | stdout の `True || noisy False` の `True` の後ろに `evaluated` と `False` の2行が入る |
| A13 | `run/strings_freed_in_branches.em` | `strings_and_let.em` にだけあった場合を足す。`++` で文字列を作る関数 (`greet`) と、2回使う文字列である。`main` の末尾に `let name = "eml"`、`println (greet name)`、`println name` を足し、コメントを直す | stdout の末尾に `Hello, eml!` と `eml` の2行が入る |

新しい内容は次のとおりである。コメントは既存の .em と同じく英語で書く。

`check-fail/not_yet_supported.em`

```
-- E0004: `data` declarations, which a later stage implements.
data Color = | Red | Green

main : Unit -> <IO> Unit
main () = println "done"
```

`check-fail/continuation_misuse.em`

```
-- E3001: the continuation of a `once` operation must be used exactly once, so resuming it twice is an error.
effect Ask where
  ask : Unit -> Int

twice : Unit -> Int
twice () =
  handle ask () with
    | ask () k -> resume k 1 + resume k 2
```

`run/strings_freed_in_branches.em` の新しい先頭のコメントと、`main` の末尾に足す行は次のとおりである。

```
-- Strings chosen by `if`, built by a function, used twice, passed to an unused parameter, and discarded
-- must all be freed.
```

```
  let name = "eml"
  println (greet name)
  println name
```

`greet` は `strings_and_let.em` と同じ `greet name = "Hello, " ++ name ++ "!"` を、`twice` の後ろに置く。

## 3. crate のテストの変更

| # | テスト | 変更 |
|---|---|---|
| A14 | `eml_syntax/tests/control.rs::arms_at_the_column_of_match_need_indentation` | 削除。`arms_at_the_column_of_match_are_read_as_arms` が同じ先頭の行と同じ E0009 を持つ |
| A15 | `eml_syntax/tests/nesting.rs::nested_let_blocks_report_one_error` (300段) | 削除。`very_deep_nested_let_blocks_do_not_overflow_the_stack` (2万段) が同じ確かめ方で含む。ほかで使わなくなる補助関数があれば一緒に除く |
| A16 | `eml_syntax/tests/declarations.rs::errors_in_one_declaration_do_not_affect_the_next` | 入力から `z : ) Int` の行を除き、`x : Int ->`、`y : Int`、`w : Int` の3行にする。項目の間の回復は `parser.rs::recovery_resumes_at_the_next_item` が確かめる。スナップショットから `z` の `SIGNATURE` と `ERROR` と、その E0011 が消える |
| B3 + C1 | `eml_hir/tests/lower.rs::constructs_of_later_stages_are_not_yet_supported` | 入力の `let g = fn y -> y` (段階2で E0004 でなくなった) を、`let swap = fn (a, b) -> (b, a)` と `let plus = (+)` の2行に置き換える。スナップショットの診断に、タプルのパターン、タプル、演算子の参照の E0004 の3件が加わる。`data` と `match` の E0004 は残る |
| B4 | `eml_types/tests/check.rs::function_typed_parameters_can_be_called_and_passed` | 削除。段階1の E0004 のテストの名残で、今は通る場合だけを確かめる。`row_variables_pass_effects_through` と `lambdas_are_checked_against_the_expected_type_or_inferred` が含む |
| C2 | `eml_types/tests/check.rs::composition_has_the_prelude_type` | 削除。`>>` のスキームは `builtin_schemes_are_exported`、変換は `eml_hir/tests/operators.rs::composition_operators_are_builtin_calls`、実行は `run/higher_order.em` が確かめる |
| C2 | `eml_syntax/src/layout.rs::aligned_arrow_lines_report_e0009_once` | 仮想トークンの列の比較だけを残し、`aligned_arrow_lines_get_empty_blocks` に改名する。E0009 が1件であることは `declarations.rs::many_aligned_signature_lines_are_still_one_error` が確かめる |
| C2 | `eml_interp/src/lib.rs::runtime_errors_are_displayed_as_before` | `runtime_errors_name_the_fault_and_the_function` に改名する。「as before」は R3b の前の文字列の形を指していて、今は比べる相手がない。中身は変えない |
| B6 | `eml_diagnostics/src/render.rs::byte_order_mark_takes_no_column`、`byte_order_mark_does_not_shift_later_lines` | 削除。R1 から `SourceFiles::add` が BOM を除くので、表示は BOM を見ない |
| B6 | `eml_diagnostics/src/source.rs::line_col_does_not_count_the_bom` | `add_strips_only_a_leading_bom` に統合する。2つの `assert_eq!(position(...), ...)` を、BOM を除いた後の位置の確認として移し、元のテストを削除する |

## 4. 確かめ方

- 変更ごとに `INSTA_UPDATE=no cargo test` を流し、失敗するスナップショットが、この文書で変わると書いたものだけであることを確かめる。新しいスナップショットは、`.snap.new` か保留中のインラインスナップショットの中身が、この文書に書いた変化 (増える行、減る行、消える診断) と一致することを確かめてから承認する
- `crates/eml_cli/tests/snapshots/` のすべての `ui__*.snap` に、対応する .em があることを確かめる。名前は `ui__run@<名前>.em.snap`、`ui__run_fail@<名前>.em.snap`、`ui__check_fail@<名前>.em.snap` である
- `.snap.new` と `.pending-snap` が残っていないことを `find` で確かめる (`.gitignore` にあるため)
- `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` を通す

## 5. 文書

- `docs/implementation/test-changes.md` の「記録」の末尾に `### テスト本体の整理 3a` を足し、1〜3章の変更ごとに箇条を1つ書く。種類 (1)、変えた理由、変わらない目的 (どのテストが同じ事実を確かめ続けるか) を書く
- `docs/implementation/testing.md` の地図は、crate のテストファイルを削除しないので変えない。消すファイル名を地図や本文が指していないことを `grep` で確かめる
- `docs/implementation/status.md` は、`multi_over_once.em` など残すファイルしか指していないので変えない。消すファイル名を指していないことを `grep` で確かめる
