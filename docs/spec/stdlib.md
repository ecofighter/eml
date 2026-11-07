# 標準ライブラリ

位置づけ: 規範。

スクリプトの起動と、標準ライブラリの関数を定める。モジュールの置き場所と読み込みの規則は [モジュールと名前解決](modules.md) の「標準ライブラリ」に、extern の実装は [オブジェクトモデル](object-model.md) の「extern」にある。

## 起動

```sh
eml wc.em -l input.txt          # eml run wc.em -l input.txt と同じ
eml run --debug-heap wc.em -l input.txt
eml run --workers 4 wc.em -l input.txt
eml run --gui stub counter.em
eml run --link openblas=/opt/lib/libopenblas.so solve.em
./wc.em -l input.txt            # 1行目が #!/usr/bin/env eml で、実行できるファイル
```

- 最初の引数が `-` で始まらず、サブコマンドの名前 (`check`、`run`、`repl`、`help`) でもなければ、`run` として扱う。`run` という名前のファイルを動かすときは `eml run run` と書く。`-` で始まる最初の引数はフラグとして読むので、`eml --verison` は使い方の誤り (終了コード 2) である
- `run`の後のファイルは、値を取るフラグの値でない、`-`で始まらない最初の引数である。値を取るフラグ (`--workers`、`--link`、`--gui`) と同じ引数は、次の引数を値として取る。そのほかの`-`で始まる引数 (`--debug-heap`、`--stats`、`--workers=4`) は、その引数だけでフラグになる。値を取るフラグはclapの`run`の定義から読む
- ファイルより後ろの引数は、`-`で始まるものも含めてそのままスクリプトに渡す。`--debug-heap`、`--stats`、`--workers`、`--link`、`--gui`はファイルより前にだけ書ける。`eml run main.em --workers 4`は、スクリプトに`--workers`と`4`を渡す
- `eml run`は、タスクを切り替える方針をいつも`Normal`にする。タスクは`Async`の操作でだけ切り替わる ([VM](vm.md)の「タスクとスケジュール」)
- `--workers <N>` は、`par` の右の子と `Async` のタスクと `compute` の仕事を走らせる worker のスレッドの数 (worker 0 を含む) を選ぶ ([VM](vm.md) の「タスクとスケジュール」)。`N` は 1 以上 1024 以下の整数である。run は worker ごとに止める語と欄を確保するので、上限を置いて、大きすぎる数を確保の失敗でなく使い方の誤りにする。`--workers` がなければ環境変数 `EML_WORKERS` を同じ規則で読み、それもなければ `available_parallelism()` の値にする (読めなければ 1、1024 を超えれば 1024)。0 と 1024 を超える数と数でない値は、フラグでも `EML_WORKERS` でも使い方の誤り (終了コード 2) で、2回書いた `--workers` も同じである。`run` だけが持ち、`repl` と `check` は持たない。`repl` は常に 1 である ([REPL の端末](repl.md) の「コマンドライン」)。ライブラリの `Limits::default()` と `eml_cli::default_limits()` も 1 で、UI テストは 1 で走らせる。`par/` と `async/` の UI テストは 2 と 8 でも走らせる ([テスト戦略](../implementation/testing.md) の「seed で流す」)
- `--gui <native|stub>`は、`Std.Gui`がウィンドウを開くbackendを選ぶ ([VM](vm.md) の「GUI の Park」)。`native`はOSのtoolkitで、`stub`は何も開かず、ウィンドウを表示するたびに閉じる要求を1回送るので、プログラムはウィンドウを待たずに終わりまで動く。`--gui`がなければ環境変数`EML_GUI`を同じ規則で読み、それもなければ`native`にする。ほかの値は、フラグでも`EML_GUI`でも使い方の誤り (`` `{値}` is not a GUI backend (native or stub) ``、終了コード2) で、`EML_GUI`の誤りは`EML_GUI: `を前に付ける。`run`だけが持ち、`repl`と`check`は持たない。`repl`にはGUIの`Park`がないので、`Gui.run`は`NotAvailable`で失敗する (下の「`Std.Gui`」)
- `--link KEY=VALUE` は、`link` の item に KEY と書いたライブラリの代わりに、VALUE を開く ([VM](vm.md) の「ライブラリと記号」)。何回でも書け、`run` と `repl` が持つ。同じ KEY を2回以上書くと、後に書いたものが効く。`check` はライブラリを開かないので持たない。KEY は `link` の item に書いた文字列そのもので、VALUE は `link` の item の名前と同じ規則で飾る。最初の `=` で KEY と VALUE に分け、`=` のない値と、KEY か VALUE が空の値は使い方の誤り (`` `{text}` is not KEY=VALUE ``、終了コード 2) である。`run` は、読み込んだどのモジュールの `link` の item にも合わない KEY を、`link error: --link names no library "{key}"` で断る (終了コード 1。[VM](vm.md) の「`--link`」)。`repl` は断らない ([REPL の端末](repl.md) の「C の関数」)
- `Env.args ()` は、ファイルより後ろの引数を返す。`eml` とスクリプトのパスは含めない
- shebang は `#!/usr/bin/env eml` と書く。Linux の `env` は `#!` の行の残りを1つの引数として渡すので、`#!/usr/bin/env eml run` は動かない
- ファイルの相対パスと子のプロセスの作業ディレクトリは、プロセスの今のディレクトリである

## extern の境界

標準ライブラリの extern は小さい核にとどめ、便利な関数は eml で書く。extern の境界では、スカラー (`Int`、`Float`、`Char`、`Byte`、`Bool`、`Unit`)、文字列、バイト列、`Foreign` だけを受け渡し、`List` と `Option` は eml の側で組む。`eml_rt` がデータの配置を知らないで済むようにするためである ([Core IR](core-ir.md) の「translate の配置」)。結果が `Char` の extern の行はスカラー値を、結果が `Byte` の extern の行は 0..=255 を返す ([LIR](lir.md) の「値の表現」)。そのため、`pub` でない extern は番兵 (見つからない位置の -1、終わりの空の文字列など) を返すことがある。番兵は標準ライブラリの中に閉じ込め、利用者には `Option` で見せる。

例外は `Std.Par.par` である。`par` は閉包を2つ受け取って呼ぶが、extern は eml のコードを呼び返さない。そのため、`par` の行は std の宣言を型検査に結ぶためだけにあり、引数をそろえた呼び出しは extern の命令でなく `par` の命令になる ([Core IR](core-ir.md) の「`par` の呼び出し」)。

extern の行は、どの位置の引数も借用にできる ([LIR](lir.md) の「extern の借用」)。`String` を読む行 (`byte_length`、`byte_at`、`char_at`、`slice`、`find_from`、`String` の `==`、`!=`、`compare`、`show`、`Float` の `can_parse` と `parse_raw`) は文字列を借りる。`Bytes` を読む行 (`length`、`get`、`slice`、`find_from`、`find_byte_from`、`append_slice` の `src`、`Bytes` の `==`、`!=`、`compare`) もバイト列を借りる。借りた値は、行が失敗しても呼ぶ側が持ったままである。

externの行は引数を預かることがある (`FunctionRow::held`。[宣言](declarations.md)の「`extern`」)。預かる行は`Std.Async`の`start_raw`、`start_main_raw`、`start_off_main_raw`、`run_raw`、`park`、`ready`、`finish_ok`、`task_cell`、`take_result`、`run_result`、`detach`、`cancel_raw`、`start_in_raw`、`park_time`、`park_drain`、`park_send`、`park_recv`、`park_io`、`park_io_until`、`park_signal`の20行と、`Std.Chan`の`push_raw`の1行で、`task_cell`だけは借りて預かる。預からない引数の型変数は、同じ節の規則のとおり`Unr`に固定するか、コールバックの結果なら固定しない。

stdの行は、`submit_*`、`Async.io_wait`、`take_*`の3つ組で、Rustの処理をdriverのブロックする呼び出しのプールで走らせられる。受け付けの行が消費した`Lin`の`Foreign`の引数は仕事へ移り、取り出しの行が新しい物体として返す ([VM](vm.md) の「プールの仕事」)。

文字列の位置は UTF-8 のバイトの位置 (`Int`) で表す。引数の順は、対象を最後に置く。`line |> String.split ","` と部分適用で使えるようにするためである。

## `Prelude`

- `println : String -> <IO> Unit` は、標準出力に1行を書く。`Stdout.write_line` と同じく、Host が持つ標準出力の buffer に書く。`Stdout` のハンドルが生きている間も使え、ハンドルの書き込みと同じ buffer に書くので、出力はプログラムの順に並ぶ (下の「`Stdout` と `Stderr`」)
- `eprintln : String -> <IO> Unit` は、標準エラーに1行を書く。`Stderr.write_line` と同じく、書くたびに書き出す。C の stdio と同じく、標準出力は書き出さない。標準出力をまとめて書いているとき (端末でないとき) は、`eprintln` の行が先に出ることがある
- `check_index : Int -> Int -> <Panic> Unit` は Prelude の中だけの extern で、利用者は名前で呼べない。translate が `var` の配列の変数の添字に呼び、添字が長さの範囲の外なら `Std.MutArray.read` と同じ文言 ("array index {index} is out of range for length {len}") の `Panic` を起こす ([Core IR](core-ir.md) の「`var` の変換」)

## `try_io`

```haskell
pub extern effect IO where
  never io_failed : IOErrorKind -> String -> a

pub data IOErrorKind =
  | NotFound
  | PermissionDenied
  | NotAvailable
  | NotUtf8
  | Other
  | Cancelled
  | TimedOut
  | ConnectionReset
  | AddrInUse
  | ResourceLimit
  | ConnectionRefused
  deriving (Eq, Show)

pub data IOError =
  | IOError { kind : IOErrorKind, message : String }

instance Show IOError where
  show e = "IOError { kind = \{show e.kind}, message = \{show e.message} }"
  display e = e.message

pub try_io : (Unit -> <IO | e> a) -> <IO | e> Result IOError a
try_io action =
  handle action () with
    | ctl io_failed kind message -> Err (IOError { kind = kind, message = message })
    | return x -> Ok x
```

`IOError` の `show` は、導出した `Show` がレコードに使う形と同じにする。導出では `display` を上書きできないので、手で書く。

- `try_io action` は `action ()` を呼び、IO の失敗がなければ `Ok` に包んで返す。IO の失敗があれば、そこで `action` の実行を止め、途中のフレームが持っていた値を後始末して `Err` を返す
- 捕まえるのは、いちばん内側の `try_io` である。`IO` は `mask` に入らないので、row の上の位置でなく、実行の時点で外側にある最も近い `try_io` が捕まえる。例外と同じ動きである。たとえば `wrap : (Unit -> <e> a) -> <IO | e> Result IOError a` が、受け取ったコールバックを `try_io` の中で呼ぶと、コールバックの IO の失敗も `wrap` の `try_io` が捕まえる。ほかのエフェクトなら `mask` がコールバックの操作を外側へ通すが、`IO` ではそうしない
- handler の節は handle の外側の `evv` で動く ([エフェクトと handler](effects.md) の「健全性」)。そのため、handle の本体の中にある `try_io` は、その handler の節の中で起きた IO の失敗を捕まえない。handle より外側の `try_io` が捕まえる
- `try_io` は、`exit`、書き出しの失敗、`Panic` (算術の誤り、位置の範囲の誤り、`panic`)、割り込み、確保量の上限、`Async` のタスクの外と `par` の子の中の深さの上限、検査モードのヒープの誤りを捕まえない。`Async` のタスクの中 (`par` の子の外) の深さの上限とメモリの柔らかい上限は、種類 `ResourceLimit` の IO の失敗になり、`try_io` が捕まえる (下の「`Std.Async`」)

`IOErrorKind` の構築子は11個で、タグは宣言の順の 0 から 10 である。この実装の extern の誤りが出すのは、最初の5つ (`NotFound` から `Other`) だけである。残りの6つは `fail_io` が出し、`fail_io` は11個のどれでも出せる。`Cancelled` は、非同期の `Wake` の構築子 `Cancelled` と名前が重なる。修飾しない名前は自分のモジュールと import を Prelude より先に引くので ([モジュールと名前解決](modules.md) の「名前の解決」)、`Wake(..)` を import したモジュールでは、種類のほうを `Prelude.Cancelled` と書く。

捕まえる失敗と `kind` は次のとおりである。`message` は、[LIR](lir.md) の「実行時エラー」の文言と1文字も違わない。

| `Fault` | 起こす関数 | `kind` |
|---|---|---|
| `Io { reason, .. }` | `FileIn.open`、`FileIn.read_all`、`FileIn.read_line`、`Fs.read_text`、`FileIn.read_all_bytes`、`FileIn.read_line_bytes`、`FileIn.read_chunk`、`FileOut.create`、`FileOut.append`、`FileOut.write`、`FileOut.write_bytes`、`FileOut.write_line`、`FileOut.flush`、`FileOut.close`、`Fs.write_text`、`Proc` と `Std.Exec` の起動 (`run`、`output`、`output_bytes`、`capture`、`capture_bytes`、`spawn`)、`Proc.Child` の読みと書き込み (`output` と `capture` の中の読みを含む) | `reason` が `NotFound` なら `NotFound`、`PermissionDenied` なら `PermissionDenied`、`NotAvailable` なら `NotAvailable`、`Other` なら `Other` |
| `Stdin(reason)` | `Stdin.read_text` と、`Stdin` のハンドルの読み (`read_all`、`read_line`、`read_all_bytes`、`read_line_bytes`、`read_chunk`) | 同上 |
| `NotUtf8(_)` | `FileIn.read_all`、`FileIn.read_line`、`Fs.read_text`、`Stdin.read_text`、`Stdin.read_all`、`Stdin.read_line`、`Proc.output`、`Proc.capture`、`Proc.read_all`、`Proc.read_line`、`Env.args`、`Env.get` | `NotUtf8` |
| `Raised { kind, .. }` | `fail_io` と、`Async.await` と `Async.run` がタスクの IO の失敗を起こし直すとき。`Async` のタスクの中の深さの上限 (下の「`Std.Async`」の「タスクの上限」) もこの形で、種類は `ResourceLimit` である。`Proc.capture`、`Proc.capture_bytes`、`Exec.capture`、`Exec.capture_bytes` の 0 でない終了も `fail_io` で起こす。`Std.Net` の失敗と、`Std.Exec` の起動の誤りのほかの失敗もこの形である (下の「`Std.Net`」と「`Std.Exec`」) | `kind` のタグが表す種類 (子の終了は `Other`) |

書き出しの失敗 (`Output`) を捕まえないのは、標準出力をまとめて書くためである ([オブジェクトモデル](object-model.md) の「Host」)。書けなかったことが分かるのは、後でバッファがあふれた `println` か、読みや起動の前の書き出しか、プログラムの終わりの書き出しになる。そのため、`try_io (fn () -> println s)` は書けなくても `Ok` を返しうるし、関係のない別の `try_io` が失敗を受け取りうる。どこで捕まるかが決まらないので、実行時エラーのままにする。

`try_io` で捕まえなかった IO の失敗は、実行時エラーである。位置を付けて表示し、終了コード 1 で止まる。このとき eml の後始末 (handler の節と利用者のコード) は走らないが、ヒープを落とすときの破棄処理が `FileOut` の buffer を書き出す (下の「`FileOut`」)。プロセスが終わるので、OS が開いたファイルを閉じる。

## `try_panic`

`Panic` の多重度は `Never` で、[型と Kind](types.md) の「Kind」に決まりがある。

```haskell
pub extern effect Panic where
  never panicked : String -> a

pub extern panic : String -> <Panic> a

pub try_panic : (Unit -> <Panic | e> a) -> <e> Result String a
try_panic action =
  handle action () with
    | ctl panicked message -> Err message
    | return x -> Ok x
```

- `panic message` は `Panic` を起こす。`exit` と同じく戻らない
- `try_panic action` は `action ()` を呼び、`Panic` がなければ `Ok` に包んで返す。`Panic` が起きれば、そこで `action` の実行を止め、途中のフレームが持っていた値を後始末して `Err` を返す。`Err` が持つのは誤りの文言である。`panic message` なら `message` で、組み込みの誤りなら実行時エラーの文言 ([LIR](lir.md) の「実行時エラー」、下の「実行時エラー」) である。誤りの種類と位置は返さない
- `try_io` と違い、結果の row から `Panic` を除く。`Panic` を起こすのは値と入力で決まる誤りなので、捕まえた後に残す理由がない
- 捕まえるのは、`try_io` と同じく実行の時点で外側にある最も近い `try_panic` である。`Panic` は `mask` に入らない。handle の本体の中にある `try_panic` は、その handler の節の中で起きた `Panic` を捕まえず、handle より外側の `try_panic` が捕まえる
- `try_panic` は、IO の失敗、`exit`、書き出しの失敗、割り込み、確保量と深さの上限、検査モードのヒープの誤りを捕まえない。`try_io` も `Panic` を捕まえない
- `try_panic` で捕まえなかった `Panic` は、実行時エラーになる (下の「実行時エラー」)。`panic message` は `panicked: message` の文言に位置を付けて表示する

`Panic` を起こすのは、`panic` と、次の誤りを返す組み込みの関数である。VM は、`Pure` でない行の誤りのうち `Fault::panics` が真のものを `Panic` に届ける。`Pure` の行は巻き戻しの地点を持たないので届けない ([VM](vm.md) の「`IO` と `Panic` への中断」)。`message` は、[LIR](lir.md) の「実行時エラー」の文言と1文字も違わない。

| `Fault` | 起こす関数 |
|---|---|
| `Arith(_)` | `+`、`-`、`*`、`/`、`%`、単項の `-`。`Float.truncate`、`Float.floor`、`Float.ceil`、`Float.round` (`Int` に収まらない値)、`Float.to_fixed` (桁数)、`Char.of_int` (スカラー値でない値)、`Byte.of_int` (0 から 255 の外の値) |
| `Utf8(_)` | `String.decode` と、それを使う std の関数 |
| `InUse(_)` | `Stdin.open`、`Stdout.open`、`Stderr.open` (同じストリームのハンドルが生きている間の2つ目の `open`) と、`Stdin.open` を使う `Stdin.read_text` (下の「ハンドル」) |
| `Index(_)` | `String.byte_at`、`String.char_at`、`String.slice`、`String.find_from` と、それらを使う std の関数。`Array.get`、`Array.set`、`Array.make`、`MutArray.read`、`MutArray.write`、`MutArray.make`、`UnboxedArray.get`、`UnboxedArray.set`、`UnboxedArray.make`、`MutUnboxedArray.read`、`MutUnboxedArray.write`、`MutUnboxedArray.make` (添字の範囲と負の長さ) と、添字の式が使う `check_index`。`Env.arg` の範囲の外の位置も `Index` だが、`Env.arg` は std の中だけで使うので、利用者のプログラムでは起きない |

## `fail_io`

```haskell
pub extern fail_io : IOErrorKind -> String -> <IO> a
```

- `fail_io kind message` は、種類 `kind` と文言 `message` の IO の失敗を起こす。`exit` と同じく戻らない
- `try_io` は、種類と文言をそのまま `IOError` の欄に入れて受け取る。`try_io` は `Io` などの extern の誤りと同じ道で捕まえる
- `try_panic` は捕まえない。`IO` の失敗は `Panic` ではないためである
- 捕まえなければ、`fail_io` を呼んだ位置の実行時エラーになる。表示は `message` で、終了コードは 1 である。実行時エラーの扱いは `try_io` の失敗と同じである
- `kind` のタグは宣言の順の数で、`fail_io` は書かれたまま渡す。`Cancelled` から `ConnectionRefused` までの6つの種類を `fail_io` に渡しても、型検査の外で作る特別な扱いはない

## `exit`

```haskell
pub extern exit : Int -> <IO> a
```

- `exit n` は、途中のフレームが持っていた値を後始末し、標準出力を書き出してから、終了コード `n & 255` で終わる。シェルが終了コードを 8 ビットで扱うのと同じである
- `try_io` も、利用者の handler も、`exit` を捕まえない。どの handler の `return` の節も呼ばない
- `Std.Async` のタスクの中の `exit n` も、プログラムを終了コード `n & 255` で終える。巻き戻すのは、`exit` を呼んだタスクのフレームだけである。ほかのタスクは巻き戻さず、その後は動かさない。ほかのタスクが持っていた値は、実行を捨てるときに古いヒープと一緒に後始末する。`FileIn` などの `Lin` の資源は、そのときに破棄処理が閉じる ([VM](vm.md) の「誤り、止めること、終わり」)。`try_all` も `try_panic` も `exit` を捕まえない
- 書き出しが失敗したら、プログラムの終わりの書き出しの失敗と同じく、位置のない実行時エラー (終了コード 1) にする
- `exit` は `<IO>` を持つので、型の上では `IO` の関数としてどこからでも呼べる。`<IO>` を持たせるのは、`par` の子から呼べないようにするためである。`par` の子の row は `IO` を含められない ([マルチコア対応の設計](../future/multicore.md) の「row の Kind」)
- `exit 1`、`exit 2`、`exit 130`、`exit 143` の終了コードは、CLI の実行時エラー (1)、使い方の誤り (2)、割り込み (130)、終了の求め (143) と区別できない。スクリプトが選んだことなので、そのまま受け入れる
- REPL では、`exit` は入力を終えるだけで、REPL は次の入力へ進む。詳しくは [REPL の端末](repl.md) の「割り込みと実行時エラー」にある

## `for` と `while`

```haskell
pub for : Int -> Int -> (Int -> <e> Unit) -> <Panic | e> Unit
pub while : (Unit -> <e> Bool) -> (Unit -> <e> Unit) -> <e> Unit
```

- `for lo hi body` は、`lo` 以上 `hi` 未満の `i` で、小さい順に `body i` を呼ぶ。`lo` が `hi` 以上なら `body` を呼ばない。`<Panic>` は添字の足し算から来る
- `while cond body` は、`cond ()` が `True` を返す間、`body ()` を呼ぶ。最初の `cond ()` が `False` なら `body` を呼ばない
- どちらも Prelude の eml のコードで、本体を末尾でなく呼んでから、自分を末尾で呼ぶ。本体と条件の閉包は、既知の閉包の複製 ([Core IR](core-ir.md) の「複製」) で直接の呼び出しになる。そのため、本体が handler に触れるなら、本体は状態を渡す形の経路に入る ([Core IR](core-ir.md) の「handler への特殊化」)

## `List`

| 関数 | 型 |
|---|---|
| `map` | `(a -> <e> b) -> List a -> <e> List b` |
| `filter` | `(a -> <e> Bool) -> List a -> <e> List a` |
| `filter_map` | `(a -> <e> Option b) -> List a -> <e> List b` |
| `fold` | `(b -> a -> <e> b) -> b -> List a -> <e> b` |
| `each` | `(a -> <e> Unit) -> List a -> <e> Unit` |
| `length` | `List a -> <Panic> Int` |
| `reverse` | `List a -> List a` |
| `append` | `List a -> List a -> List a` |
| `take` | `Int -> List a -> <Panic> List a` |
| `split_at` | `Int -> List a -> <Panic> (List a, List a)` |
| `nth` | `Int -> List a -> <Panic> Option a` |
| `zip` | `List a -> List b -> List (a, b)` |
| `index_of` | `Eq a => a -> List a -> <Panic> Option Int` |
| `contains` | `Eq a => a -> List a -> Bool` |
| `lookup` | `Eq k => k -> List (k, v) -> Option v` |
| `sort` | `Ord a => List a -> List a` |
| `sort_by` | `(a -> a -> <e> Ordering) -> List a -> <e> List a` |

- `map`、`filter`、`filter_map`、`each` は、要素の順に関数を呼ぶ。`fold f init xs` は左からたたむ (`f (f init x1) x2`)
- `append xs ys` は `xs` の後ろに `ys` を続ける
- `take n xs` は先頭の `n` 個を返す。`n` が 0 以下なら `Nil` で、長さより大きければ全体である
- `split_at n xs` は先頭の `n` 個と残りの組を返す。`n` が 0 以下なら `(Nil, xs)` で、長さ以上なら `(xs, Nil)` である
- `nth i xs` は 0 から数えた位置 `i` の要素を返す。`i` が負か長さ以上なら `None` である
- `zip` は短いほうの長さで止める
- `index_of x xs` は `x` に等しい最初の要素の位置を返し、なければ `None` である。`contains x xs` は等しい要素があるかを返す。`lookup key ps` は最初にキーが等しい組の値を返す。探すものを先に置くのは、`String.contains` と `Map.lookup` にそろえるためである
- `sort` は `sort_by compare` である。`sort_by cmp xs` は安定な並べ替えで、`cmp x y` が `GT` のときだけ `y` を `x` より先に置くので、等しい要素は元の順に残る。下からのマージソートで、どのループも末尾再帰であり、長さを数えないので `Panic` を起こさない
- `sort_by` が比較の関数を呼ぶ順と回数は決めない。エフェクトのある比較の関数も渡せるが、そのエフェクトの起きる順は決まらない。`Array` には並べ替えを置かない
- `drop` は置かない。線形の値を捨てる `drop` と名前が重なるためである
- どの関数も末尾再帰で書き、逆順に積んで最後に `reverse` する。長いリストでも VM の深さの上限に当たらないようにするためである

## `String`

| 関数 | 型 | 実装 |
|---|---|---|
| `byte_length` | `String -> Int` | extern |
| `byte_at` | `Int -> String -> <Panic> Byte` | extern |
| `char_at` | `Int -> String -> <Panic> Char` | extern |
| `slice` | `Int -> Int -> String -> <Panic> String` | extern |
| `push` | `Char -> String -> String` | extern |
| `decode` | `Bytes -> <Panic> String` | extern |
| `next` | `Int -> String -> <Panic> Int` | eml |
| `of_char` | `Char -> String` | eml |
| `of_chars` | `List Char -> String` | eml |
| `to_chars` | `String -> <Panic> List Char` | eml |
| `fold` | `(b -> Char -> <e> b) -> b -> String -> <Panic \| e> b` | eml |
| `each` | `(Char -> <e> Unit) -> String -> <Panic \| e> Unit` | eml |
| `chars` | `String -> Iter <Panic \| e> Char` | eml |
| `from_utf8` | `Bytes -> Option String` | eml |
| `find` | `String -> String -> <Panic> Option Int` | eml |
| `contains`、`starts_with`、`ends_with` | `String -> String -> <Panic> Bool` | eml |
| `split` | `String -> String -> <Panic> List String` | eml |
| `lines` | `String -> <Panic> List String` | eml |
| `join` | `String -> List String -> String` | eml |
| `trim` | `String -> <Panic> String` | eml |
| `trim_line_end` | `String -> <Panic> String` | eml |
| `to_int` | `String -> <Panic> Option Int` | eml |

- 位置はどれも UTF-8 のバイトの位置の `Int` で、位置専用の型はない。文字の境界でない位置は、`char_at` と `slice` が `Panic` で断る。位置専用の不透明な型で防げるのは `Int` との取り違えだけで、std とスクリプトは位置を `Int` の算術で組んでいる。位置の取り違えの誤りが実際のプログラムで繰り返し出たときと、共有の部分文字列 (view) を入れるときに、位置専用の型を考え直す
- 文字の単位は Unicode のスカラー値 (`Char`) である。書記素クラスタと Unicode の正規化は扱わない
- `byte_length s` は UTF-8 のバイトの数である
- `byte_at i s` は位置 `i` のバイト (0〜255) を `Byte` で返す。`0 <= i < byte_length s` でなければ `Panic` を起こす
- `char_at i s` は、位置 `i` から始まる文字である。`0 <= i < byte_length s` でないときと、`i` が文字の境界でないときは `Panic` を起こす。次の文字の位置は `next` で求める
- `next i s` は、位置 `i` の文字の次の位置で、`i + Char.utf8_length (char_at i s)` である
- `push c s` は、`s` の後ろに `c` の UTF-8 を足した文字列である。`s` は消費する。`s` が一意ならその場で伸ばし、そうでなければ写してから足す
- `slice start end s` は、位置 `start` から `end` の手前までの部分の文字列である。`0 <= start <= end <= byte_length s` でないときと、`start` か `end` が文字の境界でないときは `Panic` を起こす。文字の境界は、文字列の端か、UTF-8 の続きのバイト (`0x80`〜`0xBF`) でない位置である
- `of_char c` は `c` だけの文字列で、`of_chars cs` は `cs` の文字を順に `push` した文字列である。`to_chars s` は `s` の文字のリストである
- `fold f init s` は、先頭から文字ごとに `char_at` で読み、`f` を呼んでたたむ。`each f s` は文字ごとに `f` を呼ぶ。`chars s` は文字を順に渡す `Iter` で、`Iter.take` などでつなげる。row の `Panic` は、`s` の読みが `Panic` を起こしうるためである
- 走査の方針は、区切りがすべて ASCII なら `byte_at` と `'x'u8` のリテラルで書き、コードポイントが要る所だけ `char_at` で読む。UTF-8 の続きのバイトは ASCII と重ならないので、バイトで区切っても文字を割らない。ネイティブ化では、`byte_at` の読みの展開が最も軽い
- `find needle s` は、`needle` が最初に現れる位置である。空の `needle` は `Some 0` である
- `contains needle s`、`starts_with prefix s`、`ends_with suffix s` は、名前のとおりの判定である。バイトで比べるので、文字の境界でない位置で比べても誤りにならない
- `split sep s` は、`sep` で区切った部分を順に返す。Rust の `str::split` と同じく、続いた区切りは空の部分を作り、`split "," ""` は `[""]` である。空の `sep` は区切らず `[s]` を返す
- `lines s` は、Rust の `str::lines` と同じである。行の終わりは `\n` か `\r\n` で、最後の行の終わりは省ける。`lines "a\r\nb\n"` は `["a", "b"]`、`lines "a\r"` は `["a\r"]`、`lines ""` は `[]` である
- `join sep xs` は、要素の間に `sep` を挟んでつなぐ
- `trim s` は、`byte_at` で見て、両端の ASCII の空白 (空白、`\t`、`\n`、`\x0B`、`\x0C`、`\r`) を除く。Unicode の空白は除かない
- `trim_line_end s` は、最後の行の終わり (`\n` か `\r\n`) を1つ除く。行の終わりで終わらなければ `s` をそのまま返し、`trim_line_end ""` は `""` である。`Stdin.read_line` と `FileIn.read_line` が、読んだ行の終わりを除くのに使う
- `to_int s` は、Rust の `i64::from_str` と同じ形を読む。省略できる符号 `+` か `-` の後に、1つ以上の ASCII の数字が続く形である。空白は許さない。範囲を超えれば `None` で、`"-9223372036854775808"` は読める
- `byte_length`、`push`、`join`、`from_utf8`、`of_char`、`of_chars` のほかの関数は、`Panic` を起こす行 (`byte_at`、`char_at`、`slice`、`decode`、std の中だけの `find_from`) か算術を使うので、型の上では `Panic` を起こしうる
- 部分文字列の照合は、`needle` の先頭のバイトを8バイトずつまとめて探し、見つけた位置ごとに残りを比べる。最悪の場合は、`needle` の長さと探す範囲の長さの積に比例する時間がかかる ([オブジェクトモデル](object-model.md) の「extern」)

### `Bytes` との変換

`Bytes` から `String` への変換は、UTF-8 として妥当かを確かめる。`String` から `Bytes` への変換 `Bytes.of_string` は「`Bytes`」にある。

- `decode b` は、`b` と同じバイトの `String` である。`b` は消費する。`b` が一意なら写さずに同じブロックを使い、そうでなければ中身を写す。`b` が妥当な UTF-8 でなければ、`b` を手放して `Panic` を起こす。文言は `invalid UTF-8 at byte 3` の形で、数は最初の妥当でないバイトの位置である (下の「実行時エラー」)
- `from_utf8 b` は、`b` が妥当なら `Some (decode b)`、そうでなければ `None` である。`b` は消費する。`decode` を `try_panic` で包み、`Err` を `None` にする。確かめは `decode` の1回で、型から `Panic` が消える

## `Std.Float`

| 名前 | 型 | 実装 |
|---|---|---|
| `of_int` | `Int -> Float` | extern |
| `truncate`、`floor`、`ceil`、`round` | `Float -> <Panic> Int` | extern |
| `sqrt`、`abs`、`sin`、`cos`、`exp`、`log` | `Float -> Float` | extern |
| `pow` | `Float -> Float -> Float` | extern |
| `pi` | `Float` | eml (`3.141592653589793`) |
| `infinity`、`nan` | `Float` | eml (std の中だけの extern を呼ぶ) |
| `is_nan` | `Float -> Bool` | eml |
| `to_fixed` | `Int -> Float -> <Panic> String` | extern |
| `parse` | `String -> Option Float` | eml |

