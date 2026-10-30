# eml の概要

位置づけ: 説明。

eml の目的、言語の性格、確定した設計判断の一覧、文書全体で使う用語をまとめる。個々の規則は、各行のリンク先の文書で定める。

## 目的

- 主目的は言語デザインの実験である。「線形型 × 代数的エフェクト」の組み合わせで新しい書き心地を探り、他の人にも使ってもらえる形にする。
- 副目的は、作者自身の日常スクリプト用途で実際に使うことである。テキスト・ファイル処理、シェル自動化、データ変換、ネットワーク・並行の処理を、短く安全に書けるようにする。

言語名の eml は、effect と作者の GitHub ID `ecofighter` を掛けた名前である。ソースファイルの拡張子は `.em` とする。

## 言語の性格

- 関数型、式指向で、イミュータブルがデフォルトである。
- 副作用は `IO` エフェクトとして管理する。
- 線形型と代数的エフェクトを持つ。
- 構文は Haskell 流を基本にし、F# からいくつかの書き方を借りる。構文で迷ったときは Haskell の慣習に寄せる。
- 将来はネイティブコンパイルする。Cranelift で JIT と AOT のコードを出し、VM は参照の実装と検査モードの実行系として残す ([ネイティブ化の設計](future/native.md))。
- rowan (Red-Green Tree) の上に、リッチな診断と LSP を整える。

## 確定した設計判断

### 型システム

