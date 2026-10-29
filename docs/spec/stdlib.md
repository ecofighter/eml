# 標準ライブラリ

位置づけ: 規範。

スクリプトの起動と、標準ライブラリの関数を定める。モジュールの置き場所と読み込みの規則は [モジュールと名前解決](modules.md) の「標準ライブラリ」に、extern の実装は [オブジェクトモデル](object-model.md) の「extern」にある。

## 起動

```sh
eml wc.em -l input.txt          # eml run wc.em -l input.txt と同じ
eml run --debug-heap wc.em -l input.txt
eml run --sched seeded:7 wc.em -l input.txt
./wc.em -l input.txt            # 1行目が #!/usr/bin/env eml で、実行できるファイル
```

- 最初の引数が `-` で始まらず、サブコマンドの名前 (`check`、`run`、`repl`、`help`) でもなければ、`run` として扱う。`run` という名前のファイルを動かすときは `eml run run` と書く。`-` で始まる最初の引数はフラグとして読むので、`eml --verison` は使い方の誤り (終了コード 2) である
- `run` の後のファイルは、値を取るフラグの値でない、`-` で始まらない最初の引数である。値を取るフラグ (`--sched`) と同じ引数は、次の引数を値として取る。そのほかの `-` で始まる引数 (`--debug-heap`、`--stats`、`--sched=seeded:7`) は、その引数だけでフラグになる。値を取るフラグは clap の `run` の定義から読む
- ファイルより後ろの引数は、`-` で始まるものも含めてそのままスクリプトに渡す。`--debug-heap`、`--stats`、`--sched` はファイルより前にだけ書ける。`eml run main.em --sched bogus` は、スクリプトに `--sched` と `bogus` を渡す
- `--sched <normal|sequential|seeded:N>` は、タスクを切り替える方針を選ぶ ([VM](vm.md) の「タスクとスケジュール」)。既定は `normal` で、監視のスレッドの刻みで横取りする。`sequential` は横取りしない。`seeded:N` は `u64` の seed `N` で横取りの位置を決める。ほかの値と、2回書いた `--sched` は使い方の誤り (終了コード 2) である。`run` と `repl` が持ち、`check` は持たない。環境変数では選べない
- `Env.args ()` は、ファイルより後ろの引数を返す。`eml` とスクリプトのパスは含めない
- shebang は `#!/usr/bin/env eml` と書く。Linux の `env` は `#!` の行の残りを1つの引数として渡すので、`#!/usr/bin/env eml run` は動かない
- ファイルの相対パスと子のプロセスの作業ディレクトリは、プロセスの今のディレクトリである

## extern の境界

標準ライブラリの extern は小さい核にとどめ、便利な関数は eml で書く。extern の境界では、スカラー (`Int`、`Float`、`Char`、`Byte`、`Bool`、`Unit`)、文字列、バイト列、`Foreign` だけを受け渡し、`List` と `Option` は eml の側で組む。`eml_rt` がデータの配置を知らないで済むようにするためである ([Core IR](core-ir.md) の「translate の配置」)。結果が `Char` の extern の行はスカラー値を、結果が `Byte` の extern の行は 0..=255 を返す ([LIR](lir.md) の「値の表現」)。そのため、`pub` でない extern は番兵 (見つからない位置の -1、終わりの空の文字列など) を返すことがある。番兵は標準ライブラリの中に閉じ込め、利用者には `Option` で見せる。

例外は `Std.Par.par` である。`par` は閉包を2つ受け取って呼ぶが、extern は eml のコードを呼び返さない。そのため、`par` の行は std の宣言を型検査に結ぶためだけにあり、引数をそろえた呼び出しは extern の命令でなく `par` の命令になる ([Core IR](core-ir.md) の「`par` の呼び出し」)。

extern の行は、どの位置の引数も借用にできる ([LIR](lir.md) の「extern の借用」)。`String` を読む行 (`byte_length`、`byte_at`、`char_at`、`slice`、`find_from`、`String` の `==`、`!=`、`compare`、`show`、`Float` の `can_parse` と `parse_raw`) は文字列を借りる。`Bytes` を読む行 (`length`、`get`、`slice`、`find_from`、`find_byte_from`、`append_slice` の `src`、`Bytes` の `==`、`!=`、`compare`) もバイト列を借りる。借りた値は、行が失敗しても呼ぶ側が持ったままである。

extern の行は引数を預かることがある (`FunctionRow::held`。[宣言](declarations.md) の「`extern`」)。預かる行は `Std.Async` の `start_raw`、`run_raw`、`park`、`ready`、`finish_ok`、`task_cell`、`take_result`、`run_result`、`detach`、`cancel_raw`、`start_in_raw`、`park_time`、`park_drain`、`park_send`、`park_recv`、`park_io`、`park_io_until`、`park_signal` の18行と、`Std.Chan` の `push_raw` の1行で、`task_cell` だけは借りて預かる。預からない引数の型変数は、同じ節の規則のとおり `Unr` に固定するか、コールバックの結果なら固定しない。

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
- `try_io` は、`exit`、書き出しの失敗、`Panic` (算術の誤り、位置の範囲の誤り、`panic`)、割り込み、確保量と深さの上限、検査モードのヒープの誤りを捕まえない

`IOErrorKind` の構築子は11個で、タグは宣言の順の 0 から 10 である。この実装の extern の誤りが出すのは、最初の5つ (`NotFound` から `Other`) だけである。残りの6つは `fail_io` が出し、`fail_io` は11個のどれでも出せる。`Cancelled` は、非同期の `Wake` の構築子 `Cancelled` と名前が重なる。修飾しない名前は自分のモジュールと import を Prelude より先に引くので ([モジュールと名前解決](modules.md) の「名前の解決」)、`Wake(..)` を import したモジュールでは、種類のほうを `Prelude.Cancelled` と書く。

捕まえる失敗と `kind` は次のとおりである。`message` は、[LIR](lir.md) の「実行時エラー」の文言と1文字も違わない。

