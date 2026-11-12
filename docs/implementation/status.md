# 実装の現在地

位置づけ: 手引き。

この文書は、`.em` を書く人から見た今の言語の範囲 (実装したもの、まだないもの) と、既知の制限をまとめる。規則は `spec/` が定め、ここでは1行で挙げてリンクする。制限は、まだ作っていないことと遅いことを書き、直し方の見当があれば添える。いつ直すかは書かず、予定がある項目は [ロードマップ](../future/roadmap.md) を引く。`eml check` は検査だけを行い、`eml run` は VM で実行し ([VM](../spec/vm.md))、`eml repl` は REPL を開く ([REPL の端末](../spec/repl.md))。`tests/ui/run/scripts/` の5本のスクリプト (`wc`、`grep`、`logstat`、`csv2jsonl`、`deploy`) は、今の言語で書いた実用のプログラムの例である。

## 今の言語の範囲

### 実装したもの

- 基本型: `Int`、`Float`、`Char`、`Byte`、`Bool`、`String`、`Unit`。`Byte` は `65u8` と `'a'u8` の形のリテラルと、`Eq`、`Ord`、`Show`、算術の `Num` と `Div` を持ち、変換は `Std.Byte` にある ([標準ライブラリ](../spec/stdlib.md) の「`Std.Byte`」)。`Char` は文字のリテラル `'a'` と、`Eq`、`Ord`、`Show` を持つ。`Int` と `Byte` との変換、ASCII の判定、ASCII の大文字と小文字の変換は `Std.Char` にある ([標準ライブラリ](../spec/stdlib.md) の「`Std.Char`」)。`Bytes` はバイト列の型で、`Eq`、`Ord`、`Show` を持ち、リテラルはない。操作は `Bytes` にあり ([標準ライブラリ](../spec/stdlib.md) の「`Bytes`」)、`String` との変換は `Bytes.of_string` と `String.decode` である ([標準ライブラリ](../spec/stdlib.md) の「`Bytes` との変換」)。`Bool` は Prelude の `data Bool = | False | True` で、キーワード `true` / `false` はない。単項の `!` はなく、関数 `not` を使う
- ビット演算: Prelude のクラス `Bits` (`&&&`、`|||`、`^^^`、`complement`、`<<<`、`>>>`) と、`Int` と `Byte` の instance ([宣言](../spec/declarations.md) の「Prelude のクラス」)
- タプルと射影 `t.0`、型引数を持つ代数的データ型 (`data`)、`match` と入れ子のパターン、`Int`、`Byte`、`Char`、`String` のリテラルのパターン
- 名前付きのレコード: `data` のコンストラクタの名前付きのフィールド、作る式 `Person { name = "a", age = 3 }` と省略形、射影 `p.name`、更新 `{ p | age = 31 }`、分解のパターン、フィールドのセクション `(.name)` と `(.0)`、導出した `Show` の `Person { … }` の表示 ([直積型とレコード](../spec/records.md))。呼び出しの引数のラムダは、ほかの引数の後に型を検査する ([型と Kind](../spec/types.md) の「引数のラムダを後で検査する」)
- Prelude の `List`、`Option`、`Result`、`Either` と、その `Eq`、`Ord`、`Show` の instance ([宣言](../spec/declarations.md) の「Prelude のクラス」)。リストのリテラル `[…]` とパターン、コンストラクタ `::` (`infixr 5`。Prelude だけが宣言できる、E1044) ([式](../spec/expressions.md) の「リスト」)。リストの関数は `Std.List` にある (`map`、`filter`、`fold`、`length`、`take`、`zip`、`lookup`、安定な並べ替えの `sort` と `sort_by` など。[標準ライブラリ](../spec/stdlib.md) の「`List`」)
- 文字列: エスケープ、補間 `\{式}`、複数行の文字列 `"""`、raw 文字列 `r"…"` / `r#"…"#` ([字句](../spec/lexical.md) の「文字列」)。補間の穴は、Prelude の `Show` のメソッド `display` で文字列にする ([式](../spec/expressions.md) の「補間」)。文字列の関数は標準ライブラリの `String` にある。位置は UTF-8 のバイトの位置で、文字は `char_at`、`fold`、`chars` で読み、`push` で足す ([標準ライブラリ](../spec/stdlib.md))
- コマンドリテラル: 空白で区切った引数、引用 `'…'`、穴 `\{e}` (`e : String`)、spread の穴 `\{..xs}`。値は Prelude の `Cmd` で、HIR が Prelude の関数の呼び出しに脱糖する ([式](../spec/expressions.md) の「コマンドリテラル」)
- トップレベルの関数 (シグネチャが必須)、複数の等式による定義、ローカルの `let` と `let ... in`、ローカルの再帰関数 (`let go acc xs = …`。引数を1つ以上持つ関数の形の `let` は自分自身を参照でき、単相で、lambda lifting する。`let … and …` で書いた組の関数は互いに呼び合える。[式](../spec/expressions.md) の「ローカル関数」)、`if` (then 節が `Unit` なら `else` を省略できる)、再帰、ラムダとクロージャ、部分適用、型の明示 `(e : T)`
- シグネチャの型変数と row 変数による多相
- 1回だけ呼べる関数の型 `once a -> b` (`Lin` の矢印。`data` のフィールドにも書ける)、`data` の row の型引数 (`data Iter <e> a`、使う所は `Iter <IO> Int`)、`data` の Kind の宣言 `data T : Lin`、トップレベルの関数と extern の関数とコンストラクタとクラスのメソッドと REPL の `let` の値を `once` の位置に置くこと (参照ごとに一番外側の矢印が新しい Kind 変数になる) ([型と Kind](../spec/types.md)、[宣言](../spec/declarations.md))
- row 変数の多重度の上限 `<never e>`: シグネチャで宣言し、本体の注釈でくり返せる。上限を超える row を渡す呼び出しは E2020 になる ([型と Kind](../spec/types.md) の「row 変数の多重度の上限」)
- fixity の宣言とユーザー定義の演算子、演算子のセクション、`use`
- `drop` キーワード
- エフェクトの宣言 (`never` / `once` / `multi`、型引数)、`handle` (deep)、`k` の呼び出しによる再開、`drop k`、パラメータ付き handler (`handle ... from ... with`)。操作の節は、`k` を受け取る `ctl` の節 (`| ctl op x k -> …`) と、本体の値ですぐに再開する `fn` の節 (`| fn op x -> …`) である ([式](../spec/expressions.md) の「handler」)
- 位置つきの `mask`: 型検査が呼び出しごとに決め、エフェクトごとに内側の n 個の handler を残して続く m 個を飛ばす。`Yield Int` を受けて `Yield String` を出すような、型を変換する handler を書ける ([エフェクトと handler](../spec/effects.md) の「handler の意味」、[型と Kind](../spec/types.md) の「推論」)
- `extern` の宣言と `eml_extern` の表。`Int`、`String`、`Unit`、`IO`、`println`、算術の演算子などは Prelude の `extern` である。`Int`、`Char`、`Byte`、`String`、`Bool` の比較と表示のメソッドは、instance の中の `extern` で表の行に結ぶ。`IO` を、`println` と標準ライブラリのモジュール `FileIn` の `open`、`read_all`、`close` などが起こし、`FileIn.FileIn` は線形型である。ユーザーは `IO` を handle できず (Prelude の `try_io` だけが handle する)、ABI のない `extern` を書けない (E1033) ([エフェクトと handler](../spec/effects.md) の「組み込みの `IO`」)。extern のエフェクトの多重度は表の行が決め、`IO` は `Once`、`Panic` は `Never` である ([エフェクトと handler](../spec/effects.md) の「組み込みの `Panic`」)
- Cの関数の呼び出し: ユーザーのモジュールに`extern "C"`でCの関数と不透明なハンドルの型 (`extern "C" data T drop "f"`) を宣言し、VMから呼ぶ。属性は`pure`、`blocking`、`symbol`、整数の幅は`Int as int`のように書き、配列、`String`、`Bytes`、ハンドルを借りてCに渡す。`blocking`の関数の呼び出しは、呼んだタスクだけを止め、Cの関数をdriverのプールのスレッドで呼ぶ。そのとき、大きい物体のページにある配列、`String`、`Bytes`の引数はページを留めてプールのスレッドに貸し、それより小さい引数は写す。ライブラリは`link on <os>`のitemと、`eml run`と`eml repl`の`--link KEY=VALUE`で指定する。橋渡しの関数は`eml_native`がCraneliftのJITで作る ([宣言](../spec/declarations.md)の「`extern "C"`」、[VM](../spec/vm.md)の「C の関数の呼び出し」、[VM](../spec/vm.md)の「`blocking` の呼び出し」、[オブジェクトモデル](../spec/object-model.md)の「ページの留め」)
- IO の失敗と終了: Prelude の `try_io : (Unit -> <IO | e> a) -> <IO | e> Result IOError a` が、IO の extern の失敗 (書き出しの失敗を除く) を `IOError { kind, message }` の値にする。`fail_io : IOErrorKind -> String -> <IO> a` は、種類と文言を指定して IO の失敗を起こす。`exit : Int -> <IO> a` は途中のフレームの値を後始末し、終了コード `n & 255` で終わる。REPL では入力を終えるだけである ([標準ライブラリ](../spec/stdlib.md) の「`try_io`」、「`fail_io`」、「`exit`」)
- 標準ライブラリのツリー: リポジトリの`std/` (`Prelude.em`、`FileIn.em`、`FileOut.em`、`Fs.em`、`List.em`、`String.em`、`Env.em`、`Stdin.em`、`Stdout.em`、`Stderr.em`、`Proc.em`、`Float.em`、`Char.em`、`Byte.em`、`Bytes.em`、`Array.em`、`MutArray.em`、`UnboxedArray.em`、`MutUnboxedArray.em`、`Yield.em`、`Fail.em`、`State.em`、`Iter.em`、`Map.em`、`Set.em`、`Par.em`、`Async.em`、`Chan.em`、`Atomic.em`、`Net.em`、`Signal.em`、`Tls.em`、`Http.em`、`Stream.em`、`Exec.em`、`Gui.em`) をバイナリに埋め込む。Preludeはいつも読み込み、ほかのモジュールはimportしたものだけを読み込む。`import FileIn`か`import Std.FileIn`を書いたモジュールで、`FileIn.open`のように修飾付きで使う。予約した根は`Std`である ([モジュールと名前解決](../spec/modules.md)の「標準ライブラリ」)
- std のエフェクトと `Iter`: エフェクトのモジュール `Yield` (`for_each`、`fold`、`collect`)、`Fail` (`to_result`、`to_option`)、`State` (`run`、`eval`、`exec`) と、`once` の継続で作る引いて使うジェネレータ `Iter` とその変換。`FileIn` から1行ずつ読む `FileIn.read_line` ([標準ライブラリ](../spec/stdlib.md) の「エフェクトのモジュール」と「`FileIn`」)。`tests/ui/run/scripts/` の `grep`、`logstat`、`csv2jsonl` は std のエフェクトを使い、`csv2jsonl` の CSV の読み取りは `let … and …` の組で書く
- 標準入出力とファイルのハンドル: 線形な型 `Stdin`、`Stdout`、`Stderr`、`FileIn.FileIn`、`FileOut.FileOut`。読むハンドルは文字列とバイト列を、全部、1行ずつ、塊ごとに読み、書くハンドルは文字列とバイト列と行を書く。`FileOut` は自分の buffer を持ち、閉じずに手放しても書き出す。標準のストリームごとに、生きているハンドルは高々1つである ([標準ライブラリ](../spec/stdlib.md) の「ハンドル」)
- 子のプロセス: `Std.Proc` が、`Cmd` をシェルを介さずに起動する。起動の形は、標準入出力を受け継ぐ `run`、標準出力を取る `output` と `capture`、pipe でやりとりする線形な `Child` を返す `spawn` の3つである。作業ディレクトリ、環境変数、引数の追加、標準エラーの向き (`2>&1`、`2>/dev/null`)、標準入力 (文字列、ファイル、空)、標準出力のファイルへの書き出しと追記 (`> out`、`>> out`) を `Cmd` に設定できる。演算子 `.|` は `Cmd` をつないだパイプラインを作り、1回の `spawn` で段の間を OS の pipe でつないで起動する。`wait_all` はすべての段の終了コードを返す ([標準ライブラリ](../spec/stdlib.md) の「`Proc`」)
- 非同期のファイルと子のプロセス: `Std.Stream` が通常のファイルを driver の上で読み書きし、`Std.Exec` が子のプロセスを起動して待つ。子の標準入力、標準出力、標準エラーの pipe は `Stream.Writer` と `Stream.Reader` として取り出し、複数のタスクで読み書きできる。待ちと pipe は Linux (epoll と pidfd)、macOS (kqueue)、Windows (IOCP) の本物の driver にあり、仮想の時計では `Std.Exec` の起動が誤りになる ([標準ライブラリ](../spec/stdlib.md) の「`Std.Stream`」と「`Std.Exec`」)
- `Float` (IEEE 754 の倍精度) のリテラル、算術、比較と、Python の `repr` の形の表示。変換と数学の関数は `Std.Float` にある ([宣言](../spec/declarations.md) の「Prelude のクラス」、[標準ライブラリ](../spec/stdlib.md) の「`Std.Float`」)
- 配列: 変えられない `Array a` (`Std.Array`) と、線形な `MutArray a` (`Std.MutArray`) ([標準ライブラリ](../spec/stdlib.md) の「`Std.Array`」と「`Std.MutArray`」)。要素を箱に入れず生の 8 バイトの語で並べる `UnboxedArray a` (`Std.UnboxedArray`) と、線形な `MutUnboxedArray a` (`Std.MutUnboxedArray`) もある。要素の型は `Float` と `Int` だけで、添字の式 `e[i]` と `var` の配列の変数は使えない ([標準ライブラリ](../spec/stdlib.md) の「`Std.UnboxedArray`」と「`Std.MutUnboxedArray`」)
- 連想配列と集合: キーの順の `Map k v` (`Std.Map`) と `Set a` (`Std.Set`)。どちらも釣り合いを取った木で、キーと要素は `Unr` に限る ([標準ライブラリ](../spec/stdlib.md) の「`Std.Map`」と「`Std.Set`」)
- 2つの計算の組: `Std.Par`の`par`は2つの子を動かして両方を待ち、子の`never`の操作を親で起こし直す。子のrowの上限は`never`で、`IO`や`var`を使う子はE2020になる。`both`は同じ組を逐次に作る ([標準ライブラリ](../spec/stdlib.md)の「`Std.Par`」)。`eml run`と`eml repl`は、子を切り替える方針がいつも普通の方針 (`Normal`。自分からはタスクを切り替えない) である。逐次 (`Sequential`) とseedで決まる切り替え (`Seeded`) は、ライブラリのAPIから渡すテストのための方針である ([VM](../spec/vm.md)の「タスクとスケジュール」)。`eml run`の`--workers`でworkerのスレッドの数を選び、右の子は空いているworkerで動く ([標準ライブラリ](../spec/stdlib.md)の「起動」)
- 非同期のタスク: `Std.Async`の`run`が根のスコープを開いて本体をタスクとして動かし、`start`と`spawn`が今のスコープにタスクを足し、`await`がタスクの値を受け取るか失敗を起こし直す。`run`はスコープのすべてのタスクを待ってから戻る。`scope`が名前付きのスコープを開き、`start_in`と`spawn_in`がそこにタスクを足す。`cancel`と`cancel_scope`がタスクとスコープを取り消し、`shield`が取り消しの地点を守り、`check_cancelled`と`cancel_requested`が待たないループの中で取り消しを確かめる。ほかに`yield_now`、`try_all`、`Failure`と、時間を扱う`sleep_ms`、`timeout`、`race`、`drain_within`と、CPUを使う仕事を子が1つの`par`と同じ仕事として動かし、呼んだタスクだけを待たせる`compute`がある。タスクは`start`のときに置けるworkerをworker 1から順に回して置く。同じworkerのタスクはスケジュールの方針に従って切り替えて動く ([標準ライブラリ](../spec/stdlib.md)の「`Std.Async`」、[VM](../spec/vm.md)の「`Async`のタスク」)
- メインのworker: `eml run`では、`Machine`を持つプロセスのメインのスレッドがメインのworkerになり、`Async.start_on_main`と`Async.run_on_main`で始めた固定したタスクを動かす。`main`はworker 0で動く。固定したタスクが始めるタスクも固定し、`par`の子と`compute`の仕事はほかのタスクと同じく置く。`eml repl`ではworker 0がメインのworkerを兼ねる ([標準ライブラリ](../spec/stdlib.md)の「メインのworker」、[VM](../spec/vm.md)の「メインのworker」)
- タスクごとの上限: `Async`のタスクの中で呼び出しの深さが上限を超えると、runは止まらず、そのタスクに種類`ResourceLimit`のIOの失敗が起きる。確保量がメモリの柔らかい上限 (既定は`max_bytes`の7/8) の閾値を越えたときも、確保の数がいちばん大きいタスクに同じ失敗が起きる。どちらも関数に入るところで起き、`try_io`と`try_all`で捕まえられる。タスクの外の深さの上限と、メモリの硬い上限`max_bytes`はrunを止める ([標準ライブラリ](../spec/stdlib.md)の「タスクの上限」、[VM](../spec/vm.md)の「上限と割り込み」と「タスクの上限」)。空の物体を作る`Bytes.empty`と`MutArray.empty`は、`max_bytes`を確かめない ([オブジェクトモデル](../spec/object-model.md)の「バイト列の操作」)
- タスクの間の channel と共有の数: `Std.Chan` の channel (`send`、`recv`、`try_send`、`try_recv`、`close`、`merge`) と、`Std.Atomic` の `Counter` がある。どちらも worker をまたいで使える ([標準ライブラリ](../spec/stdlib.md) の「`Std.Chan`」と「`Std.Atomic`」、[VM](../spec/vm.md) の「channel」)
- TCP: `Std.Net` の `listen`、`accept`、`connect`、読み書き、`read_line` が、macOS と Linux の本物の driver とテストの driver の上で動く。`connect` は名前を driver の解決のスレッドで解決する。誤りは IO の失敗で、拒まれた接続、reset、使用中の番地、時間切れを種類で分ける ([標準ライブラリ](../spec/stdlib.md) の「`Std.Net`」)。driver が受け付けの時点で終えた操作は、待たずに続ける。操作の token `OpId` はヒープの物体を持たない即値である ([標準ライブラリ](../spec/stdlib.md) の「`Std.Async`」、[VM](../spec/vm.md) の「操作の記録」)。空きのある buffer への読みは、driver の操作1つで済む。空きは `Bytes.with_capacity` で作れる。`write_bytes` は書いた `Bytes` とストリームの組を返し、共有の `Bytes` も写さずに書く
- TLS: `Std.Tls`のクライアントが、`Std.Net`のストリームの上でhandshakeし、暗号にして読み書きする。TLSの状態はI/Oを持たないcrate `eml_tls`が持ち、ソケットのI/Oは`std/Tls.em`が`Std.Net`で行う。暗号はaws-lc-rsである。サーバーの証明書はOSの検証器で確かめ、OSの根に加えてPEMで渡したCAを信頼する。接続の設定とhandshakeの間の押し込みは、ブロックする呼び出しのプールで動く。テストのdriverの`tls.localhost`がTLSを終端する ([標準ライブラリ](../spec/stdlib.md) の「`Std.Tls`」)
- HTTP: `Std.Http` の HTTP/1.1 のクライアントが、`Std.Net` と `Std.Tls` の上で `http` と `https` の URL に要求を出す。要求ごとに接続を開いて閉じ、本文は `Transfer-Encoding` の chunked、`Content-Length`、EOF で区切る。リダイレクトを追い、https から http へは追わない ([標準ライブラリ](../spec/stdlib.md) の「`Std.Http`」)
- GUI: `Std.Gui`は、handler `Gui.run`の下で直接の形でネイティブのウィンドウを組み、ハンドルを通してウィジェットを変え、`Gui.wait`でメッセージを取り出す。ウィジェットはWindow、Grid、Label、Button、TextField、CheckBox、Choice、ProgressBarで、Taffyのgridが自動で置く。`eml run`はネイティブのループをメインのworkerの`Park`の中で回す。ネイティブのbackendは、macOSではAppKit、LinuxではQt 6、Windows 11ではWindows App SDK 2のWinUI 3で、feature `gui-native`で組む。既定のビルドは画面を開かないstubのbackendだけを持ち、`--gui stub`もそれを選ぶ。固定したタスクは、`Async.start_off_main`と`Async.spawn_off_main`でエフェクトを持つ仕事をメインのworkerの外で始める ([標準ライブラリ](../spec/stdlib.md)の「`Std.Gui`」、[VM](../spec/vm.md)の「GUIのPark」、[配布とGUIの前提](distribution.md))
- ネイティブのコード: `eml run --native`は、プログラムをCraneliftのJITでネイティブのコードにし、ネイティブの実行系 (`eml_nrt`) の上で走らせる。動かすのはLIRの一階の部分で、スカラー、データ、直接の呼び出しと末尾呼び出し、参照の数え方、定数、インラインに展開するstdの行45と、呼ぶ行11 (`exit`と`panic`を含む) である。クロージャ、関数の値、`apply`、エフェクトの操作とhandler、`par`、`Async`などのタスクの行、ほかのstdの行、利用者のCの関数を使うプログラムは、E0004で断る ([ネイティブの実行系](../spec/native.md)の「断る形」)。VMの実行と違うのは次の点だけである。語の数で数える深さの上限はなく、`--debug-heap`、`--stats`、`Seeded`の方針もない。ネイティブのコードは1つのworkerで動き、窓を開かないので、1以外の`--workers`と`--gui`は使い方の誤りである ([ネイティブの実行系](../spec/native.md)の「VMとの違い」)
- シグナル: `Std.Signal` の `wait` で、タスクが Ctrl-C と SIGTERM を待てる。待つタスクがなければ、`eml run` は Ctrl-C で終了コード 130、SIGTERM で 143 で終わる ([標準ライブラリ](../spec/stdlib.md) の「`Std.Signal`」)
- ブロックの中の `var` と代入 `x <- e`、添字の読み `x[i]` と書き込み `x[i] <- e`、Prelude の `for` と `while` ([式](../spec/expressions.md) の「`var`」、[標準ライブラリ](../spec/stdlib.md) の「`for` と `while`」)
- 線形性、持ち越し規則、網羅性の検査。`fail` や `exit` のような戻らない呼び出しで終わる経路では、消費しない `Lin` の値を手放さなくてよい ([線形性](../spec/linearity.md) の「暗黙の後始末」)
- 型クラス: `class` (上位クラス、既定のメソッド、メソッド自身の型変数と制約)、`instance` (文脈、標準ライブラリの中の `extern`)、シグネチャの制約 `Eq a =>`、`deriving (Eq, Ord, Show)`、Prelude のクラス `Eq`、`Ord`、`Show` と `Ordering`、タプルと `Unit` の構造的な instance。証拠は単相化で決め、実行時の辞書は持たない ([宣言](../spec/declarations.md) の「クラスと instance」、[型と Kind](../spec/types.md) の「制約」)
- モジュール: エントリのファイルのディレクトリを根にして import をたどる読み込み、`pub`、修飾した名前、別名、import の並び、`T(..)` と `E(..)`、修飾子の合流、モジュールとしての Prelude と `Prelude.` の修飾、import の循環の禁止 (E1027)、公開の範囲の検査 (E1032) ([モジュールと名前解決](../spec/modules.md))
- 抽象型と抽象エフェクト (`pub abstract`): `pub abstract data` は型の名前だけを公開し、コンストラクタと名前付きのフィールドを宣言したモジュールの中に隠す。`pub abstract effect` はエフェクトの名前だけを公開し、操作を隠す。隠した部品を外で名指すと E1058、隠したフィールドの射影と更新は E2021 である。`Std.Map`、`Std.Set`、`Std.Iter` の型と `Std.Async` の `Async` は抽象である ([宣言](../spec/declarations.md) の「`pub`」、[モジュールと名前解決](../spec/modules.md) の「公開の範囲」、[エフェクトと handler](../spec/effects.md) の「抽象エフェクト」)
- 同じ名前の別の型、エフェクト、コンストラクタは、名前を定義するモジュールが2つ以上あるとき、`Prelude.Bool` と `Main.Bool` のようにモジュール名で修飾して表示する
- REPL (`eml repl`): 入力をまたいで定義と `let` の束縛を残すセッション、定義し直し、実行時エラーと割り込みの後の回復、`:load` / `:reload` / `:type`、型に従う結果の表示器 (予算と深さと長さの上限、幅に合わせた整形) ([REPL の端末](../spec/repl.md))