`Float` の型は Prelude にあり、関数は `Std.Float` にある。関数は `import Float` を書いたモジュールで、`Float.sqrt x` のように修飾して使う ([モジュールと名前解決](modules.md) の「Prelude」と「標準ライブラリ」)。

- `of_int i` は、`i` に最も近い `Float` である。ちょうど中間なら、仮数の最後のビットが偶数の側を選ぶ
- `truncate` は 0 の側へ切り捨てる。`floor` は床、`ceil` は天井である。`round` は最も近い整数で、ちょうど中間なら偶数の側を選ぶ (Python の `round` と同じで、`round 2.5` は `2`)
- 4つの変換は、NaN、±無限大、`Int` の範囲 `[-2^63, 2^63)` を外れる結果で `Panic` を起こす。文言は `` float `1e+19` does not fit in `Int` `` で、値は `show` と同じ形で書く (下の「実行時エラー」)
- `sqrt`、`abs`、`sin`、`cos`、`exp`、`log` (自然対数)、`pow x y` (`x` の `y` 乗) は、Rust の `f64` の関数と同じ値を返す。定義域の外は `nan` である。`sqrt` と `abs` のほかは、値の最後の桁がプラットフォームの数学のライブラリで変わりうる
- 計算が作る NaN は、`Float` の算術と同じく、どれも正の quiet NaN である。そのため `compare` で、NaN はどの値よりも大きい ([宣言](declarations.md) の「Prelude のクラス」)
- `infinity` と `nan` はリテラルを持たない。値の本体は `<>` に限るので、std の中だけの `Pure` の extern `infinity_value` と `nan_value` (`Unit -> Float`) を呼んで作る
- `is_nan x` は `x != x` である。NaN だけが自分と等しくない
- `to_fixed n x` は、小数点以下 `n` 桁の文字列で、Python の `f"{x:.{n}f}"` と同じである。10進の値を正しく丸め、ちょうど中間なら偶数の側を選ぶ (`to_fixed 0 2.5` は `"2"`、`to_fixed 2 1.005` は `"1.00"`)。特別な値は `inf`、`-inf`、`nan` と書き、`-0.0` は `-` を付ける。`n` が 0 から 100 の外なら `Panic` を起こす
- `parse s` は、前後の Unicode の空白を除き、Rust の `f64::from_str` が受ける形を読む。符号、10進と指数の形 (`1.5`、`.5`、`5.`、`1E-5`、`1e+16`)、大文字小文字を問わない `inf`、`infinity`、`nan` である。Python の `float` と違い、数字の間の `_` は受けない。読めなければ `None` で、大きすぎる値は無限大になる。`parse (show x)` は、どの `x` でも同じ値を返す
- `parse` は、std の中だけの2つの `Pure` の extern で書いてある。`can_parse : String -> Bool` が読めるかを返し、`parse_raw : String -> Float` が値を返す (読めなければ NaN。読めた `nan` と区別するため、`parse` は NaN のときだけ `can_parse` を呼ぶ) (上の「extern の境界」)

## `Std.Char`

| 名前 | 型 | 実装 |
|---|---|---|
| `to_int` | `Char -> Int` | extern |
| `of_int` | `Int -> <Panic> Char` | extern |
| `of_byte` | `Byte -> Char` | extern |
| `to_byte` | `Char -> <Panic> Byte` | eml |
| `utf8_length` | `Char -> Int` | eml |
| `to_ascii_upper` | `Char -> Char` | eml |
| `to_ascii_lower` | `Char -> Char` | eml |
| `is_ascii` | `Char -> Bool` | eml |
| `is_ascii_digit` | `Char -> Bool` | eml |
| `is_ascii_upper` | `Char -> Bool` | eml |
| `is_ascii_lower` | `Char -> Bool` | eml |
| `is_ascii_alpha` | `Char -> Bool` | eml |
| `is_ascii_alphanumeric` | `Char -> Bool` | eml |
| `is_ascii_space` | `Char -> Bool` | eml |

`Float` と同じく、モジュールで修飾して使う (`Char.to_int c`)。

- `to_int c` は `c` のコードポイントで、0 から 0xD7FF か 0xE000 から 0x10FFFF の `Int` である。`Char` と `Int` は同じ語なので、実行時には何もしない
- `of_int n` は、`n` がスカラー値 (0 から 0xD7FF か 0xE000 から 0x10FFFF) のときにその `Char` を返す。ほかの値 (負の数、サロゲート、0x10FFFF を超える値) では `Panic` を起こす。文言は ``int `55296` is not a Unicode scalar value`` である (下の「実行時エラー」)
- `Option` ではなく `Panic` にするのは `Float.truncate` と合わせるためで、extern が `Option` を作れないためでもある
- `is_ascii` は U+0000 から U+007F の文字で成り立つ。`is_ascii_digit` は `'0'` から `'9'`、`is_ascii_upper` は `'A'` から `'Z'`、`is_ascii_lower` は `'a'` から `'z'` で成り立つ。`is_ascii_alpha` は upper か lower、`is_ascii_alphanumeric` は alpha か digit で成り立つ
- `is_ascii_space` は `' '` と、`'\t'` から `'\r'` (U+0009 から U+000D) で成り立つ。`String.trim` と同じ範囲である
- どの判定も、ASCII 以外の文字では偽である
- `of_byte b` は `b` を Latin-1 の文字として読む。0 から 255 はどれもスカラー値なので、失敗しない。`Byte` と `Char` は同じ語なので、実行時には何もしない
- `of_byte` は UTF-8 をデコードしない。文字列のバイトを渡すと、先頭のバイトや続きのバイトは、別の Latin-1 の文字になる。文字列から文字を読む関数は `String.char_at` である
- `to_byte c` は、`c` が U+00FF 以下ならその `Byte` を返す。それを超えるときは `Panic` を起こす。文言は ``char 'あ' does not fit in `Byte` `` である
- `utf8_length c` は `c` を UTF-8 で書いたときのバイト数で、U+007F まで 1、U+07FF まで 2、U+FFFF まで 3、それより上は 4 である
- `to_ascii_upper` と `to_ascii_lower` は、ASCII の英字だけを変え、ほかの文字はそのまま返す。ASCII 以外の大文字と小文字の変換は持たない

## `Std.Byte`

| 名前 | 型 | 実装 |
|---|---|---|
| `to_int` | `Byte -> Int` | extern |
| `of_int` | `Int -> <Panic> Byte` | extern |
| `wrap` | `Int -> Byte` | extern |

`Char` と同じく、モジュールで修飾して使う (`Byte.to_int b`)。

- `to_int b` は `b` の値で、0 から 255 の `Int` である。`Byte` と `Int` は同じ語なので、実行時には何もしない
- `of_int n` は、`n` が 0 から 255 のときにその `Byte` を返す。ほかの値では `Panic` を起こす。文言は ``int `256` does not fit in `Byte` `` である (下の「実行時エラー」)
- `wrap n` は `n` の下位8ビットを返す。2の補数で見るので、`wrap (-1)` は `255u8`、`wrap 256` は `0u8` で、`Panic` は起こさない

## `Bytes`

| 名前 | 型 | 実装 |
|---|---|---|
| `empty` | `Unit -> Bytes` | extern |
| `length` | `Bytes -> Int` | extern (バイト列を借りる) |
| `append` | `Bytes -> Bytes -> Bytes` | extern |
| `append_slice` | `Int -> Int -> Bytes -> Bytes -> <Panic> Bytes` | extern (`src` を借りる) |
| `slice` | `Int -> Int -> Bytes -> <Panic> Bytes` | extern (バイト列を借りる) |
| `truncate` | `Int -> Bytes -> <Panic> Bytes` | extern |
| `of_string` | `String -> Bytes` | extern |
| `make` | `Int -> Byte -> <Panic> Bytes` | extern |
| `with_capacity` | `Int -> <Panic> Bytes` | extern |
| `get` | `Int -> Bytes -> <Panic> Byte` | extern (バイト列を借りる) |
| `set` | `Int -> Byte -> Bytes -> <Panic> Bytes` | extern |
| `push` | `Byte -> Bytes -> Bytes` | extern |
| `find` | `Bytes -> Bytes -> <Panic> Option Int` | eml |
| `find_byte` | `Byte -> Bytes -> <Panic> Option Int` | eml |
| `trim_line_end` | `Bytes -> <Panic> Bytes` | eml |

型 `Bytes` は Prelude の extern の型で、関数は `Std.Bytes` に置く。`Std.Float` と同じく、モジュールで修飾して使う (`Bytes.length b`)。

- `Bytes` は 0 から 255 のバイトの列である。`String` と同じ配置を持つ別の型で、UTF-8 の条件はない。`String` との変換は、引数が一意なら種類だけを書き換え、そうでなければ中身を写す ([オブジェクトモデル](object-model.md) の「バイト列と文字列」)。リテラルはない。`Eq`、`Ord`、`Show` を持ち、順序はバイトの辞書式の順序で、`String` と同じである
- 位置と長さはバイトの数の `Int` である。引数の順は、対象を最後に置く
- `empty ()` は長さ 0 のバイト列である
- `length b` はバイトの数である
- `append a b` は、`a` の後ろに `b` を足したバイト列である。`a` が一意で容量が足りれば、その場で伸ばす。そうでなければ、両辺を写した新しいバイト列を作る。`String` の `++` と同じである
- `append_slice from to src dst` は、`dst` の後ろに、`src` の位置 `from` から `to` の手前までを足したバイト列である。`0 <= from <= to <= length src` でなければ `Panic` を起こす。`src` は借り、`dst` は消費する。`dst` が一意ならその場で伸ばす。同じ変数を `src` と `dst` に渡してもよく、そのときは参照が2つ要るので、`dst` を写す。`append_slice 0 (length b) b b` は `b` を倍にする ([LIR](lir.md) の「extern の借用」)
- `slice from to b` は、位置 `from` から `to` の手前までを写した新しいバイト列である。`0 <= from <= to <= length b` でなければ `Panic` を起こす
- `truncate n b` は、前の `n` バイトである。`0 <= n <= length b` でなければ `Panic` を起こす。`b` が一意ならその場で長さだけを書き換えて、容量を保つ。そうでなければ、前の部分を写す
- `of_string s` は、`s` と同じバイトのバイト列である。`s` は消費する。`s` が一意なら写さずに同じブロックを使う。リテラルと共有した文字列は写す
- `make n b` は、どのバイトも `b` の、長さ `n` のバイト列である。`n` が負なら `Panic` を起こす。`Limits::max_bytes` を超える長さは、確保の前に止める
- `with_capacity n` は、容量が `n` バイト以上の空のバイト列である。容量は長さに出ない。一意なら、容量までの `push` と `append` はその場で伸ばす。`n` が負なら `make` と同じ `Panic` を起こし、`Limits::max_bytes` を超える容量は `make` と同じく確保の前に止める
- `get i b` は、位置 `i` のバイトである。`0 <= i < length b` でなければ `Panic` を起こす。`b` は借りる
- `set i x b` は、位置 `i` のバイトを `x` にしたバイト列である。`0 <= i < length b` でなければ `Panic` を起こす。`b` が一意ならその場で書き換え、そうでなければ写してから書き換える
- `push x b` は、`b` の後ろに `x` を足したバイト列である。`b` が一意で容量が足りればその場で伸ばし、足りなければ容量を倍にして写す。空から n 回足すときに写すバイトの合計は 2n 以下で、確保は log2(n) に比例する
- `show b` は `Bytes [1, 2, 3]` の形で、要素は `Byte` の `display` の数である (`u8` を付けない)。空は `Bytes []` である。Prelude にあるので、`Std.Bytes` を import しなくても使える
- `find needle b` は、`needle` が最初に現れる位置である。空の `needle` は `Some 0` である。照合は `String` の `find` と同じで、最悪の場合は `needle` の長さと探す範囲の長さの積に比例する時間がかかる
- `find_byte x b` は、`x` が最初に現れる位置である。8バイトずつまとめて比べるので、1バイトずつ `get` で読むループより速い
- `trim_line_end b` は、最後の行の終わり (`\n` か `\r\n`) を1つ除く。行の終わりで終わらなければ `b` をそのまま返す。`length`、`get`、`truncate` の上に書いてあるので、`b` が一意なら長さだけを書き換え、写さない。`FileIn.read_line_bytes` が、読んだ行の終わりを除くのに使う
- 範囲の外の文言は、[LIR](lir.md) の「実行時エラー」にある

## `Std.Array`

| 名前 | 型 | 実装 |
|---|---|---|
| `make` | `Int -> a -> <Panic> Array a` | extern |
| `tabulate` | `Int -> (Int -> <e> a) -> <Panic \| e> Array a` | eml (`MutArray.freeze (MutArray.tabulate n f)`) |
| `from_list` | `List a -> Array a` | eml |
| `to_list` | `Array a -> List a` | eml |
| `length` | `Array a -> Int` | extern (配列を借りる) |
| `get` | `Array a -> Int -> <Panic> a` | extern (配列を借りる) |
| `set` | `Array a -> Int -> a -> <Panic> Array a` | extern |
| `thaw` | `Array a -> MutArray a` | extern |

型 `Array a` と `MutArray a` は Prelude の extern の型で、関数は `Std.Array` と `Std.MutArray` に置く。`Std.Float` と同じく、モジュールで修飾して使う (`Array.length a`)。

- `Array` は意味の上で書き換わらない。`Array a` の Kind は要素の Kind に従うが、extern の関数の引数の型変数は `Unr` に固定される ([宣言](declarations.md) の「`extern`」) ので、`Lin` の要素の配列にはどの関数も使えない
- `make n x` は、長さ `n` で、どの要素も `x` の配列である。`n` が負なら `Panic` を起こす。`n` が 0 なら `x` を手放して空の配列を返す
- `tabulate n f` は、`i` 番目の要素を `f i` にした配列である。`f` を添字の順に呼ぶ。`n` が負なら、`f` を呼ばずに `make` と同じ `Panic` を起こす
- `tabulate` は `Lin` の `MutArray` の上に作ってあるので、`e` に `multi` の操作が含まれると E3006 になる。`List.map` はそのような `f` も受け付ける
- `get a i` は `i` 番目の要素である。添字が範囲の外なら `Panic` を起こす
- `set a i x` は、`i` 番目を `x` に替えた配列である。`a` が一意 (RC が 1) ならその場で書き換え、共有されていれば写す。写すときは、残す要素の参照を1つずつ増やす
- `thaw a` は、一意な `a` をそのまま `MutArray` にし、共有されていれば写して `MutArray` にする
- `from_list` と `to_list` は、添字が範囲の外にならないので `Panic` を起こさない。中で `MutArray` の関数と算術を使うので、型の上の `Panic` を `try_panic` で除いてある
- `length` と `get` は配列を借りるので、読むたびに配列の参照を受け渡さない ([LIR](lir.md) の「extern の借用」)

## `Std.MutArray`

| 名前 | 型 | 実装 |
|---|---|---|
| `make` | `Int -> a -> <Panic> MutArray a` | extern |
| `tabulate` | `Int -> (Int -> <e> a) -> <Panic \| e> MutArray a` | eml |
| `empty` | `Unit -> MutArray a` | extern |
| `length` | `MutArray a -> (Int, MutArray a)` | extern (配列を借りて組を返す) |
| `read` | `MutArray a -> Int -> <Panic> (a, MutArray a)` | extern (配列を借りて組を返す) |
| `write` | `MutArray a -> Int -> a -> <Panic> MutArray a` | extern |
| `freeze` | `MutArray a -> Array a` | extern |

- `MutArray a` の Kind は要素によらず `Lin` で、線形に受け渡して使う。物体は作るときに `lin` ビットを立てる ([オブジェクトモデル](object-model.md) の「配列」)
- `make` と `tabulate` は `Std.Array` の同じ名前の関数と同じである。`empty ()` は長さ 0 の配列である
- `tabulate` は `Lin` の配列を引き回すので、`Std.Array.tabulate` と同じく、`e` に `multi` の操作が含まれると E3006 になる
- `length` と `read` は配列を借りて読み、結果と配列の組を返す。組は translate が作り、組を受けてすぐ開く所では最適化で消えるので、読むたびの確保は残らない ([Core IR](core-ir.md) の「組を返す extern」)。`read` は要素の参照を1つ増やす
- `write m i x` は、`i` 番目をその場で `x` に替える。添字が範囲の外なら `Panic` を起こす
- `freeze m` は、写さずに `lin` ビットを下ろして `Array` にする

## `Std.UnboxedArray`

| 名前 | 型 | 実装 |
|---|---|---|
| `make` | `Unboxed a => Int -> a -> <Panic> UnboxedArray a` | `Unboxed` のメソッド (instance ごとの extern) |
| `get` | `Unboxed a => UnboxedArray a -> Int -> <Panic> a` | `Unboxed` のメソッド (instance ごとの extern。配列を借りる) |
| `set` | `Unboxed a => UnboxedArray a -> Int -> a -> <Panic> UnboxedArray a` | `Unboxed` のメソッド (instance ごとの extern) |
| `length` | `UnboxedArray a -> Int` | extern (配列を借りる) |
| `thaw` | `UnboxedArray a -> MutUnboxedArray a` | extern |
| `tabulate` | `Unboxed a => Int -> (Int -> <e> a) -> <Panic \| e> UnboxedArray a` | eml (`MutUnboxedArray.freeze (MutUnboxedArray.tabulate n f)`) |
| `from_list` | `Unboxed a => List a -> UnboxedArray a` | eml |
| `to_list` | `Unboxed a => UnboxedArray a -> List a` | eml |
| `of_array` | `Unboxed a => Array a -> UnboxedArray a` | eml |
| `to_array` | `Unboxed a => UnboxedArray a -> Array a` | eml |

型 `UnboxedArray a` と `MutUnboxedArray a` は Prelude の extern の型で、関数は `Std.UnboxedArray` と `Std.MutUnboxedArray` に置く。`Std.Array` と同じく、モジュールで修飾して使う (`UnboxedArray.length a`)。

- 要素は生の 8 バイトの語で並び、`Float` の要素も箱に入らない。物体の形は [オブジェクトモデル](object-model.md) の「配列」にある
- 値の位置に要素の型が現れる関数は、クラス `MutUnboxed a => Unboxed a` のメソッドである。instance は `Float` と `Int` だけで、ほかの型の instance は意味を持たない。どちらの instance のメソッドも extern の行に結んであり、要素の Repr はそれぞれ `float` と `int` になる。2つの行は実行時の同じ操作を使う ([VM](vm.md) の「基本操作と extern」)。`MutUnboxed` は `Std.MutUnboxedArray` のクラスで、`Std.UnboxedArray` が import する
- 型を `Float` 用と `Int` 用の4つに分けず、多相な2つの型にクラスを掛けて extern の行に結ぶのは、4つに分けると、配列を扱う std の関数がどれも型ごとに2つずつ要り、`Float` と `Int` のどちらにも使える関数を書けなくなるためである。instance ごとに行を持つので、`Float` と `Int` は実行時の同じ操作を使いながら、要素の Repr だけを分けられる。そのため、新しい Repr は要らない
- クラスを Prelude に置かず、それぞれのモジュールに置くのは、`get`、`set`、`make`、`write` が修飾なしの名前としてどこからも見えるのを避けるためである
- `length` と `thaw` は要素の値を扱わないので、ふつうの多相な extern である
- `make`、`tabulate`、`get`、`set`、`thaw`、`length` は、`Std.Array` の同じ名前の関数と同じに振る舞う。誤りの文言も同じである (下の「実行時エラー」)
- `tabulate` は `Lin` の `MutUnboxedArray` の上に作ってあるので、`Std.Array.tabulate` と同じく、`e` に `multi` の操作が含まれると E3006 になる
- `from_list`、`to_list`、`of_array`、`to_array` は、添字が範囲の外にならないので `Panic` を起こさない。`Std.Array` の `from_list` と同じく、型の上の `Panic` を `try_panic` で除いてある
- 添字の式 `e[i]`、`var` の配列の変数 ([式](expressions.md) の「`var`」)、`Eq`、`Ord`、`Show` の instance はない

## `Std.MutUnboxedArray`

| 名前 | 型 | 実装 |
|---|---|---|
| `make` | `MutUnboxed a => Int -> a -> <Panic> MutUnboxedArray a` | `MutUnboxed` のメソッド (instance ごとの extern) |
| `read` | `MutUnboxed a => MutUnboxedArray a -> Int -> <Panic> (a, MutUnboxedArray a)` | `MutUnboxed` のメソッド (instance ごとの extern。配列を借りて組を返す) |
| `write` | `MutUnboxed a => MutUnboxedArray a -> Int -> a -> <Panic> MutUnboxedArray a` | `MutUnboxed` のメソッド (instance ごとの extern) |
| `fill` | `MutUnboxed a => MutUnboxedArray a -> a -> MutUnboxedArray a` | `MutUnboxed` のメソッド (instance ごとの extern) |
| `empty` | `Unit -> MutUnboxedArray a` | extern |
| `length` | `MutUnboxedArray a -> (Int, MutUnboxedArray a)` | extern (配列を借りて組を返す) |
| `freeze` | `MutUnboxedArray a -> UnboxedArray a` | extern |
| `tabulate` | `MutUnboxed a => Int -> (Int -> <e> a) -> <Panic \| e> MutUnboxedArray a` | eml |

- `MutUnboxedArray a` の Kind は要素によらず `Lin` である。instance は `Std.UnboxedArray` と同じく `Float` と `Int` だけで、ほかの型の instance は意味を持たない
- `make`、`tabulate`、`empty`、`length`、`read`、`write`、`freeze` は、`Std.MutArray` の同じ名前の関数と同じに振る舞う。`tabulate` の E3006 も同じである
- `fill m x` は、すべての要素をその場で `x` に替える。添字を取らないので `Panic` を起こさない

## `Std.Map`

キーの順に並べた写像である。AVL 木で、`Std.List` を import して書いてある。

```haskell
pub abstract data Map k v =
  | Empty
  | LeftHigh (Map k v) k v (Map k v)
  | Even (Map k v) k v (Map k v)
  | RightHigh (Map k v) k v (Map k v)
```

| 関数 | 型 |
|---|---|
| `empty` | `Map k v` |
| `insert` | `Ord k => k -> v -> Map k v -> Map k v` |
| `upsert` | `Ord k => k -> (Option v -> <e> v) -> Map k v -> <e> Map k v` |
| `delete` | `Ord k => k -> Map k v -> Map k v` |
| `lookup` | `Ord k => k -> Map k v -> Option v` |
| `member` | `Ord k => k -> Map k v -> Bool` |
| `size` | `Map k v -> <Panic> Int` |
| `fold` | `(b -> k -> v -> <e> b) -> b -> Map k v -> <e> b` |
| `to_list` | `Map k v -> List (k, v)` |
| `from_list` | `Ord k => List (k, v) -> Map k v` |

- 3つのコンストラクタ `LeftHigh`、`Even`、`RightHigh` は、左右の部分木の高さの差を表す。左が1高いか、同じか、右が1高いかである。高さを数で持たないので、釣り合いの計算に算術がなく、`Panic` も起きない
- 挿入と削除は、部分木の高さが増えたか減ったかを `Bool` で返す。釣り合いを直す関数は、組み直す節を受け取り、同じ関数の中で節を作る。そのため、受け取った節の箱を使い回せる ([LIR](lir.md) の「reuse」)
- `upsert key f m` は、`key` があれば値を `f (Some v)` に、なければ `f None` を入れる。`f` はちょうど1回呼び、キーは前からあるものを残す。`insert key value m` は `upsert key (fn _ -> value) m` と同じである
- `delete key m` は、`key` がなければ同じ木を返す。`lookup` は値を `Some` で返し、なければ `None` である。`member` はキーがあるかを返す
- `fold f init m` は、キーの昇順に `f acc k v` を呼んで左からたたむ。`to_list` は、キーの昇順の組のリストである
- `from_list` は、前から順に `insert` する。同じキーが複数あれば、後の組が勝つ
- `size` は木を O(n) で数える。数えるのに `+` を使い、`Int` の桁あふれが `Panic` になりうるので、型に `Panic` を持つ
- `size` のほかの関数は、型に `Panic` を持たない。トップレベルの値 (row は `<>` に限る) と `Show` の instance の本体でも使える
- キーも値も `Unr` に限る。`Lin` の値は E3001 になる。釣り合いを直す関数の `_ -> t` の枝が、値を捨てるためである
- コンストラクタは隠す (`pub abstract data`)。API は上の関数の表だけである

## `Std.Set`

要素の順に並べた集合である。`Std.Map` と同じ形の AVL 木で、節は値を持たない。`Std.List` を import して書いてある。

```haskell
pub abstract data Set a =
  | Empty
  | LeftHigh (Set a) a (Set a)
  | Even (Set a) a (Set a)
  | RightHigh (Set a) a (Set a)
```

| 関数 | 型 | 計算量 |
|---|---|---|
| `empty` | `Set a` | |
| `insert` | `Ord a => a -> Set a -> Set a` | O(log n) |
| `delete` | `Ord a => a -> Set a -> Set a` | O(log n) |
| `member` | `Ord a => a -> Set a -> Bool` | O(log n) |
| `size` | `Set a -> <Panic> Int` | O(n) |
| `fold` | `(b -> a -> <e> b) -> b -> Set a -> <e> b` | O(n)、昇順 |
| `to_list` | `Set a -> List a` | O(n)、昇順 |
| `from_list` | `Ord a => List a -> Set a` | O(n log n) |
| `union` | `Ord a => Set a -> Set a -> Set a` | O(m log n) |
| `intersection` | `Ord a => Set a -> Set a -> Set a` | O(m log n) |
| `difference` | `Ord a => Set a -> Set a -> Set a` | O(m log n) |

- 3つのコンストラクタ `LeftHigh`、`Even`、`RightHigh` は、`Map` と同じく左右の部分木の高さの差を表す。釣り合いの計算に算術がなく、`Panic` も起きない。釣り合いを直す関数が、受け取った節の箱を使い回せることも `Map` と同じである ([LIR](lir.md) の「reuse」)
- `insert x s` は、`Ord` で `x` と等しい要素が `s` にあれば、前からあるものを残す。`delete x s` は、`x` と等しい要素がなければ同じ木を返す。`member` は、等しい要素があるかを返す
- `fold f init s` は、昇順に `f acc x` を呼んで左からたたむ。`to_list` は、昇順のリストである
- `from_list` は、前から順に `insert` する。等しい要素が複数あれば、先のものが残る
- `union`、`intersection`、`difference` は、`Ord` で等しい要素のうち、左の引数のものを残す。`difference a b` は、`a` の要素のうち `b` にないものである
- 3つの演算は、低いほうの木をたどって高いほうの木に入れる。計算量の m は低いほうの木の大きさ、n は高いほうの木の大きさである。木の高さは、印の高いほうの子をたどって O(log n) で求め、`1 <<< 高さ` として数えるので、`Panic` は起きない
- `size` は木を O(n) で数える。数えるのに `+` を使い、`Int` の桁あふれが `Panic` になりうるので、型に `Panic` を持つ。`size` のほかの関数は、型に `Panic` を持たない。トップレベルの値 (row は `<>` に限る) と `Show` の instance の本体でも使える
- 要素は `Unr` に限る。`Lin` の値は E3001 になる。釣り合いを直す関数の `_ -> t` の枝が、値を捨てるためである
- コンストラクタは隠す (`pub abstract data`)。API は上の関数の表だけである

## `Env`

| 関数 | 型 |
|---|---|
| `args` | `Unit -> <IO, Panic> List String` |
| `get` | `String -> <IO> Option String` |

- `args ()` は、上の「起動」の引数である。REPL では `[]` を返す
- `get name` は環境変数の値である。設定されていなければ `None`、空の値なら `Some ""` を返す。空の名前と、`=` か NUL を含む名前は、設定されていないとみなす
- UTF-8 でない引数や値は、読んだときに実行時エラーにする

## ハンドル

`Stdin.Stdin` と `FileIn.FileIn` は読むためのハンドルで、`Stdout.Stdout`、`Stderr.Stderr`、`FileOut.FileOut` は書くためのハンドルである。ハンドルの型ごとに1つのモジュールを置き、どのモジュールも、開く関数 (`open`、`FileOut` では `create` と `append`)、`close`、次の規則の名前の読みか書きを持つ。読みと書きを型で分けるので、取り違えは型の誤り (E2001) になる。`Proc.Child` は子のプロセスとやりとりするハンドルで、子の標準入力への書きと標準出力の読みの両方を持つ。`Proc.spawn` で作り、`close` の代わりに `Proc.wait` で使い終える (下の「`Proc`」の「`Child`」)。`Child` もほかのハンドルとは別の型なので、`FileIn` の関数に渡すと型の誤りになる。

関数の名前は、std 全体で次の規則に従う。文字列を読み書きする関数が短い名前を持ち、バイト列向けの関数は `_bytes` を足す。読みは `read_all` と `read_all_bytes`、`read_line` と `read_line_bytes` で、書きは `write` と `write_bytes`、文字列だけの `write_line` である。バイト列にしかない読み (`read_chunk`) は、その名前のままである。スクリプトの大半は文字列を読み、文字列の読みが UTF-8 を確かめることが「`String` はいつも妥当な UTF-8」を支えるので、文字列向けの関数を既定にする。

- ハンドルは線形な型 (`Lin`) で、ちょうど1回使う ([線形性](linearity.md))。`Stdin`、`Stdout`、`Stderr`、`FileIn` の `close h` は `drop h` と同じで、ハンドルを手放して閉じる。`FileOut` の `close` は書き出してから閉じ、書き出せなければ誤りを返す (下の「`FileOut`」)
- 読む関数はハンドルを借りて読み、結果を先に、同じハンドルを後に置いた組で返す。`Stdin`、`FileIn`、`Child` は同じ名前と同じ形の読む関数を持つ (下の「`Stdin`」、「`FileIn`」、「`Proc`」)
- 書く関数は書くデータを借り、ハンドルを受け取って同じハンドルを返す。`Stdout`、`Stderr`、`FileOut`、`Child` は同じ名前と同じ形の書く関数を持つ (下の「`Stdout` と `Stderr`」、「`FileOut`」、「`Proc`」)
- 読めるハンドルの上の汎用のコードは、読む関数を引数で渡して書く。`count_lines Stdin.read_line h 0` も `count_lines FileIn.read_line f 0` も、直接書いたループと同じ費用で動く

  ```haskell
  count_lines : (h -> <IO, Panic> (Option String, h)) -> h -> Int -> <IO, Panic> (Int, h)
  count_lines read h n =
    match read h with
      | (None, h) -> (n, h)
      | (Some _, h) -> count_lines read h (n + 1)
  ```

- 標準のストリームのハンドルは `Stdin.open ()`、`Stdout.open ()`、`Stderr.open ()` で開く。ストリームごとに、生きているハンドルは高々1つで、生きている間の2つ目の `open` は `Panic` (`the standard input is already open`、`the standard output is already open`、`the standard error is already open`) になる。閉じた後は、また開ける。ストリームの持ち主はいつも1人なので、読む順と書く順はプログラムの順である。持ち主を1人にするのは、`Lin` のハンドルに標準のストリームを持つ意味を与え、標準入力と `FileIn` が1つの読みの道を共有するためである
- 標準入力のハンドルは、標準入力の読み手 (64 KiB の buffer) を持つ。開くときに Host から読み手を取り、閉じるときに buffer に残ったバイトごと Host に返す。そのため、次に開いたハンドルは続きから読み、`Stdin.read_text ()` のように開いて閉じる関数を続けて呼んでも、読んでいない入力は失われない
- 標準入力を持たない Host (REPL) でも `open` は通り、読みが I/O の失敗 (`cannot read the standard input: not available`) になる。この Host でも、生きている標準入力のハンドルは高々1つで、2つ目の `open` は `Panic` になる
- 標準のストリームの書くハンドルは buffer を持たない。Host がストリームごとに1つ持つ buffer に書き、`println` と `eprintln` も同じ buffer に書く。そのため、`close`、巻き戻し、`exit` のどれでハンドルを手放しても、書いた出力は失われない (下の「`Stdout` と `Stderr`」)。`FileOut` は自分の buffer を持ち、手放すときに書き出す (下の「`FileOut`」)
- ハンドルを持ったまま中断 (`try_io`、`try_panic`、`never` の操作) で巻き戻ると、巻き戻しがハンドルを手放して閉じる。読みの誤りでも、読みの extern はハンドルを手放さない。`try_io` の下では巻き戻しが手放し、実行時エラーなら実行がそこで止まる
- 標準入力の読みは、読み手の buffer を補充する前ごとに標準出力を書き出す。行の途中の補充と、buffer が空の `read_chunk` も同じである。buffer から読むときは書き出さない。`read_all` と `read_all_bytes` は、最初の補充の前に1回書き出す。入力を pipe で先読みしたとき、buffer にある2つの行の間に書いたプロンプトは、次の補充かプログラムの終わりまで出ない (C と同じ。[オブジェクトモデル](object-model.md) の「読み手」)