| `Fault` | 起こす関数 | `kind` |
|---|---|---|
| `Io { reason, .. }` | `FileIn.open`、`FileIn.read_all`、`FileIn.read_line`、`Fs.read_text`、`FileIn.read_all_bytes`、`FileIn.read_line_bytes`、`FileIn.read_chunk`、`FileOut.create`、`FileOut.append`、`FileOut.write`、`FileOut.write_bytes`、`FileOut.write_line`、`FileOut.flush`、`FileOut.close`、`Fs.write_text`、`Proc` の起動 (`run`、`output`、`output_bytes`、`capture`、`capture_bytes`、`spawn`)、`Child` の読みと書き込み (`output` と `capture` の中の読みを含む) | `reason` が `NotFound` なら `NotFound`、`PermissionDenied` なら `PermissionDenied`、`NotAvailable` なら `NotAvailable`、`Other` なら `Other` |
| `Stdin(reason)` | `Stdin.read_text` と、`Stdin` のハンドルの読み (`read_all`、`read_line`、`read_all_bytes`、`read_line_bytes`、`read_chunk`) | 同上 |
| `NotUtf8(_)` | `FileIn.read_all`、`FileIn.read_line`、`Fs.read_text`、`Stdin.read_text`、`Stdin.read_all`、`Stdin.read_line`、`Proc.output`、`Proc.capture`、`Proc.read_all`、`Proc.read_line`、`Env.args`、`Env.get` | `NotUtf8` |
| `Raised { kind, .. }` | `fail_io` と、`Async.await` と `Async.run` がタスクの IO の失敗を起こし直すとき。`Proc.capture` と `Proc.capture_bytes` の 0 でない終了も `fail_io` で起こす。`Std.Net` の失敗もこの形である (下の「`Std.Net`」) | `kind` のタグが表す種類 (`Proc` の終了は `Other`) |

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
| `Index(_)` | `String.byte_at`、`String.char_at`、`String.slice`、`String.find_from` と、それらを使う std の関数。`Array.get`、`Array.set`、`Array.make`、`MutArray.read`、`MutArray.write`、`MutArray.make` (添字の範囲と負の長さ) と、添字の式が使う `check_index`。`Env.arg` の範囲の外の位置も `Index` だが、`Env.arg` は std の中だけで使うので、利用者のプログラムでは起きない |

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
| `cwd` | `String -> Cmd -> Cmd` |
| `env` | `String -> String -> Cmd -> Cmd` |
| `arg` | `String -> Cmd -> Cmd` |
| `args` | `List String -> Cmd -> Cmd` |
| `merge_stderr` | `Cmd -> Cmd` |
| `discard_stderr` | `Cmd -> Cmd` |
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

子のプロセスを起動する。子はシェルを介さず、`Cmd` のプログラムと引数のリストをそのまま OS に渡す。起動の形は3つある。標準入出力を受け継ぐ `run`、標準出力を取る `output` と `capture`、pipe でやりとりする `spawn` である。どの形でも、引数と設定は同じ `Cmd` で組む。

### `Cmd`

`Cmd` は Prelude のレコードの型で、プログラム、引数のリスト、作業ディレクトリ、環境変数の上書き、標準エラーの向きを持つ ([式](expressions.md) の「コマンドリテラル」)。

- `Cmd` の値は、コマンドリテラルとこのモジュールの設定の関数で組む。プログラムを変数で与えるときは `` `\{prog} \{..args}` `` と書く
- `Cmd` は `Eq` と `Show` の instance を持つ。`show` は全部のフィールドを `Cmd { program = "git", args = ["log"], … }` の形で表示し、`display` は人が読む1行で、プログラムと引数を空白でつなぐ。`display` は引用を戻さないので、コマンドリテラルには戻せない
- フィールドの名前と数は処理系の都合で変わりうる。そのため、利用者のコードは `Cmd { … }` の作る式とパターンを書かない。読むだけの射影 (`cmd.program`、`cmd.args`) は書いてよい。`StderrMode` のコンストラクタも、設定の関数が書くので、利用者は書かない
  - 見直す条件: `Cmd` の設定の関数を Prelude へ移すか、読めるフィールドの `pub` を入れたときに隠す
- `Cmd` を `extern data` にしないのは、`Unr` の `Foreign` は REPL の回復で写せず ([オブジェクトモデル](object-model.md) の「種類」)、`` let c = `ls` `` の後の回復がどれも失敗するためである。`Lin` の `Foreign` にすると、同じ `Cmd` を2回起動できない
- 設定の関数は対象の `Cmd` を最後に受ける。`` `make` |> Proc.cwd "build" |> Proc.merge_stderr |> Proc.capture `` のように `|>` でつなぐためである。`with_` の名前は、handler を呼ぶ関数の慣習 ([例](examples.md) の `with_env`) に取っておく
- `cwd dir c` は作業ディレクトリを `dir` にする。相対パスは `FileIn.open` と同じく Host の `file_root` から解釈する。2回設定したら、後の設定が前の設定を置き換える。ないディレクトリは、起動の誤りの理由 `not found` (``cannot run `make`: not found``) になり、プログラムがないときと区別しない
- `env name value c` は、子の環境変数 `name` を `value` に上書きする。子はほかの環境変数を eml のプロセスから受け継ぐ。同じ名前を2回設定したら、後の組が勝つ。空の名前と `=` を含む名前は OS に渡せないので、起動の誤り (`I/O error`) にする
- `arg a c` と `args xs c` は、引数を後ろに足す
- `merge_stderr c` は、子の標準エラーを標準出力と同じ先に向ける (`2>&1`)。`discard_stderr c` は捨てる (`2>/dev/null`)。どちらも書かなければ、標準エラーは受け継ぐ

### 起動

- `run c` は、標準入出力を受け継いで子を起動し、終わるのを待って終了コードを返す
- `output c` は、子の標準出力を文字列に読み、終了コードと組で返す。子の標準入力は空 (`/dev/null`) である。`output_bytes` はバイト列向けの関数で、UTF-8 を確かめない
- `capture c` は `output` と同じく標準出力を読み、終了コードが 0 なら読んだ文字列を返す。0 でなければ IO の失敗 (`kind` は `Other`、``command `git` exited with code 128``) で、読んだ出力は捨てる。`capture_bytes` はバイト列向けの関数である。失敗するコマンドは、バグでなく IO の条件として `try_io` で扱う
- `spawn c` は、標準入力と標準出力を pipe にして子を起動し、`Child` を返す (下の「`Child`」)
- 標準エラーの向きはどの形でも `Cmd` の設定に従う。`merge_stderr` の標準エラーは、`output` と `capture` では文字列に入り、`run` では標準出力に出て、`spawn` では `read_*` で読める
- 終了コードは子の終了コードで、シグナルで終わった子は、シェルと同じく 128 + シグナルの番号にする。これは unix だけで、Windows にはシグナルで終わる子がない
- 起動の前に標準出力を書き出す。子と eml の出力が順に並ぶようにするためである。eml が標準入力から先読みしてバッファに持っているバイトは、子には届かない
- プログラムが `/` を含まなければ、`PATH` から探す。`/` を含む相対パス (`./run.sh`) は、子の作業ディレクトリ (`cwd` の設定か、なければ `file_root`) に対して解決し、Host が絶対パスにしてから起動する。Rust の `Command` は、作業ディレクトリと相対のプログラムを組み合わせたときの意味を決めていないためである
- Ctrl-C は同じプロセスグループ (Windows では同じコンソール) の子にも届く。子が終わった後、割り込みが来ていれば、子を待っていた `wait` (`run`、`output`、`capture` の中の `wait` を含む) はその場で止まる (終了コード 130)

`capture` の誤りは、次の順に出る。

1. 起動の誤り (``cannot run `git`: not found``)
2. 標準出力の読みの誤り (``cannot read the output of `git`: I/O error``、``the output of `git` is not valid UTF-8``)
3. 子を待つ誤り (``cannot wait for `git`: I/O error``)
4. 終了コード (``command `git` exited with code 128``)