| 領域 | 決定 | 詳細 |
|---|---|---|
| 線形性 | Kind で区別する。`Type<m>`、`m ∈ {Unr ≤ Lin}`。Affine は持たず、値を捨てるときは組み込みキーワード `drop` を明示する | [型と Kind](spec/types.md)、[線形性](spec/linearity.md) |
| Kind の将来 | 内部では最初から Kind 変数と部分 Kind 関係を扱う。表面の構文で書ける Kind は、`once` の矢印、row 変数の多重度の上限 `never`、`data` の row の型引数と `Lin` の宣言に限り、それ以外は書かせない。将来ユーザーが Kind を一般に書けるようにする | [型と Kind](spec/types.md) |
| エフェクト | Row 多相 (Koka 方式、scoped labels)。row 変数にも Kind `Row<s>`、`s ∈ {Never ≤ Once ≤ Multi}` を持たせる。`Never` は `never` の操作だけの row の Kind で、シグネチャに `<never e>` と書ける | [型と Kind](spec/types.md)、[エフェクトと handler](spec/effects.md) |
| 継続の多重度 | 操作ごとに `never` / `once` / `multi` を宣言する。デフォルトは `once` | [エフェクトと handler](spec/effects.md) |
| 継続 | 継続 `k` は普通の関数で、`k v` で再開する。状態のある handler では `k v st` と書く。`once` の `k` は `Lin` の矢印を持ち、`ctl` の節は、戻らない呼び出し (`fail`、`exit` など) で終わる経路を除き、`k` を呼ぶか `drop k` を必ず書く。`fn` の節は `k` を書かず、本体の値で再開する。`multi` の `k` は `Unr` の矢印を持つ | [エフェクトと handler](spec/effects.md) |
| handler の意味 | deep handler。再開した継続の中でも同じ handler が有効なまま | [エフェクトと handler](spec/effects.md) |
| `IO` | Prelude が宣言する組み込みの extern のエフェクトで、操作は `never` の `io_failed` だけである。`io_failed` は Prelude の外から見えず、Prelude の `try_io` だけが `IO` を handle する。利用者は `IO` を handle できない。`println` などは `<IO>` を持つ `extern` の関数である | [エフェクトと handler](spec/effects.md) の「組み込みの `IO`」、[標準ライブラリ](spec/stdlib.md) の「`try_io`」 |
| `Panic` | Prelude が宣言する組み込みの extern のエフェクトで、値と入力で決まる誤りを表す。整数の桁あふれとゼロ除算、文字列の範囲外の位置、範囲外のスクリプトの引数などを起こしうる関数は、最後の矢印の row に `Panic` を持つ。操作は `never` の `panicked` だけで、Prelude の外から見えない。Prelude の `panic` で起こし、`try_panic` だけが handle する | [エフェクトと handler](spec/effects.md) の「組み込みの `Panic`」、[標準ライブラリ](spec/stdlib.md) の「`try_panic`」 |
| `var` | 局所の可変の変数で、ブロックの文 `var x = e` で宣言し、`x <- e` で書き換え、配列の変数は `x[i]` で読み書きする。`var` の地点ごとに合成のエフェクトを1つ作り、変数を使う所はその row を求める | [式](spec/expressions.md) の「`var`」、[エフェクトと handler](spec/effects.md) の「`var` のエフェクト」 |
| 暗黙の後始末 | 通常の制御フローでは行わない。例外は2つで、中断では外れるフレームの値を巻き戻しが解放し、戻らない呼び出し (`fail`、`exit` など) で終わる経路では、消費しない `Lin` の値を呼び出しの前で手放す。破棄処理は物体の解放だけで、エフェクトを起こさず、失敗もしない | [線形性](spec/linearity.md) の「暗黙の後始末」、[エフェクトと handler](spec/effects.md) |
| 型付け | Bidirectional Typing + 単一化。トップレベルの関数は引数と戻り値の型注釈が必須。トップレベルの関数の row はシグネチャで決まり、省略した row は `<>` (純粋) である。Kind は推論する | [型と Kind](spec/types.md) |
| 直積型 | レコードは名前的で、`data` のコンストラクタにフィールドの名前を付けて宣言する (`data Person = \| Person { name : String, age : Int }`)。どのコンストラクタにもフィールドの名前を付けられ、射影 `p.name` と更新 `{ p \| age = 31 }` はコンストラクタが1つの型でだけ使える。タプルは構造的なままで、`t.0` で射影する。Unit は 0 要素のタプル `()` である。Kind はフィールドの Kind の join で推論する | [直積型とレコード](spec/records.md) |
| 単相化 | 関数のコードは (関数, 型引数) の instance ごとに単相化し、データの配置は一様のままにする。多相再帰で大きくなる型変数の位置と handler の節の型変数は、一様な位置 (`tobj`) として扱う | [Core IR](spec/core-ir.md) の「変換の規則」 |
| 等価、比較、表示 | 型クラス `Eq`、`Ord`、`Show` を Prelude に置く。`class` と `instance` の宣言、シグネチャの制約 `Eq a =>`、`deriving` を持つ。証拠は Core IR への変換の単相化で決め、実行時の辞書は持たない | [宣言](spec/declarations.md) の「クラスと instance」、[型と Kind](spec/types.md) の「制約」 |
| 数 | `Int` は 64 ビットの符号付き整数、`Float` は IEEE 754 の倍精度の浮動小数である。リテラルは単相で、型はそれぞれ `Int` と `Float` になる。算術は Prelude のクラス `Num` (`+ - *`、`negate`) と `Div` (`/ %`) のメソッドで、`Int`、`Float`、`Byte` がそれぞれ instance を持つ | [型と Kind](spec/types.md) の「数の型」 |
| 文字 | `Char` は Unicode のスカラー値を表し、`Int` とは別の型である。リテラルは `'a'` で、`Num` の instance を持たない。比較と表示は `Eq`、`Ord`、`Show` で、変換と ASCII の判定は `Std.Char` にある | [型と Kind](spec/types.md) の「数の型」、[標準ライブラリ](spec/stdlib.md) の「`Std.Char`」 |
| 文字列 | `String` は常に妥当な UTF-8 である。位置は UTF-8 のバイトの位置の `Int` で、位置専用の型は持たない。文字の境界でない位置は、`char_at` と `slice` が `Panic` で断る。extern は小さい核 (検証付きのデコード、エンコード、バイト長、位置から文字を読む操作、境界を検査する切り出し) にとどめ、ほかの関数は eml で書く | [標準ライブラリ](spec/stdlib.md) の「`String`」と「extern の境界」 |
| バイト | `Byte` は 0 から 255 の整数を表し、`Int` とは別の型である。リテラルは `65u8` と `'a'u8` で、`Eq`、`Ord`、`Show` を持つ。算術の `Num` と `Div` は、結果が 0 から 255 の外なら `Panic` を起こす。ビット演算は Prelude のクラス `Bits` で、`Int` と `Byte` が instance を持つ | [型と Kind](spec/types.md) の「数の型」、[字句](spec/lexical.md) の「数値と文字」、[宣言](spec/declarations.md) の「Prelude のクラス」 |
| バイト列 | `Bytes` は `Unr` で意味の上で書き換わらないバイトの列である。`String` と同じ物体の形を持つ別の種類で、変換は中身を写すか、一意なら種類だけを書き換える。更新は、RC が 1 ならその場で書き換え、そうでなければ写す | [標準ライブラリ](spec/stdlib.md) の「`Bytes`」、[オブジェクトモデル](spec/object-model.md) の「バイト列と文字列」 |
| ハンドル | 標準入出力のハンドル `Stdin`、`Stdout`、`Stderr` とファイルのハンドル `FileIn.FileIn`、`FileOut.FileOut` は `Lin` の型で、型ごとに1つのモジュールを持つ。文字列を読み書きする関数が短い名前を持ち、バイト列向けの関数は `_bytes` を足す。読む関数はハンドルを借りて結果を先に返し、書く関数はデータを借りてハンドルを返す。標準のストリームごとに、生きているハンドルは高々1つである。標準出力の書き込みの buffer は Host が持ち、`println` と共有する。`FileOut` は自分の buffer を持ち、閉じずに手放しても書き出す | [標準ライブラリ](spec/stdlib.md) の「ハンドル」 |
| 配列 | `Array a` と `MutArray a` は Prelude の extern の型である。`Array` は意味の上で書き換わらず、Kind は要素に従う。`MutArray` は Kind が常に `Lin` で、線形に受け渡して使う | [標準ライブラリ](spec/stdlib.md) の「`Std.Array`」「`Std.MutArray`」 |
| 取り消しと時間 | `shield` に入ってからタスクが最初に開くスコープは境目で、外からの取り消しは境目で止まる。境目とその内側のスコープ自身の旗 (期限、子の失敗、`cancel_scope`) は `shield` の中でも届く。時間は今からの長さを `Int` のミリ秒で表す。時計は `eml run` と `eml repl` ではいつも本物で、ライブラリの既定は仮想である。仮想の時計とテストの driver はテストのハーネスが選ぶ。仮想の時計はすべてのタスクが待つときだけ進む | [標準ライブラリ](spec/stdlib.md) の「取り消し」「時間」、[VM](spec/vm.md) の「時計と driver」 |
| channel と共有の数 | `Std.Chan` の `Chan a` は登録表の添字を持つ `Unr` の即値で、Kind は `a` によらず `Unr` である。channel は `close` で放す資源で、閉じて空になった時点でその場で放し、閉じないままの channel は run の終わりに放す。受け手の決まらない値 (channel の品物とタスクの結果) は、値を預かる構造の論理的な worker ID を途中の持ち主にして移す。共有して書き換える数は `Std.Atomic.Counter` だけである | [標準ライブラリ](spec/stdlib.md) の「`Std.Chan`」「`Std.Atomic`」、[VM](spec/vm.md) の「channel」、[オブジェクトモデル](spec/object-model.md) の「論理的な worker ID」 |
| ネットワークとシグナル | `Std.Net` の `TcpListener` と `TcpStream` は `Lin` の `Foreign` で、落とすとソケットを閉じる。待つ関数はどれも取り消しの地点で、`_within` の版は結果を `Option` に包み、`None` は期限が過ぎたことだけを表す。拒まれた接続 (`ConnectionRefused`)、reset、使用中の番地、時間切れは IO の失敗の種類で分ける。driver が受け付けで終えた操作は、待たずに続ける。読みは buffer の空きにそのまま読み、`write_bytes` は書いた `Bytes` を写さずに返す。`Std.Signal.wait` はシグナルを貯めず、同じシグナルを待つタスクをすべて起こす。待つタスクがなければ、Ctrl-C は run を止めて 130 で、SIGTERM は同じ止め方で 143 で終わる | [標準ライブラリ](spec/stdlib.md) の「`Std.Net`」「`Std.Signal`」、[VM](spec/vm.md) の「操作の記録」「上限と割り込み」 |
| TLS | `Std.Tls` はクライアントだけを持つ。TLS の状態 `Tls.Conn` は I/O を持たない `Lin` の `Foreign` で、ソケットの読み書き、待ち、取り消し、期限は `Std.Net` のものを使い、`std/Tls.em` が両者の間でバイトを汲む。close_notify のない EOF は IO の失敗にする。信頼する根は webpki-roots の根と、渡した PEM の CA である | [標準ライブラリ](spec/stdlib.md) の「`Std.Tls`」 |
| HTTP | `Std.Http` は `Net` と `Tls` の上に eml で書いた HTTP/1.1 のクライアントで、extern を持たない。要求ごとに `Connection: close` で接続を開いて閉じる。本文は `Transfer-Encoding`、`Content-Length`、EOF の順に区切り、1つの `Bytes` に読む。リダイレクトは回数の上限まで追い、https から http へは追わない | [標準ライブラリ](spec/stdlib.md) の「`Std.Http`」 |