失敗と届け方は次のとおりである。文言は [LIR](lir.md) の「実行時エラー」にある。

| 操作 | 失敗 | 届け方 |
|---|---|---|
| `open` | 同じストリームのハンドルが生きている (`InUse`) | `Panic` (`try_panic`) |
| `close` (`FileOut` のほか) | なし | — |
| `Stdin` の読み | 読めない (`Stdin(reason)`)。標準入力がなければ理由は `not available` | `IO` (`try_io`) |
| `FileIn` の読み | 読めない (`Io { op: Read, .. }`) | `IO` |
| `FileOut.create`、`append` | 開けない (`Io { op: Open, .. }`) | `IO` |
| `FileOut` の書き込み (`write`、`write_bytes`、`write_line`、`flush`、`close`) | 書けない (`Io { op: Write, .. }`)。ハンドルは閉じている (下の「`FileOut`」) | `IO` |
| `Child` の読み | 読めない (`Io { op: ReadOutput, .. }`) | `IO` |
| `Proc.wait` | 待てない (`Io { op: Wait, .. }`) | `IO` |
| `Child` の書き込み (`write`、`write_bytes`、`write_line`) | 書けない (`Io { op: Write, .. }`)。ハンドルを手放し、破棄処理が子を kill する (下の「`Proc`」) | `IO` |
| 文字列の読み | UTF-8 でない (`NotUtf8`)。バイト列の読みはこれで失敗しない | `IO` (`kind` は `NotUtf8`) |
| どの読みも | 割り込み、確保量の上限 | 位置を付けた実行時エラー |
| `Child` の書き込み、`Proc.wait` | 割り込み | 位置を付けた実行時エラー |
| 標準のストリームの書き込み (`write`、`write_bytes`、`write_line`、`flush`) | 書けない (`Output`) | 実行時エラー。`try_io` では捕まえない (上の「`try_io`」) |
| どの操作も | 検査モードのヒープの誤り | 実行時エラー |

## `Stdin`

| 関数 | 型 |
|---|---|
| `open` | `Unit -> <IO, Panic> Stdin` |
| `close` | `Stdin -> <IO> Unit` |
| `read_text` | `Unit -> <IO, Panic> String` |
| `read_all` | `Stdin -> <IO> (String, Stdin)` |
| `read_line` | `Stdin -> <IO, Panic> (Option String, Stdin)` |
| `read_all_bytes` | `Stdin -> <IO> (Bytes, Stdin)` |
| `read_line_bytes` | `Stdin -> <IO, Panic> (Option Bytes, Stdin)` |
| `read_chunk` | `Bytes -> Stdin -> <IO> (Bytes, Stdin)` |

`Stdin` は標準入力のハンドルの型である (上の「ハンドル」)。

- `read_text ()` は、ハンドルを開いて標準入力の残りをすべて読み、閉じる。`open`、`read_all`、`close` で書いた関数である
- 読む関数は、同じ名前の `FileIn` の関数と同じ意味である (下の「`FileIn`」)。`read_all` は残りをすべて読み、`read_line` は1行を読んで行の終わり (`\n` か `\r\n`) を `String.trim_line_end` で除き、終わりなら `None` を返す。行の終わりのない最後の行も1行として返す。`read_all_bytes`、`read_line_bytes`、`read_chunk` はバイト列の読みで、どのバイトも受ける
- `read_line` と `read_line_bytes` は、std の中だけの extern `read_line_raw` と `read_line_bytes_raw` の上に書いてある。どちらも行の終わりを含む1行を返し、終わりなら空を返す (上の「extern の境界」の番兵)
- 読みは `FileIn` と同じく、64 KiB ずつ読みながら割り込みと確保量の上限を見る
- REPL では、標準入力を REPL 自身が読むので、どの読みも I/O の失敗 (`cannot read the standard input: not available`) にする

## `Stdout` と `Stderr`

| 関数 | 型 |
|---|---|
| `Stdout.open` | `Unit -> <IO, Panic> Stdout` |
| `Stdout.close` | `Stdout -> <IO> Unit` |
| `Stdout.write` | `String -> Stdout -> <IO> Stdout` |
| `Stdout.write_bytes` | `Bytes -> Stdout -> <IO> Stdout` |
| `Stdout.write_line` | `String -> Stdout -> <IO> Stdout` |
| `Stdout.flush` | `Stdout -> <IO> Stdout` |

`Stdout` は標準出力のハンドルの型で、`Stderr` は標準エラーのハンドルの型である (上の「ハンドル」)。`Stderr` は、`flush` のほかの5つを同じ名前と同じ形で持つ。2つは別の型なので、一方のハンドルを他方の関数に渡すと型の誤り (E2001) になる。

- `write s h` は文字列 `s` をそのまま書き、`write_bytes b h` はバイト列 `b` をそのまま書き、`write_line s h` は `s` と `\n` を書く。`Stdout.write_line` は `println` と、`Stderr.write_line` は `eprintln` と同じ書き方である。どれもデータを借りて、受け取ったハンドルを返す
- 1回の書き込みは、ストリームの lock の下で Host を1回呼ぶ。1回の書き込みはほかの書き込みと途中で混ざらないが、2回の書き込みの間には、ほかの書き込みが入りうる
- 書き込みは、Host がストリームごとに持つ buffer に入る。標準出力は、端末なら `\n` を含む書き込みが書き出し、端末でなければ 64 KiB ずつまとめて書く。標準エラーは書くたびに書き出すので、`Stderr` は `flush` を持たない ([オブジェクトモデル](object-model.md) の「出力のバッファ」)
- `Stdout.flush h` は、標準出力の buffer を書き出す。ほかに Host が buffer を書き出すのは、`main` の終わり、`exit`、実行時エラーを出す前、割り込みか上限で止めたとき、REPL の入力の区切り、子を起動する前、標準入力の読み手の補充の前である
- 1回の書き込みは、extern の呼び出しと Host の呼び出しの費用を持つ。1バイトずつ出すときは、`Bytes.push` で `Bytes` を組んでから1回書く
- 書けないとき (`Output`) は、`println` と同じく実行時エラーで、`try_io` では捕まえない。書き込みの extern は、そのときハンドルを手放す

## `FileIn`

| 関数 | 型 |
|---|---|
| `open` | `String -> <IO> FileIn` |
| `close` | `FileIn -> <IO> Unit` |
| `read_all` | `FileIn -> <IO> (String, FileIn)` |
| `read_line` | `FileIn -> <IO, Panic> (Option String, FileIn)` |
| `read_all_bytes` | `FileIn -> <IO> (Bytes, FileIn)` |
| `read_line_bytes` | `FileIn -> <IO, Panic> (Option Bytes, FileIn)` |
| `read_chunk` | `Bytes -> FileIn -> <IO> (Bytes, FileIn)` |

`FileIn` はファイルを読むハンドルの型である (上の「ハンドル」)。`open`、`read_all`、`close` と `FileIn` の型は [エフェクトと handler](effects.md) の「組み込みの `IO`」にある。

- `open` はディレクトリを開かない。どの OS でも、権限で開けないディレクトリも含めて ``cannot open `<path>`: I/O error`` になる
- `read_line file` は、今の位置から1行を読み、行の終わり (`\n` か `\r\n`) を `String.trim_line_end` で除いて、結果を先に、同じ `FileIn` を後に置いた組で返す。終わりなら `None` を返す。行の終わりのない最後の行も1行として返す。`Stdin.read_line` と同じである
- `read_line` は、std の中だけの extern `read_line_raw : FileIn -> <IO> (String, FileIn)` の上に書いてある。`read_line_raw` は行の終わりを含む1行を返し、終わりなら空の文字列を返す (上の「extern の境界」の番兵)。読み方は `read_all` と同じく `FileIn` の読み手から読み、64 KiB を超える行も、割り込みと確保量の上限を見ながら読む ([オブジェクトモデル](object-model.md) の「読み手」)
- 読めないときと UTF-8 でない行の誤りは、文言も `try_io` での捕まえ方も `read_all` と同じである ([LIR](lir.md) の「実行時エラー」)。誤りのときも `read_all` と同じく、extern は `FileIn` を手放さない。`try_io` の下では巻き戻しが手放して閉じ、実行時エラーなら実行がそこで止まる
- バイト列の読み (`read_all_bytes`、`read_line_bytes`、`read_chunk`) は、どのバイトも受け、UTF-8 を確かめないので、`NotUtf8` で失敗しない。ほかは文字列の読みと同じで、`FileIn` を借りて、結果を先に、同じ `FileIn` を後に置いた組で返す。読めないときの誤りと、割り込みと確保量の上限の扱いも同じである
- `read_all_bytes file` は、今の位置から終わりまでをバイト列として読む。`read_all` と同じく Rust の buffer に読んでから1回写す
- `read_line_bytes file` は、今の位置から1行を読み、行の終わりを `Bytes.trim_line_end` で除いて返す。終わりなら `None` を返す。std の中だけの extern `read_line_bytes_raw : FileIn -> <IO> (Bytes, FileIn)` の上に書いてあり、`read_line_raw` と同じく、行の終わりを含む1行か、終わりなら空のバイト列を返す。読み手の buffer から行を直接ヒープのバイト列に写すので、1つの buffer に収まる行は1回だけ写す。64 KiB の補充をまたぐ行は、補充ごとにバイト列を伸ばし、伸ばすときに `Bytes.push` と同じく容量を倍にして作り直しうる (それまでの部分を写し直す)
- `read_chunk buf file` は、読み手の buffer に残ったバイトを、空なら1回補充して読めたバイトを、`buf` の後ろに足して返す。終わりなら何も足さない。`buf` は消費し、`Bytes.push` と同じく、一意で容量が足りればその場で伸ばす。読みが失敗したときは `buf` を手放してから誤りを返す。1回で足すバイトは 64 KiB 以下である

## `FileOut`

| 関数 | 型 |
|---|---|
| `create` | `String -> <IO> FileOut` |
| `append` | `String -> <IO> FileOut` |
| `close` | `FileOut -> <IO> Unit` |
| `write` | `String -> FileOut -> <IO> FileOut` |
| `write_bytes` | `Bytes -> FileOut -> <IO> FileOut` |
| `write_line` | `String -> FileOut -> <IO> FileOut` |
| `flush` | `FileOut -> <IO> FileOut` |

`FileOut` はファイルを書くハンドルの型である (上の「ハンドル」)。線形な型 (`Lin`) で、ちょうど1回使う。

- `create path` は、ファイルを作るか切り詰めて開く。`append path` は、ファイルを作るか、末尾に足す形で開く。相対パスの扱いは `FileIn.open` と同じである ([エフェクトと handler](effects.md) の「組み込みの `IO`」)。ディレクトリは開かず、どの OS でも ``cannot open `<path>`: I/O error`` になる
- 書く関数は、`Stdout` の同じ名前の関数と同じ意味である (上の「`Stdout` と `Stderr`」)。`write s f` は文字列 `s` を、`write_bytes b f` はバイト列 `b` をそのまま書き、`write_line s f` は `s` と `\n` を書く。どれもデータを借りて、受け取った `FileOut` を返す
- `FileOut` は自分の書き込みの buffer (64 KiB) を持つ。ファイルに書き出すのは、buffer があふれたとき、`flush` を呼んだとき、`close` を呼んだときの3つだけである。`fsync` はしない。`main` の終わりや実行時エラーの前の書き出しは、標準出力の buffer だけを書き出し、`FileOut` の buffer に触れない
- `close f` は、書き出してから閉じる。std の中だけの extern `close_raw : FileOut -> <IO> Unit` の上に、`close h = close_raw (flush h)` と書いてある。書き出せなければ `close_raw` に進まず、`flush` の誤りを返す
- 閉じずに手放したときも、破棄処理が buffer を書き出し、誤りを捨てる。巻き戻し (`try_io`、`try_panic`、`never` の操作)、`exit`、捕まえなかった実行時エラーのどれで手放しても、書いた中身は書き出される。失うのは、その場で終わる2回目の Ctrl-C と2回目の SIGTERM、abort のときだけである ([オブジェクトモデル](object-model.md) の「出力のバッファ」)

`write`、`write_bytes`、`write_line`、`flush`、`close` が書けなかったときは、`Io { op: Write, .. }` (``cannot write to `<path>`: <reason>``) を返す。標準出力と違って書き込みの順で誤りが決まるので、`try_io` で捕まえる。誤りを返したときの約束は次の4つである。

1. ハンドルは閉じている
2. ファイルには、そのハンドルに渡したバイトの先頭の部分が残る。どこまで書けたかは約束しない
3. 書いていない残りの buffer は捨て、それ以上書かない。そのため、誤りを返した後にファイルが伸びない
4. 誤りは、この呼び出しか、同じハンドルのそれより前の書き込みの失敗を表す

## `Fs`

| 関数 | 型 |
|---|---|
| `read_text` | `String -> <IO> String` |
| `write_text` | `String -> String -> <IO> Unit` |

パスで1回に済む関数である。ハンドルを使う読みは `FileIn` に、書きは `FileOut` にある。`Fs` は extern を持たず、どの関数も `FileIn` と `FileOut` の上に eml で書いてある。パスの操作も、足すときは `Fs` に置く。

- `read_text path` は、ファイル全体を文字列として読む。`FileIn.open`、`FileIn.read_all`、`FileIn.close` で書いた関数である
- `write_text path text` は、ファイルを作るか切り詰めて、`text` を書く。`write_text path text = FileOut.close (FileOut.write text (FileOut.create path))` である。開けないときと書けないときの誤りは、`FileOut` の関数の誤りである

## `Proc`

| 関数 | 型 |
|---|---|
| `run` | `Cmd -> <IO> Int` |
| `output` | `Cmd -> <IO> (String, Int)` |
| `output_bytes` | `Cmd -> <IO> (Bytes, Int)` |
| `capture` | `Cmd -> <IO> String` |
| `capture_bytes` | `Cmd -> <IO> Bytes` |
| `spawn` | `Cmd -> <IO> Child` |
| `.\|` | `Cmd -> Cmd -> Cmd` |
| `cwd` | `String -> Cmd -> Cmd` |
| `env` | `String -> String -> Cmd -> Cmd` |
| `arg` | `String -> Cmd -> Cmd` |
| `args` | `List String -> Cmd -> Cmd` |
| `merge_stderr` | `Cmd -> Cmd` |
| `discard_stderr` | `Cmd -> Cmd` |
| `stdin_text` | `String -> Cmd -> Cmd` |
| `stdin_file` | `String -> Cmd -> Cmd` |
| `no_stdin` | `Cmd -> Cmd` |
| `stdout_file` | `String -> Cmd -> Cmd` |
| `stdout_append` | `String -> Cmd -> Cmd` |
| `write` | `String -> Child -> <IO> Child` |
| `write_bytes` | `Bytes -> Child -> <IO> Child` |
| `write_line` | `String -> Child -> <IO> Child` |
| `close_stdin` | `Child -> <IO> Child` |
| `read_all` | `Child -> <IO> (String, Child)` |
| `read_line` | `Child -> <IO, Panic> (Option String, Child)` |
| `read_all_bytes` | `Child -> <IO> (Bytes, Child)` |
| `read_line_bytes` | `Child -> <IO, Panic> (Option Bytes, Child)` |
| `read_chunk` | `Bytes -> Child -> <IO> (Bytes, Child)` |
| `wait` | `Child -> <IO> Int` |
| `wait_all` | `Child -> <IO> List Int` |

子のプロセスを起動する。子はシェルを介さず、`Cmd` のプログラムと引数のリストをそのまま OS に渡す。起動の形は3つある。標準入出力を受け継ぐ `run`、標準出力を取る `output` と `capture`、pipe でやりとりする `spawn` である。どの形でも、引数と設定は同じ `Cmd` で組む。`Cmd` は、`.|` でつないだパイプラインでもよい (下の「パイプライン」)。

### `Cmd`

`Cmd` は Prelude のレコードの型で、プログラム、引数のリスト、作業ディレクトリ、環境変数の上書き、標準入力、標準出力、標準エラーの向きを持つ ([式](expressions.md) の「コマンドリテラル」)。

- `Cmd` の値は、コマンドリテラルとこのモジュールの設定の関数で組む。プログラムを変数で与えるときは `` `\{prog} \{..args}` `` と書く
- `Cmd` は `Eq` と `Show` の instance を持つ。`show` は全部のフィールドを `Cmd { program = "git", args = ["log"], … }` の形で表示し、`display` は人が読む1行で、プログラムと引数を空白でつなぐ。`display` は引用を戻さないので、コマンドリテラルには戻せない。パイプラインの `show` は、前の段を標準入力の `StdinFrom Cmd { … }` に入れ子にして表示し、`display` は段を先頭から ` | ` でつなぐ (`sort -r | uniq -c`)
- フィールドの名前と数は処理系の都合で変わりうる。そのため、利用者のコードは `Cmd { … }` の作る式とパターンを書かない。読むだけの射影 (`cmd.program`、`cmd.args`) は書いてよい。向きの型 `StdinMode`、`StdoutMode`、`StderrMode` のコンストラクタも、設定の関数が書くので、利用者は書かない
  - 見直す条件: `Cmd` の設定の関数を Prelude へ移すか、読めるフィールドの `pub` を入れたときに隠す
- `Cmd` を `extern data` にしないのは、`Unr` の `Foreign` は REPL の回復で写せず ([オブジェクトモデル](object-model.md) の「種類」)、`` let c = `ls` `` の後の回復がどれも失敗するためである。`Lin` の `Foreign` にすると、同じ `Cmd` を2回起動できない
- 設定の関数は対象の `Cmd` を最後に受ける。`` `make` |> Proc.cwd "build" |> Proc.merge_stderr |> Proc.capture `` のように `|>` でつなぐためである。`with_` の名前は、handler を呼ぶ関数の慣習 ([例](examples.md) の `with_env`) に取っておく
- `cwd dir c` は作業ディレクトリを `dir` にする。相対パスは `FileIn.open` と同じく Host の `file_root` から解釈する。2回設定したら、後の設定が前の設定を置き換える。ないディレクトリは、起動の誤りの理由 `not found` (``cannot run `make`: not found``) になり、プログラムがないときと区別しない
- `env name value c` は、子の環境変数 `name` を `value` に上書きする。子はほかの環境変数を eml のプロセスから受け継ぐ。同じ名前を2回設定したら、後の組が勝つ。空の名前と `=` を含む名前は OS に渡せないので、起動の誤り (`I/O error`) にする。パイプラインでは、ほかの設定と同じく、設定した段の環境だけを変える
- `arg a c` と `args xs c` は、引数を後ろに足す
- `merge_stderr c` は、子の標準エラーを標準出力と同じ先に向ける (`2>&1`)。`discard_stderr c` は捨てる (`2>/dev/null`)。どれも書かなければ、標準エラーは受け継ぐ。`Exec.pipe_stderr c` は標準エラーを pipe にする。使えるのは `Exec.spawn` の最後の段だけで、ほかは起動の誤りである (下の「パイプライン」と「`Std.Exec`」)
- `stdin_text s c` は、子の標準入力を文字列 `s` にする。`stdin_file path c` は、ファイル `path` を子の標準入力にする (`< path`)。`no_stdin c` は、子の標準入力を空 (`/dev/null`) にする
- `stdout_file path c` は、子の標準出力をファイル `path` に書かせる (`> path`)。ファイルは作るか切り詰めて開く。`stdout_append path c` は、作るか末尾に足す形で開く (`>> path`)
- 標準入力と標準出力の向きは、どちらも書かなければ起動の形が決める (下の「起動」)。2回設定したら、後の設定が前の設定を置き換える。`Cmd` の値の中では、標準入力は `StdinMode` (`DefaultStdin`、`NullStdin`、`StdinText s`、`StdinFile path`、前の段の `StdinFrom c`)、標準出力は `StdoutMode` (`DefaultStdout`、`NullStdout`、`StdoutFile path`、`StdoutAppend path`)、標準エラーは `StderrMode` (`InheritStderr`、`MergeStderr`、`DiscardStderr`、`PipeStderr`) で、`show` はこのコンストラクタで表示する

### パイプライン

- `left .| right` は、`left` の標準出力を `right` の先頭の段の標準入力につないだパイプラインである (`infixl 2`)。`|>` (`infixl 1`) より強く結ぶので、`` `sort` .| `uniq -c` |> Proc.capture `` と書ける。パイプラインも `Cmd` の値で、先頭の段の標準入力が `StdinFrom` の鎖で前の段を持つ。ほかのモジュールの演算子なので、`import Proc ((.|))` で取り込んで使う
- パイプラインの値は、その最後の段である。入力の設定 (`.|`、`stdin_text`、`stdin_file`、`no_stdin`) は `StdinFrom` の鎖をたどって先頭の段に当て、その段の標準入力を置き換える。そのため `stdin_text s (a .| b)` と、`stdin_text s a` に `.| b` をつないだものは、どちらも `a` に文字列を渡し、`x .| (a .| b)` は `x` を `a` につなぐ。ほかの設定 (`cwd`、`env`、`arg`、`args`、`merge_stderr`、`discard_stderr`、`stdout_file`、`stdout_append`、`Exec.pipe_stderr`) は、渡した値そのもの、つまりパイプラインの最後の段に当たる。前の段を設定するには、`.|` でつなぐ前に設定する
- すべての段を1回の Host の `spawn` で先頭から起動し、段の間を OS の pipe でつなぐ。親は段の間の pipe の端を1本も持たないので、後ろの段が先に終わると、前の段の書き込みに EPIPE か SIGPIPE が届く。標準入力の向きは先頭の段に、標準出力の向きは最後の段に当たる。標準エラーは段ごとの設定に従い、`merge_stderr` の段の標準エラーは次の段への pipe に入る (`2>&1 |`)
- 最後でない段の標準出力は次の段への pipe なので、ほかの向き (`stdout_file`、`stdout_append`、`NullStdout`) を設定した段があれば、子を1つも起動せずに起動の誤り (`kind` は `Other`、``cannot run `<program>`: the standard output of a stage that is not last is redirected``) にする。名前は、そう設定した先頭に近い段のプログラムである
- 標準エラーの pipe (`PipeStderr`) を読めるのは `Exec.spawn` の最後の段だけである。`Proc` のどの形と、`Exec.run`、`output`、`output_bytes`、`capture`、`capture_bytes` の `PipeStderr` と、最後でない段の `PipeStderr` は、子を1つも起動せずに起動の誤り (`kind` は `Other`、``cannot run `<program>`: the standard error pipe needs `Exec.spawn` on the last stage``) にする。黙って捨てたり受け継いだりはしない。名前は、そう設定した先頭に近い段のプログラムで、標準出力のリダイレクトの誤りと同じ段にあれば、そちらを先に出す
- `wait`、`run`、`output`、`capture` の終了コードは最後の段のものである。`wait_all h` は、すべての段の終了コードを先頭から並べたリストを返す。`pipefail` はない。読み手が先に終わって SIGPIPE で終わった段の 141 (unix) は、`wait_all` にだけ現れる
- 段 `k` を起動できなければ、それより前に起動した段を kill して待ち、段 `k` のプログラムの起動の誤り (``cannot run `<program>`: not found``) にする

### 起動

- `run c` は、標準入出力を受け継いで子を起動し、終わるのを待って終了コードを返す
- `output c` は、子の標準出力を文字列に読み、終了コードと組で返す。子の標準入力は空 (`/dev/null`) である。`output_bytes` はバイト列向けの関数で、UTF-8 を確かめない
- `capture c` は `output` と同じく標準出力を読み、終了コードが 0 なら読んだ文字列を返す。0 でなければ IO の失敗 (`kind` は `Other`、``command `git` exited with code 128``) で、読んだ出力は捨てる。`capture_bytes` はバイト列向けの関数である。失敗するコマンドは、バグでなく IO の条件として `try_io` で扱う
- `spawn c` は、標準入力と標準出力を pipe にして子を起動し、`Child` を返す (下の「`Child`」)
- 上の標準入出力は、`Cmd` が向きを設定していないときの既定である。設定した向きは、どの起動の形でもそのまま使う。`output`、`output_bytes`、`capture`、`capture_bytes` は、標準出力の向きを設定した (`DefaultStdout` でない) 子からは何も読まず、空の文字列か空のバイト列を結果にする。`stdout_file` と `stdout_append` で向けた子も、`NullStdout` の子も同じである。`spawn` の子の標準入力か標準出力が pipe でなければ、その向きの書き込みと読みは `not available` で失敗する (下の「`Child`」)
- `stdin_text` の文字列は、Host が一時ファイルに書いてから子の標準入力にする。eml は pipe にもスレッドにも書かないので、読みながら書く子でも、文字列の大きさによらず詰まらない。子には、名前のない、先頭から読める通常のファイルに見え、seek もできる。unix では作ってすぐ消し、Windows では閉じたときに OS が消す。シェルの here-string と同じ見え方である
- `stdin_file`、`stdout_file`、`stdout_append` のパスは、`FileIn.open` と同じく Host の `file_root` から解釈し、`cwd` の設定には依らない。開き方は `FileIn.open`、`FileOut.create`、`FileOut.append` と同じで、ディレクトリは開かない
- 標準エラーの向きはどの形でも `Cmd` の設定に従う。`merge_stderr` の標準エラーは、`output` と `capture` では文字列に入り、`run` では標準出力に出て、`spawn` では `read_*` で読める。`PipeStderr` は起動の誤りである (上の「パイプライン」)
- 終了コードは子の終了コードで、シグナルで終わった子は、シェルと同じく 128 + シグナルの番号にする。これは unix だけで、Windows にはシグナルで終わる子がない
- 起動の前に標準出力を書き出す。子と eml の出力が順に並ぶようにするためである。eml が標準入力から先読みしてバッファに持っているバイトは、子には届かない
- プログラムが `/` を含まなければ、`PATH` から探す。`/` を含む相対パス (`./run.sh`) は、子の作業ディレクトリ (`cwd` の設定か、なければ `file_root`) に対して解決し、Host が絶対パスにしてから起動する。Rust の `Command` は、作業ディレクトリと相対のプログラムを組み合わせたときの意味を決めていないためである
- Ctrl-C は同じプロセスグループ (Windows では同じコンソール) の子にも届く。子が終わった後、割り込みが来ていれば、子を待っていた `wait` (`run`、`output`、`capture` の中の `wait` を含む) はその場で止まる (終了コード 130)

`capture` の誤りは、次の順に出る。

1. 起動の誤り (``cannot run `git`: not found``)
2. 標準出力の読みの誤り (``cannot read the output of `git`: I/O error``、``the output of `git` is not valid UTF-8``)
3. 子を待つ誤り (``cannot wait for `git`: I/O error``)
4. 終了コード (``command `git` exited with code 128``)

この順は、`output` の本体 `let child = spawn_from … c; if piped c then (let (out, child) = read_all child; (out, wait child)) else ("", wait child)` (`piped c` は標準出力の向きが `DefaultStdout` であること) と、それを呼ぶ `capture` の本体 `let (out, code) = output c; if code == 0 then out else exited c.program code` の順そのものである。そのため、UTF-8 でない出力を出して 0 でなく終わった子は、`NotUtf8` で失敗する。読みの失敗で巻き戻るときは、`Child` の破棄処理が子を kill する (下の「`Child`」)。

std の中だけの extern は次の6つである。

- `spawn_raw : String -> Int -> String -> Int -> String -> <IO> Child` は、どの起動の形も呼ぶ。引数は次の5つである
  1. 段の列。段ごとに、プログラム、引数の列、環境の列、作業ディレクトリ (空なら設定しない)、標準エラーの向き (十進の文字列で、0 は受け継ぐ、1 は標準出力に合わせる、2 は捨てる、3 は pipe) の5つの要素を並べる。3 は `Std.Exec.spawn_raw` の最後の段だけが使え、`Proc.spawn_raw` とほかの段では起動の誤り (`I/O error`) である
  2. 標準入力の向き (0 は受け継ぐ、1 は空、2 は pipe、3 は文字列、4 はファイル)
  3. 標準入力の文字列かファイルのパス (向きが 3 か 4 のときだけ読む)
  4. 標準出力の向き (0 は受け継ぐ、1 は空、2 は pipe、3 はファイルに書く、4 はファイルに足す)
  5. 標準出力のファイルのパス (向きが 3 か 4 のときだけ読む)
- 列は各要素を「バイト長の十進、NUL、本文」の順に並べた1つの文字列である。段の列の中の引数の列と環境の列は、それ自身が1つの要素になる。環境の列は名前と値を交互に置く。組み立ては `++` と `String.byte_length` だけで書くので、`Panic` を起こさない。NUL を含む要素は OS に渡せないので、起動の誤り (`I/O error`) にする。向きの値が範囲の外のときと、段が1つもないときも同じである。段は先頭から並べる
- `launch_args : Int -> Int -> Bool -> Cmd -> <IO> (String, Int, String, Int, String)` は `Std.Exec` のためのインターフェースで、`Async.io_wait` と同じ理由で `pub` である。`Cmd` の `StdinFrom` の鎖をたどって段を先頭から並べ、`spawn_raw` の5つの引数にする。標準入力は先頭の段の、標準出力は最後の段の向きで、`Cmd` が設定していない向きは、渡した2つの向きにする。起動の形の既定は、この2つの値で決まる。最後でない段の標準出力の設定は、ここで起動の誤りにする (上の「パイプライン」)。`Bool` は、最後の段が標準エラーを pipe にしてよいかを表し、`True` を渡すのは `Exec.spawn` だけである。許さない段の `PipeStderr` も、ここで起動の誤りにする
- `stage_count : Child -> (Int, Child)` は段の数を返し、`wait_stage : Int -> Child -> <IO> (Int, Child)` は、標準入力を閉じてから段 0 から段 `k` までを先頭から待ち、段 `k` の終了コードを返す。どちらもハンドルを借りる。`wait_all` は、この2つと `wait` の上の eml の関数である
- `exited : String -> Int -> <IO> a` は `Proc` の中だけの eml の関数で、`fail_io Other` で ``command `<program>` exited with code <code>`` の IO の失敗を起こす。戻らない。`capture` と `capture_bytes` が使う
- `read_line_raw` と `read_line_bytes_raw` は、`Stdin` の同じ名前の extern と同じく、行の終わりを含む1行か、終わりなら空を返す

### `Child`

`Child` は、子のプロセスと、その標準入力への書き込む端と、標準出力の読み手を持つハンドルである (上の「ハンドル」)。線形な型 (`Lin`) で、ちょうど1回使う。

- `write`、`write_bytes`、`write_line` は、`FileOut` の同じ名前の関数と同じ形で、子の標準入力に書く。`Child` は buffer を持たず、書くたびに pipe に書き出す
- `close_stdin h` は子の標準入力を閉じ、子に EOF を届ける。閉じた後の書き込みは `not available` で失敗する
- 読む関数は `Stdin` の同じ名前の関数と同じ名前と形と意味で、子の標準出力を読む。`read_line` と `read_line_bytes` は `read_line_raw` と `read_line_bytes_raw` の上に eml で書いてある。`Stdin` と違って、補充の前に標準出力を書き出さない
- `wait h` は、標準入力を閉じてから、すべての段を先頭から待ち、最後の段の終了コードを返す。`wait_all h` も同じ順に待ち、すべての段の終了コードを返す。どちらもハンドルを消費する。子を終わりまで待って終了コードを受け取る道は `wait` と `wait_all` だけで、閉じる関数はない
- `drop h` で手放すか、巻き戻し (`try_io`、`try_panic`、`never` の操作)、捕まえなかった実行時エラー、書き込みの誤りのどれで `Child` を手放しても、破棄処理が次の順に片づける。子の終了コードは失われる。`Heap` を落とすときも同じである。`Task a` を手放すとキャンセルにする方針 (下の「取り消し」) と同じ考えである
  1. pipe を閉じる
  2. まだ待っていない段の直接の子をすべて kill する
  3. それらの子を先頭から待つ
- そのため、直接の子は `try_io` で抜けた後に残らず、zombie にもならない。kill するのは直接の子だけなので、`sh -c` が起動した孫は残りうる
- パイプラインの `Child` では、書き込みは先頭の段の標準入力に、読みは最後の段の標準出力に当たり、下の pipe の規則もこの2本に当てはまる。誤りの名前は、書き込みでは先頭の段の、読みでは最後の段の、待ちでは待てなかった段のプログラムである

eml は1度に1本の pipe しか読み書きしない。pipe の buffer は OS が持ち、ふつう 64 KiB である。詰まらないように、次の規則で読み書きする。