### まだないもの

- まだ実装していない構文: 型の別名 `type` ([ロードマップ](../future/roadmap.md) の「言語機能と構文」)。字句と文法の置き場所を用意してあり、使うと E0004 (未対応) を出して回復する。どの段階が E0004 を出すかは [診断](../spec/diagnostics.md) の「未対応の構文と E0004」にある
- row の型引数を持つ型の instance と `deriving`。書くと E0004 を出す
- Unicode の表が要る文字の操作 (ASCII でない大文字と小文字の変換、文字の分類)、置換付きの UTF-8 のデコード、`Lin` の要素の配列の API ([ロードマップ](../future/roadmap.md) の「言語機能と構文」)
- コマンドの組み合わせ: `Proc` で標準エラーを別の文字列に取ること、環境変数の削除 ([ロードマップ](../future/roadmap.md) の「言語機能と構文」)
- 可変参照 `Ref`、再エクスポート (`pub import`)、修飾した演算子の構文、`data` のフィールドの矢印の線形性の多相、`: Unr` の宣言、row の型引数の省略、`Lin` の型を受けるクラス、or パターン、ガード、as パターン、関数をまたぐ reuse、LSP、フォーマッタ ([ロードマップ](../future/roadmap.md) の「その後の項目」)
- クロージャ、エフェクト、タスク、残りのstdの行、利用者のCの関数のネイティブのコード、ネイティブのREPL、AOTのコンパイル ([ロードマップ](../future/roadmap.md)の「段の列」)
- GUIのウィジェットの追加、ダイアログ、メニュー、gridの明示の置き方、宣言的な層 ([ロードマップ](../future/roadmap.md)の「GUI」)

