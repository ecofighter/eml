# テスト戦略

位置づけ: 手引き。

テストの書き方、テストの変更の運用、層ごとの方法、UI テストの仕組みを定める。テストの変更の記録は [test-changes.md](test-changes.md) にある。

## 方針

- TDD で進める。テストを先に書き、実装をテストに合わせる
- 既存のテストは合意済みの仕様である。テストが失敗したら実装を直す。テストを変えるときは、下の「テストの変更の運用」に従う
- スナップショットは `insta` を使う。多くはインラインのスナップショット (`@"..."`) にする
- スナップショットの更新は `cargo insta review` で行う

## テストの変更の運用

テストの変更を3種類に分け、種類ごとに合意の取り方を決める。このプロジェクトで合意した運用で、「既存のテストを変えない」という原則の例外にあたる。

| 種類 | 何が変わるか | 合意と記録 |
|---|---|---|
| 1. 振る舞いの変更 | 言語として観測できる期待値。UI テストの出力、診断の番号と文言、成功か失敗か。テストの削除と移動もここに入れる | 事前に合意を取り、[test-changes.md](test-changes.md) に理由を書く |
| 2. 内部表現の変更 | 中間表現のダンプなど、内部の設計を写したスナップショットの期待値 | 作業の spec に、変わるテストと理由を列挙する。spec の承認を合意とみなし、[test-changes.md](test-changes.md) に書く |
| 3. 機械的な追随 | テストの組み立てだけが変わる。スナップショットの文字列と `assert` の値は1文字も変えない | 作業の計画で、この種類の変更を許すと宣言する。記録はコミットメッセージで足りる |

- テストを変えないことを理由に設計を曲げない。テストが壊れると分かったら、その変更が1〜3のどれに当たるかを示して、変更を提案する。過去には、テストを守るために別の enum の種類を足したり、spec に例外を足したりしたことがある ([status.md](status.md) の「リファクタリング」)
- 作業の計画の全体制約は「期待値は、このプランで名前を挙げたテストだけを変える。期待値を変えない機械的な追随は許す」と書く
- UI テストは最も強い仕様として扱い、種類1でしか変えない

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
- `crates/eml_test_support/`: 結合テストのためにパイプラインを組む関数 (`parse`、`lower`、`check`、`core`、`run`、`execute`) と、診断を文字列にする関数 (`short`、`full`)。開発専用の crate で、各 crate の `tests/` からだけ使う。段階は feature (`hir` < `types` < `core` < `run`) で選び、各 crate は自分の段階までを有効にする。下流の crate がまだ組み立たなくても、上流の段階のテストを流せるようにするためである。`src/` の `#[cfg(test)]` から使うと、テストする crate が2つ別々にリンクされて型が合わなくなる

## UI テスト

- `tests/ui/run/*.em` は、診断のエラーなしで実行が正常に終了することを確認し、stdout と stderr をスナップショットにする
- `tests/ui/check-fail/*.em` は、診断のエラーが1件以上出ることを確認し、診断の表示をスナップショットにする
- `tests/ui/run-fail/*.em` は、診断のエラーなしでコンパイルでき、実行が実行時エラーで終わることを確認する。エラーまでの出力と実行時エラーのメッセージをスナップショットにする
- テストは `crates/eml_cli/tests/ui.rs` に置き、`insta::glob!` で `tests/ui/` 以下の `.em` を走査する。`eml_cli` の lib API をプロセス内で呼び、`debug_heap` を有効にした `RunConfig` で実行する
- 成功すべきか失敗すべきかはディレクトリで決める。そのため、スナップショットの承認を誤っても、成功と失敗の入れ替わりは検出できる
- スナップショットの中のパスは `tests/ui` からの相対パスにして、実行する環境に依存しないようにする

## CLI のテスト

`crates/eml_cli/tests/cli.rs` は `eml` バイナリを起動し、終了コードが [コンパイラの構成](architecture.md) の「CLI と lib API」の定めに合うことを確認する。

## よく使うコマンド

```sh
cargo test                                           # すべてのテスト
cargo test -p eml_syntax --test parser empty_file    # 1つのテスト
cargo test -p eml_cli --test ui                      # UI テスト
cargo insta review                                   # スナップショットの承認
cargo clippy --all-targets && cargo fmt
```
