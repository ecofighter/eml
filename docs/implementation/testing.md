# テスト戦略

位置づけ: 手引き。

テストの書き方、テストの変更の運用、テストの置き場所、層ごとの方法、UI テストの仕組みを定める。テストの変更の記録は [test-changes.md](test-changes.md) にある。

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

## テストの置き場所

新しいテストの置き場所は、期待するものを上から順に見て、最初に当てはまる行で決める。

| # | 期待するもの | 置き場所 |
|---|---|---|
| 1 | 機能ごとの代表的なプログラムを `eml check` / `eml run` にかけたときにユーザーが見るもの。stdout、実行時エラー、プログラム全体が通るか落ちるか、代表的な診断の表示 | UI テスト (`tests/ui/`) |
| 2 | 1つの段階の出力 (CST、HIR、推論したシグネチャ、Core IR) と、その段階の診断の細部 (位置、回復、端のケース) | その段階の crate の結合テスト (`crates/<段階>/tests/`)。ソースから `eml_test_support` で組み立てる |
| 3 | フロントエンドからは作れない状態 (壊れた IR、`decref` の抜けた IR など) と、テストの中で生成した大きなソース | `eml_core_ir/tests/verify.rs` か `eml_interp/tests/`。Core IR は `eml_test_support::ir` で手書きする |
| 4 | 公開の API から届かないか、内部の状態を直接組まないと確かめにくい部品の振る舞い。レイアウト段、パーサのマーカー、単一化の表、Kind の制約の解消、ヒープなど | `src/` の単体テスト |
| 5 | lib API の流れ、CLI の終了コード、テスト補助そのもの | `eml_cli/tests/api.rs`、`eml_cli/tests/cli.rs`、`eml_test_support/tests/` |

### 重複させない

1つの事実は、上の表で選んだ1か所だけで確かめる。UI テストは機能ごとの代表的な筋書きを確かめ、段階の端のケースを繰り返さない。例えば診断なら、端のケースは段階の crate のテストに置き、代表的な表示を `check-fail/` に1つ置く。

### crate の中の置き方

- 結合テストは、話題ごとに1ファイルにする。ファイル名は spec の節か言語の機能から付ける (`effects.rs`、`operators.rs`)。1つのファイルが複数の話題にまたがったら、話題で分ける
- crate の結合テストが使う表示の関数は `tests/common/mod.rs` に置く。複数の crate で使う部品は `eml_test_support` に置く
- Core IR の結合テストは、確かめるパスごとのファイルに置き、`eml_test_support::core_until` でそのパスの直後の IR を見る。後のパスの書き換えや RC の命令を、確かめたいことと一緒に期待値に入れないためである
- 単体テストは、ファイルの末尾の `#[cfg(test)] mod tests` に置く。テストが300行を超え、ファイルの半分ほどを占めるようになったら、`eml_types/src/table/tests.rs` のように隣の `tests.rs` に分ける
- `crates/eml_test_support/` は、結合テストのためにパイプラインを組む関数 (`parse`、`lower`、`check`、`core`、`core_until`、`run`、`execute`) と、診断のないことを確かめて組む関数 (`parse_clean`、`lower_clean`)、診断を文字列にする関数 (`short`、`short_text`、`full`)、段階の表示に診断を足す関数 (`with_diagnostics`)、手書きの Core IR の部品 (`ir`) を持つ。開発専用の crate で、各 crate の `tests/` からだけ使う。段階は feature (`hir` < `types` < `core` < `run`) で選び、各 crate は自分の段階までを有効にする。下流の crate がまだ組み立たなくても、上流の段階のテストを流せるようにするためである。`src/` の `#[cfg(test)]` から使うと、テストする crate が2つ別々にリンクされて型が合わなくなる

### 今あるテストの地図