- 入力を読み終えてから出力を書く子 (`sort`、`wc`) には、書き終えて `close_stdin` してから読む。この順なら、入力の量に上限はない
- 読みながら書く子 (`cat`、`tr`、`grep`) に buffer より多く書いてから読むと、子は出力を書けず、eml は入力を書けず、両方が止まる。そういう子には buffer より少なく書くか、入力を分けて書くたびに読む
- `wait` の前に標準出力を読み切る。読まずに `wait` すると、buffer より多く出力する子で止まる。パイプラインも同じで、最後の段の標準出力を読み切っていれば (pipe でなければ)、どの段も終われるので、先頭から待つ `wait` と `wait_all` は止まらない。読み切らずに待つと、最後の段が書き込みで止まることがあり、待つ順を変えても避けられない
- 止まったときは Ctrl-C で終われる。unix では書き込みと読みが `EINTR` で戻り、止める語を見るためである。Windows では `eml run` の2回目の Ctrl-C で終わる。処理系は止まったことを検出しない

### 失敗

| 操作 | 失敗 | 届け方 |
|---|---|---|
| `run`、`output`、`output_bytes`、`capture`、`capture_bytes`、`spawn` | 起動できない (`Io { op: Run, name: program, .. }`。``cannot run `rsync`: not found``)。ない作業ディレクトリも `not found` である。NUL を含む引数と、空か `=` を含む環境変数の名前は `I/O error` | `IO` (`try_io`) |
| `run`、`output`、`output_bytes`、`capture`、`capture_bytes`、`spawn` | `stdin_text` の一時ファイルを作れない (`Launch`。``cannot run `sort`: cannot make the standard input file: I/O error``) か、リダイレクトのファイルを開けない (``cannot run `sort`: cannot open `in.txt`: not found``)。ディレクトリは `I/O error` である。どちらも子を起動しない | `IO` (`try_io`。種類は理由の語のもの) |
| `run`、`output`、`output_bytes`、`capture`、`capture_bytes`、`spawn` | パイプラインの最後でない段の標準出力を設定した (``cannot run `sort`: the standard output of a stage that is not last is redirected``)。子を起動しない | `IO` (`kind` は `Other`) |
| `run`、`output`、`output_bytes`、`capture`、`capture_bytes`、`spawn` | `Exec.pipe_stderr` の段がある (``cannot run `sort`: the standard error pipe needs `Exec.spawn` on the last stage``)。子を起動しない | `IO` (`kind` は `Other`) |
| `capture`、`capture_bytes` | 終了コードが 0 でない (``command `sh` exited with code 3``) | `IO` (`kind` は `Other`) |
| `Child` の読み、`output`、`capture` | 読めない (`Io { op: ReadOutput, name: program, .. }`。``cannot read the output of `sort`: I/O error``)。文字列の読みは UTF-8 でない (``the output of `sort` is not valid UTF-8``) | `IO` (UTF-8 の誤りの `kind` は `NotUtf8`) |
| `Child` の書き込み | 書けない (子が先に終わったときの EPIPE、`close_stdin` の後など)。`FileOut` と同じ `Io { op: Write, name: program, .. }` (``cannot write to `sort`: I/O error``) で、ハンドルを手放す | `IO` |
| `wait`、`wait_all`、`run`、`output`、`output_bytes`、`capture`、`capture_bytes` | 子を待てない (`Io { op: Wait, name: program, .. }`。``cannot wait for `sort`: I/O error``)。起動の誤りと違い、子は起動している | `IO` |
| `wait`、`wait_all`、読み、書き込み | 割り込み。子が終わった後に `Stopped` になる | 位置を付けた実行時エラー |

- 起動の誤り (``cannot run `<program>`: …``。一時ファイルとリダイレクトの形を含み、最後でない段の標準出力の誤りと、標準エラーの pipe の誤り (``the standard error pipe needs `Exec.spawn` on the last stage``) を除く。どちらも決まった文言である) の理由は、`not found`、`permission denied`、`not available`、`I/O error` の4つの語のどれかで、OS の文言を出さない。理由の語が誤りの種類 (`try_io` の `kind`) を決める。driver で読み書きする `Std.Stream` の読みと書きの失敗と、`Std.Exec` の pipe の読み書きと子の待ちの失敗は、`Std.Net` と同じく OS の文言を保つ (下の「`Std.Stream`」と「`Std.Exec`」)。`Std.Exec` の起動の誤りは、この4つの語を使う
- `Child` の書き込みの失敗を、`Stdout` と違って `try_io` で捕まえるのは、`Child` が自分の pipe に直に書き、共有の buffer を持たないためである (`FileOut` と同じ理由)
- 書き込みが失敗すると、ハンドルを手放すので、破棄処理が子を kill する。子の終了コードは失われる

## エフェクトのモジュール

`Yield`、`Fail`、`State` は、エフェクトとその handler を持つモジュールで、`Iter` は `Yield` の上に書いた引いて使うジェネレータである。エフェクトの型と操作を修飾せずに書くには、`import Yield (Yield(..))` のように import の一覧に入れる。一覧のない `import Yield` では、`Yield.Yield Int`、`Yield.yield x` と修飾して書く ([モジュールと名前解決](modules.md) の「import」)。

```haskell
import Yield (Yield(..))
import Iter

numbers : Unit -> <Yield Int, Panic> Unit
numbers () =
  let go n =
    yield n
    go (n + 1)
  go 1

main : Unit -> <IO, Panic> Unit
main () =
  Iter.each (fn n -> println (show n)) (Iter.take 3 (Iter.generate numbers))
```

- エフェクトごとにモジュールを分け、`Std.List` と同じく主役の型の名前をモジュール名にする。修飾子から何の handler かが読める (`Yield.for_each`) ためである。1つのモジュール `Std.Effect` にまとめると、修飾子から読めなくなる。Prelude に置かないのは、すべての検査で読み込むことになり、`get`、`put`、`fail`、`yield` のような短い名前がすべてのモジュールで修飾なしに見えるためである
- 関数の引数は、`Std.List` と同じく関数を先、対象を最後に置く。`Yield.for_each` だけは、`use x <- Yield.for_each gen` と書けるように、生産者を先、要素ごとの関数を最後に置く

## `Yield`

```haskell
pub effect Yield a where
  yield : a -> Unit
```

| 関数 | 型 |
|---|---|
| `for_each` | `(Unit -> <Yield a \| e> Unit) -> (a -> <e> Unit) -> <e> Unit` |
| `fold` | `(b -> a -> <e> b) -> b -> (Unit -> <Yield a \| e> Unit) -> <e> b` |
| `collect` | `(Unit -> <Yield a \| e> Unit) -> <e> List a` |

- `for_each producer f` は `producer ()` を呼び、出された値ごとに `f` を呼ぶ
- `fold f init producer` は、出された値を左からたたむ (`f (f init x1) x2`)。`collect producer` は、出された順のリストを返す。`collect` は `fold` の上に書いてある
- どの handler の節も、受け取ってすぐ再開する `fn` の節である ([式](expressions.md) の「handler」)
- push の形の変換 (`Yield` を受けて `Yield` を出す handler) は置かない。変換は `Iter` の側だけに置く。両方に置くと同じ変換が2組になり、push の形では `zip` を書けないためである。push の形の変換は、位置つきの `mask` と `fn` の節で、利用者が数行で書ける

## `Fail`

```haskell
pub effect Fail x where
  never fail : x -> a
```

| 関数 | 型 |
|---|---|
| `to_result` | `(Unit -> <Fail x \| e> a) -> <e> Result x a` |
| `to_option` | `(Unit -> <Fail x \| e> a) -> <e> Option a` |

- `Fail` は誤りの型を引数に持つ。誤りの形はプログラムごとに違うためである (`tests/ui/run/scripts/csv2jsonl` は、終了コードと文言の組を `fail (1, "…")` と渡す)
- `to_result action` は、`action ()` が `fail x` で止まれば `Err x` を、値 `v` を返せば `Ok v` を返す
- `to_option action` は、誤りを捨てて `None` を返す。捨てるので、誤りの型は `Unr` でなければならない。節で誤りを `_` に束縛するので、型検査が `Unr` の制約を付け、`Fail FileIn` のような誤りの型は E3001 になる。新しい規則は要らない
- row に2つの `Fail` を並べたときは、`fail` はいつも1つ目のラベルに向かう。2つ目を名指す構文はない ([ロードマップ](../future/roadmap.md) の「言語機能と構文」)

## `State`

```haskell
pub effect State s where
  get : Unit -> s
  put : s -> Unit
```

| 関数 | 型 |
|---|---|
| `run` | `s -> (Unit -> <State s \| e> a) -> <e> (a, s)` |
| `eval` | `s -> (Unit -> <State s \| e> a) -> <e> a` |
| `exec` | `s -> (Unit -> <State s \| e> a) -> <e> s` |

- `run init action` は、状態 `init` で `action ()` を動かし、結果と最後の状態の組を返す。`eval` は結果だけを、`exec` は最後の状態だけを返す。どちらも `run` の組から取り出す関数である。名前は Haskell の `runState`、`evalState`、`execState` に倣う
- `get` は状態の写しを返すので、状態の型は `Unr` でなければならない。パラメータ付き handler の `fn` の節の書き方から、型検査がこの制約を付ける ([式](expressions.md) の「パラメータ付き handler」)

## `Iter`

```haskell
pub abstract data Iter <e> a = | Iter (once Unit -> <e> Option (a, Iter <e> a))
```

| 関数 | 型 |
|---|---|
| `next` | `Iter <e> a -> <e> Option (a, Iter <e> a)` |
| `from_fn` | `(once Unit -> <e> Option (a, Iter <e> a)) -> Iter <e> a` |
| `generate` | `(Unit -> <Yield a \| e> Unit) -> Iter <e> a` |
| `yield_all` | `Iter <e> a -> <Yield a \| e> Unit` |
| `from_list` | `List a -> Iter <e> a` |
| `map` | `(a -> <e> b) -> Iter <e> a -> Iter <e> b` |
| `filter` | `(a -> <e> Bool) -> Iter <e> a -> Iter <e> a` |
| `filter_map` | `(a -> <e> Option b) -> Iter <e> a -> Iter <e> b` |
| `take` | `Int -> Iter <e> a -> Iter <Panic \| e> a` |
| `zip` | `Iter <e> a -> Iter <e> b -> Iter <e> (a, b)` |
| `enumerate` | `Int -> Iter <e> a -> Iter <Panic \| e> (Int, a)` |
| `each` | `(a -> <e> Unit) -> Iter <e> a -> <e> Unit` |
| `fold` | `(b -> a -> <e> b) -> b -> Iter <e> a -> <e> b` |
| `to_list` | `Iter <e> a -> <e> List a` |

- `Iter` は `once` の関数を持つので `Lin` である ([型と Kind](types.md) の「1回だけ呼べる関数の型」)。途中でやめるときは `drop` で捨てる。生産者が `FileIn` を持っていれば、そこで閉じる
- コンストラクタ `Iter` は隠す
- `next it` は、次の要素と残りの `Iter` の組を返し、終わりなら `None` を返す
- `from_fn pull` は、`next` で1回だけ `pull ()` を呼ぶ `Iter` である。隠したコンストラクタの代わりに、`Iter` を手で組むときに使う
- `generate`、`from_list` と変換 (`map`、`filter`、`filter_map`、`take`、`zip`、`enumerate`) は、`next` を呼ぶまで何も動かさない。そのため、どれもエフェクトを起こさず、矢印に row が要らない。Python のジェネレータが `next` まで本体を動かさないのと同じである
- `generate producer` は、生産者を `Yield` の handler で包む。名前を `from` にしないのは、`from` が `handle … from …` のキーワードだからである。`yield` の節は、受け取った値と、継続 `k` を包んだ次の `Iter` の組を返し、`return` の節は `None` を返す。`yield_all` は逆向きの変換で、`Iter` の要素を順に `yield` する
- `take n it` は、n 個を渡した後の `next` で残りを `drop` して `None` を返す。n が 0 以下なら、最初の `next` で何も取り出さずに捨てる。`take` の `Iter` そのものを `drop` したときも、中の `Iter` は一緒に捨てられる。そのため、生産者の `FileIn` が閉じるのは、渡し終えた次の `next` か `drop` の時点である
- `filter p it` は、要素を `p` に渡したうえで、その要素を返す。要素を2回使うので、要素の型は `Unr` でなければならない。型検査がこの制約を付け、`FileIn` の `Iter` に `filter` を使うと E3001 になる。`map`、`filter_map`、`take`、`zip`、`enumerate` は要素を1回だけ使うので、`Lin` の要素にも使える
- `zip` は、どちらかが終わったら、もう一方の残りを `drop` する。右が先に終わったときは、左から引いた要素も `drop` する。`enumerate start` は、番号を `start` から数える。行番号のように1から数える場面が多いためである
- `each`、`fold`、`to_list` は、終わりまで `next` を呼ぶ。`fold` は左からたたみ、`to_list` は要素の順のリストを返す
- `next` の結果を専用の型にせず `Option` の組にするのは、利用者が覚える型を `Iter` だけにするためである (Rust の `Iterator::next`、OCaml の `Seq.uncons` と同じ形)。先頭の値を先に求める形 (`| Done | More a (once Unit -> <e> Iter <e> a)`) では、作っただけで生産者が最初の `yield` まで動く
- `Std.Iter` は `Std.Yield` を import し、`Std.Yield` は `Std.Iter` を知らない。両方向の変換 (`generate` と `yield_all`) を `Std.Iter` に置くので、依存は一方向で、push の形だけを使うプログラムは `Std.Iter` を読み込まない
- 多回の再開ができるジェネレータ (`multi` の `yield`) は用意しない。生産者が `yield` をまたいで `Lin` の値を持てなくなり、`FileIn` を持つ生産者を書けないためである。要る例が出たら、`Yield` の意味を変えずに別のエフェクトとして足せる

`Iter` を使うときは、次の3つに注意する。

- 速さ: 末尾再帰の生産者を `generate` で引く形は、handler への特殊化 ([Core IR](core-ir.md) の「handler への特殊化」) で yield せず、融合 ([Core IR](core-ir.md) の「`con` の値の形についての特殊化」) で、`fold` で引く形と `map` を重ねた形は1要素ごとに何も確保しないループになる。`filter` を重ねた形では、残した要素ごとに4つの確保 (`Some`、組、`Iter`、閉包) が残る。末尾でない再帰の生産者 (木の走査など) は、1要素ごとに yield する ([実装の現在地](../implementation/status.md) の「最適化が扱わない形」)
- エフェクトの届く先: 生産者のエフェクトは、`Iter.generate` を呼んだ場所ではなく、`next` を呼んだ場所の handler に届く。そのため、`try_io` は `Iter.generate` ではなく、消費するループを包む (`try_io (fn () -> Iter.each f (Iter.generate producer))`)
- `multi` の操作: `yield` は `once` の操作なので、`Yield.for_each` の要素ごとの関数の中で `multi` の操作を起こすと E3006 になる。`yield` の継続を持ったまま、`multi` の操作をまたぐためである ([エフェクトと handler](effects.md) の「継続の多重度と持ち越し規則」)。多回の再開ができるジェネレータを用意しないことの帰結である

## `Std.Par`

```haskell
import Std.Par

main : Unit -> <IO, Panic> Unit
main () =
  let (a, b) = Par.par (fn () -> 1 + 2) (fn () -> 3)
  println (show (a + b))
```

| 名前 | 型 | 実装 |
|---|---|---|
| `par` | `(Unit -> <never e> a) -> (Unit -> <never e> b) -> <e> (a, b)` | extern (呼び出しは `par` の命令) |
| `both` | `(Unit -> <e> a) -> (Unit -> <e> b) -> <e> (a, b)` | eml |

- `par f g` は、`f ()` と `g ()` を2つの子として動かし、両方が終わるのを待って結果の組を返す。VM は、空いている worker があれば右の子をその worker で動かし、なければ2つの子を同じ worker の上で切り替えて動かす ([VM](vm.md) の「`par` の fork と join」)。`par` の意味は [マルチコア対応の設計](../future/multicore.md) の「`par` の意味」にある
- 子の row の上限は `never` である ([型と Kind](types.md) の「row 変数の多重度の上限」)。子が起こせるのは、`Panic` と、どの操作も `never` のエフェクトだけである。`IO` (`println` や `exit`)、`var` のエフェクト、`once` か `multi` の操作を持つエフェクトを子が起こしうる呼び出しは E2020 になる。子が `never` の操作だけを起こしても、そのエフェクトが `once` の操作を持てば E2020 である。上限は `par` の型にあるので、`par` を値として渡した先の呼び出しも同じく確かめる
- 子が `never` の操作で中断すると、両方の子が終わった後で、親がその操作を起こし直す。両方の子が中断すれば左の子の操作を起こし直し、もう一方の結果は捨てる。子の中の handler と `try_panic` は、子の中の操作を子の中で捕まえる
- 子の実行時エラーは評価を止め、ほかの子の結果では救わない。両方の子が誤りなら左の誤りを報告する ([VM](vm.md) の「子の誤り」)。捕まえられなかった子の `Panic` は親で起こし直した `panic` の実行時エラーになり、文言はソースの位置を持たない ([実装の現在地](../implementation/status.md) の「`par`」)
- 子の閉包は `Lin` の値を捕まえて消費でき、`Lin` の値を返してもよい。extern のスキームは、関数型の引数の矢印の Kind と、その最後の結果だけに現れる型変数を固定しないためである ([宣言](declarations.md) の「`extern`」)。返った値は組に1回入るか、もう一方の子が中断したときに落とされる。落とすのは、`Lin` の値を持つフレームの巻き戻しと同じである ([線形性](linearity.md) の「暗黙の後始末」)。そのため、`both` と `par` は同じ型を受け付ける (`par` は row の上限だけが狭い)
- `par` は Prelude から出さない。`import Std.Par` と書いて `Par.par` で呼ぶか、`import Par (par, both)` で修飾せずに使う
- `both f g` は、呼んだ側の文脈で `f ()`、`g ()` の順に呼び、結果の組を返す。row に上限がないので、`IO` を起こす計算も渡せる。`par` と同じ組を逐次に作る関数である
- 引数をそろえた `par` の呼び出しは、extern の命令でなく `par` の命令になる。部分適用 (`Par.par f`) と値として使う `Par.par` は、同じ命令を本体に持つ包む関数 `$externN` になる ([Core IR](core-ir.md) の「`par` の呼び出し」)

## `Std.Async`

`Std.Async`は複数のworkerの上でタスクを動かすモジュールである。タスクは`start`のときにworkerに置き、終わるまでそこで動く ([VM](vm.md)の「`Async`のタスク」)。

```haskell
import Async (Async)

body : Unit -> <IO, Panic, Async> Int
body () =
  let a = Async.start (fn () -> 1)
  let b = Async.start (fn () -> 2)
  Async.await a + Async.await b

main : Unit -> <IO, Panic> Unit
main () = println (show (Async.run body))
```

| 名前 | 型 | 実装 |
|---|---|---|
| `run` | `(Unit -> <IO, Panic, Async> a) -> <IO, Panic> a` | eml |
| `start` | `(Unit -> <IO, Panic, Async> a) -> <IO, Async> Task a` | eml |
| `spawn` | `(Unit -> <IO, Panic, Async> Unit) -> <IO, Async> Unit` | eml |
| `scope` | `(Scope -> <IO, Panic, Async> a) -> <IO, Panic, Async> a` | eml |
| `start_in` | `Scope -> (Unit -> <IO, Panic, Async> a) -> <IO, Panic, Async> Task a` | eml |
| `spawn_in` | `Scope -> (Unit -> <IO, Panic, Async> Unit) -> <IO, Panic, Async> Unit` | eml |
| `await` | `Task a -> <IO, Panic, Async> a` | eml |
| `yield_now` | `Unit -> <IO, Async> Unit` | eml |
| `sleep_ms` | `Int -> <IO, Async> Unit` | eml |
| `timeout` | `Int -> (Unit -> <IO, Panic, Async> a) -> <IO, Panic, Async> Option a` | eml |
| `race` | `(Unit -> <IO, Panic, Async> a) -> (Unit -> <IO, Panic, Async> b) -> <IO, Panic, Async> Either a b` | eml |
| `drain_within` | `Int -> Scope -> <IO, Async> Unit` | eml |
| `check_cancelled` | `Unit -> <IO, Async> Unit` | eml |
| `cancel_requested` | `Unit -> <IO> Bool` | eml |
| `cancel` | `Task a -> <IO> Unit` | eml |
| `cancel_scope` | `Scope -> <IO> Unit` | eml |
| `shield` | `(Unit -> <IO, Panic, Async> a) -> <IO, Panic, Async> a` | eml |
| `compute` | `(Unit -> <never e> a) -> <IO, Async \| e> a` | eml |
| `start_on_main` | `(Unit -> <IO, Panic, Async> a) -> <IO, Async> Task a` | eml |
| `start_off_main` | `(Unit -> <IO, Panic, Async> a) -> <IO, Async> Task a` | eml |
| `spawn_off_main` | `(Unit -> <IO, Panic, Async> Unit) -> <IO, Async> Unit` | eml |
| `gui_wait` | `Cell -> <IO, Async> Wake` | eml |
| `run_on_main` | `(Unit -> <IO, Panic, Async> a) -> <IO, Panic, Async> a` | eml |
| `try_all` | `(Unit -> <IO, Panic \| e> a) -> <IO \| e> Result Failure a` | eml |
| `Failure` | `IOFailed IOError` と `Panicked String` を持つ型 | eml (`deriving (Show)`) |

- `Async.run f` は根のスコープを開き、`f ()` をその中の最初のタスクとして動かす。`start` と `spawn` は新しいタスクを、呼んだタスクの今のスコープに加える。今のスコープは、タスクが `scope` で開いたいちばん内側のスコープで、開いていなければタスクが加わったスコープである。タスクの中で `run` を入れ子に呼ぶと、その `run` は呼んだタスクの今のスコープの下に自分の根を開く。外のスコープの取り消しと呼んだタスクの取り消しは、その根のタスクにも届く (下の「取り消し」)
- `run` は、根のスコープのすべてのタスクが終わってから戻る。待ったタスクかどうかによらない。失敗がなければ本体の値を返す。失敗があれば、終わった順で最初の失敗を起こし直し、本体の値は捨てる。IO の失敗は種類と文言を保ち、Panic は文言を保つ。後の失敗は報告せず、1つずつ `RunStats.dropped_failures` に数える ([VM](vm.md) の「`RunStats`」)。失敗したタスクを待ち、起こし直された失敗を捕まえなかったタスクも、失敗したタスクとして数える。失敗したタスクは同じスコープのほかのタスクを取り消す (下の「取り消し」)
- `Task a` は `Lin` で、`await t` が消費する。`await` はタスクの値を返すか、タスクの失敗を起こし直す。その失敗はスコープの失敗のままなので、`await` の周りで捕まえても `run` は起こし直す。失敗をスコープから分けるには、タスクの中で `try_all` を使う
- `drop t` と `cancel t` はタスクを取り消してハンドルを放す。終わったタスクなら、残っている結果を落とす。`spawn` はタスクを取り消さずにハンドルを放す。取り消しの地点を通らないタスクは、取り消しても最後まで動く
- ハンドルは、1つのプログラムの実行の中なら、作った `run` の外へ出してよい。そのときタスクは終わっているので、`await` はすぐ値を返すか失敗を起こし直す。`Task` は線形なので、REPL の入力をまたいで持つことはできない ([モジュールと名前解決](modules.md) の「REPL のセッション」)
- どのタスクの `exit n` も、プログラムを終了コード `n & 255` で終える。ほかのタスクは巻き戻さない (上の「`exit`」)
- `yield_now ()` は、呼んだタスクを実行待ちの列の後ろへ回す。取り消しの地点である
- `sleep_ms ms` は、呼んだタスクを `ms` ミリ秒眠らせる取り消しの地点である (下の「時間」)
- タスクは閉じた row `<IO, Panic, Async>` で始まる。始めた側の handler と `var` はタスクから見えない。`var` を捕まえた閉包を `start` や `spawn` に渡すと型の誤り (E2002) になる
- `Async` は抽象エフェクトで、操作 `await_cell`、`reschedule`、`await_time`、`await_drain`、`await_send`、`await_recv`、`await_io`、`await_io_until`、`await_signal` は `Std.Async` の外から perform も handle もできない (E1058。[エフェクトと handler](effects.md) の「抽象エフェクト」)。利用者は `await`、`yield_now` などの包む関数を呼ぶ。channel の待ちは `chan_wait_send` と `chan_wait_recv` が包む (下の「`Std.Chan`」)

`Async` のエフェクト、ランタイムが名前で呼ぶエントリ、extern の行は、eml とランタイムの間の ABI である。形を変えるときはランタイムも合わせて変える。

```haskell
pub data Wake =
  | Woke
  | Cancelled

pub abstract effect Async where
  await_cell : Cell -> Wake
  reschedule : Unit -> Wake
  await_time : Int -> Wake
  await_drain : Scope -> Wake
  await_send : Cell -> Wake
  await_recv : Cell -> Wake
  await_io : OpId -> (Wake, OpId)
  await_io_until : OpId -> Int -> (Wake, OpId)
  await_signal : Int -> Wake

pub extern data Cell
pub extern data Task a
pub extern data Scope
pub extern data OpId
pub extern data Job a

pub data Failure =
  | IOFailed IOError
  | Panicked String
  deriving (Show)
```

- `Async` は抽象エフェクト (`pub abstract effect`) で、extern のエフェクトではない。生の操作は `Std.Async` の外では perform できず、handler の節にも書けない ([エフェクトと handler](effects.md) の「抽象エフェクト」)。生の操作は9つで、どれも `once` である。`await_cell c` は `c` が指すタスクのセルが終わるか、スコープのセルが閉じるまで待つ。`reschedule ()` は実行待ちの列の後ろへ回る。`await_time ms` は `ms` ミリ秒眠り、`ms` が 0 以下なら `reschedule ()` と同じく列の後ろへ回る。`await_drain s` はスコープ `s` に属するタスクがなくなるまで待ち、`s` を閉じない。`await_send c` は channel `c` に枠が空くか閉じるまで、`await_recv c` は品物が来るか閉じるまで待つ。`await_io op` は token `op` が指す driver の操作が終わるまで待ち、再開のときに `Wake` と token の組を返す (下の `OpId` の箇条)。`await_io_until op ms` は `await_io` に今から `ms` ミリ秒の期限を付けた待ちである。期限が来ると driver に操作の取り消しを頼み、取り消した操作が終わってから `Woke` で戻る。期限が来たことは記録に残り、包みが読む (下の「`Std.Net`」)。`ms` が 0 以下なら、待ち始めるときに取り消しを頼む。`await_signal s` はシグナル `s` (0 が `Interrupt`、1 が `Terminate`) が来るまで待つ (下の「`Std.Signal`」)。どの操作でも、タスクの待ちの登録はいつも高々1つである
- `Wake` は、ランタイムが待たせたタスクを再開するときに渡す値である。ランタイムは配置の表を見ずに、宣言の順のタグ (`Woke` = 0、`Cancelled` = 1。`eml_lir::WAKE_WOKE`、`WAKE_CANCELLED`) で作る。タグと宣言の順が合うことはテストで確かめる。ランタイムは取り消したタスクを `Cancelled` で再開する
- `Cell` は登録表の添字 `(generation << 32) | index` を持つ即値で、Kind は `Unr`、Repr は `int` である。`Scope` は `Cell` と同じ形の即値で、Kind は `Unr`、Repr は `int` である。`Task a` はランタイムのタスクのハンドルを中身に持つ `Foreign` で、Kind は型引数によらず `Lin` である
- `OpId` は driver に出した操作の記録の番号 `(generation << 32) | index` を持つ token の即値で、Kind は `Lin`、Repr は `int` である。受け付けの行が作り、取り出しの行が消費する。即値なので RC を持たず、1回だけ使うことは型検査が守る。待つ間は継続だけをランタイムが預かり、再開のときに token を継続へ返す。受け付けから取り出しまでを1つの包みの関数の中で行うので、利用者のコードは `OpId` を持たず、`Chan` でも送れない。記録に触れるのは受け付けたタスクだけで、ほかのタスクが token を渡すと内部の誤り `an op is used by another task` になる。放した記録の古い token と、2度目の取り出しは内部の誤り `an op names no record` になる。std の包みからはどちらも起きない ([VM](vm.md) の「操作の記録」)
- `Job a` は仕事の token の即値 `(generation << 32) | index` を持つ型で、Kind は `a` によらず `Lin`、Repr は `int` である。`Chan a` と同じく型引数は `Unr` に固定しないが、`Chan a` と違って `Lin` なので、1回だけ使うことは型検査が守る
- ランタイムが名前で呼ぶエントリは、std の3つの関数 `start_task : (Unit -> <IO, Async> Unit) -> <IO> Unit`、`resume_task : (once Wake -> <IO> Unit) -> Wake -> <IO> Unit`、`resume_io : (once (Wake, OpId) -> <IO> Unit) -> Wake -> OpId -> <IO> Unit` である (`eml_extern::RuntimeEntry`)。どれも単相なので、名前で ABI が決まり、instance の名前が正式な名前のままになる。`start_task` はタスクの本体に handler をかぶせ、`await_cell` の節は継続を `park` に、`reschedule` の節は継続を `ready` に、`await_time` の節は継続を `park_time` に、`await_drain` の節は継続を `park_drain` に、`await_send` の節は継続を `park_send` に、`await_recv` の節は継続を `park_recv` に、`await_io` の節は token と継続を `park_io` に、`await_io_until` の節は token と期限と継続を `park_io_until` に、`await_signal` の節はシグナルの番号と継続を `park_signal` に預けて戻る。`resume_task k w` は預けた継続 `k` に `w` を渡す。`resume_io k w op` は `k` に組 `(w, op)` を渡す。extern は組を作らない ([Core IR](core-ir.md) の「translate の配置」) ので、組は eml のエントリが作る。`Panic` はタスクの底で捕まえるので、継続の row は `<IO>` である。3つのエントリが単相の std の関数であることもテストで確かめる
- タスクの底 (`bottom`) は、`try_all` で本体の失敗を捕まえ、成功した結果を `finish_ok` に、IO の失敗を種類と文言のまま `finish_io` に、`Panic` を文言のまま `finish_panic` に渡す
- externの行は次の38行である。どれも`pub`でない。VMはこれらの行を汎用のexternの命令でなく、`Scheduler`を受ける`Async`の命令で実行する ([VM](vm.md)の「`Async`のタスク」)。ただし`job_cell`は、`chan_cell`と同じく即値を写すだけの普通のexternである

| 行 | 型 | 預かる引数 |
|---|---|---|
| `start_raw` | `(Unit -> <IO, Async> Unit) -> <IO> Task a` | 閉包 |
| `start_main_raw` | `(Unit -> <IO, Async> Unit) -> <IO> Task a` | 閉包 |
| `start_off_main_raw` | `(Unit -> <IO, Async> Unit) -> <IO> Task a` | 閉包 |
| `run_raw` | `(Unit -> <IO, Async> Unit) -> <IO> Task a` | 閉包 |
| `start_in_raw` | `Scope -> (Unit -> <IO, Async> Unit) -> <IO, Panic> Task a` | 閉包 (2つ目の引数) |
| `park` | `Cell -> (once Wake -> <IO> Unit) -> <IO> Unit` | 継続 (2つ目の引数) |
| `park_time` | `Int -> (once Wake -> <IO> Unit) -> <IO> Unit` | 継続 (2つ目の引数) |
| `park_drain` | `Scope -> (once Wake -> <IO> Unit) -> <IO> Unit` | 継続 (2つ目の引数) |
| `park_send` | `Cell -> (once Wake -> <IO> Unit) -> <IO> Unit` | 継続 (2つ目の引数) |
| `park_recv` | `Cell -> (once Wake -> <IO> Unit) -> <IO> Unit` | 継続 (2つ目の引数) |
| `park_io` | `OpId -> (once (Wake, OpId) -> <IO> Unit) -> <IO> Unit` | 継続 (2つ目の引数) |
| `park_io_until` | `OpId -> Int -> (once (Wake, OpId) -> <IO> Unit) -> <IO> Unit` | 継続 (3つ目の引数) |
| `park_signal` | `Int -> (once Wake -> <IO> Unit) -> <IO> Unit` | 継続 (2つ目の引数) |
| `ready` | `(once Wake -> <IO> Unit) -> <IO> Unit` | 継続 |
| `finish_ok` | `a -> <IO> Unit` | 結果の値 |
| `finish_io` | `IOErrorKind -> String -> <IO> Unit` | なし |
| `finish_panic` | `String -> <IO> Unit` | なし |
| `task_cell` | `Task a -> (Cell, Task a)` | ハンドル (借りて預かる) |
| `take_result` | `Task a -> <IO, Panic> a` | ハンドル |
| `run_result` | `Task a -> <IO, Panic> a` | ハンドル |
| `detach` | `Task a -> <IO> Unit` | ハンドル |
| `cancel_raw` | `Task a -> <IO> Unit` | ハンドル |
| `open_scope` | `Unit -> <IO> Scope` | なし |
| `close_scope` | `Scope -> <IO> Cell` | なし |
| `leave_scope` | `Scope -> <IO, Panic> Unit` | なし |
| `cancel_scope_raw` | `Scope -> <IO> Unit` | なし |
| `fail_scope_io` | `Scope -> IOErrorKind -> String -> <IO> Unit` | なし |
| `fail_scope_panic` | `Scope -> String -> <IO> Unit` | なし |
| `shield_enter` | `Unit -> <IO> Unit` | なし |
| `shield_leave` | `Unit -> <IO> Unit` | なし |
| `cancel_point` | `Unit -> <IO> Bool` | なし |
| `spend_budget` | `Unit -> <IO> Bool` | なし |
| `claim_scope` | `Scope -> <IO> Bool` | なし |
| `cancel_members` | `Scope -> <IO> Unit` | なし |
| `op_ready` | `OpId -> Int -> <IO> (Int, OpId)` | なし (token を借りる) |
| `fork_job` | `(Unit -> <never e> a) -> <IO> Job a` | なし |
| `job_cell` | `Job a -> (Cell, Job a)` | なし (token を借りる) |
| `take_job` | `Job a -> <IO> a` | なし |
| `drop_job` | `Job a -> <IO> Unit` | なし |

