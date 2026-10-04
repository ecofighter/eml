# テスト本体の整理 3b: 大きいテストファイルの分割の設計

位置づけ: 作業用の設計文書。この作業を終えたら、この文書は削除する。移した理由は `docs/implementation/test-changes.md` に記録する。

## 目的と範囲

テストとテストの文書の整理の、サブプロジェクト3の2つ目のサイクルである (3a 削除と統合 → 3b 分割 → 3c UI テストの分類)。複数の話題にまたがるテストファイルを話題ごとに分け、ファイルの半分ほどを占める `src/` の単体テストを隣の `tests.rs` に移す。

判断の基準は、[testing.md](../../implementation/testing.md) の「テストの置き場所」の「crate の中の置き方」である。結合テストは話題ごとに1ファイルにし、名前は spec の節か言語の機能から付ける。

### ユーザーと合意済みの決定

- 次の5つを採る。S1 (`declarations.rs` から `types.rs` を分ける)、S2 (`expressions.rs` から `operators.rs` を分け、`control.rs` のラムダ、`let ... in`、`use` を `expressions.rs` に移す)、S3 (`check.rs` から `rows.rs` を分ける)、S4 (`eml_core_ir` の `lower.rs` から `simplify.rs` を分ける)、S5 (`eml_syntax/src/parser.rs` と `eml_runtime/src/heap.rs` の単体テストを隣の `tests.rs` に移す)
- `layout.rs` の単体テストは移さない
- テストの本体と名前は変えずに移す。移動は種類1の変更なので記録するが、期待値は変わらない

### 範囲の外

- `eml_core_ir/tests/verify.rs` (1つの話題で、局所的な補助関数を共有する)、`eml_syntax/tests/lexer.rs`、`ast.rs`、`handlers.rs`、`literals.rs`、`nesting.rs`、`eml_types/tests/effects.rs` (`linearity.rs` は段階5で線形性の検査パスを分けるときに切る)、`eml_hir` のテスト
- テストの中身の変更、名前の変更、重複の整理 (3a で済んだ)
- UI テストの移動 (3c)

## 1. 結合テストの分割

すべて `#[test]` の関数を本体ごと、文字を変えずに移す。移す先の順は、元のファイルでの順を保つ。

### S1: eml_syntax の declarations.rs

`crates/eml_syntax/tests/types.rs` を新しく作り、型と row の次の17件を移す。

`signature_with_function_type`、`qualified_type_names`、`effect_row_with_effects_and_tail`、`empty_row_is_split_from_one_operator_token`、`row_variable_alone`、`arrows_at_the_end_of_lines_continue_the_type`、`tuple_and_parenthesized_types`、`dot_with_spaces_in_a_qualified_name_is_an_error`、`virtual_tokens_are_described_without_articles`、`unclosed_row_is_an_error`、`row_with_something_other_than_an_effect_is_an_error`、`aligned_signature_lines_are_one_error`、`many_aligned_signature_lines_are_still_one_error`、`arrow_at_the_end_of_a_line_suggests_a_leading_arrow`、`arrows_ending_consecutive_top_level_signatures_are_each_an_error`、`arrows_ending_consecutive_operation_signatures_are_each_an_error`、`row_written_right_after_the_arrow`

項目の解析の次の3件を `crates/eml_syntax/tests/parser.rs` の末尾に移す。`parser.rs` は項目の間の回復を扱う。

`reserved_keywords_are_errors`、`lone_lowercase_name_is_not_an_item`、`stray_unterminated_string_reports_both_problems`

`declarations.rs` には、シグネチャの形、`data`、`effect`、fixity、`pub` / `type` / `import` の項目のテストが残る。

### S2: eml_syntax の expressions.rs と control.rs

`crates/eml_syntax/tests/operators.rs` を新しく作り、演算子の列、前置の `-`、セクション、被演算子の欠けの次の9件を移す。

`operator_sequence_is_flat_with_prefix_minus`、`minus_after_a_function_is_subtraction`、`sections`、`sections_with_operator_sequences`、`section_ending_with_an_operator_is_one_error`、`section_of_two_operators_is_one_error`、`missing_operand_does_not_affect_the_next_item`、`missing_operand_at_the_end_of_the_file_points_after_the_operator`、`missing_operand_before_a_comment_points_after_the_operator`

`control.rs` の次の6件を `expressions.rs` の末尾に移す。`control.rs` には `if` と `match` のテストだけが残り、testing.md の地図の説明 (`if` と `match`) と合う。

`trailing_lambda_with_a_block_body`、`lambda_parameters`、`lambda_as_an_operand`、`lambda_needs_a_parameter`、`let_in_inside_parentheses`、`use_statements`

### S3: eml_types の check.rs