## 既知の制限

### 診断の表示と言い方

- `data T <e> e = | T e` のように row の型引数と型引数が同じ名前を持つと、E1003 を出した後に、続けて E1002 も出る。2つ目は1つ目の連鎖である
- ariadne は、同じファイルの区画の見出しに、どの区画でも primary の位置を出す (`ariadne` 0.6 の `write.rs`)。そのため、secondary が primary より前の行にあって区画が分かれると (E3002 の「first used here」、E1003 の「first defined here」など)、2つ目の区画の見出しも primary の行と列になる。設定では変えられないので、直すときは表示を ariadne 以外で組むか、ariadne を書き換える
- HIR が呼び出しを1つにまとめるので、`(1 + 1) 2` は「`+` は2引数なのに3引数が渡された」と報告される。呼び出しをまとめるのは、`(f a) b` と `x |> f a` を引数の揃った1つの呼び出しとして扱うためである。まとめた呼び出しで引数が余ったときは、元の呼び出しの区切りで報告する方がよい
- E3003 の `drop x` の fix は、ファイルの改行が CRLF でも改行に `\n` を入れる。字下げは元のテキストから写すが、改行の種類は写さない。直すときは、挿入する行の改行を、その行の周りの改行に合わせる
- 網羅性の漏れの例のコンストラクタと E1013 の help の節の先頭は、表示名の表と既存の節の修飾子に従って書く。診断を出したモジュールで書ける形 (別名の修飾子を含む) にはしない
- 字下げの足りない行 (E0009) で本体を失ったラムダが文の位置にあると、`Unit` との型の誤り (E2001) が連鎖する (`fn x ->` の次の行を字下げしない場合)