- タスクの結果は値のまま渡す。`finish_ok` が結果の値を預かってタスクのセルに持ち、`take_result` と `run_result` がハンドルを消費して、同じセルに持っていた値を1回返す。ランタイムは預かった値を写さず、中身も見ない。返さないままセルを放すときは、その値を落とす。預かる引数の型変数は `Unr` に固定しないので、`Lin` の結果も thunk に包まずに渡せる ([宣言](declarations.md) の「`extern`」)
- `take_result`と`run_result`が返す値の型が、`finish_ok`に預けた値の型と合うことは、処理系は確かめない。生の行を`pub`にせず、`start`、`start_in`、`start_on_main`、`run`、`await`のシグネチャだけが本体の結果の型`a`とハンドルの`Task a`を結ぶことで、標準ライブラリが保つ。`take_job`が返す値の型が`fork_job`に渡した閉包の結果の型と合うことも、同じく`compute`のシグネチャが保つ
- 預かる引数の矢印 (`start_raw`、`start_main_raw`、`start_off_main_raw`、`run_raw`、`start_in_raw`の閉包、`park`、`park_time`、`park_drain`、`park_send`、`park_recv`、`park_io`、`park_io_until`、`park_signal`、`ready`の継続) には、閉じたrowを書く。ランタイムはそれを呼ばず、エントリの`start_task`、`resume_task`、`resume_io`が呼ぶので、rowはエントリの型が決める
- `park_io` と `park_io_until` は継続だけを預かり、token は預からずに消費する。token は即値の番号なので、再開の仕事が記録の番号から作り直して `resume_io` に渡す
- `op_ready op ms` は token を借りて、待たずに続けてよいかを 0 (続ける)、1 (待つ)、2 (先に譲る) で返し、宣言の型では結果と token の組を返す ([Core IR](core-ir.md) の「組を返す extern」)。どの呼び出しも、操作が終わっていたかによらず操作の予算を使い、書きの操作は長さに応じて多く使う (下の「操作の予算」)。記録が終わっていて取り消しの地点が投げなければ、予算が残れば待ちを1つ数えて 0 を返す。切れれば OS を1回見て集め、走れるタスクがあれば数えずに 2 を返し、なければ予算を戻し、待ちを1つ数えて 0 を返す。2 の待ちは、続く `reschedule ()` が数える。ほかは 1 で、何も数えない。待つ側の `await_io` が数える ([VM](vm.md) の「操作の記録」)
- `task_cell` はハンドルを借りてセルを返し、宣言の型ではセルとハンドルの組を返す ([Core IR](core-ir.md) の「組を返す extern」)
- `job_cell` は token を借りてセルを返し、宣言の型ではセルと token の組を返す ([Core IR](core-ir.md) の「組を返す extern」)。`fork_job` は引数の閉包を預からない
- `fork_job f` は `f` を仕事として、空いている worker の受け箱か呼んだ worker の実行待ちに置き、仕事の token を返す。閉包は仕事として動き、`fork_job` が消費する。`take_job j` は token を消費して、終わった仕事の結果を返すか、仕事の中断を起こし直す。`drop_job j` は token を消費して、終わった仕事の終わりを落とす。どちらも、仕事を出したタスクが仕事の終わった後に呼ぶ。std の `compute` は `job_cell` で借りたセルを `await_cell` で待ってから呼ぶ ([VM](vm.md) の「`compute` の仕事」)
- `chan_wait_send` と `chan_wait_recv` (`Cell -> <IO, Async> Wake`) は `Std.Chan` のためのインターフェースで、`pub` である。`Async` の操作は外のモジュールから perform できないためである ([エフェクトと handler](effects.md) の「抽象エフェクト」)。`Cell` を返すか渡す公開の関数はないので、利用者は呼べない。`Cell` を公開の関数に出すときは、この2つを見直す。channel でないセルか放したセルを渡すと、すぐ `Woke` で戻る
- `io_wait` (`OpId -> <IO, Async> (Wake, OpId)`) と `io_wait_until` (`OpId -> Int -> <IO, Async> (Wake, OpId)`) は `Std.Net` のためのインターフェースで、同じ理由で `pub` である。処理系も、`blocking` の C の関数の呼び出しの受け付けと取り出しの間で `io_wait` を呼ぶ ([VM](vm.md) の「`blocking` の呼び出し」)。`OpId` を作る公開の関数はないので、利用者は呼べない (下の「`Std.Net`」)。どちらも先に `op_ready` を呼ぶ。0 なら perform せずに `(Woke, op)` を返し、2 なら `reschedule ()` で列の後ろへ回ってから、その `Wake` を見ずに `(Woke, op)` を返す。1 のときだけ `await_io` か `await_io_until` を perform して待つ。回る間に届いた取り消しは、次の取り消しの地点で投げる
- `gui_wait` (`Cell -> <IO, Async> Wake`) は`Std.Gui`のためのインターフェースで、同じ理由で`pub`である。ゲートのセルで待つ取り消しの地点である。ゲートは旗と高々1つの待ち手を持つ登録表のセルで、前の待ちの後に知らせが届いていれば旗を下ろしてすぐ`Woke`で戻る。取り消しは旗より先に見る ([VM](vm.md) の「セルと登録表」、下の「`Std.Gui`」)。ゲートを作る公開の関数はないので、利用者は呼べない
- `io_wait_shielded` (`OpId -> <IO, Async> OpId`) も `Std.Net` のためのインターフェースである。`shield` と同じ深さの印の中で `io_wait` を呼び、`Wake` を捨てて token を返す。中の待ちは外の取り消しで外れず、いつも操作が終わってから戻る。`shield` は本体の行に `Panic` を求めるので、`Panic` を持たない `Std.Net` の包みのためにこの形にする
- `signal_wait` (`Int -> <IO, Async> Wake`) は `Std.Signal` のためのインターフェースで、同じ理由で `pub` である。0 と 1 のほかの番号は、待たずに種類 `NotAvailable` の IO の失敗 (`no such signal`) にする。利用者が呼んでも、`Signal.wait` と同じ待ちになるだけである (下の「`Std.Signal`」)

### スコープ

```haskell
body : Unit -> <IO, Panic, Async> Int
body () =
  Async.scope (fn s ->
    let a = Async.start_in s (fn () -> 1)
    Async.spawn_in s (fn () -> println "child")
    Async.await a)
```

- `scope f` は、呼んだタスクの今のスコープの子のスコープを開いて今のスコープにし、同じタスクの中で `f s` を動かす。`f s` が値を返しても、失敗しても、取り消されても、スコープのすべてのタスクが終わるまで待ってから今のスコープを元に戻す。この待ちは `shield` の中で行うので、取り消しで外れない
- `scope` の結果はスコープに最初に記録した失敗 (本体か子かによらず、終わった順) を起こし直すか、記録がなければ本体の結果 (値、失敗、`Cancelled`) である。本体の失敗も記録し、記録した時点で子をすべて取り消す。本体が種類 `Cancelled` の失敗で終わったときは、記録はしないが、子はすべて取り消す。2つ目からの失敗は報告せず、`RunStats.dropped_failures` に数える ([VM](vm.md) の「`RunStats`」)
- `start_in s f` と `spawn_in s f` は名前を付けたスコープ `s` にタスクを加える。`s` が閉じていれば `Panic` `the scope is closed` を起こす。本体が終わって閉じかけでも、まだタスクの残るスコープには加えられる。旗の立ったスコープに加えたタスクは、作るときに取り消す
- `Scope` は `Unr` の即値で、子のタスクに渡せ、`scope` の外へも出せる。REPL の束縛にも置ける。離れたスコープの値と前の run の値は、閉じたスコープとして扱う ([VM](vm.md) の「セルと登録表」)
- タスクは自分のスコープより長く生きない。スコープはタスクがすべて終わるまで閉じず、タスクが開いたスコープはタスクの底より前に閉じる

### タスクの上限

- タスクの中で呼び出しの深さが上限を超えると、run は止まらず、そのタスクに種類 `ResourceLimit`、文言 `the call depth limit is exceeded` の IO の失敗が起きる。深さはタスクごとに数え、どのタスクも `max_depth` をまるごと使う。`par` の子と入れ子の `run` の本体は、呼んだタスクの深さの続きから数える
- ただし、`par` の子 (左も右も) の中の超過は、タスクの中でも run を止める実行時エラーで、捕まえられない。子の row は `IO` を含められず、失敗を起こす先がないためである (上の「`Std.Par`」)
- 投げるのは普通の IO の失敗なので、タスクのフレームを巻き戻して `Lin` の値を落とし、`try_io` と `try_all` が捕まえる。捕まえれば、同じタスクはそのまま続けられ、もう一度超えればまた投げる。捕まえなければタスクの失敗になり、スコープの失敗として `run` と `scope` が起こし直す。`main` でも捕まえなければ、ほかの IO の失敗と同じく `run_result` の位置の実行時エラーになり、終了コードは 1 である
- 失敗は関数に入るところでだけ起きる。戻りの途中で深さが上限を超えたままなら、次の呼び出しで起きる。継続の再開と handler の出入りのために処理系が足した補助の関数に入るところでは起きず、その後の次の関数で起きる
- 確保量が柔らかい上限 `soft_bytes` の閾値を越えると、処理系は確保の数がいちばん大きいタスクを選んで上限の印を付け、そのタスクに種類 `ResourceLimit`、文言 `the memory soft limit is exceeded` の IO の失敗を起こさせる。run は止まらない。起きる所と捕まえ方は深さの上限と同じで、捕まえれば同じタスクは続けられ、`main` でも捕まえなければ `run_result` の位置の実行時エラーになる。印を付けるのは確保の数で選んだ1つのタスクで、そのタスクが確保を手放して小さくしたかは見ない。タスクの外の `main`、`par` の右の子、`compute` の仕事の確保はどのタスクの数にも入らない。それでも閾値を越えれば、そのときの数のいちばん大きいタスクに印が付く。印を付けるタスクが1つもなければ (`Async` のタスクのない run など)、硬い上限 `max_bytes` だけが run を止める ([VM](vm.md) の「上限と割り込み」)
- タスクの外 (`main` の本体、`compute` の仕事、タスクの外の `par` の子) の深さの上限は、run を止める実行時エラーで、捕まえられない
- 上限の印を受けたときに `IO` の失敗を起こせない所 (`par` の子の中と、タスクの始めと終わりの std のコード) にいれば、印は捨てずに残り、起こせる所に戻った後の次の呼び出しで失敗が起きる

### 取り消し

- 取り消しは2つの所から来る。タスクの取り消しのビットは、`cancel t`、`drop t`、属するスコープの取り消し、取り消したスコープに加えることで立つ。スコープの取り消しの旗は、`cancel_scope`、スコープのタスクの失敗、`scope` の本体の失敗、親のスコープの取り消しで立ち、開くときに親の旗を写す (`shield` の境目は写さない。下の `shield` の箇条)。どちらも一度立てたら下ろさない
- タスクにビットが立つと、そのタスクが開いているスコープと、入れ子の `run` で待っている根にも旗が立つ。ただし `shield` の中で開いたスコープには立たない。ビットが立った後に `shield` の外で開くスコープと、ビットが立った後に `shield` の外で呼ぶ入れ子の `run` の根も、開くときに旗を立てる
- `scope` の本体を動かすタスクは、そのスコープの中にいる間だけ旗で取り消される (ビットは立てない)。スコープを出た後は、外の旗かビットが立っていなければ取り消されていない
- 取り消しの地点は `await`、`yield_now`、`sleep_ms`、`drain_within`、`check_cancelled`、`compute` と、`Std.Chan` の `send` と `recv` と、`Std.Net`、`Std.Stream`、`Std.Exec` の待つ関数である。取り消しの地点は、`shield` の外では、タスクのビットかタスクの今のスコープの旗が立っていれば (`shield` の中の規則は下の `shield` の箇条にある)、種類 `Prelude.Cancelled`、文言 `task was cancelled` の IO の失敗を投げる。待つ前に立っていれば待たずに投げ、待っている間に立てば待ちを外して投げる。`yield_now` で実行待ちの列にいる間に立てば、その `yield_now` が投げる。待っていた相手が終わって起こされた後に立ったときは、`await` は結果を受け取り、次の取り消しの地点が投げる。`compute` はこの規則の例外である。仕事の前や途中に立った取り消しは待ちを外さず、仕事が終わって起こされた後、再開するまでに立った取り消しでも、`compute` は結果を落として投げる (下の「`compute`」)
- `send` と `recv` は、待つ前と待っている間に取り消しが立てば投げる。起こされた後は、`await` と同じく、まず操作をやり直し (枠か品物があれば送るか受け取る)、次の取り消しの地点が投げる。やり直しで待ち直すときは、待つ前の規則で投げる
- `Std.Net` の待つ関数は、待つ前に取り消しが立っていれば、何も受け付けずにストリームかリスナーを落として投げる。待っている間に立てば、driver に取り消しを頼み、操作の完了を待ってから、完了が作ったハンドル、記録の buffer、渡したストリームかリスナーを落として投げる。driver が buffer を使い終わるまで返せないためである (下の「`Std.Net`」)。`Std.Stream` の待つ関数も同じで、走っているファイルの仕事は取り消せないので、仕事の完了を待つ (下の「`Std.Stream`」)。`blocking` の C の関数の呼び出しも同じである ([宣言](declarations.md) の「`extern "C"`」)。`Std.Exec` の待つ関数も同じで、子の待ちの取り消しは driver がその場で終える。取り消しで落とす `Child` は、まだ待っていない段を kill してから待つ (下の「`Std.Exec`」)
- 失敗したタスクを同じスコープの中から `await` で待つタスクは、失敗が記録されたときにもう待っていればその失敗を受け取り、まだ待っていなければスコープの取り消しを受けて `Cancelled` を受け取る。どちらになるかはスケジュールで決まる。スコープの最初の失敗はどちらでも同じなので、`run` と `scope` が起こし直す失敗は変わらない。相手のスコープを `shield` の外で開いたときは、`await` を `shield` で囲めば、待つ相手の失敗を確かに受け取る
- 取り消しは協調的で、取り消しの地点を通らないコードは止まらない。待たないループを取り消せるようにするには、ループの中で `check_cancelled ()` を呼ぶ
- `cancel_requested ()` は、取り消しの地点が投げるときだけ真を返す。`shield` の中では、今のスコープを今の `shield` の中で自分が開いたときだけ、その旗を読む。タスクの外では偽である。`await` から出た `Cancelled` を捕まえたタスクは、`cancel_requested ()` で自分が取り消されたかを確かめられる
- 投げるのは普通の IO の失敗なので、タスクのフレームを巻き戻して `Lin` の値を落とし、`try_io` と `try_all` が捕まえる。捕まえても取り消しは続き、次の取り消しの地点がまた投げる
- 種類 `Cancelled` の IO の失敗で終わったタスクは、スコープの失敗にならず、旗も立てない。利用者が `fail_io Prelude.Cancelled` で投げた失敗も同じである。`await` はそれを種類と文言のまま起こし直し、待たないタスクなら報告しない。これに対し、`scope` の本体が種類 `Cancelled` の失敗で終わると、失敗は記録しないが、スコープに旗を立てて子をすべて取り消す
- `Cancelled` が `await`、`run`、`scope` から出ると、呼んだタスクの普通の IO の失敗になる。`scope` を囲む `try_io` で捕まえれば、旗だけで取り消されていたタスクは続けて動ける。捕まえない `Cancelled` は実行時エラー `task was cancelled` になる
- `await t` で待つ側が取り消されたときは、`t` も取り消す
- `shield f` の中の取り消しの地点は、タスクのビットと、`shield` の外で開いたスコープの旗を見ない。`shield` の中の待ちは外の取り消しで外れない
- `shield` に入ってからタスクが最初に開くスコープ (`scope` のスコープ、`timeout`、`race`、`drain_within` の子のスコープ、入れ子の `run` の根) は境目である。境目は開くときに親の旗もタスクのビットも写さず、外からの印付けは境目で止まる。境目の内側で開くスコープは親の旗を写すが、タスクのビットは写さない
- 境目とその内側のスコープの旗は、`shield` の中でも、その中の取り消しの地点に届く。取り消しの地点は、今のスコープを今の `shield` の中で自分が開いたときだけ、その旗を見る。そのため、さらに内側の `shield` の中では届かない
- 守るのはスコープである。`shield` の中から外のスコープに加えたタスクは、外の取り消しを受ける。外のタスクが境目に `start_in` で加えたタスクは受けない
- `shield` を出た後の最初の取り消しの地点が、外の取り消しで投げる
- `shield` は上限の失敗 (上の「タスクの上限」) を後に回さない。`shield` の中でも上限の失敗はすぐに起き、取り消しの失敗の巻き戻しの途中に来た上限の印は、着地の後の次の呼び出しで起きる。取り消されたタスクが上限の印も受けると、先に地点に来たほうの失敗で終わるので、`Cancelled` でなく `ResourceLimit` (スコープの失敗) で終わることがある
- 閉じたスコープへの `cancel_scope` は何もしない。`cancel` と `cancel_scope` の row は `Async` を持たないので、タスクの外でも呼べる。`run` は待つ間も取り消しの地点でない

### 操作の予算

待たずに済む速い道は、タスクごとの予算を1つずつ使う。予算は 128 である。予算を使い切った速い道は、`yield_now` でほかのタスクに worker を渡す。`Std.Net` の待ちの渡し方は次の段落に書く。予算を使うのは、`check_cancelled` と、`Std.Chan` の `send`、`recv`、`try_send`、`try_recv` の速い道と、`Std.Net` の待ち (`Async.io_wait` と `io_wait_until`) である。

`Std.Net` の待ちは、操作がもう終わっていて待たずに続けるときも、終わっていなくて park するときも、1回ごとに予算を使う。`shutdown_write` と `set_nodelay` は、別の worker の driver に回して待つときだけ予算を使う (下の「`Std.Net`」)。読みとほかの待ちは1つ使う。書きの待ちは、1 に、プログラムが渡した長さを 512 で割った商を足した数を使い、残りより多ければ残りをすべて使う。64 KiB の書きは1回で予算を使い切る。読めた量はスケジュールで変わるが、書く長さはプログラムが決めるので、書きだけを重くする。予算を使い切った待ちは譲る。終わっていない操作の待ちは park して譲る。終わった操作の待ちは、まず OS を1回見て、届いた完了を集める ([VM](vm.md) の「時計と driver」)。そのうえで走れるタスクがあれば `reschedule ()` で譲り、その `Wake` を見ない。操作の結果を受け取ってから、次の取り消しの地点で投げるためである。走れるタスクがなければ、譲らずに予算を戻して続ける。譲るか続けるかはスケジュールで変わるが、どちらでも予算を戻し、待ちを1つ数えるので、予算の切れる位置と `RunStats.parks` は変わらない。同じ worker の上で書き手の書きが受け付けで終わり続けると、読み手は OS を見るまで起きず、書いたバイトが送りの列にたまって、読むときの写しが重くなる。書きの重みと OS の確かめで、書き手は 64 KiB ごとに読み手を起こす。予算は、タスクが始まるときと、操作の待ちでない再開のときと、予算を使い切った操作の待ちからの再開のときと、予算を使い切った待ちが譲らずに続けるときに 128 に戻す。予算の残る操作の待ちからの再開では戻さない。操作の待ちが park するかは、操作が受け付けで終わったかでスケジュールによって変わる。そこで予算を戻したり、終わった操作の待ちだけで予算を使ったりすると、予算の切れる位置と `RunStats.parks` もスケジュールで変わるためである。`parks` は、どの `Std.Net` の待ちも1つずつ数える。

`try_send`と`try_recv`のrowは`Async`を持たないので、予算を使うだけで譲らず、次に予算を見る`send`、`recv`、`check_cancelled`が譲る。待ちから起こされた直後のやり直しは、予算を使っても譲らない。譲った後のやり直しは予算をもう1つ使う。`cancel_requested`は予算を使わない。この予算は、テストのための方針`Seeded`が切り替える安全点を数える予算とは別で、その切り替えでは戻さない。戻す時点は上のとおりプログラムだけで決まるので、channelを使わないrunでは`RunStats.parks`はスケジュールによらず、ネイティブのコードでも同じに動く。予算はタスクのセルに持ち、`Ctx`には置かない。予算を使う行はどれも登録表に触れるので、`eml_rt`を変えずに済むためである。

### 時間

```haskell
body : Unit -> <IO, Panic, Async> (Option Int)
body () =
  Async.timeout 50 (fn () ->
    Async.sleep_ms 100
    1)
```

- 時間は今からの長さを `Int` のミリ秒で表す。0 以下は過ぎた期限である。時計は [VM](vm.md) の「時計と driver」にある
- `_within` の関数 (`Net.accept_within`、`Net.read_chunk_within`、`Net.read_line_within`) は driver の操作の待ちに期限を付ける。期限は待ちの属性で、子のスコープもタスクも作らない。期限までに終わらなければ操作を取り消し、取り消しが終わってから `None` と、渡したストリームかリスナーを返す。ストリームもリスナーも閉じず、続けて使える。期限と競って届いたデータは `Some` で返す。`_within` は取り消しの地点で、外からの取り消しは期限より優先して `Cancelled` を投げる (下の「`Std.Net`」)
- `sleep_ms ms` は `ms` ミリ秒待つ取り消しの地点である。0 以下なら `yield_now` と同じく列の後ろへ回る
- `timeout ms f` は子のスコープ `p` を開き、`ms` が正のときだけ `p` に期限のタスクを1つ始め、呼んだタスクで `scope (fn _ -> f ())` を動かす。期限のタスクは眠り切ると `p` に旗を立て、本体と本体が始めたタスクを取り消す。`ms` が 0 以下ならタスクを始めず、すぐ旗を立てる。`ms` が正のとき、期限のタスクを `RunStats.tasks` に1つ数える
- `timeout` 自身は取り消しの地点でない。期限は本体の取り消しの地点でだけ届く。本体が `await` で待つ外のタスクは、`await` の規則で取り消される (上の「取り消し」)
- 結果は次の表のとおりである。`None` は期限が過ぎたかだけで決まり、本体の `Cancelled` が何から来たかは見ない。期限が過ぎたとは、期限のタスクが眠り切って終わったか、`ms` が 0 以下のことである。どの場合も、本体が始めたタスクと期限のタスクが終わってから戻る

| 本体の終わり | 期限が過ぎた | `p` を離れた後に外から取り消されている | 結果 |
|---|---|---|---|
| 値 `x` (`Lin` も可) | どれでも | どれでも | `Some x` |
| IO の `Cancelled` | はい | いいえ | `None` |
| IO の `Cancelled` | はい | はい | 起こし直す |
| IO の `Cancelled` | いいえ | どれでも | 起こし直す |
| ほかの失敗 | どれでも | どれでも | 起こし直す。スコープの失敗にならない |

- 本体の失敗は呼んだタスクの失敗として出るので、`timeout` を囲む `try_all` が捕まえる
- `race f g` は子のスコープ `p` に腕のタスクを2つ始め、`shield` の中で両方の終わりを待つ。腕は `scope (fn _ -> f ())` を動かした後、値を返したときにまだ取り消されていなければ `p` に旗を立てて勝つ。旗が立つと、もう一方の腕とその腕が始めたタスクが取り消される。勝った側が `Left x` か `Right y` で、負けた側の値は巻き戻しが落とすので、`Lin` の値も漏れない。どちらの腕も `RunStats.tasks` に1つ数える
- 勝ちは1つの行`claim_scope s`で決める。呼んだタスクのビットと今のスコープの旗を読み、どちらも立っていなければ、同じ行で`s`に旗を立てて真を返す。立っていれば取り消しが届いたものとして`RunStats.cancels`に数え、偽を返す。確かめることと旗を立てることが1つの行なので、タスクがどこで切り替わっても、勝つのは1つである。`claim_scope`は`shield`の中のビットと旗の区別を見ない
- 同時に終わる2つの腕のどちらが勝つかは、スケジュールで決まる。テストの出力には、どちらが勝つかが両方ありうる形を出さない
- 腕の失敗 (種類 `Cancelled` を除く) は `p` の最初の失敗になり、もう一方を取り消す。勝った側があっても `race` から起こし直す。後の失敗は報告せず、`RunStats.dropped_failures` に数える
- 種類 `Cancelled` で終わった腕は勝たず、もう一方も取り消さない。両方がそう終われば、`f` の失敗を起こし直す
- 腕が勝つ前に呼んだタスクが取り消されたときは、`Cancelled` を起こし直す。勝った後の取り消しは `shield` の中の待ちに届かないので、`race` は勝った側を返し、戻った後の最初の取り消しの地点が投げる。`race` 自身は取り消しの地点でない。`shield` の中の `race` には外の取り消しが届かない。`race` の子のスコープが境目になるためである (上の「取り消し」)
- `shield` の中の `timeout` には、外の取り消しが届かず、期限は届く。`timeout` の子のスコープが境目になるためである (上の「取り消し」)。本体の中でさらに `shield` を開けば、その中には期限が届かない
- `drain_within ms s` は、スコープ `s` に属するタスクがなくなるまで、最大 `ms` ミリ秒待つ。期限にタスクが残っていれば、残ったタスクと、`s` が空になるまでに `s` に加わるタスクを取り消し、空になるまで待つ。空になった後に加わるタスクは普通に動く。`s` を閉じないので、`s` の本体は戻った後も `s` にタスクを加えられる
- 期限で取り消すのは `s` のタスクだけで、`s` の旗は立てない。そのため、呼んだタスクも `s` の本体も取り消さない。待ちは `s` が空になるのを待つ生の操作を `timeout ms` で区切ったもので、期限が取り消すのは呼んだタスクのこの待ちだけである。`timeout` が `None` を返したら、呼んだタスク自身が生の行 `cancel_members s` を呼び、`s` が空になるまで待ち直す。そのため、`drain_within` が戻った後に期限が `s` に触れることはなく、戻るときには加わるタスクを取り消す印は下りている。`timeout` の子のスコープが境目になるので、`shield` の中でも期限が効く。`ms` が正なら、`s` が閉じていても空でも `timeout` の期限のタスクを始め、`RunStats.tasks` に1つ数える
- 結果は次の表のとおりである

| 場合 | 結果 |
|---|---|
| `s` が閉じたか空のスコープか、離れたか前の run のスコープ | 時間を待たずに戻る |
| 期限より前に `s` が空になる | 戻る。期限のタスクは取り消す |
| 期限に `s` のタスクが残る | 残ったタスクと、`s` が空になるまでに加わるタスクを取り消し、空になるまで待つ |
| `ms` が 0 以下 | すぐ取り消してから、空になるまで待つ |
| 期限より前に呼んだタスクが取り消される | `s` に触れずに `Cancelled` を投げる |
| 期限が、`s` が空になるか呼んだタスクが取り消されるのと同じ時刻に来る | どちらが先かはスケジュールで決まる。どちらでも、戻った後に期限が `s` に触れることはない |
| 呼んだタスクが `s` に属する | 自分の終わりも待つ。`shield` の外なら、期限で自分も取り消される。`shield` の中では戻らず、ほかに進むタスクも満ちるタイマーもなくなると、実行時エラー `deadlock` で止まる ([VM](vm.md) の「時計と driver」) |

### `compute`

```haskell
count : Int -> Int -> <Panic> Int
count n acc = if n == 0 then acc else count (n - 1) (acc + 1)

body : Unit -> <IO, Panic, Async> Int
body () =
  let t = Async.start (fn () -> 1)
  Async.compute (fn () -> count 1000000 0) + Async.await t
```

- `compute f` は `f ()` を、子が1つの `par` と同じ仕事として動かし、仕事が終わるまで呼んだタスクだけを待たせる。待つタスクは継続だけになり、その間 worker はほかのタスクと仕事を動かす。仕事の結果は呼んだタスクに返る
- 仕事は `par` の右の子と同じく、空いている worker があればその受け箱に置いてその worker で動かし、なければ呼んだ worker の実行待ちの列の後ろに積む ([VM](vm.md) の「`compute` の仕事」)。worker が1つの run (REPL、`--workers 1`) では、仕事はその worker のほかのタスクと交互に動く
- `f` の row の上限は `never` である。`par` の子と同じく、`IO`、`Async`、`var` のエフェクト、`once` か `multi` の操作を持つエフェクトを `f` が起こしうる呼び出しは E2020 になる (上の「`Std.Par`」)。`f` は `Lin` の値を捕まえて消費でき、`Lin` の値を返してもよい
- 仕事が `never` の操作で中断すると、仕事が終わった後で、呼んだタスクがその操作を起こし直す。起こし直す先は、仕事の中の操作が届いた handler で、`compute` の周りの `mask` も守る。仕事の中の handler と `try_panic` は、仕事の中の操作を仕事の中で捕まえる。仕事の中で捕まえない `Panic` も、呼んだタスクで起こし直す。呼んだタスクの `try_panic` か `try_all` が捕まえ、捕まえなければタスクの失敗 `Panicked` になる
- 仕事の中の、`Panic` でない実行時エラー (呼び出しの深さの上限など) は、その場で run を止める。呼んだタスクの `try_all` も捕まえない。仕事はタスクの外なので、深さの上限は上の「タスクの上限」の `ResourceLimit` にならない
- `compute` は取り消しの地点だが、取り消しは仕事が終わってからでないと届かない。仕事の前や途中に立った取り消しは、仕事を止めない。仕事が終わった後、呼んだタスクが再開するときに取り消しの地点の規則 (上の「取り消し」) を満たしていれば、`compute` は仕事の結果を落として `Cancelled` を投げる。仕事が終わってから再開するまでに立った取り消しも、この規則で届く。`shield` の中では外の取り消しが届かないので、結果を返す
- そのため、`timeout` で囲んだ `compute` は、期限が過ぎても仕事が終わるまで戻らず、仕事の後で `None` を返す。`race` で負けた腕の仕事も最後まで動く
- `compute` 1回で `RunStats.parks` を1つ数え、`RunStats.tasks` は数えない ([VM](vm.md) の「`RunStats`」)

### メインのworker

```haskell
body : Unit -> <IO, Panic, Async> Unit
body () =
  let n = Async.run_on_main (fn () -> 20 + 1)
  println (show n)
```

- `start_on_main f`は、`f`をメインのworkerのタスクとして始め、そのハンドルを返す。メインのworkerは`Machine`を持つスレッドで、`eml run`ではプロセスのメインのスレッド、`eml repl`ではworker 0である ([VM](vm.md)の「メインのworker」)。`run_on_main f`は`await (start_on_main f)`である
- `start_on_main`で始めたタスクを固定したタスクと呼ぶ。固定したタスクが始めるタスク (入れ子の`Async.run`の根も含む) も固定し、`par`の子と`compute`の仕事は固定しない ([VM](vm.md)の「`Async` のタスク」)。固定したタスクはほかのworkerへ動かない
- `start_off_main f`と`spawn_off_main f`は`start`と`spawn`と同じだが、呼んだタスクが固定したタスクでも、新しいタスクを固定せずにworkerの輪に置く。新しいタスクは`Finished.pinned_tasks`に数えず、そのタスクが始めるタスクも固定しない。固定したタスクが開いたスコープの子にもなり、スコープを閉じるときはその終わりを待ち、取り消しも届く。メインのworkerの外で動かす仕事 (GUIを待たせないI/Oなど) に使う
- スレッドに結び付いた資源 (GUIのツールキット) は、固定したタスクからだけ使う

### `Failure` と `try_all`

```haskell
pub try_all : (Unit -> <IO, Panic | e> a) -> <IO | e> Result Failure a
```