この順は本体 `let (out, child) = read_all (spawn_with … c); let code = wait child; if code == 0 then out else exited c.program code` の順そのものである。そのため、UTF-8 でない出力を出して 0 でなく終わった子は、`NotUtf8` で失敗する。読みの失敗で巻き戻るときは、`Child` の破棄処理が子を kill する (下の「`Child`」)。

std の中だけの extern は次の4つである。

- `spawn_raw : String -> String -> String -> String -> Int -> Int -> Int -> <IO> Child` は、どの起動の形も呼ぶ。引数は次の7つである
  1. プログラム
  2. 引数の列
  3. 環境の列
  4. 作業ディレクトリ (空なら設定しない)
  5. 標準入力の向き (0 は受け継ぐ、1 は空、2 は pipe)
  6. 標準出力の向き (標準入力と同じ)
  7. 標準エラーの向き (0 は受け継ぐ、1 は標準出力に合わせる、2 は捨てる)
- 列は各要素を「バイト長の十進、NUL、本文」の順に並べた1つの文字列である。環境の列は名前と値を交互に置く。組み立ては `++` と `String.byte_length` だけで書くので、`Panic` を起こさない。NUL を含む要素は OS に渡せないので、起動の誤り (`I/O error`) にする。向きの値が範囲の外のときも同じである
- `exited : String -> Int -> <IO> a` は `Proc` の中だけの eml の関数で、`fail_io Other` で ``command `<program>` exited with code <code>`` の IO の失敗を起こす。戻らない。`capture` と `capture_bytes` が使う
- `read_line_raw` と `read_line_bytes_raw` は、`Stdin` の同じ名前の extern と同じく、行の終わりを含む1行か、終わりなら空を返す

### `Child`

`Child` は、子のプロセスと、その標準入力への書き込む端と、標準出力の読み手を持つハンドルである (上の「ハンドル」)。線形な型 (`Lin`) で、ちょうど1回使う。

- `write`、`write_bytes`、`write_line` は、`FileOut` の同じ名前の関数と同じ形で、子の標準入力に書く。`Child` は buffer を持たず、書くたびに pipe に書き出す
- `close_stdin h` は子の標準入力を閉じ、子に EOF を届ける。閉じた後の書き込みは `not available` で失敗する
- 読む関数は `Stdin` の同じ名前の関数と同じ名前と形と意味で、子の標準出力を読む。`read_line` と `read_line_bytes` は `read_line_raw` と `read_line_bytes_raw` の上に eml で書いてある。`Stdin` と違って、補充の前に標準出力を書き出さない
- `wait h` は、標準入力を閉じてから子が終わるのを待ち、終了コードを返す。ハンドルを消費する。子を終わりまで待って終了コードを受け取る道は `wait` だけで、閉じる関数はない
- `drop h` で手放すか、巻き戻し (`try_io`、`try_panic`、`never` の操作)、捕まえなかった実行時エラー、書き込みの誤りのどれで `Child` を手放しても、破棄処理が次の順に片づける。子の終了コードは失われる。`Heap` を落とすときも同じである。`Task a` を手放すとキャンセルにする方針 (下の「取り消し」) と同じ考えである
  1. pipe を閉じる
  2. 直接の子を kill する
  3. 子を待つ
- そのため、直接の子は `try_io` で抜けた後に残らず、zombie にもならない。kill するのは直接の子だけなので、`sh -c` が起動した孫は残りうる

eml は1度に1本の pipe しか読み書きしない。pipe の buffer は OS が持ち、ふつう 64 KiB である。詰まらないように、次の規則で読み書きする。

- 入力を読み終えてから出力を書く子 (`sort`、`wc`) には、書き終えて `close_stdin` してから読む。この順なら、入力の量に上限はない
- 読みながら書く子 (`cat`、`tr`、`grep`) に buffer より多く書いてから読むと、子は出力を書けず、eml は入力を書けず、両方が止まる。そういう子には buffer より少なく書くか、入力を分けて書くたびに読む
- `wait` の前に標準出力を読み切る。読まずに `wait` すると、buffer より多く出力する子で止まる
- 止まったときは Ctrl-C で終われる。unix では書き込みと読みが `EINTR` で戻り、止める語を見るためである。Windows では `eml run` の2回目の Ctrl-C で終わる。処理系は止まったことを検出しない

### 失敗

| 操作 | 失敗 | 届け方 |
|---|---|---|
| `run`、`output`、`output_bytes`、`capture`、`capture_bytes`、`spawn` | 起動できない (`Io { op: Run, name: program, .. }`。``cannot run `rsync`: not found``)。ない作業ディレクトリも `not found` である。NUL を含む引数と、空か `=` を含む環境変数の名前は `I/O error` | `IO` (`try_io`) |
| `capture`、`capture_bytes` | 終了コードが 0 でない (``command `sh` exited with code 3``) | `IO` (`kind` は `Other`) |
| `Child` の読み、`output`、`capture` | 読めない (`Io { op: ReadOutput, name: program, .. }`。``cannot read the output of `sort`: I/O error``)。文字列の読みは UTF-8 でない (``the output of `sort` is not valid UTF-8``) | `IO` (UTF-8 の誤りの `kind` は `NotUtf8`) |
| `Child` の書き込み | 書けない (子が先に終わったときの EPIPE、`close_stdin` の後など)。`FileOut` と同じ `Io { op: Write, name: program, .. }` (``cannot write to `sort`: I/O error``) で、ハンドルを手放す | `IO` |
| `wait`、`run`、`output`、`output_bytes`、`capture`、`capture_bytes` | 子を待てない (`Io { op: Wait, name: program, .. }`。``cannot wait for `sort`: I/O error``)。起動の誤りと違い、子は起動している | `IO` |
| `wait`、読み、書き込み | 割り込み。子が終わった後に `Stopped` になる | 位置を付けた実行時エラー |

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

