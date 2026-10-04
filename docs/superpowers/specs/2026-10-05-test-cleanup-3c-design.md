# テスト本体の整理 3c: UI テストの分類の設計

位置づけ: 作業用の設計文書。この作業を終えたら、この文書は削除する。移した理由は `docs/implementation/test-changes.md` に記録する。

## 目的と範囲

テストとテストの文書の整理の、サブプロジェクト3の最後のサイクルである (3a 削除と統合 → 3b 分割 → 3c UI テストの分類)。UI テストを [testing.md](../../implementation/testing.md) の「UI テスト」の「分類」に従ってサブディレクトリに移し、`crates/eml_cli/tests/ui.rs` がサブディレクトリを走査するようにする。これでテストの整理は終わる。

### ユーザーと合意済みの決定

- スナップショットの名前は、`ui.rs` で明示して固定する。最上位のディレクトリからの相対パスを接尾辞にし、`ui__run@basics__hello.em.snap` の形にする。insta の既定 (すべての一致に共通の接頭辞を除く) では、分類が1つしかない `run-fail/` の名前に分類が入らず、分類が増えたときに名前が切り替わるためである
- `effect_loop.em` と `multi_loop.em` は `runtime/` に置く。確かめるのは実装の性質 (継続が伸びないこと、複製がリークしないこと) で、`tail_calls.em` のエフェクト版に当たる
- 複数の範囲の番号を出す `check-fail/` の3つは、主な番号で置き、中身は変えない。`missing_indented_block.em` (主は E0009。シグネチャがないため E1004 も出る) と `tab_indentation.em` (主は E0006。同じく E1004 も出る) は `syntax/`、`effect_arguments.em` (主は E1015。E2001 も1件出る) は `names/` に置く
- `check-fail/not_yet_supported.em` は、`not-yet-supported/not_yet_supported.em` と名前が重なるので、`not-yet-supported/data_declarations.em` に改名する

### 範囲の外

- .em の中身の変更 (改名のほかは変えない)
- 3a と 3b で済んだ削除、統合、分割

## 1. 分類の表

`run/` (29件)

| 分類 | ファイル |
|---|---|
| `basics/` | `comments_only.em`、`hello.em`、`operators.em`、`short_circuit_conditions.em`、`pipe_evaluation_order.em`、`fibonacci.em`、`factorial.em` |
| `functions/` | `closures.em`、`higher_order.em`、`partial_application.em`、`point_free_main.em` |
| `effects/` | `continuation_values.em`、`effect_abort.em`、`effect_deep.em`、`effect_drop_k.em`、`effect_parameters.em`、`effect_resume.em`、`operation_values.em`、`multi_captured.em`、`multi_choice.em`、`multi_resume.em`、`multi_over_once.em` |
| `runtime/` | `strings_freed_in_branches.em`、`saved_across_calls.em`、`deep_recursion.em`、`join_points.em`、`tail_calls.em`、`effect_loop.em`、`multi_loop.em` |

`run-fail/` (2件)

| 分類 | ファイル |
|---|---|
| `basics/` | `division_by_zero.em`、`integer_overflow.em` |

`check-fail/` (22件)

| 分類 | ファイル |
|---|---|
| `syntax/` | `multiple_errors.em`、`missing_indented_block.em`、`tab_indentation.em` |
| `names/` | `undefined_name.em`、`unknown_type_variable.em`、`duplicate_definition.em`、`missing_signature.em`、`missing_equation.em`、`non_associative.em`、`operation_declarations.em`、`handle_io.em`、`handler_clauses.em`、`resume_and_drop_arity.em`、`effect_arguments.em` |
| `types/` | `type_mismatch.em`、`missing_io_row.em`、`pipe_into_function_parameter.em`、`invalid_main_type.em`、`infinite_row_through_effect_argument.em` |
| `linearity/` | `continuation_misuse.em`、`once_continuation_through_effect_argument.em` |
| `not-yet-supported/` | `data_declarations.em` (元の `not_yet_supported.em`) |

## 2. ui.rs