### 構文

構文は、次の3つを目標に設計した。

- 「線形型 × 代数的エフェクト」のプログラムが自然に読み書きできること
- 日常のスクリプト (テキスト・ファイル処理、シェル自動化、データ変換、ネットワーク・並行) が短く安全に書けること
- 文法に曖昧さがなく、rowan とイベント方式のパーサで素直に実装できること

| 領域 | 決定 | 詳細 |
|---|---|---|
| 全体の方針 | Haskell 流を土台にする。F# から、逐次実行 (`do` なしで行を並べる)、`\|>` のパイプライン、`match` / `handle` の `with` と `\|` の枝、`List.map` 形式のモジュール修飾を借りる | [文法](spec/grammar.md) |
| レイアウト | オフサイドルール。開始トークン (`=` `->` `with` `then` `else` `where`) で行が終わり、次の行が深いときだけブロックを開く | [レイアウト規則](spec/layout.md) |
| トップレベルのシグネチャ | Haskell / Idris と同じく、別の行に書く。必須であることは HIR で検査する。複数の等式で定義できる | [宣言](spec/declarations.md) |
| ラムダ | `fn x y -> e` | [式](spec/expressions.md) |
| `if` | then 節が `Unit` なら `else` を省略できる | [式](spec/expressions.md) |
| `match` / handler の枝 | `\|` を必須にし、字下げする。1行でも書ける。直和型の宣言も同じ形 | [式](spec/expressions.md)、[宣言](spec/declarations.md) |
| 型の宣言 | `data` は直和型、`type` は型の別名 | [宣言](spec/declarations.md) |
| エフェクトの宣言 | `effect Name a where` の後に操作を並べる。操作のカリー化を許す | [宣言](spec/declarations.md) |
| `use` | ブロックの残りを、最後の引数のラムダとして渡す糖衣構文 | [式](spec/expressions.md) |
| handler の節 | 操作の節は `\| ctl op x k -> …` (継続を操作する) か `\| fn op x -> …` (本体の値ですぐに再開する) で、`return` の節にはどちらも書かない。名前は Koka に合わせた | [式](spec/expressions.md) |
| ローカル関数 | `let f x = …` で定義し、`and g y = …` で続けた組は互いに呼び合える (OCaml の `let rec … and …` と同じ) | [式](spec/expressions.md) |
| パラメータ付き handler | `handle e from init with`。状態を handler が持ち、節の最後の引数で受ける。状態のある handler では `k v st` で次の状態を渡す。状態があるかどうかは `from` の有無で決まる | [式](spec/expressions.md) |
| 線形値の受け渡し | 糖衣構文はない。`FileIn` を持つ生産者は `Iter` で書ける (`tests/ui/run/std/iter_file/`)。糖衣は [ロードマップ](future/roadmap.md) の「言語機能と構文」の項目にある。再束縛、`use`、パラメータ付き handler で吸収する | [式](spec/expressions.md) |
| 文字列 | 補間は `"\{x}"`。複数行の `"""`、raw の `r"..."` | [字句](spec/lexical.md) |
| コマンドリテラル | バッククォート。シェルを介さず、引数のリストを組む (Julia 方式)。空白で引数を分け、`'…'` で空白をまとめ、穴 `\{e}` は `String` だけを受ける。値は Prelude の `Cmd` で、`Std.Proc` が起動する | [字句](spec/lexical.md)、[式](spec/expressions.md) |
| fixity | ユーザーが宣言する。優先順位は整数 0〜9。CST では演算子の列を平たいまま持ち、HIR で組み直す | [宣言](spec/declarations.md)、[式](spec/expressions.md) |
| モジュール | 1ファイル = 1モジュール。`pub` で公開する。`pub abstract` の型とエフェクトは名前だけを公開し、コンストラクタと操作をモジュールの中に隠す。import は既定で修飾付き。標準ライブラリのモジュール (`Fs` など) も、Prelude のほかは import して修飾付きで使う | [モジュールと名前解決](spec/modules.md) |
| 組み込みの宣言 | 修飾子 `extern` で組み込みの関数、型、エフェクトを宣言する (`pub extern println : String -> <IO> Unit`)。標準ライブラリはバイナリに埋め込んだ `std/` のツリーから読み、Prelude は `std/Prelude.em` である。ファイルを読む操作は標準ライブラリのモジュール `FileIn` (`FileIn.open` など) に、パスで済む操作は `Fs` (`Fs.read_text` など) にある | [宣言](spec/declarations.md)、[モジュールと名前解決](spec/modules.md) |
| 並行処理 | 新しい構文は追加しない。エフェクトにより直接の形で書ける | [式](spec/expressions.md) |
| コメント | `--` と、入れ子にできる `{- -}` | [字句](spec/lexical.md) |