- `par f g` は、`f ()` と `g ()` を2つの子として動かし、両方が終わるのを待って結果の組を返す。今の VM は、2つの子を1つの worker の上で切り替えて動かす ([VM](vm.md) の「`par` の fork と join」)。`par` の意味は [マルチコア対応の設計](../future/multicore.md) の「`par` の意味」にある
- 子の row の上限は `never` である ([型と Kind](types.md) の「row 変数の多重度の上限」)。子が起こせるのは、`Panic` と、どの操作も `never` のエフェクトだけである。`IO` (`println` や `exit`)、`var` のエフェクト、`once` か `multi` の操作を持つエフェクトを子が起こしうる呼び出しは E2020 になる。子が `never` の操作だけを起こしても、そのエフェクトが `once` の操作を持てば E2020 である。上限は `par` の型にあるので、`par` を値として渡した先の呼び出しも同じく確かめる
- 子が `never` の操作で中断すると、両方の子が終わった後で、親がその操作を起こし直す。両方の子が中断すれば左の子の操作を起こし直し、もう一方の結果は捨てる。子の中の handler と `try_panic` は、子の中の操作を子の中で捕まえる
- 子の実行時エラーは評価を止め、ほかの子の結果では救わない。両方の子が誤りなら左の誤りを報告する ([VM](vm.md) の「子の誤り」)。捕まえられなかった子の `Panic` は親で起こし直した `panic` の実行時エラーになり、文言はソースの位置を持たない ([実装の現在地](../implementation/status.md) の「`par`」)
- 子の閉包は `Lin` の値を捕まえて消費でき、`Lin` の値を返してもよい。extern のスキームは、関数型の引数の矢印の Kind と、その最後の結果だけに現れる型変数を固定しないためである ([宣言](declarations.md) の「`extern`」)。返った値は組に1回入るか、もう一方の子が中断したときに落とされる。落とすのは、`Lin` の値を持つフレームの巻き戻しと同じである ([線形性](linearity.md) の「暗黙の後始末」)。そのため、`both` と `par` は同じ型を受け付ける (`par` は row の上限だけが狭い)
- `par` は Prelude から出さない。`import Std.Par` と書いて `Par.par` で呼ぶか、`import Par (par, both)` で修飾せずに使う
- `both f g` は、呼んだ側の文脈で `f ()`、`g ()` の順に呼び、結果の組を返す。row に上限がないので、`IO` を起こす計算も渡せる。`par` と同じ組を逐次に作る関数である
- 引数をそろえた `par` の呼び出しは、extern の命令でなく `par` の命令になる。部分適用 (`Par.par f`) と値として使う `Par.par` は、同じ命令を本体に持つ包む関数 `$externN` になる ([Core IR](core-ir.md) の「`par` の呼び出し」)

## `Std.Async`

`Std.Async` は1つの worker の上でタスクを動かすモジュールである。

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

pub data Failure =
  | IOFailed IOError
  | Panicked String
  deriving (Show)