- 3つの `insta::glob!` のパターンを `run/**/*.em`、`run-fail/**/*.em`、`check-fail/**/*.em` にする
- 各クロージャの中で、最上位のディレクトリ (`run/` など) からの相対パス (`basics/hello.em`) をスナップショットの接尾辞に設定してから `assert_snapshot!` を呼ぶ。insta はアサーションの時点の設定から名前を作るので、`glob!` が設定した接尾辞を上書きできる。ファイル名は `/` が `__` になり、`ui__run@basics__hello.em.snap` になる
- 最上位のディレクトリの直下に .em があれば、テストを失敗させる。メッセージで分類のサブディレクトリに置くよう伝える。testing.md の分類の規則を、テスト自身で確かめるためである
- 先頭の `//!` のコメントに、分類のサブディレクトリと名前の固定の理由を足す

## 3. スナップショット

- 古い `.snap` を削除し、`cargo insta test --accept` で新しい名前の `.snap` を作る
- 新旧のスナップショットをファイルごとに比べるスクリプトで、違いが次の3種類だけであることを確かめる
  - ヘッダの `input_file:` の行 (新しいパスになる)
  - `check-fail/` の診断の表示の `╭─[ check-fail/... ]` の行のパス (サブディレクトリと、`data_declarations.em` の改名が入る)
  - `run-fail/` のヘッダの `expression:` の行 (古いスナップショットに `{message}` が残っていて、`ui.rs` の今の式 `{error}` に合わせて書き直される)
- stdout、診断の文言、行と列、実行時エラーのメッセージは1文字も変わらない
- 古い名前のスナップショットが残らず、すべての `.snap` に対応する .em があることを確かめる

## 4. 参照の更新

- `crates/eml_cli/tests/cli.rs` のパスを直す。`run/comments_only.em` → `run/basics/comments_only.em`、`check-fail/multiple_errors.em` → `check-fail/syntax/multiple_errors.em`、`run-fail/division_by_zero.em` → `run-fail/basics/division_by_zero.em`、`run/hello.em` → `run/basics/hello.em`
- `docs/implementation/testing.md` の「UI テスト」
  - `tests/ui/run/*.em` などの説明を、分類のサブディレクトリの下の .em (`tests/ui/run/**/*.em` など) の説明にする
  - 「分類」の「ただし、今の `ui.rs` は各ディレクトリの直下の `.em` だけを走査し、…新しい UI テストも … 直下に置く」の文を除き、`ui.rs` が直下の .em を拒むことと、スナップショットの名前を最上位のディレクトリからの相対パスで固定することを書く
- `CLAUDE.md` の Testing の節の UI テストの説明を、`tests/ui/run/**/*.em` などと、分類のサブディレクトリの形にする
- `docs/implementation/status.md`
  - `multi_over_once.em` のパスを `tests/ui/run/effects/multi_over_once.em` にし、段階5で移す先を `check-fail/linearity/` にする
  - 「次の作業の注意点」の「テストの整理の残り」の項目を除く。テストの整理はこのサイクルで終わる
- `docs/implementation/test-changes.md` の「記録」の末尾に `### テスト本体の整理 3c` を足し、UI テストの移動とスナップショットの改名、`not_yet_supported.em` の改名を種類1として書く。スナップショットの中身は、パスとヘッダのほかは変わっていないことを書く
- ほかに UI テストの個別のパスを指す記述を `grep` で探して直す。`test-changes.md` のこれまでの項目と、`docs/superpowers/` の完了した作業の文書は履歴なので直さない

## 5. 確かめ方

- `INSTA_UPDATE=no cargo test` が通る。`.snap.new` と `.pending-snap` が残らない
- 3章のスナップショットの比較のスクリプトが、許した3種類のほかの違いを出さない
- `tests/ui/` の最上位のディレクトリの直下に .em がない (ui.rs のチェックも通る)
- 試しに `tests/ui/run/` の直下に .em を1つ置くと `ui.rs` の `run` が失敗し、消すと通る (チェックが働くことの確認。コミットしない)
- `cargo test -p eml_cli --test cli` が通る
- `cargo clippy --all-targets` と `cargo fmt --check` が通る