- `try_all action` は、`action ()` の IO の失敗と `Panic` を両方捕まえて `Failure` の値にする。IO の失敗は `IOFailed` に `IOError` のまま入れ、`Panic` は `Panicked` に文言を入れる。成功すれば `Ok` で結果を返す
- `try_io` の中で `try_panic` を呼ぶ形で書く。`action` の `Panic` は内側の `try_panic` が、IO の失敗は外側の `try_io` が捕まえる (上の「`try_io`」と「`try_panic`」)
- タスクの底は `try_all` でタスクの失敗を捕まえる。接続ごとの分離のように、タスクの中で失敗を受け止めるときにも使う

## `Std.Chan`

`Std.Chan` はタスクの間で値を受け渡す channel のモジュールである。1つの型で、送り手と受け手をいくつでも持てる。送り手と受け手は別の worker で動いてよい。

```haskell
import Async (Async)
import Chan

body : Unit -> <IO, Panic, Async> Unit
body () =
  let c = Chan.new 0
  let r = Async.start (fn () -> Chan.recv c)
  println (show (Chan.send 2 c))
  println (show (Async.await r))
  Chan.close c
```

| 名前 | 型 | 実装 |
|---|---|---|
| `new` | `Int -> <IO> Chan a` | eml |
| `send` | `a -> Chan a -> <IO, Async> SendResult a` | eml |
| `recv` | `Chan a -> <IO, Async> Option a` | eml |
| `try_send` | `a -> Chan a -> <IO> TrySendResult a` | eml |
| `try_recv` | `Chan a -> <IO> TryRecvResult a` | eml |
| `close` | `Chan a -> <IO> Unit` | eml |
| `merge` | `List (Chan a) -> <IO, Async> Chan a` | eml |
| `SendResult` | `Sent` と `Rejected a` を持つ型 | eml (`deriving (Eq, Show)`) |
| `TrySendResult` | `Accepted`、`Full a`、`Refused a` を持つ型 | eml (`deriving (Eq, Show)`) |
| `TryRecvResult` | `Got a`、`Empty`、`Drained` を持つ型 | eml (`deriving (Eq, Show)`) |

- `Chan a` は登録表の添字を持つ `Unr` の即値で、Kind は `a` によらず `Unr` である。channel は `close` で放す資源で、閉じて空になった時点でその場で放す。閉じないままの channel と、閉じても品物の残った channel は run の終わりに放し、残った品物をそのとき落とす。放した channel と前の run の channel の値は、閉じて空の channel として振る舞う (`Scope` と同じ)。1回だけの受け渡しには `Task a` と `await` を使う。`Task a` は `Lin` で、捨てれば後始末が走るためである
- `new n` は容量 `n` の channel を作る。0 は待ち合わせで、負の数は 0 として扱う。積めるのは、積んだ品物の数が容量と待っている受け手の数の和より少ないときである。そのため、容量 0 でも受け手が待っていれば1つ積める
- `send x c` は、積めれば積んで `Sent` を返し、積めなければ枠が空くか閉じるまで待つ。閉じていれば `x` を `Rejected x` で返す。`recv c` は、品物があれば取って `Some` で返し、なければ品物が来るか閉じるまで待つ。閉じて空なら `None` を返す
- `try_send` と `try_recv` は待たない。`try_send` は、積めれば `Accepted`、満ちていれば `Full x`、閉じていれば `Refused x` を返す。`try_recv` は、品物があれば `Got x`、空なら `Empty`、閉じて空なら `Drained` を返す。どちらも row に `Async` を持たないので、タスクの外でも呼べる
- `close c` は何度呼んでもよく、2回目からは何もしない。待っている受け手と送り手をすべて起こす (同じ channel で両方が待つことはない)。積んであった品物は閉じた後も順に取れ、取り切ると `recv` は `None` を返す。起こされた送り手は `Rejected` を受け取る
- 閉じる前に枠を確かめた送りは、閉じた後に積まれることがある。そのとき受け手は閉じて空とせずに待ち、品物を受け取る
- 待っている受け手が開いた枠を送りが予約した後に、その受け手が取り消されると、品物は積まれて、次の受け手が取るまで channel に残る。容量によらず、その間 channel は容量を超えて品物を持つことがあり、ほかの送りは積めない
- 待ち手は側ごとに FIFO で起こす。起こされたタスクより先に速い道のタスクが品物や枠を取ることがあり、そのときは列の後ろで待ち直す。約束する順は起こす順だけである
- `send` と `recv` は取り消しの地点である (上の「取り消し」)。取り消しの巻き戻しは `close` を飛ばす。取り消されうるタスクで作った channel は、`try_all` の handler か `shield` の中で `close` する
- 品物の値は、積むときに送り手が印を付け、取るときに受け手が受け取る ([オブジェクトモデル](object-model.md) の「論理的な worker ID」)。断られた値と、待っている間に取り消された送り手の値は channel に入らない。送り手の値のまま戻るか、巻き戻しが落とす
- 待っているタスクのほかに進むものがなくなると、実行時エラー `deadlock` で止まる ([VM](vm.md) の「時計と driver」)
- `merge srcs` は容量 0 の出力を作り、入力ごとに転送のタスクを1つと、終わりを集めて出力を閉じるタスクを1つ、呼んだタスクの今のスコープに `spawn` する。空の並びなら閉じた出力を返し、タスクを作らない。出力の順はスケジュールで決まる。入力がすべて閉じて空になると出力を閉じる。`merge` の結果をそのスコープの外へ出すと、スコープが閉じるときに転送は取り消される。出力を先に閉じると、転送のタスクは次に受け取った品物を落として終わる。閉じない入力を待つ転送は、入力が閉じるかスコープが取り消されるまで待つ。転送の終わりは容量 0 の `Chan Unit` で集め、終わった後にその channel も閉じるので、`merge` は run の終わりまで残る資源を作らない
- REPL の入力をまたいだ channel の扱いは、[モジュールと名前解決](modules.md) の「REPL のセッション」にある

`Std.Chan` の extern の行は次の7行で、どれも `pub` でない。`chan_cell` のほかの6行は `Async` の命令で実行する ([VM](vm.md) の「channel」)。

| 行 | 型 | 預かる引数 |
|---|---|---|
| `new_raw` | `Int -> <IO> Chan a` | なし |
| `chan_cell` | `Chan a -> Async.Cell` | なし |
| `reserve_send` | `Chan a -> Bool -> <IO> Int` | なし |
| `push_raw` | `a -> Chan a -> <IO> Unit` | 品物 (1つ目の引数) |
| `reserve_recv` | `Chan a -> Bool -> <IO> Int` | なし |
| `pop_raw` | `Chan a -> <IO> a` | なし |
| `close_raw` | `Chan a -> <IO> Unit` | なし |

- `reserve_send` と `reserve_recv` は、0 (予約した。続けて確定する)、1 (待つ)、2 (閉じている。受け取りでは閉じて空)、3 (先に譲る。2つ目の引数が真のときだけ) を返す
- `reserve_*` が 0 を返すと、枠か品物を1つ予約し、続く `push_raw` か `pop_raw` が確定する。間には安全点も、タスクの切り替えも、失敗しうる命令も置かない (`Int` の比べと分岐と、積む品物の箱詰めは置いてよい)。予約を持ったまま切り替えたり抜けたりしないためである。標準ライブラリがこの形を保ち、処理系は予約があることだけを確かめる ([VM](vm.md) の「channel」)
- 待ちは `Std.Async` の `chan_wait_send` と `chan_wait_recv` (`Cell -> <IO, Async> Wake`) を通す

```haskell
send : a -> Chan a -> <IO, Async> SendResult a
send x c = if Async.cancel_requested () then raise_cancelled () else send_loop x c True

recv : Chan a -> <IO, Async> Option a
recv c = if Async.cancel_requested () then raise_cancelled () else recv_loop c True

send_loop : a -> Chan a -> Bool -> <IO, Async> SendResult a
send_loop x c yields =
  let r = reserve_send c yields
  if r == 0 then
    push_raw x c
    Sent
  else if r == 2 then Rejected x
  else if r == 3 then
    Async.yield_now ()
    send_loop x c True
  else match Async.chan_wait_send (chan_cell c) with
    | Async.Woke -> send_loop x c False
    | Async.Cancelled -> raise_cancelled ()

recv_loop : Chan a -> Bool -> <IO, Async> Option a
recv_loop c yields =
  let r = reserve_recv c yields
  if r == 0 then Some (pop_raw c)
  else if r == 2 then None
  else if r == 3 then
    Async.yield_now ()
    recv_loop c True
  else match Async.chan_wait_recv (chan_cell c) with
    | Async.Woke -> recv_loop c False
    | Async.Cancelled -> raise_cancelled ()
```

## `Std.Atomic`

`Std.Atomic` はタスクと REPL の入力の間で共有する数 `Counter` のモジュールである。

```haskell
import Atomic

body : Unit -> <IO> Int
body () =
  let c = Atomic.new 0
  Atomic.add 2 c
```

| 名前 | 型 | 実装 |
|---|---|---|
| `Counter` | `Unr` の即値の型 | extern |
| `new` | `Int -> <IO> Counter` | extern |
| `add` | `Int -> Counter -> <IO> Int` | extern |
| `load` | `Counter -> <IO> Int` | extern |
| `store` | `Int -> Counter -> <IO> Unit` | extern |

- `Counter` は、run をまたいで `Machine` が持つ表の添字を持つ `Unr` の即値である。表の項は `SeqCst` の atomic で、どの worker のタスクも lock を取らずに `add`、`load`、`store` を行う。`add n c` は `fetch_add` で足して、足した後の値を返し、64 ビットで巻き戻る。`Counter` は放さない。数の少ない共有の状態のためのものである
- 行は `Async` の命令で動くので、`Std.Atomic` は `Std.Async` を読み込む。row は `IO` だけで、タスクの外でも、REPL の入力をまたいでも使え、実行時エラーで捨てた run の `add` も残る

## `Std.Net`

`Std.Net` は TCP のモジュールである。`TcpListener` と `TcpStream` は `Lin` の `Foreign` で、落とすとソケットを閉じる。待つ関数はどれも取り消しの地点で、受け付け、待ち、取り出しの3段で動く ([VM](vm.md) の「操作の記録」)。

```haskell
import Async
import Net

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let t = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    s)
  Net.close (Net.connect "127.0.0.1" port)
  Net.close (Async.await t))
```

| 名前 | 型 | 実装 |
|---|---|---|
| `TcpListener` | `Lin` の `Foreign` の型 | extern |
| `TcpStream` | `Lin` の `Foreign` の型 | extern |
| `listen` | `String -> Int -> <IO, Async> TcpListener` | eml |
| `local_port` | `TcpListener -> (Int, TcpListener)` | extern |
| `accept` | `TcpListener -> <IO, Async> (TcpStream, TcpListener)` | eml |
| `accept_within` | `Int -> TcpListener -> <IO, Async> (Option TcpStream, TcpListener)` | eml |
| `close_listener` | `TcpListener -> <IO> Unit` | eml |
| `connect` | `String -> Int -> <IO, Async> TcpStream` | eml |
| `read_chunk` | `Bytes -> TcpStream -> <IO, Async> (Bytes, TcpStream)` | eml |
| `read_chunk_within` | `Int -> Bytes -> TcpStream -> <IO, Async> (Option Bytes, TcpStream)` | eml |
| `read_line` | `TcpStream -> <IO, Panic, Async> (Option String, TcpStream)` | eml |
| `read_line_within` | `Int -> TcpStream -> <IO, Panic, Async> (Option (Option String), TcpStream)` | eml |
| `write` | `String -> TcpStream -> <IO, Async> TcpStream` | eml |
| `write_bytes` | `Bytes -> TcpStream -> <IO, Async> (Bytes, TcpStream)` | eml |
| `write_line` | `String -> TcpStream -> <IO, Async> TcpStream` | eml |
| `shutdown_write` | `TcpStream -> <IO, Async> TcpStream` | eml |
| `close` | `TcpStream -> <IO> Unit` | eml |
| `peer_addr` | `TcpStream -> (String, TcpStream)` | extern |
| `set_nodelay` | `Bool -> TcpStream -> <IO, Async> TcpStream` | eml |

- `TcpListener` と `TcpStream` の Repr は `obj` である。落としたハンドルは、driver が次の待ち、完了の取り出し、`quiesce` のどれかのときに閉じる。`close s` と `close_listener l` は落とすだけである
- `listen host port` の `host` は IP アドレスの綴り (`0.0.0.0`、`127.0.0.1`、`::`、`::1` など) で、名前は受け付けない。`port` は 0 から 65535 で、0 なら空いているポートを使う。使ったポートは `local_port` で読む
- `connect host port` の `host` は IP アドレスの綴りか名前で、`port` は 1 から 65535 である。名前は driver の解決のスレッドで解決し、得た番地を順に試して、最後に試した番地の誤りを報告する。取り消し、期限、`quiesce` は名前の解決を待たない ([VM](vm.md) の「時計と driver」)
- `accept l` はリスナーに来た接続を1つ受け、そのストリームとリスナーを返す。接続が来るまで待つ
- `accept_within ms l` は `accept l` を `ms` ミリ秒の期限で区切り、結果を `Option` に包む。`None` は期限が過ぎたことだけを表し、リスナーは続けて使える。期限と競って届いた接続は `Some` で返す (上の「時間」)
- `read_chunk b s` は `s` を1回読み、読んだバイトを `b` の後ろに足して、その `Bytes` とストリームを返す。足すのは 1 バイト以上 65 536 バイト以下で、EOF のときだけ何も足さずに `b` の中身のまま返す。EOF は失敗でない (`FileIn.read_chunk` と同じ形)。読めるものがなければ、相手が書くか閉じるまで待つ。`b` が一意で空きが 8 バイト以上あれば、その空きの 65 536 バイトまでに読む。なければ、ストリームが読めるようになってから容量を広げて、65 536 バイトまで読む。広げる容量は `長さ + max(長さ, 65 536)` 以上で、足し続ける読みの写しは合わせて線形に収まる。空きは `Bytes.with_capacity` か、使い終えた buffer の `truncate 0` で作れる ([VM](vm.md) の「操作の記録」)
- 読みは次のことを保証する。受け取ったバイトはすべて、受け取った順に `read_chunk` か `read_line` に届く。`read_line` で一度も読んでいないストリームは隠れた読みの buffer を持たないので、読んだバイトは `read_chunk` が返した `Bytes` にしかない。`read_line` が読みすぎたバイトはストリームの buffer に残り、次の `read_chunk` がそれを先に、1回に 65 536 バイトまで、driver の操作を出さずに返す。そのため `read_line` と `read_chunk` を混ぜても、バイトは失われず、順も入れ替わらない
- `read_chunk_within ms b s` は `read_chunk b s` を `ms` ミリ秒の期限で区切り、結果を `Option` に包む。`None` は期限が過ぎたことだけを表し、そのとき読んだバイトはない。`b` は捨て、ストリームは閉じずに続けて使える。期限と競って届いたデータは `Some` で返す。期限と競った本物の誤り (reset など) は、期限のない読みと同じく投げる。そのため `None` はバイトを失わず、誤りも隠さない。`ms` が 0 以下の過ぎた期限は、待ち始める前に読みが終わったかを見るだけである。データのないストリームではいつも `None` だが、データのあるストリームでは、待ち始める前に読みが終わったかがスケジュールで変わるので、`Some` と `None` のどちらにもなる
- `read_line s` は `s` から1行を読み、終わりの改行 (`\n` か `\r\n`) を除いた行とストリームを返す。改行のない最後の行はそのまま返し、残りのない EOF では `None` を返す (`Stdin.read_line` と同じ形)。読んだバイトはストリームの buffer に入る。buffer にもう1行あれば driver の操作を出さずに返し、なければ改行か EOF が来るまで読みを繰り返す。buffer に空きがなければ、読む前に読めるようになるのを待つ。行の中身は 1 MiB (1 048 576 バイト) までである。中身は終わりの `\n` か `\r\n` を除いたバイトで、EOF の前の最後の行では残りのすべてである。終わりの改行は上限に数えない。中身が上限を超える行は、改行が届いたときか、改行の届く前に読んだ中身が上限を超えたときに、種類 `Other` の IO の失敗 `cannot read the stream: a line is longer than 1048576 bytes` になる。そのため結果は、バイトが読みにどう分かれて届くかによらない。UTF-8 でない行は、種類 `NotUtf8` の IO の失敗 `cannot read the stream: the line is not valid UTF-8` になる。どちらの失敗でも、包みはストリームを落としてから投げるので、ストリームは呼ぶ側に戻らない
- `read_line_within ms s` は `read_line s` を `ms` ミリ秒の期限で区切り、結果を `Option` に包む。期限は1行の全体に付き、読みの1回ごとには付かない。`None` は期限が過ぎたことだけを表し、そのとき読んだ途中の行はストリームの buffer に残って、次の読みが続きと合わせて返す。期限と競って行がそろったときは `Some` で返す
- `_within` の関数 (`accept_within`、`read_chunk_within`、`read_line_within`) は、期限のない関数の結果を `Option` に包んで返す。`None` は期限が過ぎたことだけを表し、リスナーかストリームは続けて使える
- `write_bytes b s` は `b` をすべて書き、`b` とストリームの組を返す。返す `b` は渡した物体そのもので、共有の `Bytes` でも写さない ([VM](vm.md) の「操作の記録」)。返った `b` は次の書きにも、`truncate 0` で空にして読みにも使える。本物の driver は、短く終わった送りを出し直す。`write str s` は `str` の UTF-8 のバイトを書き、`write_line str s` は `str` と改行を書く。どれも `write_bytes` を呼ぶ
- 書きの期限は `Async.timeout ms (fn () -> Net.write_bytes b s)` で付ける。期限が来ると、巻き戻しがストリームを閉じる
- `local_port` と `peer_addr` は、作ったときに記録した値を読む。引数を借りて組で返し ([Core IR](core-ir.md) の「組を返す extern」)、効果を持たない。`peer_addr` は Rust の `SocketAddr` の表示の形 (`127.0.0.1:49153`、`[::1]:8080`) で返す
- `shutdown_write s` は送る側を閉じ、相手は残りを読みきってから EOF を受ける。`set_nodelay on s` は TCP_NODELAY を設定する。どちらも driver の操作で、driver は受け付けで終える。包みは `submit_shutdown_write` か `submit_set_nodelay` で token を作り、`op_done` で記録が終わったかを読む。持ち主の worker が host なら受け付けで終わっていて `op_done` が真なので、待たずに `op_stream` でストリームを取り戻し、`finish` で記録を放す。このときは操作の予算を使わず、`parks` も数えない。別の worker の driver に回したときだけ、host が受け付けるまで `Async.io_wait_shielded` で待ってから同じく取り戻し、そのときだけ予算を使う。待つかどうかはタスクを置いた worker で決まり、スケジュールでは変わらない。待ちが park して `parks` を数えるかは、host が結果を置くのが待ちの前か後かで決まる。`op_done` は host が置いた結果を当てずに読むためである ([VM](vm.md) の「操作の記録」)。どちらも取り消しの地点でなく、host が受け付けるまでの待ちも外の取り消しで外れない。届いていた取り消しは、呼んだ後の最初の取り消しの地点で投げる。失敗はそれぞれ IO の失敗 `cannot shut down the stream: …` と `cannot set the no-delay option: …` で、誤りの種類によらず `finish` が投げる。種類は次の箇条の、extern が投げる誤りと同じ規則で決める
- 拒まれた接続は種類 `ConnectionRefused`、reset と EPIPE と ECONNABORTED は `ConnectionReset`、使用中の番地は `AddrInUse`、OS の時間切れは `TimedOut` である。この4つは包みが `fail_io` で投げる。ほかの誤りは extern が投げ、種類は最初の5つから OS の誤りで決める。解決できない名前は `NotFound`、権限のない操作 (1024 より小さいポートの `listen` など) は `PermissionDenied`、扱えない操作は `NotAvailable`、残りは `Other` である (上の「`try_io`」)
- 文言は prefix と理由を `: ` でつないだものである。prefix は `cannot listen on {host}:{port}`、`cannot connect to {host}:{port}`、`cannot accept a connection`、`cannot read the stream`、`cannot write to the stream` のどれかである。理由は、包みが投げる4つの種類では `connection refused`、`connection reset`、`address in use`、`timed out` で、ほかは driver の誤りの表示である。テストの driver では、解決できない名前の理由は `unknown host` になり、相手が閉じたストリームへの書きは `cannot write to the stream: connection reset` (種類 `ConnectionReset`) になる
- `listen` の `host` が IP アドレスの綴りでないときと、`listen` と `connect` の `port` が範囲の外のときは、操作を出さずに種類 `Other` の IO の失敗を起こす。理由はそれぞれ `the host is not an IP address` と `the port is out of range` である
- 待つ関数の包みは同じ形である。取り消しの地点が投げるなら、渡すリスナーかストリームがあれば落とし、何も受け付けずに種類 `Cancelled` の失敗を投げる。そうでなければ、受け付けの行で token を作り、`Async.io_wait` (`_within` は `Async.io_wait_until`) で待つ。操作がもう終わっていて取り消しの地点が投げなければ、待たずに続ける (上の「`Std.Async`」の `io_wait`)。driver が受け付けの時点で決められる操作は、受け付けで終わる。`accept` と `accept_within` は `op_listener` でリスナーを、読みと書きは `op_stream` でストリームを取り戻す。続けて `op_outcome` で結果の番号を読む。再開の `Wake` が `Cancelled` なら、`finish` で記録を放し、リスナーかストリームがあれば落として `Cancelled` を投げる。`Woke` なら次の表で分ける。表は上の行から順に見て、最初に合う行を使う

| 記録の状態 | 番号 | 包みがすること |
|---|---|---|
| 結果が成功 (期限と競ったときも) | 0 | 取り出しの行で結果を受け取る。`_within` は `Some` に包む。書きは結果を受け取らず、`finish` で記録を放す |
| 期限が来て、完了が期限の取り消しだけで終わった | 1 | `finish` で記録を放し、`None` とリスナーかストリームを返す |
| 誤りの種類が ConnectionReset、BrokenPipe、ConnectionAborted | 2 | `finish` で記録を放し、`ConnectionReset` を投げる |
| 誤りの種類が ConnectionRefused | 3 | 同じく `ConnectionRefused` を投げる |
| 誤りの種類が AddrInUse | 4 | 同じく `AddrInUse` を投げる |
| 誤りの種類が TimedOut | 5 | 同じく `TimedOut` を投げる |
| ほかの誤り | 6 | `finish` が、driver の誤りの種類を最初の5つに写して投げる |

- 期限と競った本物の誤りは、期限に負けたとせずに報告する。誤りはそれを報告した syscall が消費するので、後の操作は同じ誤りを見られない。番号 1 は期限があるときだけ出るので、見るのは `_within` の包みだけである
- extern の行は次の19行である。`local_port` と `peer_addr` のほかは `pub` でない。VM は `Std.Async` の行と同じく `Async` の命令で実行する ([VM](vm.md) の「`Async` のタスク」)

| 行 | 型 | 借りる引数 |
|---|---|---|
| `submit_listen` | `String -> Int -> <IO> Async.OpId` | なし |
| `submit_accept` | `TcpListener -> <IO> Async.OpId` | なし |
| `submit_connect` | `String -> Int -> <IO> Async.OpId` | なし |
| `submit_read` | `Bytes -> TcpStream -> <IO> Async.OpId` | なし |
| `submit_read_line` | `TcpStream -> <IO> Async.OpId` | なし |
| `submit_write` | `Bytes -> TcpStream -> <IO> Async.OpId` | なし |
| `submit_shutdown_write` | `TcpStream -> <IO> Async.OpId` | なし |
| `submit_set_nodelay` | `Bool -> TcpStream -> <IO> Async.OpId` | なし |
| `op_listener` | `Async.OpId -> <IO> (TcpListener, Async.OpId)` | token |
| `op_stream` | `Async.OpId -> <IO> (TcpStream, Async.OpId)` | token |
| `op_outcome` | `Async.OpId -> <IO> (Int, Async.OpId)` | token |
| `op_done` | `Async.OpId -> <IO> (Bool, Async.OpId)` | token |
| `take_listener` | `Async.OpId -> <IO> TcpListener` | なし |
| `take_stream` | `Async.OpId -> <IO> TcpStream` | なし |
| `take_bytes` | `Async.OpId -> <IO> Bytes` | なし |
| `take_line` | `Async.OpId -> <IO> String` | なし |
| `finish` | `Async.OpId -> <IO> Unit` | なし |
| `local_port` | `TcpListener -> (Int, TcpListener)` | リスナー |
| `peer_addr` | `TcpStream -> (String, TcpStream)` | ストリーム |

- 受け付けの行は引数を消費して記録に移し、token を返す。`submit_listen` と `submit_connect` は `host` を写して手放し、文言の prefix を記録に持たせる。`submit_read` と `submit_write` は受け付けで buffer を写さない。`submit_read` は一意でない buffer (共有、不死、ほかの参照) を広げるときに1回だけ写し、`submit_write` は一意でない buffer もそのまま貸す。その場で書き換える操作はどれも一意を求めるので、driver が読む間に書き換わらない ([VM](vm.md) の「操作の記録」)。`submit_read` は、ストリームの buffer に `read_line` が残したバイトがあれば、それを buffer に足して、driver の操作を出さずに終わった記録を作る。`submit_read_line` はストリームだけを記録に移す
- `op_listener` と `op_stream` は、終わった記録から受け付けに渡したものを移し出す。`op_outcome` は結果の番号を返す。`op_done` は記録が終わったかを返し、予算を使わず、取り消しの地点も読まない
- `take_listener` と `take_stream` は token を消費して記録を放し、完了から作ったリスナーかストリームを返す。`take_bytes` は token を消費して記録を放し、記録の buffer を返す。読みでは読んだバイトを足した buffer、書きでは書いた buffer そのものである。`take_line` は token を消費して記録を放し、記録が切り出した行を終わりの改行を含めて返す。残りのない EOF では空の文字列を返し、包みがそれを `None` にする。どれも、番号が 0 でない記録では内部の誤り `an op is taken after it failed` になる
- `finish` は token を消費して記録を放し、記録がまだ持つもの (完了から作ったハンドルを含む) を落とす。番号 6 の誤りは、取り消しを頼んだ記録でなければ IO の失敗にする

## `Std.Stream`

`Std.Stream` は、driver で読み書きする通常のファイルと子のプロセスの pipe のモジュールである。`Reader` と `Writer` は `Lin` の `Foreign` で、落とすとファイルか pipe の端を閉じる。子の pipe の読み手と書き手は `Std.Exec` が作り、この節のファイルの規則のうち開くことと仕事の取り消しには従わない (下の「`Std.Exec`」)。待つ関数はどれも取り消しの地点で、`Std.Net` と同じく受け付け、待ち、取り出しの3段で動く ([VM](vm.md) の「操作の記録」)。ファイルの読み書きは driver のプールの仕事なので、タスクがファイルを待っても、同じ worker のほかのタスクは止まらない。`Async.run` の外で読み書きするときは、ブロックする `FileIn`、`FileOut`、`Fs` を使う。2つの組はハンドルの型を共有しない。

```haskell
import Async
import Stream

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let w = Stream.create "out.txt"
  let w = Stream.write_line "hello" w
  Stream.close_writer w
  let r = Stream.open "out.txt"
  let (text, r) = Stream.read_all r
  Stream.close r
  println text)
```

| 名前 | 型 | 実装 |
|---|---|---|
| `Reader` | `Lin` の `Foreign` の型 | extern |
| `Writer` | `Lin` の `Foreign` の型 | extern |
| `open` | `String -> <IO, Async> Reader` | eml |
| `create` | `String -> <IO, Async> Writer` | eml |
| `append` | `String -> <IO, Async> Writer` | eml |
| `read_chunk` | `Bytes -> Reader -> <IO, Async> (Bytes, Reader)` | eml |
| `read_line` | `Reader -> <IO, Panic, Async> (Option String, Reader)` | eml |
| `read_line_bytes` | `Reader -> <IO, Panic, Async> (Option Bytes, Reader)` | eml |
| `read_all` | `Reader -> <IO, Async> (String, Reader)` | eml |
| `read_all_bytes` | `Reader -> <IO, Async> (Bytes, Reader)` | eml |
| `write` | `String -> Writer -> <IO, Async> Writer` | eml |
| `write_bytes` | `Bytes -> Writer -> <IO, Async> (Bytes, Writer)` | eml |
| `write_line` | `String -> Writer -> <IO, Async> Writer` | eml |
| `close` | `Reader -> <IO> Unit` | eml |
| `close_writer` | `Writer -> <IO> Unit` | eml |

- `Reader` と `Writer` の Repr は `obj` である。落としたハンドルは、`Std.Net` のハンドルと同じく、driver が次の待ち、完了の取り出し、`quiesce` のどれかのときに閉じる。`close r` と `close_writer w` は落とすだけである。`Writer` は書きの buffer を持たないので、書き出すものはなく、`flush` もない
- `open path` は読むために、`create path` は作るか切り詰めて、`append path` は作るか末尾に足す形で書くために開く。パスは `FileIn.open` と同じく `file_root` からの相対パスか絶対パスである。開くのは Host で ([オブジェクトモデル](object-model.md) の「Host」)、開いたファイルは開いたタスクの I/O を受け付ける worker の driver に取り込む。それから後の読み書きは、その driver のプールの仕事になる ([VM](vm.md) の「時計と driver」)
- 開くのは通常のファイルだけである。あるのに通常のファイルでないもの (ディレクトリ、FIFO、端末、`/dev/stdin`、ソケット、デバイス) は、どの向きでも、どの OS でも、種類 `NotAvailable` の IO の失敗 ``cannot open `<path>`: not a regular file`` になる。読み手のない FIFO を書くために開くときやソケットのファイルのように、OS が開くこと自体を断るときも、この失敗にする。FIFO を開く呼び出しは止まらず (unix の `O_NONBLOCK`)、開けたものは通常のファイルでないと分かったら閉じるので、プールのスレッドを止めたままにするファイルは driver に届かない。こうしたファイルはブロックする `FileIn` と `FileOut` で読み書きする。応答しないネットワークのファイルシステムは、ブロックする行と同じく仕事を止めうる。そのとき `quiesce` は仕事が終わるまで待つ
- 開けないファイルは、`FileIn.open` と `FileOut.create` と同じ種類と文言の IO の失敗になる (``cannot open `missing.txt`: not found``)。この失敗は driver に何も出さずに起きる
- `read_chunk b r` は `Net.read_chunk` と同じ形で、1回の読みで 1 バイト以上 65 536 バイト以下を `b` の後ろに足し、その `Bytes` と読み手を返す。EOF のときだけ何も足さない。`b` が一意で空きが 8 バイト以上あれば、その空きの 65 536 バイトまでに読む。なければ、受け付けで容量を `長さ + max(長さ, 65 536)` 以上に広げて、65 536 バイトまで読む。ソケットと違い、読めるようになるのを待ってから広げることはない ([VM](vm.md) の「操作の記録」)
- `read_line r` は `Net.read_line` と同じ形で、終わりの改行を除いた行を返し、残りのない EOF では `None` を返す。行の中身の上限 (1 MiB) も同じで、上限を超える行は種類 `Other` の IO の失敗 ``cannot read `<path>`: a line is longer than 1048576 bytes``、UTF-8 でない行は種類 `NotUtf8` の ``cannot read `<path>`: the line is not valid UTF-8`` になる。読みすぎたバイトは読み手の buffer に残り、次の `read_chunk` か `read_line` が先に返す
- `read_line_bytes r` は `read_line` と同じ1行を、`FileIn.read_line_bytes` と同じく終わりの改行を除いたバイト列で返す。UTF-8 でない行も失敗にしない。上限と読みすぎたバイトの扱いは `read_line` と同じである
- `read_all_bytes r` は `read_chunk` を1つの `Bytes` に繰り返し、何も足さない読みで終える。`read_all r` はそれを UTF-8 の文字列にし、UTF-8 でなければ読み手を落として、種類 `NotUtf8` の IO の失敗 ``cannot read `<path>`: the text is not valid UTF-8`` にする
- `write_bytes b w` は `b` をすべて書き、`b` と書き手の組を返す。返す `b` は渡した物体そのもので、共有の `Bytes` でも写さない。`write s w` は `s` の UTF-8 のバイトを書き、`write_line s w` は `s` と改行を書く。どれも1回の操作ですべてを書き、書いた後のファイルは、同じ run のほかの読み手からすぐ読める
- 走り出したファイルの仕事は取り消せない。読みか書きを待つタスクが取り消されると、そのタスクは仕事が終わるまで待ち、終わってから読み手か書き手を落として種類 `Cancelled` の失敗を投げる。読みの仕事が読んだバイトは確定してから捨てる。`Async.timeout ms (fn () -> Stream.read_chunk b r)` も、期限が来たときに仕事が走っていれば、仕事が終わってから戻る。テストの driver は待ちの区切りでファイルの仕事を終え、仮想の時計はそれを待ってから進むので、仮想の時計の期限が走っている仕事より先に来ることはない ([VM](vm.md) の「時計と driver」)
- ファイルの読み書きはdriverのプールの仕事で、`blocking`のCの呼び出しと同じプールを使う。`blocking`の呼び出しは、タスクを置いたworkerのdriverのプールで走る。1つのdriverで同時に走る`blocking`の呼び出しは24までで、この上限は`Std.Tls`の仕事と分け合う。それを超えた呼び出しは順に待つ。ファイルの読み書きのために、別に8本を残す。workerをN個使うrunでは上限はdriverごとにあり、タスクをどのworkerに置くかはランタイムが決める。仮想の時計 (テスト) では、仕事はworker 0の上でその場で動き、上限はない。`blocking`の呼び出しの中で、同じrunのほかの`blocking`の呼び出しの結果を待つCの関数は、上限に達するとデッドロックする ([宣言](declarations.md) の「`extern "C"`」)
- 待つ関数の包みは `Std.Net` と同じ形で、結果の番号も同じ 0 から 6 である (上の「`Std.Net`」の表)。番号 2 (OS の誤りの種類が ConnectionReset、BrokenPipe、ConnectionAborted) は、種類 `ConnectionReset` の ``<prefix>: broken pipe`` にする。番号 3 から 5 は `Std.Net` と同じ種類と文言である。番号 6 は `finish` が投げ、種類は `Std.Net` と同じく OS の誤りから最初の5つに写す。文言は prefix と OS の誤りの表示を `: ` でつないだもので、OS の文言を保つ。そのため、ディスクの満杯や読み取り専用の書き先は `Other`、書く権限のないファイルは `PermissionDenied` である。prefix は、開くことでは ``cannot open `<path>` ``、読みでは ``cannot read `<path>` ``、書きでは ``cannot write to `<path>` `` で、パスは開いたときに渡したままのものである。番号 2 から 5 の文言の prefix は、包みが `op_prefix` で記録から読む
- extern の行は次の15行で、どれも `pub` でない。VM は `Std.Net` の行と同じく `Async` の命令で実行する ([VM](vm.md) の「`Async` のタスク」)

