# eml の文書

eml を実装するエージェントとプログラマのための文書群である。言語の仕様、処理系の構成と現在地、将来の方針をまとめる。

## 文書の地図

| 文書 | 位置づけ | 内容 |
|---|---|---|
| [overview.md](overview.md) | 説明 | 目的、言語の性格、確定した設計判断の一覧、用語 |
| **spec/** | 規範 | 何を実装するか |
| [spec/lexical.md](spec/lexical.md) | 規範 | 字句 (識別子、キーワード、演算子、リテラル、文字列、コマンドリテラル) |
| [spec/layout.md](spec/layout.md) | 規範 | レイアウト規則 (仮想トークン `OPEN` / `SEP` / `CLOSE`) |
| [spec/grammar.md](spec/grammar.md) | 規範 | 文法と文法上の補足 |
| [spec/declarations.md](spec/declarations.md) | 規範 | シグネチャと等式、`data` と `type`、`effect`、`extern`、fixity と標準の演算子表、`pub` |
| [spec/expressions.md](spec/expressions.md) | 規範 | 式と脱糖 (`let`、ラムダ、`if`、`match`、演算子の列、セクション、`use`、handler の構文) |
| [spec/records.md](spec/records.md) | 規範 | 直積型とレコード (タプルと Unit を含む) |
| [spec/modules.md](spec/modules.md) | 規範 | モジュール、import、標準ライブラリ、名前の解決、名前空間、公開の範囲 |
| [spec/types.md](spec/types.md) | 規範 | Kind、関数型、型推論 |
| [spec/linearity.md](spec/linearity.md) | 規範 | 線形性と `drop`、線形性の検査パス |
| [spec/effects.md](spec/effects.md) | 規範 | 操作の多重度、持ち越し規則、handler の意味、extern のエフェクトの `IO`、標準ライブラリの `File` |
| [spec/exhaustiveness.md](spec/exhaustiveness.md) | 規範 | 網羅性の検査 |
| [spec/core-ir.md](spec/core-ir.md) | 規範 | Core IR の構成 (基本ブロックの列と構造の規則、値の表現、データの配置、位置の規則)、評価と所有権の意味、パス (box の挿入、縮約、Perceus) とその境界の不変条件、verifier、実行時エラー |
| [spec/runtime.md](spec/runtime.md) | 規範 | ヒープと参照カウント、オブジェクトのヘッダ、不死の物体、`eml_runtime` の API、handler の連鎖、文字列の連結、`debug_heap`、実行の API |
| [spec/diagnostics.md](spec/diagnostics.md) | 規範 | 診断のデータ構造、診断の順、番号の範囲と各番号の意味、連鎖する診断の抑止 |
| [spec/examples.md](spec/examples.md) | 説明 | まだ動かない、本番の構文で書いたプログラム例 |
| **implementation/** | 手引き | どう作るか、今どこまでできているか |
| [implementation/architecture.md](implementation/architecture.md) | 手引き | プログラム全体の構成と採らなかった形、crate の構成、各段階の規律、エラー回復、各 crate の内部、CLI と lib API、translate の組み立て、継続のフレーム、構文解析と名前解決の回復 |
| [implementation/testing.md](implementation/testing.md) | 手引き | テスト戦略、テストの変更の運用、テストの置き場所、UI テスト、Core IR のテキストの形、文書の引用の検査 |
| [implementation/status.md](implementation/status.md) | 手引き | 今の言語の範囲、既知の制限 |
| [implementation/benchmarks.md](implementation/benchmarks.md) | 手引き | 基準のプログラム、回数のテスト、命令の数の測り方、段ごとの記録 |
| [implementation/diagnostics.md](implementation/diagnostics.md) | 手引き | 番号ごとの診断が指す場所、help と fix の文言と付ける条件、型エラー・線形性・網羅性の診断の表示 |
| **future/** | 将来の設計 | まだ実装しない方針 |
| [future/roadmap.md](future/roadmap.md) | 将来の設計 | 再設計の段 (S4〜S13)、その後の言語の項目と処理系の項目 |
| [future/multicore.md](future/multicore.md) | 将来の設計 | マルチコア対応の設計 (共有の印方式の RC、`par`、並行処理、継続の移動) |
| [future/stdlib.md](future/stdlib.md) | 将来の設計 | 標準ライブラリ spec への申し送り |
| [future/evidence-passing.md](future/evidence-passing.md) | 将来の設計 | ネイティブ化でのエフェクトの実装 (generalized evidence passing、すぐに再開する節、多重度ごとの実装) |
| **reports/** | 調査 | 実装の方針を決めるための調査レポート。書いた時点のコミットに基づき、後から更新しない |
| [reports/eml/](reports/eml/) | 調査 | VM とネイティブコード生成 (2026-10-09)、REPL 向けのランタイム (2026-10-09) |

## 読む順

- 初めて読むときは、[overview.md](overview.md)、[implementation/status.md](implementation/status.md)、[implementation/architecture.md](implementation/architecture.md) の順に読む。その後、作業する段階に対応する spec を読む。本番の構文で書いたプログラムの例は [spec/examples.md](spec/examples.md) にある。
- 段階と spec の対応は次のとおり。

| 段階 (crate) | 主に読む spec |
|---|---|
| `eml_syntax` | lexical、layout、grammar |
| `eml_hir` | declarations、expressions、records、modules |
| `eml_types` | types、linearity、effects、exhaustiveness、records |
| `eml_core_ir` / `eml_interp` | core-ir、effects |
| `eml_runtime` | runtime |
| 診断を出すすべての段階 | diagnostics |

## 文書の扱い

- `spec/` が実装の規範である。仕様を変えるときは、まず `spec/` の該当文書を直す。
- `implementation/status.md` は実装の現在地を書く。実装が進んだら更新する。
- `future/` の内容はまだ実装しない。実装に進むときは、決まった部分を `spec/` に移す。今の実装に入れた予防的な決定 (row の Kind の束の `Never` など) は、すでに `spec/` の該当文書に反映してある。
- 今後の段と将来の論点の一覧は [future/roadmap.md](future/roadmap.md) を正とする。マルチコア、標準ライブラリ、エフェクトのネイティブな実装の詳細は、それぞれ [future/multicore.md](future/multicore.md)、[future/stdlib.md](future/stdlib.md)、[future/evidence-passing.md](future/evidence-passing.md) にある。
- 構文で迷ったときは Haskell の慣習に寄せる。
- 文書は日本語で書く。