### コンパイルの時間と深さ

- 演算子の列は、演算子1つを入れ子の1段と数え、それまでの被演算子の高さの最大も足す ([文法](../spec/grammar.md))。そのため、外側の入れ子がなければ、1つの列に書ける演算子は比較と `||` を合わせて約250個までである (`x == 0 || x == 1 || …` なら約125項)。括弧に入れた列を左に重ねると、各段の演算子の数の和が上限に数えられる。`use` の文も1つで1段と数えるので、1つのブロックに書ける `use` は約250個までである
- パターンのリストは、k 番目の要素を k - 1 段深く読む ([文法](../spec/grammar.md) の「文法上の補足」)。そのため、外側の入れ子がなく、要素が変数か `_` なら、1つのパターンのリストに書ける要素は256個までである (`f [x1, …, x256] = 0` は通り、257個で E0013)。式のリストは要素の数を数えない
- 呼び出しを要素に持つリストのリテラル (`[f 1, f 2, …]`) では、末尾でない呼び出しのたびに、それまでに評価した要素がすべて生きている。そのため、VM のスロットの割り当ての干渉グラフの辺の数が、要素の数の2乗になる。タプルも同じである。要素を先に評価して逆順のリストに積み、後で反転すれば避けられるが、確保が倍になるので採らない。要素が変数とリテラルだけなら線形である ([Core IR](../spec/core-ir.md) の「変換の規則」)
- 引数の多い1つの呼び出しでは、持ち越しのパスが、生きている値と矢印の組ごとに制約を作る (`carry.rs` の `carry` と `row_multiplicities`)。そのため、`ident ident … ident 5` と、引数を渡さない部分適用 (`let g = ident ident … ident`) は、適用の数の2乗の時間がかかる (`ident : a -> a`)
- Kind の検査は、値を使うたびにその型をたどる (`kind_at_most`)。そのため、`let p1 = (p0, p0)` のように前の値の組を作る `let` の列は、`let` の数の2乗の時間がかかる
- 単一化は、単一化を終えた複合の型の組を、1回の呼び出しの中でだけ覚える ([コンパイラの構成](architecture.md) の「`eml_types` の内部」)。そのため、同じ大きな型の組を何度も単一化すると、そのたびに推論の表の大きさに比例する時間がかかる。2本の `let` の列の後で、`let h1 = if c then f1 else g1`、`let h2 = if c then f2 else g2` のように、対応する値を1つずつ合わせる形がこれに当たる
- 単一化、occurs の検査、書き出し、表示の再帰の深さは、型の深さに比例する。そのため、非常に深い型ではスタックが尽きる。既定で 8 MiB のメインスレッドを持つ release の CLI では、`eml check` は、`let` の列 (`let f1 = f0 ident` の列) 1本なら長さ35000まで通り、40000で尽きた。その列を2本作って `if` で合わせる形は、長さ30000まで通った。単相化の型の置換 (`TypeStore::substitute`) も型の深さまで再帰する。そのため、同じ列が総称な関数 (`run : a -> a` で、最後が `f{n} x`) の中にあると、`eml run` は Core IR への変換で先に尽きる。同じ release の CLI では、長さ17500まで通り、20000で尽きた。再帰を作業の列に置き換えれば、型の深さによらなくなる (置換の再帰も同様である)
- 状態を持つ handler が `return` を省くと、`usage.rs` は状態の型を、そのたびに新しい `Exporter::diagnostic` で書き出す。状態の型が大きな共有の型のとき、そうした handler が多いと、handler の数と型の大きさの積の時間がかかる。使用の検査の全体で1つの記録を共有すれば避けられるが、`Exporter` は推論の表を借りている間しか生きない、という決まりを手放すことになる ([コンパイラの構成](architecture.md))
- 型の誤りの文言は、型を木として表示する (`TypeStore::display`)。そのため、部分を多く共有する型を文言に含む誤り (長い `let` の列の後の `let x : Int = f0` など) では、文言の長さが列の長さの指数になる。型の表そのものの大きさは、列の長さに比例したままである
- 単相化は instance の総数に上限を置かない ([Core IR](../spec/core-ir.md) の「変換の規則」)。多相再帰がなくても、関数の鎖の各段で次の関数を2通りの型で呼ぶと (`fi` の本体が `f(i+1) (x, 1)` と `f(i+1) (x, "s")` を呼ぶ形)、instance の数が段ごとに倍になり、鎖の長さの指数になる
- instance の鍵は型そのものなので、`List Int` と `List String` のように Repr が同じ型引数でも別の instance になり、同じ形のコードが複数できる。まとめるのは処理系の最適化に回す ([ロードマップ](../future/roadmap.md) の「処理系」)
- 決定木の再帰 (`translate/pattern.rs` の `decide`) の深さは、1つの `match` のパターンの大きさで決まる。コンストラクタやタプルの入れ子の深さは E0013 で抑えられるが、決定木の再帰の深さそのものは抑えていない。リテラルの列は1つの `switch` になり、複数の葉から届く枝は合流のブロックとして平らに並ぶので、リテラルが1000個の `match` や、そうした枝の多い `match` (`match (a, b) with | (0, 0) -> 0 | (_, 1) -> 1 … | (_, 999) -> 999 | _ -> 7`) は debug ビルドでも動く
- Kind のスキームを作る処理 (`residual`) は、1つの SCC の中で、関数ごとに残す成分から残さない領域をたどる。DAG の形によっては、残さない成分を関数ごとに何度もたどる。たとえば環状の相互再帰で、各関数の矢印の Kind 変数がどれも定数の境界を持つと、その成分は何もない成分にならず、関数の数の2乗の時間がかかる。また、持ち越しの制約を関数ごとに SCC 全体から見るので、関数の数と持ち越しの制約の数の積がかかる。どちらも、大きな SCC が多くの持ち越しの制約を持つ場合だけに起きる
- 1つの SCC の各メンバーが、トップレベルの矢印を返す関数の長い鎖 (`q_n ≤ … ≤ q_1`) を参照すると、既存の Kind のスキームの計算 (`residual`) が、鎖の長さの2乗の時間がかかる。書き出しの要約 (`kind/summary.rs`) の費用ではない
- 直前のラムダを捕まえるラムダの鎖 (`let q{i} = fn x -> q{i-1} x`) は、型の書き出しより前の段階で2乗の時間がかかる。どのパスが原因かは調べていない

### 実行の費用

