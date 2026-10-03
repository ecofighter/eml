# テスト戦略

位置づけ: 手引き。

テストの書き方、層ごとの方法、UI テストの仕組み、テストの変更に関する合意済みの例外を定める。マルチコア対応の段階でのテスト方針も扱う。

## 方針

- TDD で進める。テストを先に書き、実装をテストに合わせる
- 既存のテストは合意済みの仕様である。テストが失敗したら実装を直す。例外は、下の「テストの変更に関する合意済みの例外」だけである
- スナップショットは `insta` を使う。多くはインラインのスナップショット (`@"..."`) にする
- スナップショットの更新は `cargo insta review` で行う

## 層ごとの方法

| 層 | 方法 |
|---|---|
| 字句 | トークン列のスナップショット |
| パーサ | `insta` で CST をダンプしたスナップショット。壊れた入力から回復できるかのケースを多めに用意する |
| 名前解決と脱糖 | HIR の pretty printer で変換結果 (演算子の組み直しと脱糖を含む) をダンプしたスナップショット |
| 型推論 | 推論したシグネチャ (Kind と row を含む) のスナップショット |
| Core IR | Core IR の pretty printer で、`dup` / `decref` の位置を含めてダンプしたスナップショット |
| ランタイム | ヒープの単体テスト (確保と解放、世代番号による解放済みアクセスの検出、リークの数え方、長い連鎖の解放) |
| 診断 | 表示した診断テキストのスナップショット |
| 線形性 | 不正なプログラム (二重使用、消費漏れ、`_` での破棄、`multi` をまたぐ、継続の扱い忘れ) と、正しく通るべきプログラムの対 |
| 網羅性 | 網羅されていない `match`、到達しない枝、反駁可能な `let` |
| 全体 (UI テスト) | 下の「UI テスト」 |
| CLI | `eml run` / `eml check` の終了コードと引数の誤りを数件確認する |
| RC | すべての実行テストで `debug_heap` を有効にする。違反があればテストを失敗させる |

現在あるテストの置き場所は次のとおり。

- `crates/eml_syntax/tests/`: 字句 (`lexer.rs`)、リテラルの値の解釈 (`literals.rs`)、パーサ (`parser.rs`、`declarations.rs`、`expressions.rs`、`control.rs`、`handlers.rs`)、入れ子の深さの上限 (`nesting.rs`)、型付き AST ラッパ (`ast.rs`)、コーパス (`corpus.rs`)。コーパスのソースは `crates/eml_syntax/tests/corpus/` にあり、`s1.em` は S1 の構文、`later_stages.em` は S2 以降の構文を含む
- `crates/eml_hir/tests/`、`crates/eml_types/tests/`、`crates/eml_core_ir/tests/`、`crates/eml_interp/tests/`: 各段階の変換結果と診断のスナップショット、実行の結果。ランタイムのヒープの単体テストは `crates/eml_runtime/src/heap.rs` にある
- `crates/eml_cli/tests/ui.rs`: UI テスト
- `crates/eml_cli/tests/api.rs`: lib API (`check` / `compile` / `execute`) の流れ
- `crates/eml_cli/tests/cli.rs`: CLI の終了コード

## UI テスト

- `tests/ui/run/*.em` は、診断のエラーなしで実行が正常に終了することを確認し、stdout と stderr をスナップショットにする
- `tests/ui/check-fail/*.em` は、診断のエラーが1件以上出ることを確認し、診断の表示をスナップショットにする
- `tests/ui/run-fail/*.em` は、診断のエラーなしでコンパイルでき、実行が実行時エラーで終わることを確認する。エラーまでの出力と実行時エラーのメッセージをスナップショットにする
- テストは `crates/eml_cli/tests/ui.rs` に置き、`insta::glob!` で `tests/ui/` 以下の `.em` を走査する。`eml_cli` の lib API をプロセス内で呼び、`debug_heap` を有効にした `RunConfig` で実行する
- 成功すべきか失敗すべきかはディレクトリで決める。そのため、スナップショットの承認を誤っても、成功と失敗の入れ替わりは検出できる
- スナップショットの中のパスは `tests/ui` からの相対パスにして、実行する環境に依存しないようにする

## CLI のテスト