### コンパイラと実行系

処理系はバッチ型のパイプラインで、各段階を純粋な関数にし、後でクエリ化 (salsa など) できるようにしてある。実行系は、型付き Core IR (前向きの辺だけを持つ基本ブロックの列で、エフェクトを明示する) を EP の後に LIR (RC と box を明示し、エフェクトの命令を持たない) に写し、LIR をレジスタ型のバイトコードの VM で実行する。メモリは Perceus 方式の参照カウントで管理し、`Lin` の値を2回消費することはない。VM はシングルスレッドで出し、後から並列にできる形で作ってある。`par` の2つの子も、1つの worker の上でタスクを切り替えて動かす ([VM](spec/vm.md) の「`par` の fork と join」)。並列化は、VM とネイティブのランタイムの両方に入れ、複数の worker が OS のスレッドの上でタスクを動かす。詳細は [コンパイラの構成](implementation/architecture.md)、[Core IR](spec/core-ir.md)、[LIR](spec/lir.md)、[VM](spec/vm.md)、[オブジェクトモデル](spec/object-model.md)、[マルチコア対応の設計](future/multicore.md) にある。

## 用語

| 用語 | 意味 |
|---|---|
| `Unr` / `Lin` | 型の線形性を表す Kind の要素。`Unr ≤ Lin`。`Lin` の値はちょうど1回使わなければならない |
| row | 関数が起こし得るエフェクトの並び。`<IO>` は閉じた row、`<IO \| e>` は row 変数 `e` で開いた row |
| `Row<s>` | row の Kind。`s ∈ {Never ≤ Once ≤ Multi}` は、その row に含まれてよい操作の上限を表す |
| 多重度 | 操作ごとに宣言する `never` / `once` / `multi`。handler 側の継続 `k` の有無と線形性 (`once` なら `Lin`、`multi` なら `Unr`、`never` には `k` がない) を決める |
| 持ち越し規則 | 操作の呼び出しをまたいで生きていてよい値の規則。`multi` の操作をまたげるのは `Unr` の値だけである |
| 中断 | 継続が再開されずに捨てられること。`never` の操作、`try_io` が捕まえる IO の失敗、`exit` は、継続を組み立てずに、タスクのスタックを行き先のフレームまで巻き戻す。`drop k` は継続を捨てる。どちらも、捨てたフレームや継続が持っていた値を解放する |
| 戻らない呼び出し | 型スキームを持つ宣言を引数をそろえて呼ぶ呼び出しで、結果の型が、宣言の型のほかの所に現れない制約のない型変数であるもの (`fail`、`exit`)。その型の値を作れないので、普通には戻らない。この呼び出しで終わる経路では、消費しない `Lin` の値を手放さなくてよい |
| 破棄処理 | `Lin` 型に宣言された捨て方。標準ライブラリのリソース `FileIn.FileIn` では `FileIn.close` |
| deep handler | 再開した継続の中でも同じ handler が有効なままの handler |
| パラメータ付き handler | `handle e from init with` の形で、状態を節の最後の引数として受け渡す handler。状態は handler の実行ごとの evidence に置き、脱糖しない |
| レイアウト段 | lexer と parser の間で、仮想トークン `OPEN` / `SEP` / `CLOSE` (コードでは `LAYOUT_OPEN` / `LAYOUT_SEP` / `LAYOUT_CLOSE`) を挿入する段 |
| Perceus | 参照カウントの `dup` / `decref` / `release` を静的に挿入する方式。reuse analysis も持つ ([LIR](spec/lir.md) の「reuse」)。借用の最適化はない ([ロードマップ](future/roadmap.md) の「段の列」) |
| Core IR | 型付き HIR から変換する、前向きの辺だけを持つ基本ブロックの列の IR。すべての中間値に名前を付け、エフェクトの命令を明示する。RC と box は持たない |
| LIR | EP の後の Core IR から写す、基本ブロックの列の IR。エフェクトの命令を持たず、box、RC の命令、末尾呼び出しを明示する。VM と後のネイティブ化の入力である |
| worker | タスクを動かす OS のスレッド1つ。自分の `Ctx` とヒープを持ち、スケジューラの下で仕事を取る。[マルチコア対応の設計](future/multicore.md) の「スレッドと worker」にある |