- 操作は evidence passing で実装し、evidence を `evv` の先頭から探す ([LIR](../spec/lir.md) の「evidence を探す」)。探す費用は今有効な handler の数に比例し、呼び出しの深さにはよらない。すぐに再開する操作 (状態の `get` と `put` など) は、その場で節を実行するので、yield も継続の組み立てもない。ただし、再帰の各段で別のエフェクトの handler を設ける形では、`evv` の長さが深さに比例するので、1回の探す費用も深さに比例する。探す場所ごとのキャッシュか静的な添字で解く ([ロードマップ](../future/roadmap.md) の「処理系」)
- すぐに再開しない操作は handler まで yield し、戻る途中の各段が自分の続きを継続に足す。1回の yield の費用は、操作と handler の間のフレームの数に比例する。これは方式の性質である
- 中断 (`never` の操作、`try_io` が捕まえる IO の失敗、`exit`) は継続を組み立てず、タスクのスタックを巻き戻す ([VM](../spec/vm.md) の「巻き戻し」)。費用は外すフレームの数と手放す値の数に比例する。`IO` と `never` の操作だけのエフェクトは yield の地点に数えないので ([型と Kind](../spec/types.md) の「row の具体化の記録」)、`<IO>` の中で呼んだ総称な handler は、`println` を呼ぶコールバックを渡しても yield しない。外側にユーザーのエフェクト (`State` など) の handler がある所でも、純粋なコールバックを渡せば yield しない。呼び出し先の row が今の row を受けず、どの引数からもエフェクトが入らなければ `<>` に閉じるためである ([型と Kind](../spec/types.md) の「呼び出しの包含を解く」)。ただし、`Yield a` を `Yield b` に変える handler の `yield` の節は、節が外側の `yield` を起こすので、1要素ごとに yield する ([計測の基準](benchmarks.md) の「`Yield` を変える handler の yield」)
- すぐに再開する操作は、1回の操作で、evidence を探す基本操作とエントリを読む基本操作、エントリの関数の呼び出し、状態の出し入れの基本操作、yield の確認を実行する。節の本体が何かを呼ぶときは、エントリの under の基本操作 (`evv_get`、`ev_evv`、`evv_set` と、戻す `evv_set`) も加わる ([Core IR](../spec/core-ir.md) の「`perform` と節の3つの形」)。
- yield しうる要素を n 個並べたリストのリテラルでは、yield の地点ごとの遅い道が生きている変数を捕獲するので、遅い道の捕獲の大きさが n の2乗になる ([Core IR](../spec/core-ir.md) の「継続の関数」)
- `++` は一意な左辺をその場で伸ばすが、先頭に足す連結 (`"x" ++ acc`) と、共有された左辺への連結は、両辺を写した新しい文字列を作る。ループで先頭に足していくと、伸びていく右辺を毎回写すので、全体で2乗の時間がかかる ([オブジェクトモデル](../spec/object-model.md) の「文字列の操作」)
- 末尾の位置の呼び出しは、結果の Repr が呼び出し元の結果と互換なときだけ末尾呼び出しになる。結果が型どおりの関数が結果が一様な関数を末尾で呼ぶ所ではフレームが1つ残るが、結果が一様なコードは変種への末尾呼び出しで型どおりのコードへ戻らないので、末尾の位置の呼び出しの鎖ごとに残るフレームは多くとも1つである ([LIR](../spec/lir.md) の「末尾の位置の呼び出し」)。`mask` 付きの呼び出しはフレームを残すが、その数は handler の数で抑えられる
- 呼び出しの値が、文を持つ合流のブロックを通って `return` に届く形 (`let r = if n == 0 then 0 else f (n - 1) in let s = "unused" in r`) は、末尾の位置の呼び出しでなく、末尾呼び出しにならない。translate の転送は文のない合流のブロックにしか効かないので、この形のループは反復の数に比例してフレームを積む。縮約が使われない `let` を消した後で、`return` だけになった合流のブロックへの転送をやり直せば、保証の範囲を広げられる
- eml で1バイトずつ回すループは、Python が C で書かれた組み込みの関数に任せる処理より遅い。`Stdin.read_line` や `String.find_from` のような extern に任せられる処理は速い ([計測の基準](benchmarks.md) の「1バイトずつ回すループ」)
- 末尾でない再帰の呼び出しをまたいで token を持つと、フレームごとに token のスロットが1つ増えるので、深い末尾でない再帰ではスタックの上限に早く当たる ([計測の基準](benchmarks.md) の「reuse の token で増えるスタック」)
- 関数の ABI は型と `split` で決まる ([LIR](../spec/lir.md) の「関数の ABI」)。関数の値は捕獲より後ろの引数と結果がいつも一様なので、特殊化されなかったクロージャに `Float`、`Int`、構造体を渡すと、`apply` のたびに引数と結果を箱に入れる。値として使う型どおりの関数は、一様な変種 `<f>$uniform` を通る。`split` を持つ関数の一様な位置 (ラムダの引数と結果、handle の本体の結果、節の `k` と状態と結果、値の参照が残る継続の関数の再開の値と結果など) を直接呼ぶ所でも、値を箱に入れて受け渡す。`optimize` の後に値の参照の残らない持ち上げた関数と、handler の特殊化が `k` として渡す継続の関数は、型どおりの ABI のまま直接呼ぶ ([Core IR](../spec/core-ir.md) の「直接だけ呼ばれる持ち上げた関数」)
- 物体の引数を読むだけの関数は、引数を借りて受け取る。呼んだ側は呼び出しの間も参照を持ち続け、どちらの側もRCの命令を足さない ([LIR](../spec/lir.md)の「借用の推論」)。印は関数ごとに、その関数から直接の呼び出しで届くコードから推論する。消費する引数、閉包が捕まえる引数、コンストラクタに入れる引数、reuseの元になる引数は所有のままで、`apply`、節、ランタイムから入る関数の引数も所有である。借用の引数を持つ関数への値の参照は、すべての引数を所有にした完全な写し`<f>$owned`へ向くので、そうした関数はコードを2つ持つ ([LIR](../spec/lir.md)の「関数の ABI」)。呼び出し元が所有する値を貸す末尾呼び出しは、呼ばれる関数が`tail apply`と`tail clause`に届かなければ普通の呼び出しになり、フレームを1つ残す。1つの呼び出しの鎖でこれが足すフレームは、関数の数で抑えられる ([LIR](../spec/lir.md)の「末尾の位置の呼び出し」)。貸した値は呼び出しが戻った後で呼び出し元が手放すので、呼ばれる関数が消費していたときより少し長く生きることがある
- 小さな単一コンストラクタの型とタプル (葉が 6 語以下で、再帰しない型) は、ヒープの箱でなく葉の値の並びとして持つ ([Core IR](../spec/core-ir.md) の「構造体にする型」、[LIR](../spec/lir.md) の「構造体の形」と「開く」)。再帰する宣言の組に入る型 (`Iter` など)、葉が 6 語を超える型、葉に一様な位置の型変数 (`_`) を含む型は、ヒープの箱である。配置で構造体の型を宣言したフィールドと、配列の要素の構造体も箱に入れて置くので、配列に構造体を書くたびに箱を作る ([計測の基準](benchmarks.md) の「配列に書く構造体の箱」)
- 構造体の値を一様な位置へ渡すたびに、箱を作る。`(Int, Float)` のように型変数のフィールドを持つ配置の箱では、スカラーの葉も箱に入る。同じ関数の中で箱から取り出した葉をそのまま箱に戻す形は、葉の単位の覗き穴で箱を作り直さない ([LIR](../spec/lir.md) の「開く」)

### 最適化が扱わない形

- 末尾再帰の生産者を `Iter.generate` で引く形は yield せず、`Iter.fold` で引く形と `Iter.map` を重ねた形は1要素ごとに何も確保しないループになる ([Core IR](../spec/core-ir.md) の「`con` の値の形についての特殊化」)。`Iter.filter` を重ねた形では、残した要素ごとに `Some`、組、`Iter`、閉包の4つの確保が残る。`filter` が要素を飛ばすループは自分を呼ぶ複製になり、展開しないので、作って返す `Some ((x, Iter k))` が相手の分からない呼ぶ側に届くためである。これをなくす特殊化は [ロードマップ](../future/roadmap.md) の「処理系」の「Core IR の上の最適化パス」にある
- 既知の閉包への特殊化 ([Core IR](../spec/core-ir.md) の「最適化」) は、同じ関数の中で `closure` か `&f` で束縛した閉包、複製が捕獲に開いた引数の組、同じ関数の中の既知の `con` のフィールドの閉包 (展開した関数が作った `con` を含む)、既知の閉包をフィールドに持つ `con` を受ける関数の複製だけを既知とする。ブロック引数を通って合流する閉包、データに入れてから取り出した閉包、展開されない関数が返したデータや相手の分からないデータの中の閉包は特殊化しない
- 呼び出し先がすでに特殊化した複製である呼び出しは、それ以上特殊化しない。鍵の入れ子の深さが上限 (`MAX_KEY_DEPTH`) を超える位置でも、閉包を作り直して渡す
- 一様な instance (`_:poly`) の中では、既知の閉包への `apply` でも、`unr` でない引数を箱を要する Repr (スカラーと構造体) の引数で受ける閉包の関数には、直接の呼び出しにしない。値が型変数の印 (`poly`) のままで、直接の呼び出しにすると Perceus が `poly` の値を `decref` で手放すことになるためである ([LIR](../spec/lir.md) の「Kind の印の検査」の V1)
- 展開するのは、直接の呼び出しの関係で再帰せず、書き換えた後の文の数が上限以下で、`handle`、`resume`、`clause` を持たない関数だけである ([Core IR](../spec/core-ir.md) の「インライン化」)。末尾でない再帰に小さな関数を展開すると、1つのフレームの slot が増え、スタックの上限に早く当たる ([計測の基準](benchmarks.md) の「インライン化で増えるスタック」)
- reuse ([LIR](../spec/lir.md) の「reuse」) は関数の中で閉じる。token を呼び出しの引数や戻り値で渡さないので、節を作るのが呼び出し先である形は reuse しない。Okasaki の書き方の赤黒木 (`balance c l k r`) がこれに当たる。`balance` が組み直す節を受ける形 (`bench/rbtree.em`) に書けば reuse する。token は合流するブロックにも渡さないので、`release` と `con` の間に合流があると組にならない。どちらも [ロードマップ](../future/roadmap.md) の「処理系」の Perceus の最適化にある
- handler への特殊化 ([Core IR](../spec/core-ir.md) の「handler への特殊化」) は、handle の本体が既知の関数のときだけ動く。本体がブロック引数を通って合流する閉包か、データに入れてから取り出した閉包なら、handle は EP の道に残る
- 末尾でない再帰の生産者 (木の走査など) は特殊化しない。`perform` までの経路に末尾でない `yields` の呼び出しがあるためである。特殊化するには、フレームごとに継続を作って渡す CPS の拡張が要る ([ロードマップ](../future/roadmap.md) の「処理系」の「Core IR の上の最適化パス」)
- データを通って届く生産者も特殊化しない。handle の本体がデータから取り出した生産者を呼ぶ形は、相手の分からない `yields` の `apply` になり、経路は末尾の位置の既知の直接の呼び出しだけでたどるためである
- 経路に `mask` の付いた呼び出しか入れ子の `handle` があるとき、操作がすべて `never` のエフェクト (`Std.Fail`、`IO`) の handle、`never` の操作を持つエフェクトで、付け直す前の印が立った `yields` でない関数を経路から呼ぶときも、handle は EP の道に残る。本体に `handle` を持つ節は、節の写し `<g>{k}` を呼ぶ形で特殊化するが、写しの中の `handle` は特殊化しない
- 融合 ([Core IR](../spec/core-ir.md) の「融合」) が扱わない形がある。2つの `con` を合流させるブロック引数を通って届く閉包と `con` は、捕獲が違えば既知にならず、`Iter.fold` は総称な複製に戻る。`Iter.take` の中の `Iter` は、`drop` する枝と `next` で引く枝の2か所で使うので開かず、`take` を重ねた形は一部しか融合しない。2つの関数が互いの閉包を返す `Iter` (2つ以上の複製の輪) を引く `Iter.fold` は、輪の複製を展開しないので融合しない ([Core IR](../spec/core-ir.md) の「融合が止まること」)。定数どうしの extern の比較 (`1 > 10`) は畳まないので、展開した後の `switch` も残る