`crates/eml_cli/tests/cli.rs` は `eml` バイナリを起動し、終了コードが [コンパイラの構成](architecture.md) の「CLI と lib API」の定めに合うことを確認する。

## テストの変更に関する合意済みの例外

- 暫定構文で書いたテストのソースは、構文の段階 S1 で本番の構文に書き直した。構文の差し替えに伴う機械的な書き換えとして、事前に合意した例外である。テストの期待値 (意味) は変えていない
- `tests/ui/run/empty.em`、`comments_only.em` と、`crates/eml_cli/tests/api.rs` の `compile_returns_a_program_without_errors`、`execute_runs_a_compiled_program` は、`main` を持たない「空のプログラムが実行できる」ことを前提にしていた。`main` を入口とする spec と合わないので、縦の貫通の段階1で `main : Unit -> <IO> Unit` と `main () = ()` を足した。コメントを読み飛ばすことと lib API の流れを確かめる目的は変わらない
- 本番の構文では `$` と `@` が演算子の文字になる。そのため、「認識できない文字」のテスト (`unexpected_character.em`、`multiple_errors.em`) は、本番の構文でも認識できない文字 `€` に置き換えた
- 縦の貫通の段階2で、段階1が E0004 を期待していた関数値、ラムダ、型変数、row 変数、`>>` / `<<` を通すようになった。そのため、次のテストの期待値を新しい結果に合わせた。後の段階の構文を E0004 にすることは、段階2でも E0004 のまま残る構文 (`data`、`match`、タプルのパターン、`(+)` など) で確かめ続ける
  - `eml_hir`: `lower.rs` の `constructs_of_later_stages_are_not_yet_supported` (ラムダ)、`rows_and_types_of_later_stages` を `row_variables_type_variables_and_unknown_effects` に改名、`operators.rs` の `mixed_associativity_is_rejected_in_both_orders` (`>>`)
  - `eml_types`: `check.rs` の `function_values_are_not_yet_supported` を `function_values_and_partial_application` に、`function_typed_parameters_cannot_be_called_or_passed` を `function_typed_parameters_can_be_called_and_passed` に改名
  - UI テスト: `check-fail/not_yet_supported.em` のスナップショットから関数値とラムダの E0004 が消えた。`check-fail/function_value_parameter.em` はエラーにならなくなったので削除し、同じ内容を `run/higher_order.em` で実行して確かめる
- `eml_types::Type::Fn` に row の末尾 (`tail`) を足したので、`ty.rs` の単体テスト `function_types_are_displayed_like_the_surface_syntax` の型の組み立てに `tail: None` を足した。構造体へのフィールドの追加に伴う機械的な書き換えで、期待値は変えていない。一方、`check.rs` の `main_with_an_erroneous_row_is_not_reported_again` の期待値のシグネチャが `Unit -> <_> Unit` になった。`export` が開いた row を残すようになったためで、機械的な書き換えではない。未定義のエフェクトの row は、どのエフェクトも受け入れる開いた row として残る。E2004 を重ねて出さないことを確かめる目的は変わらない。

## よく使うコマンド

```sh
cargo test                                           # すべてのテスト
cargo test -p eml_syntax --test parser empty_file    # 1つのテスト
cargo test -p eml_cli --test ui                      # UI テスト
cargo insta review                                   # スナップショットの承認
cargo clippy --all-targets && cargo fmt
```

## マルチコア対応の段階のテスト

マルチコア対応は将来の設計であり、マイルストーン1 では実装しない ([マルチコア対応の設計](../future/multicore.md))。その段階では次の方針でテストする。

- 本物のマルチコアで動かすと、並行処理の出力の順序が変わり、UI テストのスナップショットが安定しない。UI テストは `threads = 1` か、シードを固定した決定的なシミュレーションで実行する
- `debug_heap` を拡張し、共有の印のないオブジェクトを所有者でないスレッドから触ったら検出する。印の付け忘れを見つけるためで、リーク検出や解放済みアクセスの検出と同じ位置づけにする。所有者は、`debug_heap` が有効なときだけ別のテーブルに記録する
- ランタイムの `unsafe` な部分 (あれば) は、Miri と loom (並行処理のモデル検査) でテストする