```

- `Async` は抽象エフェクト (`pub abstract effect`) で、extern のエフェクトではない。生の操作は `Std.Async` の外では perform できず、handler の節にも書けない ([エフェクトと handler](effects.md) の「抽象エフェクト」)。生の操作は9つで、どれも `once` である。`await_cell c` は `c` が指すタスクのセルが終わるか、スコープのセルが閉じるまで待つ。`reschedule ()` は実行待ちの列の後ろへ回る。`await_time ms` は `ms` ミリ秒眠り、`ms` が 0 以下なら `reschedule ()` と同じく列の後ろへ回る。`await_drain s` はスコープ `s` に属するタスクがなくなるまで待ち、`s` を閉じない。`await_send c` は channel `c` に枠が空くか閉じるまで、`await_recv c` は品物が来るか閉じるまで待つ。`await_io op` は token `op` が指す driver の操作が終わるまで待ち、再開のときに `Wake` と token の組を返す (下の `OpId` の箇条)。`await_io_until op ms` は `await_io` に今から `ms` ミリ秒の期限を付けた待ちである。期限が来ると driver に操作の取り消しを頼み、取り消した操作が終わってから `Woke` で戻る。期限が来たことは記録に残り、包みが読む (下の「`Std.Net`」)。`ms` が 0 以下なら、待ち始めるときに取り消しを頼む。`await_signal s` はシグナル `s` (0 が `Interrupt`、1 が `Terminate`) が来るまで待つ (下の「`Std.Signal`」)。どの操作でも、タスクの待ちの登録はいつも高々1つである
- `Wake` は、ランタイムが待たせたタスクを再開するときに渡す値である。ランタイムは配置の表を見ずに、宣言の順のタグ (`Woke` = 0、`Cancelled` = 1。`eml_lir::WAKE_WOKE`、`WAKE_CANCELLED`) で作る。タグと宣言の順が合うことはテストで確かめる。ランタイムは取り消したタスクを `Cancelled` で再開する
- `Cell` は登録表の添字 `(generation << 32) | index` を持つ即値で、Kind は `Unr`、Repr は `int` である。`Scope` は `Cell` と同じ形の即値で、Kind は `Unr`、Repr は `int` である。`Task a` はランタイムのタスクのハンドルを中身に持つ `Foreign` で、Kind は型引数によらず `Lin` である
- `OpId` は driver に出した操作の記録の番号 `(generation << 32) | index` を持つ token の即値で、Kind は `Lin`、Repr は `int` である。受け付けの行が作り、取り出しの行が消費する。即値なので RC を持たず、1回だけ使うことは型検査が守る。待つ間は継続だけをランタイムが預かり、再開のときに token を継続へ返す。受け付けから取り出しまでを1つの包みの関数の中で行うので、利用者のコードは `OpId` を持たず、`Chan` でも送れない。記録に触れるのは受け付けたタスクだけで、ほかのタスクが token を渡すと内部の誤り `an op is used by another task` になる。放した記録の古い token と、2度目の取り出しは内部の誤り `an op names no record` になる。std の包みからはどちらも起きない ([VM](vm.md) の「操作の記録」)
- ランタイムが名前で呼ぶエントリは、std の3つの関数 `start_task : (Unit -> <IO, Async> Unit) -> <IO> Unit`、`resume_task : (once Wake -> <IO> Unit) -> Wake -> <IO> Unit`、`resume_io : (once (Wake, OpId) -> <IO> Unit) -> Wake -> OpId -> <IO> Unit` である (`eml_extern::RuntimeEntry`)。どれも単相なので、名前で ABI が決まり、instance の名前が正式な名前のままになる。`start_task` はタスクの本体に handler をかぶせ、`await_cell` の節は継続を `park` に、`reschedule` の節は継続を `ready` に、`await_time` の節は継続を `park_time` に、`await_drain` の節は継続を `park_drain` に、`await_send` の節は継続を `park_send` に、`await_recv` の節は継続を `park_recv` に、`await_io` の節は token と継続を `park_io` に、`await_io_until` の節は token と期限と継続を `park_io_until` に、`await_signal` の節はシグナルの番号と継続を `park_signal` に預けて戻る。`resume_task k w` は預けた継続 `k` に `w` を渡す。`resume_io k w op` は `k` に組 `(w, op)` を渡す。extern は組を作らない ([Core IR](core-ir.md) の「translate の配置」) ので、組は eml のエントリが作る。`Panic` はタスクの底で捕まえるので、継続の row は `<IO>` である。3つのエントリが単相の std の関数であることもテストで確かめる
- タスクの底 (`bottom`) は、`try_all` で本体の失敗を捕まえ、成功した結果を `finish_ok` に、IO の失敗を種類と文言のまま `finish_io` に、`Panic` を文言のまま `finish_panic` に渡す
- extern の行は次の33行である。どれも `pub` でない。VM はこれらの行を汎用の extern の命令でなく、`Scheduler` を受ける `Async` の命令で実行する ([VM](vm.md) の「`Async` のタスク」)

| 行 | 型 | 預かる引数 |
|---|---|---|
| `start_raw` | `(Unit -> <IO, Async> Unit) -> <IO> Task a` | 閉包 |
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

- タスクの結果は値のまま渡す。`finish_ok` が結果の値を預かってタスクのセルに持ち、`take_result` と `run_result` がハンドルを消費して、同じセルに持っていた値を1回返す。ランタイムは預かった値を写さず、中身も見ない。返さないままセルを放すときは、その値を落とす。預かる引数の型変数は `Unr` に固定しないので、`Lin` の結果も thunk に包まずに渡せる ([宣言](declarations.md) の「`extern`」)
- `take_result` と `run_result` が返す値の型が、`finish_ok` に預けた値の型と合うことは、処理系は確かめない。生の行を `pub` にせず、`start`、`start_in`、`run`、`await` のシグネチャだけが本体の結果の型 `a` とハンドルの `Task a` を結ぶことで、標準ライブラリが保つ
- 預かる引数の矢印 (`start_raw`、`run_raw`、`start_in_raw` の閉包、`park`、`park_time`、`park_drain`、`park_send`、`park_recv`、`park_io`、`park_io_until`、`park_signal`、`ready` の継続) には、閉じた row を書く。ランタイムはそれを呼ばず、エントリの `start_task`、`resume_task`、`resume_io` が呼ぶので、row はエントリの型が決める
- `park_io` と `park_io_until` は継続だけを預かり、token は預からずに消費する。token は即値の番号なので、再開の仕事が記録の番号から作り直して `resume_io` に渡す
- `op_ready op ms` は token を借りて、待たずに続けてよいかを 0 (続ける)、1 (待つ)、2 (先に譲る) で返し、宣言の型では結果と token の組を返す ([Core IR](core-ir.md) の「組を返す extern」)。どの呼び出しも、操作が終わっていたかによらず操作の予算を1つ使う。記録が終わっていて取り消しの地点が投げなければ、予算が残れば待ちを1つ数えて 0 を返し、切れれば数えずに 2 を返す。2 の待ちは、続く `reschedule ()` が数える。ほかは 1 で、何も数えない。待つ側の `await_io` が数える ([VM](vm.md) の「操作の記録」)
- `task_cell` はハンドルを借りてセルを返し、宣言の型ではセルとハンドルの組を返す ([Core IR](core-ir.md) の「組を返す extern」)
- `chan_wait_send` と `chan_wait_recv` (`Cell -> <IO, Async> Wake`) は `Std.Chan` のためのインターフェースで、`pub` である。`Async` の操作は外のモジュールから perform できないためである ([エフェクトと handler](effects.md) の「抽象エフェクト」)。`Cell` を返すか渡す公開の関数はないので、利用者は呼べない。`Cell` を公開の関数に出すときは、この2つを見直す。channel でないセルか放したセルを渡すと、すぐ `Woke` で戻る
- `io_wait` (`OpId -> <IO, Async> (Wake, OpId)`) と `io_wait_until` (`OpId -> Int -> <IO, Async> (Wake, OpId)`) は `Std.Net` のためのインターフェースで、同じ理由で `pub` である。`OpId` を作る公開の関数はないので、利用者は呼べない (下の「`Std.Net`」)。どちらも先に `op_ready` を呼ぶ。0 なら perform せずに `(Woke, op)` を返し、2 なら `reschedule ()` で列の後ろへ回ってから、その `Wake` を見ずに `(Woke, op)` を返す。1 のときだけ `await_io` か `await_io_until` を perform して待つ。回る間に届いた取り消しは、次の取り消しの地点で投げる
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

### 取り消し

- 取り消しは2つの所から来る。タスクの取り消しのビットは、`cancel t`、`drop t`、属するスコープの取り消し、取り消したスコープに加えることで立つ。スコープの取り消しの旗は、`cancel_scope`、スコープのタスクの失敗、`scope` の本体の失敗、親のスコープの取り消しで立ち、開くときに親の旗を写す (`shield` の境目は写さない。下の `shield` の箇条)。どちらも一度立てたら下ろさない
- タスクにビットが立つと、そのタスクが開いているスコープと、入れ子の `run` で待っている根にも旗が立つ。ただし `shield` の中で開いたスコープには立たない。ビットが立った後に `shield` の外で開くスコープと、ビットが立った後に `shield` の外で呼ぶ入れ子の `run` の根も、開くときに旗を立てる
- `scope` の本体を動かすタスクは、そのスコープの中にいる間だけ旗で取り消される (ビットは立てない)。スコープを出た後は、外の旗かビットが立っていなければ取り消されていない
- 取り消しの地点は `await`、`yield_now`、`sleep_ms`、`drain_within`、`check_cancelled` と、`Std.Chan` の `send` と `recv` と、`Std.Net` の待つ関数である。取り消しの地点は、`shield` の外では、タスクのビットかタスクの今のスコープの旗が立っていれば (`shield` の中の規則は下の `shield` の箇条にある)、種類 `Prelude.Cancelled`、文言 `task was cancelled` の IO の失敗を投げる。待つ前に立っていれば待たずに投げ、待っている間に立てば待ちを外して投げる。`yield_now` で実行待ちの列にいる間に立てば、その `yield_now` が投げる。待っていた相手が終わって起こされた後に立ったときは、`await` は結果を受け取り、次の取り消しの地点が投げる
- `send` と `recv` は、待つ前と待っている間に取り消しが立てば投げる。起こされた後は、`await` と同じく、まず操作をやり直し (枠か品物があれば送るか受け取る)、次の取り消しの地点が投げる。やり直しで待ち直すときは、待つ前の規則で投げる
- `Std.Net` の待つ関数は、待つ前に取り消しが立っていれば、何も受け付けずにストリームかリスナーを落として投げる。待っている間に立てば、driver に取り消しを頼み、操作の完了を待ってから、完了が作ったハンドル、記録の buffer、渡したストリームかリスナーを落として投げる。driver が buffer を使い終わるまで返せないためである (下の「`Std.Net`」)
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
- 閉じたスコープへの `cancel_scope` は何もしない。`cancel` と `cancel_scope` の row は `Async` を持たないので、タスクの外でも呼べる。`run` は待つ間も取り消しの地点でない

### 操作の予算

待たずに済む速い道は、タスクごとの予算を1つずつ使う。予算は 128 である。予算を使い切った速い道は、`yield_now` でほかのタスクに worker を渡す。予算を使うのは、`check_cancelled` と、`Std.Chan` の `send`、`recv`、`try_send`、`try_recv` の速い道と、`Std.Net` の待ち (`Async.io_wait` と `io_wait_until`) である。

`Std.Net` の待ちは、操作がもう終わっていて待たずに続けるときも、終わっていなくて park するときも、1回ごとに予算を1つ使う。予算を使い切った待ちは譲る。終わっていない操作の待ちは park して譲り、終わった操作の待ちは `reschedule ()` で譲って、その `Wake` を見ない。操作の結果を受け取ってから、次の取り消しの地点で投げるためである。予算は、タスクが始まるときと、操作の待ちでない再開のときと、予算を使い切った操作の待ちからの再開のときに 128 に戻す。予算の残る操作の待ちからの再開では戻さない。操作の待ちが park するかは、操作が受け付けで終わったかでスケジュールによって変わる。そこで予算を戻したり、終わった操作の待ちだけで予算を使ったりすると、予算の切れる位置と `RunStats.parks` もスケジュールで変わるためである。`parks` は、どの `Std.Net` の待ちも1つずつ数える。

`try_send` と `try_recv` の row は `Async` を持たないので、予算を使うだけで譲らず、次に予算を見る `send`、`recv`、`check_cancelled` が譲る。待ちから起こされた直後のやり直しは、予算を使っても譲らない。譲った後のやり直しは予算をもう1つ使う。`cancel_requested` は予算を使わない。予算は横取りの予算と別で、横取りでは戻さない。戻す時点は上のとおりプログラムだけで決まるので、channel を使わない run では `RunStats.parks` はスケジュールによらず、横取りのないネイティブのコードでも同じに動く。予算はタスクのセルに持ち、`Ctx` には置かない。予算を使う行はどれも登録表に触れるので、`eml_rt` を変えずに済むためである。

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
- 勝ちは1つの行 `claim_scope s` で決める。呼んだタスクのビットと今のスコープの旗を読み、どちらも立っていなければ、同じ行で `s` に旗を立てて真を返す。立っていれば取り消しが届いたものとして `RunStats.cancels` に数え、偽を返す。確かめることと旗を立てることが1つの行なので、横取りがどこに入っても、勝つのは1つである。`claim_scope` は `shield` の中のビットと旗の区別を見ない
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

### `Failure` と `try_all`

```haskell
pub try_all : (Unit -> <IO, Panic | e> a) -> <IO | e> Result Failure a
```

- `try_all action` は、`action ()` の IO の失敗と `Panic` を両方捕まえて `Failure` の値にする。IO の失敗は `IOFailed` に `IOError` のまま入れ、`Panic` は `Panicked` に文言を入れる。成功すれば `Ok` で結果を返す
- `try_io` の中で `try_panic` を呼ぶ形で書く。`action` の `Panic` は内側の `try_panic` が、IO の失敗は外側の `try_io` が捕まえる (上の「`try_io`」と「`try_panic`」)
- タスクの底は `try_all` でタスクの失敗を捕まえる。接続ごとの分離のように、タスクの中で失敗を受け止めるときにも使う

## `Std.Chan`

`Std.Chan` はタスクの間で値を受け渡す channel のモジュールである。1つの型で、送り手と受け手をいくつでも持てる。

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

- `reserve_send` と `reserve_recv` は、0 (確定してよい)、1 (待つ)、2 (閉じている。受け取りでは閉じて空)、3 (先に譲る。2つ目の引数が真のときだけ) を返す
- `push_raw` と `pop_raw` は、0 を返した `reserve_*` の直後に、`Int` を比べるほかは呼び出しを挟まずに呼ぶ。標準ライブラリがこの形を保ち、処理系は確かめない
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

- `Counter` は、run をまたいで `Machine` が持つ表の添字を持つ `Unr` の即値である。`add n c` は足した後の値を返し、64 ビットで巻き戻る。`Counter` は放さない。数の少ない共有の状態のためのものである。worker のスレッドを入れるときは、表を `SeqCst` の atomic にする
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
| `write_bytes` | `Bytes -> TcpStream -> <IO, Async> TcpStream` | eml |
| `write_line` | `String -> TcpStream -> <IO, Async> TcpStream` | eml |
| `shutdown_write` | `TcpStream -> <IO> TcpStream` | extern |
| `close` | `TcpStream -> <IO> Unit` | eml |
| `peer_addr` | `TcpStream -> (String, TcpStream)` | extern |
| `set_nodelay` | `Bool -> TcpStream -> <IO> TcpStream` | extern |

- `TcpListener` と `TcpStream` の Repr は `obj` である。落としたハンドルは、driver が次の待ち、完了の取り出し、`quiesce` のどれかのときに閉じる。`close s` と `close_listener l` は落とすだけである
- `listen host port` の `host` は IP アドレスの綴り (`0.0.0.0`、`127.0.0.1`、`::`、`::1` など) で、名前は受け付けない。`port` は 0 から 65535 で、0 なら空いているポートを使う。使ったポートは `local_port` で読む
- `connect host port` の `host` は IP アドレスの綴りか名前で、`port` は 1 から 65535 である。名前は driver の解決のスレッドで解決し、得た番地を順に試して、最後に試した番地の誤りを報告する。取り消し、期限、`quiesce` は名前の解決を待たない ([VM](vm.md) の「時計と driver」)
- `accept l` はリスナーに来た接続を1つ受け、そのストリームとリスナーを返す。接続が来るまで待つ
- `accept_within ms l` は `accept l` を `ms` ミリ秒の期限で区切り、結果を `Option` に包む。`None` は期限が過ぎたことだけを表し、リスナーは続けて使える。期限と競って届いた接続は `Some` で返す (上の「時間」)
- `read_chunk b s` は `s` を1回読み、読んだバイトを `b` の後ろに足して、その `Bytes` とストリームを返す。足すのは 1 バイト以上 65 536 バイト以下で、EOF のときだけ何も足さずに `b` の中身のまま返す。EOF は失敗でない (`FileIn.read_chunk` と同じ形)。読めるものがなければ、相手が書くか閉じるまで待つ。buffer の空きは、ストリームが読めるようになってから足す ([VM](vm.md) の「操作の記録」)
- 読みは次のことを保証する。受け取ったバイトはすべて、受け取った順に `read_chunk` か `read_line` に届く。`read_line` で一度も読んでいないストリームは隠れた読みの buffer を持たないので、読んだバイトは `read_chunk` が返した `Bytes` にしかない。`read_line` が読みすぎたバイトはストリームの buffer に残り、次の `read_chunk` がそれを先に、1回に 65 536 バイトまで、driver の操作を出さずに返す。そのため `read_line` と `read_chunk` を混ぜても、バイトは失われず、順も入れ替わらない
- `read_chunk_within ms b s` は `read_chunk b s` を `ms` ミリ秒の期限で区切り、結果を `Option` に包む。`None` は期限が過ぎたことだけを表し、そのとき読んだバイトはない。`b` は捨て、ストリームは閉じずに続けて使える。期限と競って届いたデータは `Some` で返す。期限と競った本物の誤り (reset など) は、期限のない読みと同じく投げる。そのため `None` はバイトを失わず、誤りも隠さない。`ms` が 0 以下の過ぎた期限は、待ち始める前に読みが終わったかを見るだけである。データのないストリームではいつも `None` だが、データのあるストリームでは、待ち始める前に読みが終わったかがスケジュールで変わるので、`Some` と `None` のどちらにもなる
- `read_line s` は `s` から1行を読み、終わりの改行 (`\n` か `\r\n`) を除いた行とストリームを返す。改行のない最後の行はそのまま返し、残りのない EOF では `None` を返す (`Stdin.read_line` と同じ形)。読んだバイトはストリームの buffer に入る。buffer にもう1行あれば driver の操作を出さずに返し、なければ改行か EOF が来るまで、読めるようになるのを待つ操作と読みを繰り返す。行の中身は 1 MiB (1 048 576 バイト) までである。中身は終わりの `\n` か `\r\n` を除いたバイトで、EOF の前の最後の行では残りのすべてである。終わりの改行は上限に数えない。中身が上限を超える行は、改行が届いたときか、改行の届く前に読んだ中身が上限を超えたときに、種類 `Other` の IO の失敗 `cannot read the stream: a line is longer than 1048576 bytes` になる。そのため結果は、バイトが読みにどう分かれて届くかによらない。UTF-8 でない行は、種類 `NotUtf8` の IO の失敗 `cannot read the stream: the line is not valid UTF-8` になる。どちらの失敗でも、包みはストリームを落としてから投げるので、ストリームは呼ぶ側に戻らない
- `read_line_within ms s` は `read_line s` を `ms` ミリ秒の期限で区切り、結果を `Option` に包む。期限は1行の全体に付き、読みの1回ごとには付かない。`None` は期限が過ぎたことだけを表し、そのとき読んだ途中の行はストリームの buffer に残って、次の読みが続きと合わせて返す。期限と競って行がそろったときは `Some` で返す
- `_within` の関数 (`accept_within`、`read_chunk_within`、`read_line_within`) は、期限のない関数の結果を `Option` に包んで返す。`None` は期限が過ぎたことだけを表し、リスナーかストリームは続けて使える
- `write_bytes b s` は `b` をすべて書き、ストリームを返す。本物の driver は、短く終わった送りを出し直す。`write str s` は `str` の UTF-8 のバイトを書き、`write_line str s` は `str` と改行を書く。どれも `write_bytes` を呼ぶ
- 書きの期限は `Async.timeout ms (fn () -> Net.write_bytes b s)` で付ける。期限が来ると、巻き戻しがストリームを閉じる
- `local_port` と `peer_addr` は、作ったときに記録した値を読む。引数を借りて組で返し ([Core IR](core-ir.md) の「組を返す extern」)、効果を持たない。`peer_addr` は Rust の `SocketAddr` の表示の形 (`127.0.0.1:49153`、`[::1]:8080`) で返す
- `shutdown_write s` は送る側を閉じ、相手は残りを読みきってから EOF を受ける。`set_nodelay on s` は TCP_NODELAY を設定する。どちらも driver の操作を待たない。失敗はそれぞれ IO の失敗 `cannot shut down the stream: …` と `cannot set the no-delay option: …` で、種類は次の箇条の、extern が投げる誤りと同じ規則で決める
- 拒まれた接続は種類 `ConnectionRefused`、reset と EPIPE と ECONNABORTED は `ConnectionReset`、使用中の番地は `AddrInUse`、OS の時間切れは `TimedOut` である。この4つは包みが `fail_io` で投げる。ほかの誤りは extern が投げ、種類は最初の5つから OS の誤りで決める。解決できない名前は `NotFound`、権限のない操作 (1024 より小さいポートの `listen` など) は `PermissionDenied`、扱えない操作は `NotAvailable`、残りは `Other` である (上の「`try_io`」)
- 文言は prefix と理由を `: ` でつないだものである。prefix は `cannot listen on {host}:{port}`、`cannot connect to {host}:{port}`、`cannot accept a connection`、`cannot read the stream`、`cannot write to the stream` のどれかである。理由は、包みが投げる4つの種類では `connection refused`、`connection reset`、`address in use`、`timed out` で、ほかは driver の誤りの表示である。テストの driver では、解決できない名前の理由は `unknown host` になり、相手が閉じたストリームへの書きは `cannot write to the stream: connection reset` (種類 `ConnectionReset`) になる
- `listen` の `host` が IP アドレスの綴りでないときと、`listen` と `connect` の `port` が範囲の外のときは、操作を出さずに種類 `Other` の IO の失敗を起こす。理由はそれぞれ `the host is not an IP address` と `the port is out of range` である
- Windows の本物の driver はまだソケットを扱わないので、`listen`、`accept`、`connect` は種類 `NotAvailable` の IO の失敗 (`cannot listen on 127.0.0.1:0: unsupported`) になる。テストの driver はどの OS でも同じに動く
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
- extern の行は次の18行である。`local_port`、`peer_addr`、`shutdown_write`、`set_nodelay` のほかは `pub` でない。VM は `Std.Async` の行と同じく `Async` の命令で実行する ([VM](vm.md) の「`Async` のタスク」)

| 行 | 型 | 借りる引数 |
|---|---|---|
| `submit_listen` | `String -> Int -> <IO> Async.OpId` | なし |
| `submit_accept` | `TcpListener -> <IO> Async.OpId` | なし |
| `submit_connect` | `String -> Int -> <IO> Async.OpId` | なし |
| `submit_read` | `Bytes -> TcpStream -> <IO> Async.OpId` | なし |
| `submit_read_line` | `TcpStream -> <IO> Async.OpId` | なし |
| `submit_write` | `Bytes -> TcpStream -> <IO> Async.OpId` | なし |
| `op_listener` | `Async.OpId -> <IO> (TcpListener, Async.OpId)` | token |
| `op_stream` | `Async.OpId -> <IO> (TcpStream, Async.OpId)` | token |
| `op_outcome` | `Async.OpId -> <IO> (Int, Async.OpId)` | token |
| `take_listener` | `Async.OpId -> <IO> TcpListener` | なし |
| `take_stream` | `Async.OpId -> <IO> TcpStream` | なし |
| `take_read` | `Async.OpId -> <IO> Bytes` | なし |
| `take_line` | `Async.OpId -> <IO> String` | なし |
| `finish` | `Async.OpId -> <IO> Unit` | なし |
| `local_port` | `TcpListener -> (Int, TcpListener)` | リスナー |
| `peer_addr` | `TcpStream -> (String, TcpStream)` | ストリーム |
| `shutdown_write` | `TcpStream -> <IO> TcpStream` | なし |
| `set_nodelay` | `Bool -> TcpStream -> <IO> TcpStream` | なし |

- 受け付けの行は引数を消費して記録に移し、token を返す。`submit_listen` と `submit_connect` は `host` を写して手放し、文言の prefix を記録に持たせる。`submit_read` と `submit_write` は buffer が一意でなければ (共有、不死、ほかの参照)、同じ中身の新しい `Bytes` に写して元を手放す。driver が読み書きする間に書き換わらないためである。`submit_read` は、ストリームの buffer に `read_line` が残したバイトがあれば、それを buffer に足して、driver の操作を出さずに終わった記録を作る。`submit_read_line` はストリームだけを記録に移す
- `op_listener` と `op_stream` は、終わった記録から受け付けに渡したものを移し出す。`op_outcome` は結果の番号を返す
- `take_listener` と `take_stream` は token を消費して記録を放し、完了から作ったリスナーかストリームを返す。`take_read` は token を消費して記録を放し、読んだバイトを足した buffer を返す。`take_line` は token を消費して記録を放し、記録が切り出した行を終わりの改行を含めて返す。残りのない EOF では空の文字列を返し、包みがそれを `None` にする。どれも、番号が 0 でない記録では内部の誤り `an op is taken after it failed` になる
- `finish` は token を消費して記録を放し、記録がまだ持つもの (完了から作ったハンドルを含む) を落とす。番号 6 の誤りは、取り消しを頼んだ記録でなければ IO の失敗にする

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

- `Interrupt` は Ctrl-C で、unix の SIGINT と Windows の `CTRL_C_EVENT` である。`Terminate` は unix の SIGTERM である。Windows には `Terminate` を起こす OS の出来事がない
- `wait s` は、シグナル `s` が来るまでタスクを待たせる。取り消しの地点で、取り消されると IO の失敗 `Prelude.Cancelled` を投げる。取り消された待ちは待ち手から外れる。待ちは `Std.Async` の `signal_wait` を通す (上の「`Std.Async`」)
- シグナルが来ると、そのとき同じシグナルを待つタスクをすべて、待ち始めた順に起こす。起こしたタスクは実行待ちの列の後ろに入る。シグナルは貯めない。待つタスクがなければ既定の動きをする。`Interrupt` は run を止めて終了コードを 130 にし、`Terminate` も同じ止め方で 143 にする ([VM](vm.md) の「上限と割り込み」)。起こされてから次の `wait` までに来たシグナルも既定の動きをする
- 届ける前に同じシグナルが続けて来ると、1つにまとまる。`eml run` では、前の同じシグナルをまだ届けていない間か、それがもう run を止めている間に来たシグナルは、その場でプロセスを終わらせる。Ctrl-C は `_exit(130)`、SIGTERM は `_exit(143)` である。届けた後のシグナルは1回目として扱う。2つの種類は互いを数えない ([VM](vm.md) の「上限と割り込み」)
- worker が別のタスクを動かしていても、シグナルは次の安全点で届き、待つタスクは実行待ちの列の後ろに入る。標準入力、`FileIn`、`Child` の同期の読みで worker が止まっている間に来たシグナルは、読みが戻ってから届く
- 本物のシグナル (Ctrl-C と SIGTERM) が `wait` に届くのは、本物の driver の run だけである。シグナルを待つタスクのいる run は、ほかに進むものがなくてもデッドロックにならず、シグナルが来るまで眠る ([VM](vm.md) の「時計と driver」)。来ることのないシグナルを待つタスクも同じで、Windows と `eml repl` の `Terminate` の待ちは、ほかに進むものがなければいつまでも眠る。そのような run は Ctrl-C かプロセスを殺すことでしか終わらない
- テストの driver の run では、本物のシグナルはいつも既定の動きをする。そこで `wait` に届くのは、テストのハーネスが `RunConfig::with_signals` か `Machine::with_signals` で予定した仮想のシグナルだけである。予定は本物の driver の run でも同じに満ちる ([VM](vm.md) の「時間の待ち」)。テストの driver の run では、予定の残っていないシグナルだけを待つ run は、ほかに進むものがなければデッドロックで止まる
- `eml repl` は SIGTERM を受けず、SIGTERM は OS の既定の動作でプロセスを終わらせる。`Signal.wait Interrupt` は REPL でも使える

## 実行時エラー

標準ライブラリの extern の誤りの文言は次のとおりである。I/O の失敗の理由は `not found`、`permission denied`、`not available`、`I/O error` のどれかである。位置は extern を呼んだ所で、標準ライブラリの関数の中の extern なら `<std>/…` の位置になる。`Async.run` と `Async.await` が起こし直すタスクの失敗は、タスクの中の文言を保ち、位置は std の中の呼び出し (`run` なら `run_result` の呼び出し) になる (上の「`Std.Async`」)。

I/O の失敗の一部は `try_io` で捕まえられる (上の「`try_io`」)。`byte_at`、`char_at`、`slice` の誤りと、`Bytes` の範囲の誤りと、`decode` の妥当でない UTF-8 と、`Std.Float` の変換と `to_fixed` の誤りと、配列の添字と長さの誤りと、標準のストリームの2つ目のハンドルと、`Async.start_in` と `spawn_in` の閉じたスコープは `Panic` で、`try_panic` で捕まえられる (上の「`try_panic`」)。捕まえなかった失敗は、この表の文言の実行時エラーになる。

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
| `Bytes.make` の負の長さ | ``bytes length -1 is negative`` |
| `String.decode` の妥当でない UTF-8 | ``invalid UTF-8 at byte 3`` (数は最初の妥当でないバイトの位置) |
| 配列の添字の範囲の外 (`get`、`set`、`read`、`write`) | ``array index 3 is out of range for length 3`` |
| 配列の負の長さ (`make`、`tabulate`) | ``array length -1 is negative`` |
| `FileOut.create`、`append` | ``cannot open `out.txt`: permission denied`` |
| `FileOut.write`、`write_bytes`、`write_line`、`flush`、`close` | ``cannot write to `out.txt`: I/O error`` |
| `Proc` の起動 (`run`、`output`、`output_bytes`、`capture`、`capture_bytes`、`spawn`) | ``cannot run `rsync`: not found`` |
| `Proc.capture`、`capture_bytes` の 0 でない終了 | ``command `sh` exited with code 3`` |
| `Child` の読み | ``cannot read the output of `sort`: I/O error`` |
| UTF-8 でない子の出力 | ``the output of `sort` is not valid UTF-8`` |
| `Child` の書き込み | ``cannot write to `sort`: I/O error`` |
| `Proc.wait` と、子を待つ起動の関数 (`run`、`output`、`capture` など) | ``cannot wait for `sort`: I/O error`` |
| 標準入力の2つ目のハンドル | ``the standard input is already open`` |
| 標準出力の2つ目のハンドル | ``the standard output is already open`` |
| 標準エラーの2つ目のハンドル | ``the standard error is already open`` |
| `Async.start_in`、`spawn_in` の閉じたスコープ | ``the scope is closed`` |
| 標準入力の読み | ``cannot read the standard input: not available`` |
| UTF-8 でない標準入力 | ``the standard input is not valid UTF-8`` |
| UTF-8 でない引数 | ``argument 2 is not valid UTF-8`` (`Env.args ()` のリストの 0 から数えた位置) |
| UTF-8 でない環境変数 | ``environment variable `LANG` is not valid UTF-8`` |

標準ライブラリの中だけで起きる誤り (`String.find_from`、`Bytes.find_from`、`Bytes.find_byte_from` の開始位置、`Env.arg` の位置) の文言は、[LIR](lir.md) の「実行時エラー」にある。