| 行 | 型 | 借りる引数 |
|---|---|---|
| `submit_open` | `String -> Int -> <IO> Async.OpId` | なし |
| `submit_read` | `Bytes -> Reader -> <IO> Async.OpId` | なし |
| `submit_read_line` | `Reader -> <IO> Async.OpId` | なし |
| `submit_write` | `Bytes -> Writer -> <IO> Async.OpId` | なし |
| `op_reader` | `Async.OpId -> <IO> (Reader, Async.OpId)` | token |
| `op_writer` | `Async.OpId -> <IO> (Writer, Async.OpId)` | token |
| `op_outcome` | `Async.OpId -> <IO> (Int, Async.OpId)` | token |
| `op_prefix` | `Async.OpId -> <IO> (String, Async.OpId)` | token |
| `take_reader` | `Async.OpId -> <IO> Reader` | なし |
| `take_writer` | `Async.OpId -> <IO> Writer` | なし |
| `take_bytes` | `Async.OpId -> <IO> Bytes` | なし |
| `take_line` | `Async.OpId -> <IO> String` | なし |
| `take_line_bytes` | `Async.OpId -> <IO> Bytes` | なし |
| `finish` | `Async.OpId -> <IO> Unit` | なし |
| `source` | `Reader -> (String, Reader)` | 読み手 |

- `submit_open path mode` は、`mode` が 0 なら読むために、1 なら作るか切り詰めて、2 なら作るか末尾に足す形で開く。開けなければ記録を作らずに失敗を投げる。開けたファイルの取り込みは host の driver が受け付けで終えるので、開くタスクが待つのは、host がほかの worker のとき (仮想の時計の worker 1 以降) の知らせの往復の間だけである
- `op_reader` と `op_writer` は `Std.Net.op_stream` と同じく、終わった記録から受け付けに渡した読み手か書き手を移し出す。`op_prefix` は終わった記録の誤りの文言の prefix を返す。`take_reader` と `take_writer` は、`open` と、`create` か `append` の記録から作った読み手か書き手を返す。`take_line_bytes` は、`take_line` と同じ行の記録から、行を UTF-8 かどうかによらず `Bytes` で返す。ほかの行は `Std.Net` の同じ名前の行と同じである。`source` は、読み手を開いたパスを `` ` `` で囲んだ文字列 (子の pipe の読み手では ``the output of `<program>` ``) を返し、`read_all` が UTF-8 の失敗の文言に使う。借りて組で返し、効果を持たない

## `Std.Exec`

`Std.Exec` は、子のプロセスを driver の上で起動して待つモジュールである。`Child` は `Lin` の `Foreign` で、段ごとの子のプロセスと、まだ取り出していない標準入力、標準出力、標準エラーの pipe の端を持つ。起動の設定は `Proc` と同じ `Cmd` で組み (上の「`Cmd`」と「パイプライン」)、pipe は `Std.Stream` の `Writer` と `Reader` で読み書きする。待つ関数はどれも取り消しの地点で、`Std.Stream` と同じく受け付け、待ち、取り出しの3段で動く ([VM](vm.md) の「操作の記録」)。子を待つ間も pipe を読み書きする間も、タスクのスレッドは OS の中で止まらないので、同じ worker のほかのタスクは止まらない。`Async.run` の外で子を起動するときは、ブロックする `Proc` を使う。`Proc.Child` と `Exec.Child` は別の型である。

```haskell
import Async
import Exec
import Proc ((.|))
import Proc

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  println (Exec.capture (`sort` .| `uniq -c` |> Proc.stdin_text "b\na\nb\n")))
```

| 名前 | 型 | 実装 |
|---|---|---|
| `Child` | `Lin` の `Foreign` の型 | extern |
| `spawn` | `Cmd -> <IO, Async> Child` | eml |
| `run` | `Cmd -> <IO, Async> Int` | eml |
| `output` | `Cmd -> <IO, Async> (String, Int)` | eml |
| `output_bytes` | `Cmd -> <IO, Async> (Bytes, Int)` | eml |
| `capture` | `Cmd -> <IO, Async> String` | eml |
| `capture_bytes` | `Cmd -> <IO, Async> Bytes` | eml |
| `stdin` | `Child -> <IO> (Stream.Writer, Child)` | extern |
| `stdout` | `Child -> <IO> (Stream.Reader, Child)` | extern |
| `stderr` | `Child -> <IO> (Stream.Reader, Child)` | extern |
| `pipe_stderr` | `Cmd -> Cmd` | eml |
| `wait` | `Child -> <IO, Async> Int` | eml |
| `wait_all` | `Child -> <IO, Async> List Int` | eml |

- 起動の形と、`Cmd` が向きを設定していないときの既定は `Proc` と同じである (上の「起動」)。`run` は標準入出力を受け継ぎ、`output` と `capture` は標準入力を空にして標準出力を読み、`spawn` は標準入力と標準出力を pipe にする。設定した向きはどの形でもそのまま使う。`output`、`output_bytes`、`capture`、`capture_bytes` は、標準出力の向きを設定した子からは `Proc` と同じく何も読まず、空の文字列か空のバイト列を結果にする。`capture` と `capture_bytes` は、終了コードが 0 でなければ `Proc` と同じ IO の失敗 (``command `git` exited with code 128``) にする。`stdin_text`、`stdin_file`、`stdout_file`、`stdout_append`、`merge_stderr`、`discard_stderr`、パイプラインの意味も `Proc` と同じで、`merge_stderr` の最後の段の標準エラーは標準出力の pipe に入る
- `pipe_stderr c` は、`c` の標準エラーを pipe にする。パイプラインでは、ほかの設定と同じく最後の段に当たる。使えるのは `spawn` だけで、`run`、`output`、`output_bytes`、`capture`、`capture_bytes` と `Proc` の起動と、最後でない段の `pipe_stderr` は、子を起動せずに起動の誤り (``cannot run `<program>`: the standard error pipe needs `Exec.spawn` on the last stage``) になる (上の「パイプライン」)。標準出力と標準エラーを別のタスクで読めば、片方の pipe の buffer が詰まって子が止まることはない
- 起動の関数は、起動の前に取り消しの地点を通る。取り消しが立っていれば、子を起動せずに `Cancelled` を投げる。起動そのものは待たない
- pipe は起動した worker の driver が作る ([VM](vm.md) の「時計と driver」)。親の端は driver に縛らずに `Child` が持ち、`stdin c`、`stdout c`、`stderr c` がそれを取り出して `Stream.Writer` と `Stream.Reader` にする。取り出した端は、最初に操作を出した worker の driver が取り込むので、ほかの worker に置いたタスクに渡して読み書きしてよい。標準入力は先頭の段の、標準出力と標準エラーは最後の段の pipe である。2回目の取り出しと、pipe にしなかった向きの取り出しは、種類 `NotAvailable` の IO の失敗 (``cannot take the standard input of `sort`: not available``、``cannot take the output of `sort`: not available``、``cannot take the errors of `sort`: not available``) になる
- 子の pipe の読み書きは `Std.Stream` の関数で行う。読みはファイルと同じく受け付けで容量を広げて貸し、読めるようになるのを待たない。pipe の読み書きはプールの仕事でなく、ソケットと同じく driver の歩で進むので、ファイルの仕事と違って取り消せる。取り消しの手順は `Std.Net` の待つ関数と同じである (上の「取り消し」)。誤りの文言の prefix は、標準出力の読みでは ``cannot read the output of `<program>` ``、標準エラーの読みでは ``cannot read the errors of `<program>` ``、書きでは ``cannot write to `<program>` `` で、`read_all` の UTF-8 の失敗は ``cannot read the output of `<program>`: the text is not valid UTF-8`` (標準エラーは ``the errors of``) である。読み手が終わった子への書きは、番号 2 の ``cannot write to `<program>`: broken pipe`` (種類 `ConnectionReset`) になる。eml のプロセスは SIGPIPE を無視したままにする ([VM](vm.md) の「時計と driver」)
- 取り出した読み手と書き手は `Child` と別に生きる。`Child` を落として子を kill しても、取り出した読み手は EOF まで読める。EOF は書く端を持つものがすべて閉じたときに届くので、pipe を受け継いだ孫が EOF を遅らせうる。期限を付ける道は `Async.timeout` で、期限が来ると巻き戻しが読み手を落とす
- `wait c` は、取り出していない標準入力の端を閉じてから、すべての段を先頭から待ち、最後の段の終了コードを返す。`wait_all c` も同じ順に待ち、すべての段の終了コードを先頭から並べる。どちらも `Child` を消費する。段ごとに、driver に OS の終わりの知らせを待たせ (`Op::Wait`)、知らせの後で Host の `Process::wait` がその段を回収する。driver は回収しない。終了コードの規則 (128 + シグナルの番号) と、`pipefail` がないことは `Proc` と同じである
- 最後の段の標準出力か標準エラーを pipe にして読まずに待つと、pipe の buffer より多く書く子は書き込みで止まり、待ちも終わらない。`wait c` が閉じるのは標準入力の端だけで、出力の端は `Child` を落とすまで開いたままである。`Proc` と違い、読むタスクと待つタスクを分ければ、読みながら待てる
- 待つタスクが取り消されると、`Std.Net` と同じく driver に取り消しを頼み、完了を待ってから `Cancelled` を投げる。driver は子の待ちの取り消しをその場で終えるので、この待ちは長くならない。巻き戻しが落とす `Child` は破棄処理で片づく。取り出していない端を閉じ、まだ回収していない段の直接の子をすべて kill してから、先頭から待つ。誤りは捨てる。取り消したスコープでの `drop`、`exit`、捕まえない実行時エラー、`wait` を囲む `Async.timeout` の期限も同じである。直接の子は kill (unix の SIGKILL、Windows の `TerminateProcess`) で終わるので、待ちはすぐ終わる。孫は kill しない。テストの Host は、受け継いだ標準出力と標準エラーを読むスレッドも待つので、それを受け継いだ孫が待ちを遅らせうる。Host は `Std.Exec` の pipe の端を読まない
- Ctrl-C と `Terminate` の止める語は、どの OS でも次の安全点で読まれる。タスクのスレッドは pipe の読み書きでも子の待ちでも OS の中で止まらないためである ([VM](vm.md) の「上限と割り込み」)。`Proc` の「止まったときは Ctrl-C で終われる」の制限は、ブロックする行だけのものである
- OS の子を持たない Host (組み込み先など、`Process::raw` が `None`) の段は、driver に待たせず、`wait` がその場で Host のブロックする `wait` を呼ぶ。その間は worker のほかのタスクも止まる
- 仮想の時計の run では、テストの driver が子を扱わないので、起動の関数はどの worker でも、driver に触れる前に種類 `NotAvailable` の IO の失敗 ``cannot run `<program>`: the test driver has no processes`` になる。名前は最後の段のプログラムである。子は起動しない ([VM](vm.md) の「時計と driver」)
- pidfd のない Linux (5.3 より前のカーネル) では、driver が子の終わりを待てないので、`wait` が種類 `NotAvailable` で失敗する
- 起動の誤りは `Proc` と同じ文言と、4つの理由の語を使う (上の「失敗」)。driver の読み、書き、待ちの番号 6 の誤りは、`Std.Stream` と同じく OS の文言を保つ。待ちの prefix は ``cannot wait for `<program>` `` で、名前は待てなかった段のプログラムである。知らせの後の回収が失敗したときは、`Proc` と同じく理由の語を使う (``cannot wait for `sort`: I/O error``)
- 待つ関数の包みは `Std.Stream` と同じ形で、結果の番号も同じ 0 から 6 である。`wait_all` は段ごとに `submit_wait`、`Async.io_wait`、`op_child`、`op_outcome` の順に呼び、番号 0 なら `finish` で記録を放してから `reap` で回収する。`wait` は `wait_all` の最後の要素である。`+` は `Panic` を持つので、段の番号は `try_panic` で包んで数える
- extern の行は次の11行で、`stdin`、`stdout`、`stderr` のほかは `pub` でない。VM は `Std.Stream` の行と同じく `Async` の命令で実行する ([VM](vm.md) の「`Async` のタスク」)

| 行 | 型 | 借りる引数 |
|---|---|---|
| `spawn_raw` | `String -> Int -> String -> Int -> String -> <IO> Child` | なし |
| `stdin` | `Child -> <IO> (Stream.Writer, Child)` | 子 |
| `stdout` | `Child -> <IO> (Stream.Reader, Child)` | 子 |
| `stderr` | `Child -> <IO> (Stream.Reader, Child)` | 子 |
| `stage_count` | `Child -> (Int, Child)` | 子 |
| `submit_wait` | `Int -> Child -> <IO> Async.OpId` | なし |
| `op_child` | `Async.OpId -> <IO> (Child, Async.OpId)` | token |
| `op_outcome` | `Async.OpId -> <IO> (Int, Async.OpId)` | token |
| `op_prefix` | `Async.OpId -> <IO> (String, Async.OpId)` | token |
| `finish` | `Async.OpId -> <IO> Unit` | なし |
| `reap` | `Int -> Child -> <IO> (Int, Child)` | 子 |

- `spawn_raw` の引数は `Proc.spawn_raw` と同じ5つで、`Proc.launch_args` が作る (上の「起動」)。pipe の向き (標準入力と標準出力の 2、標準エラーの 3) の端は、Host の pipe でなく driver の `pipe` が作ったものを渡す
- `submit_wait k c` は、取り出していない標準入力の端を閉じ、段 `k` の終わりを待つ記録を作る。記録の持ち主と host は、待つタスクの worker である。回収した段と、OS の子を持たない Host の段の記録は、driver に何も出さずに受け付けで終わる。driver も、もう終わった子の待ちを受け付けで終えうる
- `reap k c` は段 `k` を回収して、その終了コードを返す。知らせの後なので止まらない。回収した段は、覚えた終了コードを返す。`op_child` は終わった記録から子を移し出し、`stage_count` は段の数を返す。ほかの行は `Std.Stream` の同じ名前の行と同じである

## `Std.Tls`

`Std.Tls`はTLSのクライアントのモジュールである。`Conn`はrustlsの接続と、復号してまだ返していない平文の列を持つ`Lin`の`Foreign`で、I/Oを持たない。`Stream`は`Conn`と`Net.TcpStream`の組のemlのデータで、`std/Tls.em`のコードが両者の間でバイトを汲む。ソケットの読み書き、待ち、取り消し、期限はすべて`Std.Net`のものである。`Conn`を作ることは、根を作るときにOSの根のファイルを読むことがあるので、プールの仕事として走る ([VM](vm.md)の「プールの仕事」)。handshakeの間の押し込みも、証明書を確かめることがあり、OSの検証器がブロックするかネットワークに出ることがあるので、プールの仕事として走る。rustlsが証明書を確かめるのはhandshakeの間だけなので、handshakeの後の押し込みはworkerの上で走る。仕事が走る間も、そのworkerのほかのタスクは走り続ける。仕事はdriverの24の`Op::Job`の枠を`blocking`の呼び出しと分け合うので (上の「`Std.Stream`」)、接続が枠を待つことがある。仮想の時計では、仕事はworker 0の上でその場で走る。暗号はaws-lc-rsである。サーバーの証明書はOSの検証器で確かめる。macOSではSecurity.framework、WindowsではCryptoAPI、LinuxではOSの根のファイルの上のrustlsで、Linuxの根のファイルは`SSL_CERT_FILE`と`SSL_CERT_DIR`で替えられる。PEMで渡したCAは、OSの根に加えて信頼する。OSによっては、失効の確認や中間の証明書の取得のために、検証がネットワークに出ることがある。テストのdriverのrunはOSの検証器を使わず、自分の根も持たない ([VM](vm.md)の「時計と driver」)。TLS 1.2と1.3を受ける。serverの側のAPIはない。

```haskell
import Async
import Fs
import Tls

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let s = Tls.connect_with_ca (Fs.read_text "ca.pem") "localhost" 8443
  let s = Tls.write_line "hello" s
  let (line, s) = Tls.read_line s
  match line with
    | Some text -> println text
    | None -> println "eof"
  Tls.close s)
```

| 名前 | 型 | 実装 |
|---|---|---|
| `Conn` | `Lin` の `Foreign` の型 | extern |
| `Stream` | `Stream { conn : Conn, sock : Net.TcpStream }` | eml |
| `connect` | `String -> Int -> <IO, Async> Stream` | eml |
| `connect_with_ca` | `String -> String -> Int -> <IO, Async> Stream` | eml |
| `wrap` | `String -> Net.TcpStream -> <IO, Async> Stream` | eml |
| `wrap_with_ca` | `String -> String -> Net.TcpStream -> <IO, Async> Stream` | eml |
| `read_chunk` | `Bytes -> Stream -> <IO, Async> (Bytes, Stream)` | eml |
| `read_line` | `Stream -> <IO, Panic, Async> (Option String, Stream)` | eml |
| `write` | `String -> Stream -> <IO, Async> Stream` | eml |
| `write_bytes` | `Bytes -> Stream -> <IO, Async> Stream` | eml |
| `write_line` | `String -> Stream -> <IO, Async> Stream` | eml |
| `shutdown_write` | `Stream -> <IO, Async> Stream` | eml |
| `close` | `Stream -> <IO, Async> Unit` | eml |

- `connect host port`は`Net.connect host port`で接続し、`host`をSNIと証明書の名前の照合に使ってhandshakeまで行う。根はOSの根だけである。`connect_with_ca ca host port`は、PEMの`ca`の証明書をOSの根に加えて信頼する。`ca`が空の文字列なら`connect`と同じである。`wrap host s`は、既にある`TcpStream`の上で同じhandshakeを行い、根はOSの根だけである。`wrap_with_ca ca host s`は`connect_with_ca`と同じく`ca`をOSの根に加えて信頼し、`ca`が空の文字列なら`wrap`と同じである。テストのdriverのrunには根がないので、`connect`、`wrap`と、空の`ca`の`_with_ca`は、根の証明書がない失敗を投げる。CAを渡さずにOSの検証器を作れないときも、どのOSでも同じ失敗を投げる。よくあるのは、LinuxでOSの根のファイルがないときである。静的な配布物や最小のコンテナのように根のファイルのない所で動くプログラムは、`connect_with_ca`でCAを渡す。サーバーの名前とPEMの誤りと根がないことは、接続を開く前に投げる
- どの関数も期限を持たない。期限は`Async.timeout ms (fn () -> Tls.connect host port)`のように付ける。期限が来ると巻き戻しが`Stream`を落とすので、期限の後に同じ`Stream`は使えない。`Net.read_line_within`のような、期限の後も読みを続けられる`_within`の関数はない。走っている仕事は取り消せない。仕事の間に期限か取り消しが来ると、仕事が終わるのを待ってから、close_notifyを送らずに`Conn`とソケットを落とし、`Cancelled`を投げる。`Async.timeout`が戻るのはそのときである。仮想の時計では、期限が仕事の途中に来ることはない。仕事の間にrunが割り込まれると、runは仕事の予算まで仕事を待ち、それより長い仕事は見捨てて、runはほかと同じく終わる ([VM](vm.md)の「時計と driver」)
- `read_chunk b s` は、平文を `b` の後ろに 1 バイト以上 65 536 バイト以下足して返す。`Conn` に平文がなければ、ソケットを読んで復号するのを平文が届くまで繰り返す。相手が close_notify で閉じた後は、何も足さずに `b` の中身のまま返す。これが EOF である。close_notify のない EOF は、届いた平文をすべて返した後で、種類 `ConnectionReset` の IO の失敗になる。TLS の誤りを見つけた押し込みで復号した平文は返さない
- `read_line s` は1行を読み、終わりの改行を除いて返す。改行、上限、UTF-8 の扱いと文言は `Net.read_line` と同じで、行は同じ関数で切り出す (上の「`Std.Net`」)。EOF は close_notify の後だけで、改行のない最後の行はそのまま返し、残りがなければ `None` を返す。close_notify のない EOF は `read_chunk` と同じ失敗である。読みすぎた平文は `Conn` に残り、次の `read_chunk` か `read_line` が先に返す。そのため2つを混ぜても、平文は失われず、順も入れ替わらない
- `read_chunk`と`read_line`は、入口でいつも取り消しを確かめる取り消しの地点である。取り消されると`Stream`を落とし、`Cancelled`を投げる。平文が`Conn`に残っていて`Net`の読みを呼ばないときも同じである。`connect`、`connect_with_ca`、`wrap`、`wrap_with_ca`は、接続を設定するところで取り消しの地点である
- `write_bytes b s` は `b` を暗号にし、同じ呼び出しの中で `Net.write_bytes` で書き出す。`write str s` は `str` の UTF-8 のバイトを、`write_line str s` は `str` と改行を書く。書き出すときは `Net.write_bytes` と同じく取り消しの地点である。`shutdown_write` の後の書きは、`Net` の書きと同じ失敗になる。テストの driver では種類 `ConnectionReset`、文言 `cannot write to the stream: connection reset` である
- `shutdown_write s` は close_notify を書き出してから、`Net.shutdown_write` で送る側を閉じる。`close s` は `shutdown_write` してから `Stream` を落とす。相手がもう閉じていれば close_notify は書けないので、`close` は書きの失敗を捨てる。`Cancelled` だけはそのまま投げ、ほかの失敗では失敗しない
- `Stream` を落とすと、巻き戻しの間も含めて、close_notify を送らずにソケットを閉じる。取り消しで落ちた `Stream` も同じである
- `Conn` の平文、rustls の buffer、`pull_tls` が埋める `Vec` は Rust の確保で、`max_bytes` に数えない ([VM](vm.md) の「上限と割り込み」)。平文は、`Conn` に平文がないときの `read_chunk`、行のそろっていないときの `read_line`、handshake のときにだけ、`Net.read_chunk` の1回分 (暗号文で 65 536 バイトまで) を押し込んで増える。上限を超えた開いた行は投げるので、返していない平文は `Conn` ごとに 1 048 576 + 2 × 65 536 バイトを超えない。書きの間に rustls が持つ暗号文は、書く平文とレコードの余分の分で、呼び出しが終われば残らない。受け取りの途中のレコードは1つで、16 KiB とレコードの余分までである
- feature `tls` を外した処理系では、`connect`、`connect_with_ca`、`wrap`、`wrap_with_ca` は種類 `NotAvailable` の IO の失敗 `cannot set up TLS for {host}: not available` を投げる。テストの driver の `tls.localhost` と `tls-cut.localhost` も `NotFound` になる ([VM](vm.md) の「時計と driver」)

失敗の種類と文言は次のとおりである。

| 場合 | 種類 | 文言 |
|---|---|---|
| サーバーの名前を使えない | `Other` | `cannot set up TLS for {host}: the server name is not valid` |
| 足す CA の PEM の誤り | `Other` | `cannot set up TLS for {host}: the CA certificates are not valid PEM` |
| 根の証明書がない (CAを渡さずにOSの検証器を作れなかった。よくあるのはLinuxでOSの根のファイルがないときである。または、テストのdriverのrunでCAを渡さなかった) | `Other` | `cannot set up TLS for {host}: no root certificates are available` |
| TLS のない処理系 | `NotAvailable` | `cannot set up TLS for {host}: not available` |
| TLSの仕事を走らせられない (パニックなど) | 失敗の種類 | `cannot run TLS: {理由}` |
| 証明書の拒否 | `Other` | `cannot complete the TLS handshake with {host}: the server certificate was rejected` |
| handshake のほかの TLS の誤り | `Other` | `cannot complete the TLS handshake with {host}: {rustls の文言}` |
| handshake の途中で相手が閉じた (EOF か close_notify) | `ConnectionReset` | `cannot complete the TLS handshake with {host}: the peer closed the connection` |
| 読みの TLS の誤り | `Other` | `cannot read the stream: {rustls の文言}` |
| close_notify のない EOF | `ConnectionReset` | `cannot read the stream: the peer closed the connection without close_notify` |
| 長すぎる行、UTF-8 でない行 | `Other`、`NotUtf8` | `Net.read_line` と同じ |
| `shutdown_write` の後の書き | `Net` の書きと同じ | `Net` の書きと同じ |
| 取り消し | `Cancelled` | `task was cancelled` |
| ソケットの誤り | `Std.Net` と同じ | `Std.Net` の文言 |

証明書の拒否は、理由によらず同じ文言で、rustlsとOSの詳細を付けない。検証の中でOSの側が失敗したとき (Windowsの証明書のエンジンの誤りなど) も、証明書の拒否として報告する。

包みの形は次のとおりである。`status` は `Conn` の状態を次の印の和で返す。

| 印 | 名前 | 立つとき |
|---|---|---|
| 1 | handshaking | handshake の途中である |
| 2 | has_outgoing | `pull_tls` で書き出す暗号文がある |
| 4 | peer_closed | 相手の close_notify を受けた |
| 8 | truncated | close_notify のないまま EOF を押し込んだ。handshake の途中も含む |
| 16 | failed | TLS の誤りを記録した |

- handshake は、書き出すものを書き出し、`status` が handshaking でなくなるまで、`Net.read_chunk` の1回分を押し込んでは書き出すことを繰り返す。peer_closed か truncated が立てば、handshake の途中で相手が閉じた失敗にする
- 押し込みは、呼び手が直前に読んだ`status`のhandshakingで行を選ぶ。handshakingが立っていれば`submit_push`と`take_conn`で、立っていなければ`push_tls`で押し込む
- 押し込みの後に failed が立てば、`error_message` で理由を読み、alert を `try_io` の中で書き出してから、種類 `Other` の IO の失敗を投げる。alert を書けなくても TLS の誤りのほうを報告する。書き出しが `Cancelled` で失敗したときだけは `Cancelled` を投げる。failed が立たなければ、書き出すもの (TLS 1.3 の KeyUpdate への応答など) を書き出す
- `line_ready` は、`Conn` の平文の先頭の行の状態を 0 (行も EOF もまだない)、1 (行がある)、2 (中身が上限を超えた)、3 (UTF-8 でない) で返す。EOF とみなすのは peer_closed のときだけである。`Conn` は前の `line_ready` が探し終えた位置を持ち、次は新しく届いた平文だけを探す。平文を返すと位置を先頭に戻す。1 のときだけ `take_line` を呼び、行を終わりの改行ごと文字列にして返す。残りのない EOF では空の文字列で、包みがそれを `None` にする
- externの行は次の13行で、どれも`pub`でない。`submit_client`、`submit_push`、`take_conn`はプールの仕事の行 ([VM](vm.md)の「プールの仕事」) で、VMは`Async`の命令として実行する。ほかの行は待たず、普通のexternの行として実行する。投げるのは`take_conn`だけで、ほかの行のTLSの誤りは`status`のfailedと`error_message`に残る

| 行 | 型 | 借りる引数 |
|---|---|---|
| `submit_client` | `String -> String -> <IO> Async.OpId` | 両方 |
| `submit_push` | `Bytes -> Conn -> <IO> Async.OpId` | バイト列 |
| `take_conn` | `Async.OpId -> Async.Wake -> <IO> Conn` | なし |
| `push_tls` | `Bytes -> Conn -> <IO> Conn` | バイト列 |
| `push_eof` | `Conn -> <IO> Conn` | なし |
| `pull_tls` | `Conn -> <IO> (Bytes, Conn)` | `Conn` |
| `write_plain` | `Bytes -> Conn -> <IO> Conn` | バイト列 |
| `read_plain` | `Bytes -> Conn -> <IO> (Bytes, Conn)` | `Conn` |
| `status` | `Conn -> <IO> (Int, Conn)` | `Conn` |
| `error_message` | `Conn -> <IO> (String, Conn)` | `Conn` |
| `send_close_notify` | `Conn -> <IO> Conn` | なし |
| `line_ready` | `Conn -> <IO> (Int, Conn)` | `Conn` |
| `take_line` | `Conn -> <IO> (String, Conn)` | `Conn` |

- `submit_client host ca`は`host`と`ca`を写し、仕事の中で`Conn`を作る。信頼する根はrunのdriverの種類で決まる ([VM](vm.md)の「時計と driver」)。`submit_push b conn`は`b`を写し、`conn`の中身を仕事へ移して、そこで暗号文を押し込む。`take_conn`はどちらの仕事の`Conn`も新しい物体として返すか、接続の設定の誤りを投げる。`push_tls`はhandshakeの後の暗号文を押し込み、handshakeの間に呼ぶと内部の誤り`Std.Tls.push_tls runs during the handshake`になる。`push_eof`はEOFを記録する。`pull_tls`は書き出す暗号文をすべて新しい`Bytes`にする。failedの後もalertはここから出る。`write_plain`は平文を暗号にして積む。failedの後は捨てる。`read_plain`は平文の先頭の65 536バイトまでを`Bytes`の後ろに足して返す。`send_close_notify`は1度だけclose_notifyを積む。致命的なalertの後は何もしない。`take_line`を行のそろっていないときに呼ぶと、内部の誤りになる

## `Std.Http`

`Std.Http` は HTTP/1.1 のクライアントのモジュールである。`std/Http.em` が `Std.Net` と `Std.Tls` の上に eml で書いてあり、extern を持たない。要求ごとに接続を開き、応答を読み終えると閉じる。`http` の URL は `Net.connect` で、`https` の URL は `Tls.connect_with_ca` でつなぐ。待ち、取り消し、期限は `Net` と `Tls` のもので、`Std.Http` は何も足さない。

```haskell
import Async
import Bytes
import Fs
import Http
import List
import String

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let options = { Http.defaults | ca = Fs.read_text "ca.pem", headers = [("Accept", "text/plain")] }
  let r = Http.request "GET" "https://localhost:8443/hello" (Bytes.empty ()) options
  println (show r.status)
  match List.lookup "content-type" r.headers with
    | Some t -> println t
    | None -> println "no type"
  println (String.decode r.body))