### 型と row の推論

- ローカル関数は単相で、多相にできない。シグネチャと結果の型の明示もない (引数の後の `:` は誤りになる)。相互再帰は `let … and …` の組で書く ([ロードマップ](../future/roadmap.md) の「言語機能と構文」)
- `FileIn` を捕まえた再帰するローカル関数が、自分を呼ぶ前に `multi` の操作を起こすと、E3001 のほかに、その関数に冗長な E3006 も出る。`let … and …` の組でも同じで、組の関数が `multi` の操作を起こしてから組の別の関数を呼ぶと、呼ばれる関数に冗長な E3006 が出る (`and b n = if choose () then a n else a n` の `a`)。自分への参照がネストしたラムダの中にだけある場合、E3001 はそのラムダを指す
- 持ち越し規則は、`handle` の外側の row と `k` の呼び出しの row (handle の外側の row) を、handle の位置の今の row とみなす。そのため、今の row に `multi` があれば、本体が `multi` の操作を起こさなくても、handle をまたぐ `FileIn` が拒否される。健全な側に外れるだけである。本体の中の呼び出しの row から handle の本体が起こすエフェクトを集めれば、外れなくなる
- 高階のローカル関数の結果を射影する形は E2013 になる。`<State Person>` の本体で `let run h = h ()` を `(run get).name` と使うと、`run` の row に `State` が入るのは本体の後で包含を解くときなので、射影の時点では `get` の結果の型が決まっていない。結果の型を明示すれば通る (`(run get : Person).name`)。呼び出しの包含は、包含を積む時点で分かっているラベルの型引数だけをその場で単一化する ([型と Kind](../spec/types.md) の「推論」)
- 同じローカル関数を、row が閉じた別々の関数型の位置に値として渡す形 (`Int -> <IO> Int` の引数と `Int -> Int` の引数の両方に渡す) は、E2001 か E2002 になる。値の参照が関数の row 変数を共有するためで、単相のローカル関数の制限である ([型と Kind](../spec/types.md) の「ローカル関数の推論」)
- 有限の解のない組や呼び出しの輪の E2002 は、本体の大きさに比例する手間をかけてから見つかる。ラベルの固定点の上限を本体全体の包含のラベルで数えるので、輪は上限に達するまでラベルを足し続けるためである ([型と Kind](../spec/types.md) の「呼び出しの包含を解く」)。足したラベルは今の row に残るが、E2002 の文言はエフェクトの名前を1回ずつ書く
- 同じ SCC の中では、上限を SCC 全体の解の上で確かめるので、E2020 が上限を破った呼び出しとは別の呼び出しに出ることがある。`h : Bool -> (Unit -> <never e> Int) -> <IO | e> Int` の本体 `h b f = if b then f () else h True (fn () -> 1) + h True noisy` では、上限に収まる row を渡した1つ目の `h` にも出る。SCC の外の `helper : (Unit -> <never e> Int) -> <e> Int` から上限を推論する関数の輪では、輪の中で `noisy` を渡した呼び出しには出ず、輪の中の `helper g` の呼び出しに出る。どちらでも、上限を破った呼び出しは「`IO` is performed here」の secondary に現れる ([型と Kind](../spec/types.md) の「推論」)。handler の節で操作の引数の row を上限を持つ宣言に渡す形 (`| fn ask g -> helper g`) は、E2020 のほかに E2002 も出る ([診断の出し方](diagnostics.md) の E2020)
- 射影と更新は、その時点で分かっている型からフィールドを引く。呼び出しの引数のラムダは後で検査するが、後に回したラムダどうしは左から検査するので、後のラムダが前のラムダの引数の型を決める形 (`compose (fn b -> b.name) (fn p -> p.boss) x`) は E2013 になる。`let` で束縛したラムダの中の射影 (`let f = fn p -> p.name`) も、`f` を呼ぶ前に検査するので E2013 になる。どちらも引数の型を明示して書く ([型と Kind](../spec/types.md) の「引数のラムダを後で検査する」)

### 型クラス

- メソッドの呼び出しの評価の順は、メソッドのシグネチャの矢印の数で決まり、instance の等式の引数の数によらない ([式](../spec/expressions.md) の「関数適用」)。どの instance を選んでも同じ順になるようにするためである。そのため、等式の引数が矢印より少ない instance のメソッドは、等式の引数で呼んだ結果に残りを `apply` する。等式の数の引数で1回に呼ぶ形より、呼び出しが1つ増える
- 一様な位置のグラフで、関数の参照の制約を解いた先は、instance のすべてのメソッドへ辺を引く。既定のメソッドに解決したメソッドの参照も、根の制約の instance の節点へ辺を引く ([Core IR](../spec/core-ir.md) の「一様な位置と制約付きの多相再帰」)。どちらも保守的なので、E2012 が偽の循環を報告しうる。クラスの制約を使わない既定のメソッド (`differ x y = True`) を、instance のメソッドが大きな型で呼ぶ形 (`same (Box x) (Box y) = differ (Box (Box x)) (Box (Box y))`) は、単相化が止まるのに E2012 になる。既定のメソッドの本体が使う証拠を辺に写せば、偽の循環を減らせる
- 演算子のメソッドの等式で、被演算子に括弧のないコンストラクタのパターンを書くと (`Box x == Box y = …`)、等式として読まれない。演算子の等式の先読み (`grammar/items.rs` の `at_operator_equation`) が、演算子の前に単純なパターン (`patterns::apat_len`) を1つしか認めないため。パーサは「expected a method equation」(E0011) を出す。そのメソッドに既定のメソッドがなければ、等式がないことの E1036 も続く。`(Box x) == (Box y) = …` と括弧に入れて書く。手で書く instance ではこの形がよく現れる
- 導出した instance のフィールドの E2006 は、instance ごとに最初のフィールドの1つだけを報告する。ほかのフィールドの誤りは、最初の誤りを直した後に出る
- instance のメソッドの関数と生成した関数の名前は、クラスと型を診断と同じ表示で書く。そのため、別のモジュールが同じ名前の型を足すと、Core IR の関数の名前が変わることがある

### `par`

- `par` の右の子をほかの worker で動かすのは、空いている worker があるときだけである。fork は右の子を空いている worker の受け箱に置き、なければ親の worker の上で切り替えて動かす。盗みはなく、置く粒度の下限もない ([VM](../spec/vm.md) の「`par` の fork と join」、[ロードマップ](../future/roadmap.md) の「マルチコア対応」)
- REPL と、セッションの根を持つ run は worker 1つで動くので、そこでは `par` で計算は速くならない。ライブラリの既定も worker 1つである ([VM](../spec/vm.md) の「タスクとスケジュール」)
- 捕まえられなかった子の `Panic` の実行時エラーは、``panicked: <message> in `reperform$Prelude.Panic.panicked` `` の文言で、ソースの位置を持たない。子の `panic` を親で起こし直す関数の中で報告するためである ([VM](../spec/vm.md) の「子の誤り」)

### `Async`