`crates/eml_types/tests/rows.rs` を新しく作り、エフェクトの row (E2002、row 変数、未定義のエフェクト) の次の11件を移す。

`effects_must_be_in_the_signature`、`main_with_an_erroneous_row_is_not_reported_again`、`a_call_reports_a_missing_effect_once`、`a_value_cannot_perform_effects`、`row_variables_pass_effects_through`、`a_rigid_row_variable_must_be_in_the_ambient_row`、`a_lambda_cannot_perform_effects_its_expected_type_does_not_allow`、`an_undefined_effect_row_is_fresh_at_each_call`、`an_undefined_effect_row_does_not_pass_the_body_effects_to_callers`、`a_missing_effect_points_at_the_arrow_of_the_body`、`an_undefined_effect_does_not_hide_an_unrelated_missing_effect`

`check.rs` には、推論の中心 (関数、適用、ラムダ、多相、注釈、`main`、組み込み、Kind) が残る。

### S4: eml_core_ir の lower.rs

`crates/eml_core_ir/tests/simplify.rs` を新しく作り、`simplify` の次の6件を移す。

`a_join_point_that_returns_its_value_is_forwarded_and_removed`、`a_join_point_that_passes_its_value_on_is_forwarded`、`known_tags_jump_straight_to_their_arm`、`an_arm_reached_twice_stays_a_join_point`、`split_arms_use_the_known_tag`、`join_points_left_without_jumps_are_removed`

### 新しいファイルの先頭

新しいファイルは、元のファイルと同じく `mod common;` と `use common::{...};` で始め、使う関数だけを `use` に挙げる。元のファイルからも、使わなくなった名前を `use` から除く。新しいファイルには、何の話題のテストかを1行で書いた `//!` のコメントを付ける (例: `//! 型と row の構文。`)。

## 2. 単体テストの移動 (S5)

- `crates/eml_syntax/src/parser.rs` の `#[cfg(test)] mod tests { ... }` の中身を `crates/eml_syntax/src/parser/tests.rs` に移し、`parser.rs` には `#[cfg(test)] mod tests;` だけを残す
- `crates/eml_runtime/src/heap.rs` についても同じことをして、`crates/eml_runtime/src/heap/tests.rs` に移す
- 中身は字下げを1段 (4文字) 浅くするほかは変えない。`use super::*;` などの `use` はそのまま `tests.rs` の先頭に来る
- edition 2024 では、`parser.rs` と `parser/tests.rs` を並べて置ける。`mod.rs` に変える必要はない

## 3. 確かめ方

純粋な移動であることを、作業の前 (`main`) と後を比べるスクリプトで確かめる。

- crate ごとに、`tests/*.rs` の `#[test]` の関数の名前の集合が前後で同じである
- 各テスト関数の本体の文字列 (`#[test]` の行から、その関数の閉じ括弧の行まで) が、前後で1文字も変わらない
- S5 は、前の `mod tests { ... }` の中身の各行から先頭の4文字の空白を除いたものと、後の `tests.rs` の内容が一致する
- `INSTA_UPDATE=no cargo test` が通り、スナップショットが変わらない。`.snap.new` と `.pending-snap` ができない
- `cargo clippy --all-targets` (使わない `use` の警告を含む) と `cargo fmt --check` が通る

## 4. 文書

- `docs/implementation/testing.md` の「今あるテストの地図」を直す
  - `eml_syntax` の結合テストに `types.rs` (型と row) と `operators.rs` (演算子の列、セクション、被演算子の欠け) を足す。`parser.rs` の説明に項目の解析 (予約語、項目にならない名前) を、`expressions.rs` の説明にラムダ、`let ... in`、`use` を足し、`declarations.rs` の説明から型と row を除く
  - `eml_syntax` の単体テストの `parser.rs` を `parser/tests.rs` にする
  - `eml_types` の結合テストに `rows.rs` (エフェクトの row と E2002) を足す
  - `eml_core_ir` の結合テストに `simplify.rs` (`simplify` の書き換え) を足し、`lower.rs` の説明は変えない
  - `eml_runtime` の単体テストの `heap.rs` を `heap/tests.rs` にする
- `testing.md` の「crate の中の置き方」の「大きくなったら、`eml_types/src/table/tests.rs` のように隣の `tests.rs` に分ける」を、「テストがファイルの半分ほどを占めるようになったら、`eml_types/src/table/tests.rs` のように隣の `tests.rs` に分ける」にする
- `docs/implementation/test-changes.md` の「記録」の末尾に `### テスト本体の整理 3b` を足し、S1〜S5 の移動を種類1として書く。本体と名前と期待値は変えていないことと、移した理由 (話題ごとに1ファイル) を書く
- 移したテストのファイルの場所を指す文書とコードのコメントを `grep` で探し、今の場所を指すように直す。`test-changes.md` のこれまでの項目は履歴なので直さない