```

| 名前 | 型 | 実装 |
|---|---|---|
| `Options` | `Options { headers : List (String, String), ca : String, max_redirects : Int }` | eml |
| `Response` | `Response { status : Int, headers : List (String, String), body : Bytes }` | eml |
| `defaults` | `Options` | eml |
| `get` | `String -> <IO, Panic, Async> Response` | eml |
| `request` | `String -> String -> Bytes -> Options -> <IO, Panic, Async> Response` | eml |

- `Options`は要求の設定である。`headers`は要求に足すヘッダー、`ca`は根に足すCAの証明書のPEM (`Tls.connect_with_ca`の`ca`で、空ならOSの根だけ。テストのdriverのrunには根がない)、`max_redirects`は追うリダイレクトの回数の上限である。`defaults`は`Options { headers = [], ca = "", max_redirects = 5 }`である。`Options`と`Response`は`Eq`と`Show`を導出する
- `get url` は `request "GET" url (Bytes.empty ()) defaults` である
- `request method url body options` は、接続の前に `method`、`options.headers` の各項目、`url` の順に確かめる。`method` は RFC 9110 の token (英数字と ``!#$%&'*+-.^_`|~`` の1文字以上) で、綴りのまま送る。メソッドは書いたとおりに比べるので、下の `HEAD`、`POST`、`PUT`、`PATCH` の扱いは大文字の綴りにだけ当たる。ヘッダーの名前は token で、値は HTAB のほかの制御文字 (CR、LF、NUL を含む) と DEL を含まない。0x80 からのバイトは UTF-8 の文字として通す。名前が大文字小文字を区別せずに `Host`、`Connection`、`Content-Length`、`Transfer-Encoding` のどれかなら失敗にする。この4つは本文の区切りと接続の扱いを決めるので、`Std.Http` が書く
- URL は `scheme://authority` に path と query が続く形である。最初の `#` から後ろの fragment は除き、送らない。残りのバイトはどれも 0x21 から 0x7E でなければならない。空白、制御文字、ASCII でない文字は失敗で、百分率の符号化は呼ぶ側が行う。scheme は最初の `://` の前で、大文字小文字を区別せずに `http` か `https` である。authority は `://` の後から最初の `/` か `?` の手前までで、`host` か `host:port` である。`@` を含む authority (userinfo) と `[` で始まる authority (IPv6 のアドレス) は扱わない。host は英数字、`-`、`.`、`_` の1文字以上で、小文字にして使う。port は1桁から5桁の数字で、1 から 65535 である。省くと、`http` は 80、`https` は 443 である。空の path は `/` にし、`?` で始まるときは前に `/` を足す。オリジンは scheme、host、port の組である
- 要求の頭は、要求の行 `{method} {path と query} HTTP/1.1` に続けて、`Host`、`User-Agent: eml`、`Connection: close`、`Content-Length`、`options.headers` の順に並べ、空の行で終える。行の終わりは CRLF である。`Host` の値は host で、port が scheme の既定でなければ `host:port` にする。`options.headers` に `User-Agent` があれば、既定の `User-Agent: eml` を省く。`Content-Length` は、本文が空でないときと、メソッドが `POST`、`PUT`、`PATCH` のときに付ける (RFC 9110 の 8.6)。`options.headers` は渡した順と綴りのまま書く。頭と本文は別々に書く
- https の接続は `Tls.connect_with_ca options.ca host port` で開き、host を SNI と証明書の名前に使う。http の接続は `Net.connect host port` で開く
- 本文を書き終える前に相手が応答を送って閉じると、本文の書きは `Net` か `Tls` の書きの失敗になり、応答は読まない。書きの包みはストリームを落としてから投げるので、読むストリームが残らないためである
- 応答は `Net` か `Tls` の `read_chunk` で読み、行は `Std.Http` が切り出す。`read_chunk` は読みすぎたバイトを戻せないので、chunk の区切りを越えて読んだバイトを `Std.Http` の buffer に残すためである。行は LF で終わり、直前の CR を1つ除く。行の中身 (LF と直前の CR を除いたバイト) は 65 536 バイトまでで、超えた行は、LF が届いたときか、LF のないまま 65 537 バイトを超えて届いたときに失敗になる。そのため結果は、バイトが読みにどう分かれて届くかによらない。UTF-8 でない行も失敗である。LF の前の EOF では、途中の行を捨てて EOF とみなす。状態行、ヘッダー、chunk の大きさの行、chunk の後の行、trailer の行が、どれもこの規則に従う。ヘッダーと trailer の行の数には上限を置かないので、短い行を送り続ける相手からは戻らない。期限は `Async.timeout` で付ける
- 状態行は、`HTTP/1.` と数字1つ、空白、100 から 599 の3桁の番号で、その後に空白と理由の句が続いてもよい。理由の句は捨てる。100 から 199 の応答は中間の応答で、ヘッダーまで読んで捨て、次の応答を読む
- ヘッダーの行は `名前:値` で、名前は token である。名前と `:` の間に空白は置けない。値は HTAB のほかの制御文字を含まず、前後の空白と HTAB を除く。空白か HTAB で始まる行 (obs-fold) は、直前のヘッダーの値に空白1つでつなぐ (RFC 9112 の 5.2)。最初のヘッダーの行が obs-fold なら失敗である
- `Response` の `status` は最後の応答の番号で、`headers` はそのヘッダーを届いた順に並べたものである。名前は ASCII の小文字にし、同じ名前のヘッダーはまとめずに残す。中間の応答のヘッダーと trailer は入れない。名前は `List.lookup "content-type" r.headers` のように小文字で引く
- 本文の長さは次の順に決める (RFC 9112 の 6.3)。メソッドが `HEAD` か、番号が 204 か 304 なら、本文は空で、何も読まない。`Transfer-Encoding` があれば、すべての値を `, ` でつないで小文字にした列が `chunked` のときだけ chunked で読み、`Content-Length` は見ない。ほかの coding は失敗にする。`Content-Length` があれば、その長さを読む。すべての `Content-Length` の値を `,` で分けた部分は、前後の空白を除いてどれも1桁以上の数字で、`Int` に収まる同じ値でなければならない (RFC 9110 の 8.6)。どちらのヘッダーもなければ、EOF までを本文にする
- `Content-Length` の本文は、その長さのバイトである。後ろに届いたバイトは捨てる。長さに届く前の EOF は失敗である
- chunked の本文では、chunk の大きさの行は1つ以上の16進の数字 (大文字と小文字) で、後に空白と HTAB、`;` で始まる拡張が続いてもよい。拡張は捨てる。値が `Int` に収まらない行は失敗である。chunk の中身の後の行は空でなければならない。大きさ 0 の chunk の後は、空の行まで trailer の行を読んで捨てる。終わりの空の行の前の EOF は失敗である
- EOF までの本文は、`read_chunk` が何も足さなくなるまで読む。https で close_notify のないまま閉じた接続は、`Tls.read_chunk` の `ConnectionReset` の失敗をそのまま伝える。`Content-Length` と chunked の本文は終わりまで読んだところで閉じるので、その後の close_notify のない EOF は失敗にならない
- 本文は `read_chunk` で1つの `Bytes` の後ろに足していき、足した部分を写し直さない。`Content-Length` の本文は、長さと 64 MiB の小さいほうに 8 バイトを足した容量を前もって取るので、64 MiB までの本文は広げ直さない。8 バイトは、`Net.read_chunk` が空きの 8 バイト未満の buffer を広げる分である (上の「`Std.Net`」)。容量が埋まると、`Std.Http` は読んだ長さの 16 倍と `Content-Length` の小さいほうに 8 バイトを足した容量へ自分で広げて写し、`read_chunk` には広げさせない
- `max_redirects` が 1 以上なら、番号が 301、302、303、307、308 で `Location` のある応答のリダイレクトを追う。`Location` のない応答はほかの応答と同じく返す。`max_redirects` が 0 以下なら追わずに、リダイレクトの応答もほかの応答と同じく返す。追うときは、リダイレクトの応答の本文を読まずに接続を閉じる。追った回数が `max_redirects` に達していれば失敗にする。`Location` は前後の空白を除いた最初の値である。`/` で始まって `//` で始まらなければ絶対パスとして今のオリジンに付け、大文字小文字を区別せずに `http://` か `https://` で始まれば絶対 URL として上の規則で読む。ほかの形は失敗である。https から http へのリダイレクトは失敗にし、http から https へは追う
- 301、302、303 では、メソッドが `HEAD` ならそのまま、ほかは `GET` にし、本文を空にして、呼ぶ側の `Content-Type` を落とす。307 と 308 は同じメソッド、本文、ヘッダーで送る。行き先のオリジンが今の URL のオリジンと違えば、呼ぶ側の `Authorization`、`Cookie`、`Proxy-Authorization` を落とす。落としたヘッダーは、後で元のオリジンに戻っても戻さない
- 接続は、応答を返す前と、リダイレクトを追う前に閉じる。https は `Tls.close` で、http は `Net.close` で閉じる。`Std.Http` が失敗を投げるときは、close_notify を送らずに接続を落としてから投げる。`Net` と `Tls` の関数が失敗したときは、その関数がストリームを落としている
- 取り消しの地点は `Net` と `Tls` の待つ関数で、取り消されると巻き戻しが接続を落とす。期限は `Async.timeout ms (fn () -> Http.get url)` で付ける
- `Std.Http` が投げるのは IO の失敗だけで、`Panic` は起こさない。row の `Panic` は、使う `String` と `Bytes` の関数から来る
- 本文の大きさの上限、ストリーミングの本文、gzip などの coding、HTTP/2、keep-alive、プロキシ、cookie、IPv6 のアドレスの URL はない

失敗の種類と文言は次のとおりである。

| 場合 | 種類 | 文言 |
|---|---|---|
| token でないメソッド | `Other` | `invalid request: the method {method} is not valid` |
| token でないヘッダーの名前か、制御文字を含む値 | `Other` | `invalid request: the header {name} is not valid` |
| `Std.Http` が書くヘッダーを渡した | `Other` | `invalid request: the header {name} is set by Std.Http` |
| URL の誤り | `Other` | `invalid URL {url}: {理由}` |
| 長すぎる行 | `Other` | `cannot read the response: a line is longer than 65536 bytes` |
| UTF-8 でない行 | `NotUtf8` | `cannot read the response: a line is not valid UTF-8` |
| 状態行の誤り | `Other` | `cannot read the response: the status line is not valid` |
| ヘッダーの行の誤り | `Other` | `cannot read the response: a header line is not valid` |
| `Content-Length` の誤り | `Other` | `cannot read the response: the Content-Length is not valid` |
| chunked のほかの transfer coding | `Other` | `cannot read the response: the transfer coding {coding} is not supported` |
| chunked の本文の誤り | `Other` | `cannot read the response: the chunked body is not valid` |
| ヘッダーの終わりの前の EOF | `ConnectionReset` | `cannot read the response: the connection closed before the end of the headers` |
| `Content-Length` か chunked の本文の終わりの前の EOF | `ConnectionReset` | `cannot read the response: the connection closed before the end of the body` |
| リダイレクトの回数が上限を超えた | `Other` | `cannot follow the redirect to {location}: too many redirects (the limit is {n})` |
| `Location` の誤り | `Other` | `cannot follow the redirect to {location}: {理由}` |
| https から http へのリダイレクト | `Other` | `cannot follow the redirect to {location}: it goes from https to http` |
| 接続、TLS、読み書き、取り消しの失敗 | `Std.Net` と `Std.Tls` と同じ | `Std.Net` と `Std.Tls` の文言 |

URL の誤りの理由は、`the URL contains a space, a control or a non-ASCII character`、`the scheme is not http or https`、`userinfo is not supported`、`IPv6 addresses are not supported`、`the host is not valid`、`the port is not valid` のどれかである。`Location` の誤りの理由は、これに `the location is not an absolute URL or an absolute path` を足したものである。`{url}` は渡した URL、`{coding}` は `Transfer-Encoding` の値を `, ` でつないだもの、`{location}` は前後の空白を除いた `Location` の値、`{n}` は `max_redirects` である。

## `Std.Gui`

`Std.Gui`はネイティブのGUIのtoolkitである。プログラムはウィンドウを直接の形で1回だけ組み、ハンドルを通してウィジェットを変え、`wait`でメッセージを取り出す。toolkitの層とbackendは`eml_gui`にある (docs/implementation/architecture.mdの「`eml_gui`の内部」)。利用者が入れるものと、Qtなどのライセンスは、docs/implementation/distribution.mdにある。

```haskell
counter : Unit -> <Gui Msg, IO, Panic, Async> Unit
counter () =
  use _ <- Gui.window "Counter" Quit
  use Gui.grid [Fr 1, Fr 1] []
  let shown = Gui.cell { Gui.default_cell | col_span = 2 } (fn () -> Gui.label "0")
  let _ = Gui.button "-" Dec
  let _ = Gui.button "+" Inc
  let loop n =
    match Gui.wait () with
      | Inc ->
        Gui.text shown (show (n + 1))
        loop (n + 1)
      | Dec ->
        Gui.text shown (show (n - 1))
        loop (n - 1)
      | Quit -> println "final \{show n}"
  loop 0

main : Unit -> <IO, Panic> Unit
main () =
  use Async.run
  Gui.run counter
```

ウィジェットのメッセージは、作るときにそのウィジェットに結び付ける。ウィジェットのハンドルは`Unr`のidである。handler `run`はtoolkitのハンドル`Ui` (`Lin`) をhandlerの状態として持ち、ほかのコードからは見えない。

公開の名前は次のとおりである。データの型`Track` (`Auto`、`Fr Int`、`Px Int`)、`Align` (`Start`、`Center`、`End`、`Stretch`)、`Cell` (`col_span`、`row_span`、`halign`、`valign`) と`default_cell` (幅は1、`Stretch`)。抽象の型`Window`、`Grid`、`Label`、`Button`、`TextField`、`CheckBox`、`Choice`、`ProgressBar`、`Sink msg`。クラス`Widget w` (`wid : w -> Int`)。17の操作を持つエフェクト`Gui msg`。包みの関数`window`、`grid`、`cell`、`text`、`enable`、`value`、`select`、`checked`、`post`、`run`。エフェクトは抽象ではないので、テストは自分のhandlerで扱える。

- `run app` (`(Unit -> <Gui msg, IO, Panic, Async> a) -> <IO, Panic, Async> a`) は、`Async.run_on_main`で`app`を固定したタスクとして動かす (上の「メインのworker」)。`Gui`の操作は自分では`IO`も`Async`も持たない
- 開くのは2段である。タスクは`want_gui_raw`でゲートのセルを作り、`Async.gui_wait`でそこで待つ。メインのworkerのGUIの`Park`がbackendを作ってネイティブのループに入り、ループの中からゲートに知らせる。タスクはそれから`open_ui_raw`を呼ぶので、toolkitの呼び出しはどれもネイティブのループの中で動く ([VM](vm.md)の「GUI の Park」)
- ゲートで待つ間に取り消された`run`は、`abandon_raw`でゲートを放し、`Cancelled`で失敗する。次の`run`は開ける
- `wait ()`は次の順に見る。取り消しの地点、flush (変わったウィンドウの配置と新しいウィンドウの表示)、次のネイティブのイベント、次に送られたメッセージ、そしてウィンドウが1つも開いておらず`sink`も取っていなければ失敗`Other "gui: wait with no open window and no sink"`。どれもなければゲートで待ち、初めから見直す。引き出しと待ちの間に届いた知らせはゲートの旗を立てるので、待ちはすぐに戻る
- `sink ()`は、`run`のchannel (容量64) とゲートを持つ`Sink msg`を返す。`post sink m`は、どのworkerのどのタスクからも呼べ、`m`を送ってからゲートに知らせる。`run`が戻った後に送ったメッセージは捨てる。channelは閉じていて、放したゲートへの知らせは何もしないからである
- `app`が戻るか、失敗するか、取り消されると、`run`はchannelを閉じて`Ui`を手放す。`Ui`を手放すことは旗を立てるだけで、ウィンドウは`Park`が次の段ですべて閉じる。そのため、正常な終わり、panic、取り消しのどれでもウィンドウは閉じる
- 求めるか開いているGUIは一度に1つだけである。ほかの`run`が開くのを待っているか`Ui`を開いている間の`run`は、`Other "cannot open the GUI: it is already open"`で失敗する。GUIの`Park`のないrun (`eml repl`、`eml run`の`Park`を置かないrun、feature `gui`のない処理系) では、`run`は`NotAvailable "cannot open the GUI: not available"`で失敗する
- GUIを求めるか開いている間は、プログラムをデッドロックと判定しない。利用者を待っていることがあるからである。開くのに失敗したか、ウィンドウができる前に取り消された`run`はその期間を終えるので、その後のデッドロックはまた報告される

ウィジェットのidは`(window_no << 32) | seq`である。`window_no`は開いた`Ui`ごとに1から数え、`seq`の0はウィンドウ自身である。idは使い回さない。閉じたウィジェットのidは`NotFound`で失敗し、閉じたウィジェットのイベントは捨てる。`close_window`は、idの上位32ビットがそのウィンドウと同じウィジェットのhandlerをすべて忘れる。

行が1つのイベントで渡すのは`Int`と多くても1つの`String`だけで、`msg`の値はemlの側で作る。`poll_raw`は次のイベントのウィジェットのidを返し、イベントがなければ0を返す。`event_int_raw`と`event_text_raw`はそのイベントの中身を読み、どちらを読むかはそのidに登録したhandlerが決める。`create_raw`の種類の番号はLabel 1、Button 2、TextField 3、CheckBox 4、Choice 5、ProgressBar 6、`set_int_raw`の性質の番号はvalue 1、checked 2、enabled 3、揃えの番号はStart 0、Center 1、End 2、Stretch 3である。trackは`a`、`f<n>`、`p<n>`を`,`でつないだ文字列で渡す。正でない`Fr`と負の`Px`は範囲の外である。

| 性質 | ウィジェット | 値 |
|---|---|---|
| text | Window (題名)、Label、Button、TextField、CheckBox | どの文字列も |
| value | ProgressBar、Choice | ProgressBarは0..=100に収める。Choiceの番号は`0 <= i < 項目の数`でなければならない |
| checked | CheckBox | `Bool` |
| enabled | Button、TextField、CheckBox、Choice、ProgressBar | `Bool` |

空のChoiceへの`select`は範囲の外で、`choice`は空の列では最初の`select`をしない。イベントは、Buttonのクリック、編集のたびのTextFieldの文字、CheckBoxの切り替え、Choiceの選択、ウィンドウを閉じる要求である。

| 誤り | IOの失敗 |
|---|---|
| 開いた入れ物がない | `Other "gui: no open container"` |
| 知らないか閉じたウィジェット | `NotFound "gui: unknown or closed widget"` |
| ウィジェットにない性質 | `Other "gui: the widget has no such property"` |
| 範囲の外の値 | `Other "gui: a value is out of range"` |
| ネイティブのtoolkitがない、メインのスレッドでない | backendの文言の`NotAvailable` (`cannot open the GUI: built without the native toolkit`、`cannot open the GUI: AppKit needs the main thread`、`cannot open the GUI: Qt needs the main thread`、`cannot open the GUI: the GUI opens only on the main thread`) |
| Windows 11より古いWindows | `NotAvailable "cannot open the GUI: WinUI needs Windows 11 or later"` |
| Windows App SDK 2のruntimeがない (Windows) | `NotAvailable "cannot open the GUI: the Windows App SDK 2 runtime is not installed (https://learn.microsoft.com/windows/apps/windows-app-sdk/downloads)"` |
| 画面がない (Linuxで`DISPLAY`も`WAYLAND_DISPLAY`もない) | `Other "cannot open the GUI: no display (DISPLAY and WAYLAND_DISPLAY are unset)"` |
| ほかのbackendの誤り | backendの文言の`Other` |

配置はTaffyが置くgridで、置き方は自動だけである。gridはtrack (`Auto | Fr n | Px n`) の列と行を持ち、`rows = []`は暗黙の`Auto`の行である。子は行の順に埋める。子は直前の`cell`の`col_span`、`row_span`、`halign`、`valign` (なければ`default_cell`) を持ち、gridは入れ子にできる。間隔は8、ウィンドウの余白は12の論理pxで、どちらも固定である。ウィンドウは自然な大きさで開き、最小の幅はmin-contentの幅である。座標、大きさ、DPIの値はemlに渡らない。

externの行は次の19行で、どれも`pub`でない。`want_gui_raw`、`open_ui_raw`、`poke_raw`、`abandon_raw`は登録表のゲートのセルに触れるので、VMは`Async`の命令として実行する ([VM](vm.md)の「GUI の Park」)。ほかの行は待たず、普通のexternの行としてメインのスレッドのtoolkitに触れる。`Ui`を消費して返す行は、失敗するときは`Ui`を手放してから投げる。

| 行 | 型 |
|---|---|
| `want_gui_raw` | `Unit -> <IO> Async.Cell` |
| `open_ui_raw` | `Async.Cell -> <IO> Ui` |
| `close_ui` | `Ui -> <IO> Unit` |
| `begin_window_raw` | `String -> Ui -> <IO> (Int, Ui)` |
| `begin_grid_raw` | `String -> String -> Ui -> <IO> (Int, Ui)` |
| `end_raw`、`flush_raw` | `Ui -> <IO> Ui` |
| `next_cell_raw` | `Int -> Int -> Int -> Int -> Ui -> <IO> Ui` |
| `create_raw` | `Int -> String -> Int -> Ui -> <IO> (Int, Ui)` |
| `add_item_raw`、`set_text_raw` | `Int -> String -> Ui -> <IO> Ui` |
| `set_int_raw` | `Int -> Int -> Int -> Ui -> <IO> Ui` |
| `close_window_raw` | `Int -> Ui -> <IO> Ui` |
| `open_windows_raw`、`poll_raw`、`event_int_raw` | `Ui -> <IO> (Int, Ui)` |
| `event_text_raw` | `Ui -> <IO> (String, Ui)` |
| `poke_raw`、`abandon_raw` | `Async.Cell -> <IO> Unit` |

放したゲートで待つのは、ほかのセルと同じく内部の誤りである。stdはそうしない。`Ui`はどれも自分のゲートを持ち、`abandon_raw`は開かなかったゲートを放し、runより長く残るゲートの値は`Sink`の中のものだけで、それを使うのは`poke_raw`だけであり、放したゲートへの知らせは何もしないからである。

## `Std.Signal`

`Std.Signal` はタスクがシグナルを待つモジュールである。

```haskell
import Async
import Signal

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  Signal.wait Signal.Terminate
  println "terminating")
```

| 名前 | 型 | 実装 |
|---|---|---|
| `Signal` | `Interrupt` と `Terminate` を持つ型 | eml (`deriving (Eq, Show)`) |
| `wait` | `Signal -> <IO, Async> Unit` | eml |

- `Interrupt` は Ctrl-C で、unix の SIGINT と Windows の `CTRL_C_EVENT` である。`Terminate` は unix の SIGTERM と、Windows の `CTRL_CLOSE_EVENT` と `CTRL_SHUTDOWN_EVENT` で、どれも `eml run` だけが受ける。`CTRL_LOGOFF_EVENT` は写さない。Windows では、`Terminate` を受けたプログラムは OS の待ち時間 (閉じるは既定で 5 秒) のうちに終わらなければならない。OS がシャットダウンを送るのはサービスなど一部のプロセスだけである ([VM](vm.md) の「上限と割り込み」)
- `wait s` は、シグナル `s` が来るまでタスクを待たせる。取り消しの地点で、取り消されると IO の失敗 `Prelude.Cancelled` を投げる。取り消された待ちは待ち手から外れる。待ちは `Std.Async` の `signal_wait` を通す (上の「`Std.Async`」)
- シグナルが来ると、そのとき同じシグナルを待つタスクをすべて、待ち始めた順に起こす。起こしたタスクは実行待ちの列の後ろに入る。シグナルは貯めない。待つタスクがなければ既定の動きをする。`Interrupt` は run を止めて終了コードを 130 にし、`Terminate` も同じ止め方で 143 にする ([VM](vm.md) の「上限と割り込み」)。起こされてから次の `wait` までに来たシグナルも既定の動きをする
- 届ける前に同じシグナルが続けて来ると、1つにまとまる。`eml run` では、前の同じシグナルをまだ届けていない間か、それがもう run を止めている間に来たシグナルは、その場でプロセスを終わらせる。unix では Ctrl-C は `_exit(130)`、SIGTERM は `_exit(143)` で、Windows の Ctrl-C は `TerminateProcess` で 130 である。Windows の閉じることとシャットダウンは続けて来ても終わらせず、OS の待ち時間に任せる。届けた後のシグナルは1回目として扱う。2つの種類は互いを数えない ([VM](vm.md) の「上限と割り込み」)
- worker が別のタスクを動かしていても、シグナルは次の安全点で届く。届けるのは最初に安全点に着くか起こされた worker で、待つタスクはそのタスクを動かす worker の実行待ちの列の後ろに入る。どの worker も標準入力、`FileIn`、`Child` の同期の読みで止まっている間に来たシグナルは、読みが戻ってから届く
- 本物のシグナル (Ctrl-C、SIGTERM、Windows の閉じることとシャットダウン) が `wait` に届くのは、本物の driver の run だけである。シグナルを待つタスクのいる run は、ほかに進むものがなくてもデッドロックにならず、シグナルが来るまで眠る ([VM](vm.md) の「時計と driver」)。来ることのないシグナルを待つタスクも同じで、`eml repl` の `Terminate` の待ちは、ほかに進むものがなければいつまでも眠る。そのような run は Ctrl-C かプロセスを殺すことでしか終わらない
- テストの driver の run では、本物のシグナルはいつも既定の動きをする。そこで `wait` に届くのは、テストのハーネスが `RunConfig::with_signals` か `Machine::with_signals` で予定した仮想のシグナルだけである。予定は本物の driver の run でも同じに満ちる ([VM](vm.md) の「時間の待ち」)。テストの driver の run では、予定の残っていないシグナルだけを待つ run は、ほかに進むものがなければデッドロックで止まる
- `eml repl` は SIGTERM も Windows の閉じることとシャットダウンも受けず、OS の既定の動作でプロセスが終わる。`Signal.wait Interrupt` は REPL でも使える
- Windows でコンソールを閉じた後は、コンソールへの書き込みと読みが失敗しうる。そのときは先に起きた誤りで run が終わる (書き込みの誤りなら終了コード 1)。出力をファイルか pipe に向けていれば、`Terminate` の後も 143 かプログラムの終了コードで終わる

## 実行時エラー

標準ライブラリの extern の誤りの文言は次のとおりである。I/O の失敗の理由は `not found`、`permission denied`、`not available`、`I/O error` のどれかである。ただし `Std.Net`、`Std.Stream`、`Std.Exec` の driver の誤りは、OS の誤りの表示を理由にする。位置は extern を呼んだ所で、標準ライブラリの関数の中の extern なら `<std>/…` の位置になる。`Async.run` と `Async.await` が起こし直すタスクの失敗は、タスクの中の文言を保ち、位置は std の中の呼び出し (`run` なら `run_result` の呼び出し) になる (上の「`Std.Async`」)。

I/O の失敗の一部は `try_io` で捕まえられる (上の「`try_io`」)。`Async` のタスクの中の深さの上限は、種類 `ResourceLimit` の IO の失敗 `the call depth limit is exceeded` で、メモリの柔らかい上限も同じ種類の IO の失敗 `the memory soft limit is exceeded` である (どちらも上の「`Std.Async`」の「タスクの上限」)。捕まえなければ `Async.run` の `run_result` の位置の実行時エラーになる。`byte_at`、`char_at`、`slice` の誤りと、`Bytes` の範囲の誤りと、`decode` の妥当でない UTF-8 と、`Std.Float` の変換と `to_fixed` の誤りと、配列の添字と長さの誤りと、標準のストリームの2つ目のハンドルと、`Async.start_in` と `spawn_in` の閉じたスコープは `Panic` で、`try_panic` で捕まえられる (上の「`try_panic`」)。捕まえなかった失敗は、この表の文言の実行時エラーになる。

| 失敗 | 文言 |
|---|---|
| `byte_at`、`char_at` の範囲の外 | ``string index 5 is out of range for length 3`` |
| `char_at` の境界 | ``string index 1 is not on a character boundary`` |
| `slice` の範囲の外 | ``string slice 2..9 is out of range for length 3`` |
| `slice` の境界 | ``string slice 1..3 is not on a character boundary`` |
| `Float.truncate`、`floor`、`ceil`、`round` の範囲の外 | `` float `1e+19` does not fit in `Int` `` (値は `show` の形) |
| `Float.to_fixed` の桁数 | `` `to_fixed` takes 0 to 100 digits, not 101 `` |
| `Char.of_int` の引数 | ``int `55296` is not a Unicode scalar value`` |
| `Byte.of_int` の引数 | ``int `256` does not fit in `Byte` `` |
| `Bytes.slice`、`append_slice`、`truncate` の範囲の外 | ``bytes slice 2..9 is out of range for length 3`` |
| `Bytes.get`、`set` の範囲の外 | ``bytes index 5 is out of range for length 3`` |
| `Bytes.make`、`with_capacity` の負の長さ | ``bytes length -1 is negative`` |
| `String.decode` の妥当でない UTF-8 | ``invalid UTF-8 at byte 3`` (数は最初の妥当でないバイトの位置) |
| 配列の添字の範囲の外 (`Array`、`MutArray`、`UnboxedArray`、`MutUnboxedArray` の `get`、`set`、`read`、`write`) | ``array index 3 is out of range for length 3`` |
| 配列の負の長さ (`Array`、`MutArray`、`UnboxedArray`、`MutUnboxedArray` の `make`、`tabulate`) | ``array length -1 is negative`` |
| `FileOut.create`、`append` | ``cannot open `out.txt`: permission denied`` |
| `FileOut.write`、`write_bytes`、`write_line`、`flush`、`close` | ``cannot write to `out.txt`: I/O error`` |
| `Stream.open`、`create`、`append` | ``cannot open `in.txt`: not found`` |
| `Stream.open`、`create`、`append` の通常のファイルでないもの | ``cannot open `.`: not a regular file`` |
| `Stream` の読み (`read_chunk`、`read_line`、`read_all`、`read_all_bytes`) | ``cannot read `in.txt`: `` と OS の誤りの表示 |
| UTF-8 でない `Stream.read_all` と `read_line` | ``cannot read `in.txt`: the text is not valid UTF-8`` (`read_line` は `the line is not valid UTF-8`) |
| `Stream` の書き (`write`、`write_bytes`、`write_line`) | ``cannot write to `out.txt`: `` と OS の誤りの表示 |
| `Proc` と `Std.Exec` の起動 (`run`、`output`、`output_bytes`、`capture`、`capture_bytes`、`spawn`) | ``cannot run `rsync`: not found`` |
| `Proc` と `Std.Exec` の起動の `stdin_text` の一時ファイル | ``cannot run `sort`: cannot make the standard input file: I/O error`` |
| `Proc` と `Std.Exec` の起動のリダイレクトのファイル | ``cannot run `sort`: cannot open `in.txt`: not found`` |
| 仮想の時計の `Std.Exec` の起動 | ``cannot run `sort`: the test driver has no processes`` |
| `Exec.spawn` の最後の段のほかの `Exec.pipe_stderr` | ``cannot run `sort`: the standard error pipe needs `Exec.spawn` on the last stage`` |
| `Proc.capture`、`capture_bytes`、`Exec.capture`、`capture_bytes` の 0 でない終了 | ``command `sh` exited with code 3`` |
| `Child` の読み | ``cannot read the output of `sort`: I/O error`` |
| UTF-8 でない子の出力 | ``the output of `sort` is not valid UTF-8`` |
| `Child` の書き込み | ``cannot write to `sort`: I/O error`` |
| `Proc.wait` と、子を待つ起動の関数 (`run`、`output`、`capture` など) | ``cannot wait for `sort`: I/O error`` |
| `Exec.stdin`、`stdout`、`stderr` の2回目か pipe でない向き | ``cannot take the standard input of `sort`: not available`` (`stdout` は ``cannot take the output of `sort`: not available``、`stderr` は ``cannot take the errors of `sort`: not available``) |
| `Std.Exec` の子の pipe の読み | ``cannot read the output of `sort`: `` と OS の誤りの表示 (標準エラーは ``cannot read the errors of `sort`: ``) |
| `Std.Exec` の子の pipe の書き | ``cannot write to `sort`: `` と OS の誤りの表示 (読み手が終わった子は ``cannot write to `sort`: broken pipe``) |
| `Exec.wait`、`wait_all` と、子を待つ起動の関数 | ``cannot wait for `sort`: `` と OS の誤りの表示 (知らせの後の回収の誤りは ``cannot wait for `sort`: I/O error``) |
| 標準入力の2つ目のハンドル | ``the standard input is already open`` |
| 標準出力の2つ目のハンドル | ``the standard output is already open`` |
| 標準エラーの2つ目のハンドル | ``the standard error is already open`` |
| `Async.start_in`、`spawn_in` の閉じたスコープ | ``the scope is closed`` |
| 標準入力の読み | ``cannot read the standard input: not available`` |
| UTF-8 でない標準入力 | ``the standard input is not valid UTF-8`` |
| UTF-8 でない引数 | ``argument 2 is not valid UTF-8`` (`Env.args ()` のリストの 0 から数えた位置) |
| UTF-8 でない環境変数 | ``environment variable `LANG` is not valid UTF-8`` |

標準ライブラリの中だけで起きる誤り (`String.find_from`、`Bytes.find_from`、`Bytes.find_byte_from` の開始位置、`Env.arg` の位置) の文言は、[LIR](lir.md) の「実行時エラー」にある。