- driverの操作 (受け付け、待ち、取り出し) を使うのは`Std.Net`、`Std.Stream`、`Std.Exec`と、`blocking`のCの関数の呼び出しだけである。`Std.Stream`が開くのは通常のファイルだけで、子のプロセスのpipeは`Std.Exec`が作る。stdの今の読み書きの行 (`FileIn`、`Stdin`、`Proc`など) はタスクの中でその場で動き、終わるまでworkerを持ち続ける。その間、同じworkerに置いたタスクは待つ。workerが2つ以上あれば`start`はworker 0にタスクを置かないので、`main`の同期の読み込みの間もタスクは動く。ただし、worker 0のdriverが受け持つI/O (`main`が作ったソケットと`Std.Stream`のファイル、仮想の時計ではすべてのI/O) は、読み込みが戻るまで待つ。タスクの中では`Std.Stream`、`Std.Exec`、`compute`、`blocking`を使う。その場の読み込みが同じworkerのタスクを通してしか終わらないプログラムは止まったままになり、処理系はそれを見つけない ([標準ライブラリ](../spec/stdlib.md)の「`Std.Net`」、「`Std.Stream`」、「`Std.Exec`」)
- `Std.Net` に `resolve` と `write_within` はない。`listen` は名前を受けず、書きの期限は `Async.timeout` で付ける ([ロードマップ](../future/roadmap.md) の「非同期」)
- `Std.Tls`に期限を持つ読み (`_within`) とserverのAPIはない。期限は`Async.timeout`で付け、期限が来ると`Stream`を失う。信頼する根はOSの根と、渡したPEMのCAである。Linuxの最小のコンテナのようにOSの根のファイルがない所では、プログラムがCAを渡す。WindowsのOSの検証器はEd25519を扱わないので、Ed25519の鍵を持つサーバーの証明書を拒む。macOSでも同じだという報告がある ([標準ライブラリ](../spec/stdlib.md) の「`Std.Tls`」、[ロードマップ](../future/roadmap.md) の「非同期」)
- `Std.Http` に keep-alive、HTTP/2、gzip などの coding、プロキシ、cookie、ストリーミングの本文、本文の大きさの上限、IPv6 のアドレスの URL はない。ヘッダーと trailer の行の数にも上限がなく、期限は `Async.timeout` で付ける。本文を書き終える前に閉じた相手の応答は読めない ([標準ライブラリ](../spec/stdlib.md) の「`Std.Http`」、[ロードマップ](../future/roadmap.md) の「非同期」)
- 本物の driver は、OS を呼ばない核と、自前の OS ごとの backend (Linux は epoll、Apple は kqueue、Windows は IOCP) からなる。Windows の IOCP の backend もソケットを扱い、本物のソケットのテストは3つの OS で流す。本物の driver の VM のテストは Rust のテスト (`eml_io` と、`crates/eml_vm/tests/` の `api.rs` と `signal.rs` と、`Std.Tls` の `crates/eml_cli/tests/tls.rs`) で書き、UI テストはいつもテストの driver の上で流す ([VM](../spec/vm.md) の「時計と driver」)
- Windows の `eml` は `rustyline` が使う `clipboard-win` を通して user32.dll を link する (ソースで確かめた。実行ファイルの import は確かめていない)。user32.dll を読み込んだプロセスには、Windows 10 までは OS がログオフとシャットダウンの事象を送らないので、`Terminate` を起こすのはコンソールを閉じることだけである。本物の閉じる事象を送るテストはなく、handler の判断の関数と返らないことを単体テストで確かめる ([VM](../spec/vm.md) の「上限と割り込み」)
- 本物の driver の epoll と kqueue の backend は、ソケットを1回だけ edge で登録して以後変えない。1回の待ちは、知らせのあった操作とスレッドの答えが当たった操作にだけ触れるので、その手間は保留の操作の数によらない ([VM](../spec/vm.md) の「時計と driver」、[計測の基準](benchmarks.md) の「driver の syscall の目標」)
- 監視のスレッドを始められなかった眠る driver の run では、VM が1回の待ちを 10 ms で切って止める語を読む。その run では Ctrl-C の後に driver を起こす道がないためで、眠る worker に Ctrl-C と確保量の上限が届くまで、10 ms と OS の遅れだけかかる ([VM](../spec/vm.md) の「時計と driver」)
- 本物のdriverはブロックする仕事のプールを持ち、driverごとに32本までのスレッドで、`Std.Stream`のファイルの読み書きと、`Op::Job` (`blocking`のCの関数の呼び出しと、`Std.Tls`の接続の設定とhandshakeの押し込み) を動かす。同時に走る`Op::Job`は24までで、すべてのスレッドが仕事中なら、仕事はdriverの中の列で待つ ([VM](../spec/vm.md) の「時計と driver」)
- 時間は `sleep_ms`、`timeout`、`race`、`drain_within` で扱う。`eml run` と `eml repl` はいつも本物の時計で動く。テストとライブラリの既定は仮想の時計で、すべてのタスクが待つときだけ進む ([標準ライブラリ](../spec/stdlib.md) の「時間」、[VM](../spec/vm.md) の「時計と driver」)
- 取り消しは協調的で、取り消しの地点 (`await`、`yield_now`、`sleep_ms`、`drain_within`、`check_cancelled` と、`Std.Chan` の `send`、`recv` と、`Std.Net` と `Std.Stream` と `Std.Exec` の待つ関数と、`blocking` の C の関数の呼び出しと、仕事が終わってから届く `compute`) でだけ届く。読み書きの extern と `par` の子は取り消しの地点でない ([標準ライブラリ](../spec/stdlib.md) の「取り消し」)
- `compute` の仕事は、空いた worker があるときだけほかの worker で動く。呼んだ worker の実行待ちに積んだ仕事は、後でほかの worker が空いても動かない。`par` の右の子と同じ制限で、盗みを入れる段で解く ([VM](../spec/vm.md) の「`compute` の仕事」、[ロードマップ](../future/roadmap.md) の `par-steal`)
- `compute` の仕事は取り消せない。取り消しは仕事が終わってから届くので、`timeout` で囲んだ `compute` は期限が過ぎても仕事が終わるまで戻らず、仕事の後で `None` を返す。`race` で負けた腕の仕事も最後まで動く ([標準ライブラリ](../spec/stdlib.md) の「`compute`」)
- `compute` の仕事の中の、`Panic` でない実行時エラーは、呼んだタスクの `try_all` も捕まえず、その場で run を止める。捕まえない `Panic` は、呼んだタスクの `Panicked` になる ([標準ライブラリ](../spec/stdlib.md) の「`compute`」)
- タスクの中の `par` は、`compute` と違ってスタックを持ったまま子を待ち、その間は取り消しの地点でない ([VM](../spec/vm.md) の「待ちの2種類」)
- タスクの中で `Async.run` を入れ子に呼ぶと、中のスコープのタスクがすべて終わるまで、呼んだタスクはスタックを持ったまま待ち、その間は取り消しの地点でない。中のタスクには取り消しが届く。`shield` の中では、入れ子の `run` の根が境目になる ([VM](../spec/vm.md) の「待ちの2種類」)
- `shield` の中で自分の属するスコープを `drain_within` で待つなど、自分を待つ形は、ほかに進むものがなくなったときにだけ実行時エラー `deadlock` で止まる。ほかのタスクが動くか眠る間は、互いを待つタスクも待ち続ける ([VM](../spec/vm.md) の「時計と driver」)
- `race` の勝ちは、run に1つの登録表の lock の下で、1つの行が決める。同時に終わる2つの腕のどちらが勝つかは、スケジュールと腕を置いた worker で決まる ([標準ライブラリ](../spec/stdlib.md) の「時間」、[VM](../spec/vm.md) の「セルと登録表」)
- タスクは置いたworkerの上で終わるまで動く。置いた後にworkerの間で仕事を動かすものはない。タスクは`Async`の操作でだけ切り替わり、CPUを使うタスクやその場の読み込みは、待つか戻るまで同じworkerのほかのタスクを待たせる。`compute`はforkのときにCPUの仕事を空いたworkerへ移し、空いたworkerがなければ呼んだworkerで動かす。タスクを置くworkerは順に回すだけで、workerの混み具合を見ない ([ロードマップ](../future/roadmap.md)の「マルチコア対応」、[ロードマップ](../future/roadmap.md)の`par-steal`)
- 仮想の時計では、同じ期限のタイマーの worker をまたいだ順は決まらない。区画ごとに持ち主の worker が自分の集める地点で外すためである ([VM](../spec/vm.md) の「時計と driver」、[ロードマップ](../future/roadmap.md) の「マルチコア対応」)
- worker をまたぐ容量 0 の channel の往復は、1つの worker の上の往復より費用が大きい。4つの worker で往復だけを繰り返すプログラムは、1つの worker より遅い ([計測の基準](benchmarks.md) の「worker の数と実時間」、[ロードマップ](../future/roadmap.md) の「マルチコア対応」)
- スコープの失敗は、終わった順で最初のものだけを `run` と `scope` が起こし直す。2つ目からは報告せず、`RunStats.dropped_failures` に数えるだけである
- 操作の予算を使うのは、`check_cancelled` と、`Std.Chan` の速い道と、`Std.Net` の待ちである。`Std.Net` の待ちは操作が終わっていたかによらず1回ごとに使い、書きの待ちは長さに応じて多く使う。64 KiB の書きは1回で予算を使い切る。`try_send` と `try_recv` は使うだけで譲らない ([標準ライブラリ](../spec/stdlib.md) の「操作の予算」)
- 閉じない channel は run の終わりまで登録表に残り、残った品物もそのときまで落とさない (`RunStats.channels_swept`。[ロードマップ](../future/roadmap.md) の「非同期」)
- `Counter` の表は `Machine` が持ち、放さない ([標準ライブラリ](../spec/stdlib.md) の「`Std.Atomic`」、[ロードマップ](../future/roadmap.md) の「非同期」)
- `Async` のタスクの中でも、`par` の子 (左も右も) の深さの超過は run を止める。子の row は `IO` を含められず、`ResourceLimit` を起こす先がないためである。子の中で受けたタスクの上限の印は、join の後の親で起こす ([VM](../spec/vm.md) の「タスクの上限」、[ロードマップ](../future/roadmap.md) の「非同期」と「マルチコア対応」)
- メモリの柔らかい上限が選ぶタスクは、原因のタスクとは限らない。数えるのはタスクが動いている間に取ったページだけで、手放しても減らない。待っているタスクと、本当の待ちから起きてまだ動いていないタスクは候補にならない。`par` の右の子と `compute` の仕事の確保はどのタスクにも数えないので、そうした増え方は硬い上限で run を止める。柔らかい上限と硬い上限の間の1回の大きな確保も run を止める。印が失われると、次の閾値まで確保が増えてから印が付く。監視のスレッドを始められなかった run には柔らかい上限がない ([VM](../spec/vm.md) の「タスクの上限」、[ロードマップ](../future/roadmap.md) の「マルチコア対応」)
- 上限の近くで待ちに入ったタスクを再開すると、再開の後の深さは待ちに入る前より大きくなる。深さが上限と猶予 (2^18 語) の和を超えると、タスクの失敗でなく run の誤りで止まる。上限との間なら、論理的な深さが上限に届いていなくても `the call depth limit is exceeded` の `ResourceLimit` が起きる ([VM](../spec/vm.md) の「タスクの上限」、[ロードマップ](../future/roadmap.md) の「非同期」)
- `IOErrorKind` の `Cancelled`、`TimedOut`、`ConnectionReset`、`AddrInUse`、`ResourceLimit`、`ConnectionRefused` を出すのは `fail_io` と、`Async` のタスクの上限の `ResourceLimit` だけで、extern の誤りはこれらの種類を出さない ([標準ライブラリ](../spec/stdlib.md) の「`try_io`」と「タスクの上限」)

