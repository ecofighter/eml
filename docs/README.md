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
| [spec/declarations.md](spec/declarations.md) | 規範 | シグネチャと等式、`data` と `type`、`effect`、`extern`、fixity と標準の演算子表、クラスと instance と `deriving`、Prelude のクラス、`pub` |
| [spec/expressions.md](spec/expressions.md) | 規範 | 式と脱糖 (`let`、ラムダ、`if`、`match`、演算子の列、セクション、`use`、handler の構文)、ローカル関数、リスト、レコード、補間、コマンドリテラル、`var`、並行処理、型の明示 |
| [spec/records.md](spec/records.md) | 規範 | 直積型とレコード (タプルと Unit を含む) |
| [spec/modules.md](spec/modules.md) | 規範 | モジュール、import、標準ライブラリ、名前の解決、名前空間、instance の一貫性、公開の範囲、REPL のセッション |
| [spec/repl.md](spec/repl.md) | 規範 | REPL の端末: `eml repl` のコマンド、行のまとめ方、結果の表示、割り込み |
| [spec/stdlib.md](spec/stdlib.md) | 規範 | 標準ライブラリ: スクリプトの起動、extern の境界、`List`、`String`、`Env`、ハンドル、`Stdin`、`Stdout` と `Stderr`、`FileIn`、`FileOut`、`Fs`、`Proc`、エフェクトのモジュール (`Yield`、`Fail`、`State`、`Iter`)、実行時エラー、`Prelude`、`try_io`、`try_panic`、`exit`、`for` と `while`、`Std.Float`、`Std.Char`、`Std.Byte`、`Bytes`、`Std.Array`、`Std.MutArray`、`Std.Map`、`Std.Set`、`Std.Par`、`Std.Async` (取り消しと時間を含む)、`Std.Chan`、`Std.Atomic` |
| [spec/types.md](spec/types.md) | 規範 | Kind、関数型、型推論、型クラスの制約、数の型、フィールドの解決、`var` の変数、REPL の入力 |
| [spec/linearity.md](spec/linearity.md) | 規範 | 線形性と `drop`、線形性の検査パス、暗黙の後始末、レコードとタプル、配列の変数 |
| [spec/effects.md](spec/effects.md) | 規範 | 操作の多重度、持ち越し規則、handler の意味、extern のエフェクトの `IO`、標準ライブラリの `FileIn`、組み込みの `Panic`、抽象エフェクト、`var` のエフェクト、健全性 |
| [spec/exhaustiveness.md](spec/exhaustiveness.md) | 規範 | 網羅性の検査 |
| [spec/core-ir.md](spec/core-ir.md) | 規範 | Core IR の命令、型の Repr、translate の配置、変換の規則 (単相化と row のビット、`yields` の印、メソッドの解決、導出の生成器)、パス (translate と evidence passing)、`par` の子の中断、LIR への変換、Core IR の verifier |
| [spec/lir.md](spec/lir.md) | 規範 | LIR の命令、2つの IR に共通する規則 (ブロックの列と構造の規則、値の表現、データの配置、位置の規則、末尾の位置の呼び出し)、基本操作と実行の文脈、LIR のパス (box の挿入、開くパス、縮約、Perceus、reuse) とその境界の不変条件、LIR の verifier、実行の意味、実行時エラー |
| [spec/object-model.md](spec/object-model.md) | 規範 | `eml_rt` の核: 語、ヘッダと種類、物体の形、ページの確保器と生存の列挙、局所と共有の RC、`share`、`transfer_back`、`lin` ビット、核の操作、バイト列と文字列、凍結した領域、reuse の部品、検査モード (論理的な worker ID を含む)、実行文脈 `Ctx` (タスクの切り替えを含む)、extern (語の ABI、誤り、Host と出力のバッファ、読み手と `FileIn`、仕事の回数)、受け入れる制約、配列、ダミーの値、`unsafe` の核 |
| [spec/vm.md](spec/vm.md) | 規範 | VM の置き場所と実行の API、`CodeBase` と `FnEntry`、スロットとフレーム、命令の形、スロットの割り当て、呼び出しと戻り、命令ごとの実行、誤りと終わりの処理、上限と割り込み、タスクとスケジュール、時計と driver、`par` の fork と join、`Async` のタスク、channel、`RunStats`、並列化への備え、巻き戻し、extern の中断 |
| [spec/diagnostics.md](spec/diagnostics.md) | 規範 | 診断のデータ構造、診断の順、番号の範囲と各番号の意味、連鎖する診断の抑止、他の言語の書き方へのヒント |
| [spec/examples.md](spec/examples.md) | 説明 | 本番の構文で書いたプログラム例。実装していない構文を含む |
| **implementation/** | 手引き | どう作るか、今どこまでできているか |
| [implementation/architecture.md](implementation/architecture.md) | 手引き | プログラム全体の構成と採らなかった形、crate の構成、各段階の規律、エラー回復、各 crate の内部、CLI と lib API、translate の組み立て、継続と evidence、構文解析と名前解決の回復、リポジトリ、ソースファイルと位置、外部 crate |
| [implementation/testing.md](implementation/testing.md) | 手引き | テスト戦略、テストの変更の運用、テストの置き場所、`eml_rt` の検証 (Verus、単体テスト、期待値の表、Miri)、`eml_vm` のテスト、UI テスト (2つのモードで流すこと、seed で流すこと)、LIR と Core IR のテキストの形、文書の引用の検査、REPL のトランスクリプト、CLI のテスト、OS ごとのテスト、性能のテスト、feature の組み合わせ、CI、よく使うコマンド |
| [implementation/status.md](implementation/status.md) | 手引き | 今の言語の範囲 (実装したもの、まだないもの)、既知の制限 |
| [implementation/benchmarks.md](implementation/benchmarks.md) | 手引き | 基準のプログラム、回数のテスト、測り方、今の数値、所見 |
| [implementation/diagnostics.md](implementation/diagnostics.md) | 手引き | 番号ごとの診断が指す場所、help と fix の文言と付ける条件、型エラー・線形性・網羅性の診断の表示 |
| **future/** | 将来の設計 | まだ実装しない方針 |
| [future/roadmap.md](future/roadmap.md) | 将来の設計 | 段の進め方と閉じる手順、段の列、その後の項目 |
| [future/async.md](future/async.md) | 将来の設計 | 非同期の設計 (`Async` のタスクとスコープ、取り消し、時間、チャネル、I/O の driver、ネットワークとシグナル) |
| [future/native.md](future/native.md) | 将来の設計 | ネイティブ化の設計 (Cranelift のコード生成、呼び出し規約、中断と実行時エラー、安全点、生の要素の配列、AOT と配布) |
| [future/ffi.md](future/ffi.md) | 将来の設計 | FFI と拡張の設計 (`extern "C"` の宣言、型の対応、長く走る呼び出し、拡張、GUI) |
| [future/multicore.md](future/multicore.md) | 将来の設計 | マルチコア対応の設計 (スレッドと worker、スケジューラ、共有の印方式の RC、`par`、継続の移動、止める手順) |
| [future/stdlib.md](future/stdlib.md) | 将来の設計 | 標準ライブラリ spec への申し送り |
| [future/verification.md](future/verification.md) | 将来の設計 | Verus で検証する範囲 (Kind 推論の解く部分を証明する方針、型検査と evidence passing を扱わない理由) |
| **plans/** | 作業中 | 段の設計と計画。段の最初のコミットで入れ、段を閉じるときに消す。コードと文書から引かない |
| **reports/** | 調査 | 実装の方針を決めるための調査レポート。書いた時点のコミットに基づき、後から更新しない |
| [reports/eml/](reports/eml/) | 調査 | VM のマルチコア対応 |

## 読む順

- 初めて読むときは、[overview.md](overview.md)、[implementation/status.md](implementation/status.md)、[implementation/architecture.md](implementation/architecture.md) の順に読む。その後、作業する段階に対応する spec を読む。本番の構文で書いたプログラムの例は [spec/examples.md](spec/examples.md) にある。
- 段階と spec の対応は次のとおり。

| 段階 (crate) | 主に読む spec |
|---|---|
| `eml_syntax` | lexical、layout、grammar |
| `eml_hir` | declarations、expressions、records、modules |
| `eml_types` | types、linearity、effects、exhaustiveness、records |
| `eml_core_ir` | core-ir、lir、effects |
| `eml_lir` | lir |
| `eml_vm` | vm、lir、object-model |
| `eml_rt` | object-model |
| 診断を出すすべての段階 | diagnostics |

## 文書の扱い

- `spec/` が実装の規範である。仕様を変えるときは、`spec/` の該当文書を、規則を実装するコミットと同じコミットで直す。どのコミットでも、`spec/` が書く規則はそのコミットのコードが満たす。コードのコメントは `spec/` の節を引く。
- `docs/` の文書は、今の状態を書く。段の名前と時期、前の状態との対比、決定の日付は書かない。経緯は git の履歴に任せる。例外は `reports/` で、書いた時点のまま残す。コミットのハッシュを書くのは `implementation/benchmarks.md` だけである。
- 規則の理由と、採らなかった案は、その規則を定める文書の隣に書く。時期は書かず、「〜にすると〜になるため採らない」のように今の形の理由として書く。
- 制限の置き場所は、診断で検査するか意味を定めた制限なら `spec/`、まだ作っていないことと遅いことなら `implementation/status.md` である。未対応の構文は、どの段階が E0004 を出すかを `spec/` に書き、`status.md` の「まだないもの」は機能を1行で挙げて `future/roadmap.md` の項目を引く。`status.md` は `spec/` の制限を1行で挙げてリンクする。
- 計測の数値は `implementation/benchmarks.md` だけが持つ。ほかの文書は定性的に書き、根拠が要る所で `benchmarks.md` の「所見」を引く。
- `implementation/status.md` と `implementation/architecture.md` と `benchmarks.md` の表は、段の途中では直さず、段を閉じるときに直す ([ロードマップ](future/roadmap.md) の「進め方」)。
- `future/` の内容はまだ実装しない。実装に進むときは、決まった部分を `spec/` に移す。今後の項目の一覧は [future/roadmap.md](future/roadmap.md) を正とする。非同期、ネイティブ化、FFI、マルチコア、標準ライブラリの詳細は、`future/` のそれぞれの設計文書にある。
- 段の設計と計画は `docs/plans/` に置き、段の最初のコミットで入れ、段を閉じるときに消す。コードと `docs/` から `plans/` と `reports/` の見出しを引かない。
- 見出しは題名 (規則、概念、計測の題) にし、段の名前と日付を付けない。コードと文書が引くのは題名の見出しで、見出しを引く文はその見出しがあるコミットに入れ、見出しを消すか改名するときは同じコミットで引く側を直す ([テスト戦略](implementation/testing.md) の「文書の引用の検査」)。
- 構文で迷ったときは Haskell の慣習に寄せる。
- 文書は日本語で書く。