| crate | 結合テスト (`tests/`) | 単体テスト (`src/`) |
|---|---|---|
| `eml_diagnostics` | なし | `lib.rs` (診断の番号と E0004)、`source.rs` (`SourceFiles` と行と列)、`render.rs` (診断の表示) |
| `eml_syntax` | `lexer.rs` (字句)、`literals.rs` (リテラルの値の解釈)、`parser.rs` (空のファイル、項目の解析と項目の間の回復、BOM と shebang)、`declarations.rs` (シグネチャの形、`data`、`effect`、fixity、`pub` / `type` / `import`)、`types.rs` (型と row)、`expressions.rs` (等式、パターン、適用やフィールドの参照などの式、ブロックと `let`、ラムダ、`use`、括弧の回復)、`operators.rs` (演算子の列、前置の `-`、セクション、被演算子の欠け)、`control.rs` (`if` と `match`)、`handlers.rs` (handler)、`nesting.rs` (入れ子の深さの上限)、`ast.rs` (型付き AST ラッパ。`data`、`match`、コンストラクタのパターン、型の適用の取り出し口)、`corpus.rs` (コーパス。ソースは `tests/corpus/` にあり、`s1.em` は S1 の構文、`later_stages.em` は S2 以降の構文を含む) | `layout.rs` (レイアウト段)、`parser/tests.rs` (パーサのマーカー、先読み、診断の位置)、`grammar/scan.rs` (回復の範囲の走査)、`syntax_kind.rs` と `token_set.rs` (構文の種類の表) |
| `eml_hir` | `lower.rs` (名前解決と脱糖)、`operators.rs` (演算子の組み直し)、`effects.rs` (エフェクトと操作)、`structure.rs` (HIR のデータ構造と走査)、`data.rs` (`data` の宣言、コンストラクタ、型の適用、パターン、`match`)、`tuples.rs` (タプルの式・型・パターン、リテラルのパターン、射影の E0004) | `builtin.rs` (組み込みの表)、`lower/scope.rs` (名前空間) |
| `eml_types` | `check.rs` (推論と型の診断)、`rows.rs` (エフェクトの row と E2002)、`effects.rs` (エフェクト、handler、継続、線形な継続 (E3001))、`data.rs` (型構成子の引数、データ型の Kind、パターンと `match` の型検査)、`exhaustive.rs` (網羅性の検査と漏れの例。タプルとリテラルを含む)、`tuples.rs` (タプルの型検査、リテラルのパターン、`==` の比べ方の決定と E2006) | `table/tests.rs` (単一化と型の書き出し)、`kind.rs` (Kind の制約の解消)、`ty.rs` (型の表示)、`scc.rs` (関数の呼び出しの強連結成分)、`check/mod.rs` (診断の文言) |
| `eml_core_ir` | `translate.rs` (Core IR への変換。変換の直後)、`simplify.rs` (`simplify` の書き換え。`simplify` の直後)、`perceus.rs` (`dup` / `decref` の位置と `saved`。Perceus の直後)、`verify.rs` (手書きの Core IR による verifier) | なし |
| `eml_runtime` | なし | `heap/tests.rs` (確保と解放、世代番号、リーク、フレームと継続の解放、`take` と `take_or_copy` による複製)、`output.rs` (`OutputSink`) |
| `eml_interp` | `run.rs` (手書きの Core IR や生成したソースによる実行)、`closures.rs` (手書きの Core IR によるクロージャ)、`data.rs` (手書きの Core IR による `data` の値と `Switch` の分解) | `lib.rs` (実行時エラーの表示と `RunConfig`) |
| `eml_cli` | `ui.rs` (UI テスト)、`api.rs` (lib API の `check` / `compile` / `execute` の流れ)、`cli.rs` (CLI の終了コード) | なし |
| `eml_test_support` | `support.rs` (テスト補助そのもの) | なし |

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
| 線形性 | 線形な値 (`once` の操作の `k` など) を1回でなく使う不正なプログラム (E3001) と、正しく通るべきプログラムの対 |
| 全体 (UI テスト) | 下の「UI テスト」 |
| CLI | `eml run` / `eml check` の終了コードと引数の誤りを数件確認する |
| RC | すべての実行テストで `debug_heap` を有効にする。違反があればテストを失敗させる |

### 後の段階で足すテスト

- 線形性 (段階5b): `multi` の呼び出しをまたぐ持ち越しの規則について、不正なプログラムと正しく通るべきプログラムの対を足す。二重使用、消費漏れ、`_` での破棄、継続の扱い忘れの対は段階5aで足した

## UI テスト

- `tests/ui/run/**/*.em` は、診断のエラーなしで実行が正常に終了することを確認し、stdout と stderr をスナップショットにする
- `tests/ui/check-fail/**/*.em` は、診断のエラーが1件以上出ることを確認し、診断の表示をスナップショットにする
- `tests/ui/run-fail/**/*.em` は、診断のエラーなしでコンパイルでき、実行が実行時エラーで終わることを確認する。エラーまでの出力と実行時エラーのメッセージをスナップショットにする
- テストは `crates/eml_cli/tests/ui.rs` に置き、`insta::glob!` で `tests/ui/` 以下の `.em` を走査する。`eml_cli` の lib API をプロセス内で呼び、`debug_heap` を有効にした `RunConfig` で実行する
- 成功すべきか失敗すべきかは最上位のディレクトリで決める。そのため、スナップショットの承認を誤っても、成功と失敗の入れ替わりは検出できる
- スナップショットの中のパスは `tests/ui` からの相対パスにして、実行する環境に依存しないようにする

### 分類

`run/`、`check-fail/`、`run-fail/` の下に、分類のサブディレクトリを切る。成功すべきか失敗すべきかは、今までどおり最上位のディレクトリで決まる。`ui.rs` は最上位のディレクトリの直下の .em を拒み、テストを失敗させる。

- `run/` と `run-fail/` は言語の機能で分け、両方で同じ名前を使う
  - `basics/`: 値、演算子、`let`、`if`、短絡評価
  - `functions/`: クロージャ、高階関数、部分適用
  - `effects/`: エフェクト、`multi`、継続
  - `data/`: `data`、コンストラクタ、`match`、パターン
  - `runtime/`: 実装の性質を確かめるテスト。メモリの解放、スタックの深さ、join point、末尾呼び出し
- 実行テストは `.em` のあるディレクトリを `open` の基準ディレクトリにする。入力のファイルはテストの隣に置く。`*.em` だけがテストとして数えられる
- `check-fail/` は、主なエラーの番号の範囲 ([診断](../spec/diagnostics.md) の「番号の範囲」) で分ける。機能で分けると、エフェクトの誤りのように E1xxx と E2xxx にまたがるものの置き場所が決まらないためである
  - `syntax/` (E0xxx)、`names/` (E1xxx)、`types/` (E2xxx)、`linearity/` (E3xxx)。`exhaustiveness/` (E4xxx)
  - E3001 は今は `eml_types` が出すが、出す crate ではなく番号の範囲に従って `linearity/` に置く
  - E0004 (まだ対応していない構文) は、どの段階が出しても `not-yet-supported/` に置く
- サブディレクトリの名前は、親の `check-fail` と同じくケバブケースにする
- スナップショットの名前は、最上位のディレクトリからの相対パスで固定する (`run/basics/hello.em` は `ui__run@basics__hello.em.snap`)。insta の既定では、分類が1つしかないディレクトリの名前に分類が入らず、分類が増えたときに名前が変わるためである。テストのパスが名前になるので、UI テストの移動はスナップショットの名前を変え、種類1の変更になる

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