### メインのworker

- `END`は動いている固定したタスクを止めない。runが終わるのは、タスクが安全点に着くか、そのexternかツールキットのコールバックが戻ったときである。モーダルなループに留まる`Park`や、releaseのビルドで`Progress::Stopping`を無視する`Park`は、runの最後の終わりだけを遅らせる ([VM](../spec/vm.md)の「メインのworkerの待ち」)
- 仮想の時計の時間は、メインのworkerを含むすべてのworkerが眠っている間だけ進む。動いている固定したタスクは、ほかの動いているタスクと同じく時計とデッドロックの判定を止める。本物の時計で待つのは判定だけである ([VM](../spec/vm.md)の「時計と driver」)
- メインのworkerの同期の読みには`EINTR`が届かない ([VM](../spec/vm.md)の「上限と割り込み」)
- メインのworkerは中継したシグナルを届けない。runの全体の務めで、mailから取り出すのはworker 0..N-1である。`--workers 1`では、待つタスクが固定したものかによらず、シグナルが届くのはworker 0が集める地点に着いたときだけである。`main`がその場の読みで止まっている間に来た2回目のシグナルでは、ハンドラがその規則でその場でプログラムを終わらせる (`_exit(130)`、`_exit(143)`。[VM](../spec/vm.md)の「メインのworker」、[標準ライブラリ](../spec/stdlib.md)の「`Std.Signal`」)
- `eml run`はGUIの`Park`を置き、それはプログラムがGUIを求めるまで`DefaultPark`と同じに動く。ライブラリのAPIから渡す、替える`Park`の下で固定したタスクのI/Oが終わるのは、`Park`が`default_wait`で待つか、`Pending::io()`の源 (unix) か10 msの上限 (Windows) で待って自分で`run`を呼ぶときである ([VM](../spec/vm.md)の「メインのworkerの待ち」)

### GUI

- ネイティブのbackendの画面は、3つのOSのどれでもまだ手で確かめていない。CIはネイティブのbackendを組んでつなぐが、テストはどれもstubで流す ([テスト戦略](testing.md)の「GUIの手での確認」)
- WinUIのbackendは、Windows App SDKのruntimeを入れたWindowsで動かしたことがない。CIのrunnerにはruntimeがない。そのため、bootstrapが入れてある2.6.4以降のruntimeを見つけるかは確かめていない ([配布とGUIの前提](distribution.md)の「Windows App SDKのruntime」)
- WinUIの`Application`を作れなかったときに、`EnqueueEventLoopExit`で`Application::Start`を戻す道は試していない
- `eml run`の外のプロセスで、ネイティブのGUIをrunをまたいで開くこと (`Machine`のAPIのhost) は試していない。WinUIのbackendは、Rustのruntimeが`main`と名づけたスレッドでだけ開く。AppKitとQtのbackendは、プロセスの最初のスレッドで開く ([コンパイラの構成](architecture.md)の「`eml_gui`の内部」)
- ウィンドウのネイティブの最小の大きさは初めて表示するときに決め、後で中身が増えても変えない
- 拡張の継ぎ目はcfgのfeatureで、`Plugin`のtraitはない ([FFI と拡張の設計](../future/ffi.md)の「拡張」)
- `eml repl`ではGUIを開けず、`Gui.run`は`NotAvailable`で失敗する ([ロードマップ](../future/roadmap.md)の「REPL」)
- GUIのタスクが待つタスク (`await`するタスクや、`app`の中の`Async.run`の本体) から満ちたchannelへの`Gui.post`は、いつまでも待つ。GUIが開いている間はrunの外の待ちの印が立つので、デッドロックとしても報告しない ([標準ライブラリ](../spec/stdlib.md)の「`Std.Gui`」)

### 子のプロセス

- `Proc` の `Child` は、1度に1本の pipe しか読み書きしない。読みながら書く子に pipe の buffer より多く書いてから読むと、両方が止まる。処理系はこの状態を検出せず、Ctrl-C で終える。守る順は [標準ライブラリ](../spec/stdlib.md) の「`Child`」にある。標準入力と標準エラーの pipe を別のタスクで読み書きするには `Std.Exec` を使う
- `Std.Stream` は通常のファイルしか開かず、seek も、位置を指定する読み書きも、読み書き両用の開き方もない。ファイルの読み書きはプールのスレッドで動き、取り消しても終わるまで続く。`Writer` は buffer を持たず、`flush` もない ([標準ライブラリ](../spec/stdlib.md) の「`Std.Stream`」)
- `Std.Exec` は、仮想の時計 (UI テストの既定) では子を起動できず、本物の時計の run でだけ動く。pidfd のない Linux (5.3 より前のカーネル) では `wait` が `NotAvailable` で失敗する。パイプラインの途中の段の標準エラーは読めない ([標準ライブラリ](../spec/stdlib.md) の「`Std.Exec`」)
- 手放した `Child` の破棄処理が kill するのは直接の子だけである。`sh -c` が起動した孫は残りうる
- `Cmd` のコンストラクタとフィールドは公開されている。`Std.Proc` が別のモジュールから作って更新するので、抽象型にしていない。`Cmd { … }` の作る式とパターンを書かない決まりで守る ([標準ライブラリ](../spec/stdlib.md) の「`Cmd`」)

### C の関数の呼び出し

- 1つの driver で同時に走る `blocking` の呼び出しは24までで、ファイルの読み書きのために8本を残す。戻らない呼び出しを run の終わりに待つ予算は、run 全体で1秒である。どちらも測らずに決めた値で、`eml` からは変えられない ([宣言](../spec/declarations.md) の「`extern "C"`」、[ロードマップ](../future/roadmap.md) の「非同期」)
- 走っている`blocking`の呼び出しは止められない。戻らない呼び出しを抱えたrunは、`exit`やCtrl-Cで止めると予算だけ終わりが遅れる。その後runはほかと同じく終わってヒープを放すが、呼び出しはdriverの`Op::Job`のスレッド1本と、引数の写しと、大きい引数のページを持ち続け、そのハンドルのdtorは呼び出しが戻ったときに走る。1つのdriverで24の呼び出しを見捨てたREPLのセッションでは、後の`blocking`の呼び出しはどれも列で待つ ([VM](../spec/vm.md)の「時計と driver」)
- 失敗したスコープの中の `blocking` の呼び出しは、取り消されても C の関数が戻るまで待つ。戻らない呼び出しを抱えて失敗した `run` は戻らず、Ctrl-C でしか終わらない。Ctrl-C の後は予算が効く ([宣言](../spec/declarations.md) の「`extern "C"`」)
- 仮想の時計の run (テストとライブラリの既定) では、`blocking` の仕事は worker 0 の上でその場で動く。戻らない呼び出しは `exit` でも Ctrl-C でも止まらず、run は終わらない。ほかのタスクを待つ C の関数も、worker 0 を止めたまま戻らない ([宣言](../spec/declarations.md) の「`extern "C"`」、[テスト戦略](testing.md) の「UI テスト」)
- コールバック、C の構造体、可変長の引数は渡せない。型の対応は表にある形だけである ([FFI と拡張の設計](../future/ffi.md) の「未決の論点と将来の拡張」)
- C に貸せる配列は、要素が 64 bit の `UnboxedArray` と `MutUnboxedArray` (`Float` と `Int`) だけである。`float` や 32 bit の整数の buffer を求める API (LP64 の LAPACK など) は呼べない
- 橋渡しのコードは `MAP_JIT` を使わずに確保するので、macOS の hardened runtime の下では C の関数を呼べない。処理系は hardened runtime を扱わない ([VM](../spec/vm.md) の「橋渡しの関数」)

### Windows

- 待っている読み (標準入力、`FileIn`、`Child`) は Ctrl-C で止まらない。コンソールの標準入力の読みは Ctrl-C で戻るが、std がその場で読み直す。pipe とファイルの読みは Ctrl-C で戻らない。止める語を見るのは、データが来るか相手が閉じて読みが戻った後である。待っている読みからは `eml run` の2回目の Ctrl-C で抜けられる ([VM](../spec/vm.md) の「上限と割り込み」)。std を通さない読み手は入れていない ([ロードマップ](../future/roadmap.md) の「マルチコア対応」)
