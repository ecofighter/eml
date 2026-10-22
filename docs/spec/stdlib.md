# 標準ライブラリ

位置づけ: 規範。

スクリプトの起動と、標準ライブラリの関数を定める。モジュールの置き場所と読み込みの規則は [モジュールと名前解決](modules.md) の「標準ライブラリ」に、extern の実装は [オブジェクトモデル](object-model.md) の「extern」にある。

## 起動

```sh
eml wc.em -l input.txt          # eml run wc.em -l input.txt と同じ
eml run --debug-heap wc.em -l input.txt
./wc.em -l input.txt            # 1行目が #!/usr/bin/env eml で、実行できるファイル
```

- 最初の引数が `-` で始まらず、サブコマンドの名前 (`check`、`run`、`repl`、`help`) でもなければ、`run` として扱う。`run` という名前のファイルを動かすときは `eml run run` と書く。`-` で始まる最初の引数はフラグとして読むので、`eml --verison` は使い方の誤り (終了コード 2) である
- ファイルより後ろの引数は、`-` で始まるものも含めてそのままスクリプトに渡す。`--debug-heap` はファイルより前にだけ書ける
- `Env.args ()` は、ファイルより後ろの引数を返す。`eml` とスクリプトのパスは含めない
- shebang は `#!/usr/bin/env eml` と書く。Linux の `env` は `#!` の行の残りを1つの引数として渡すので、`#!/usr/bin/env eml run` は動かない
- ファイルの相対パスと子のプロセスの作業ディレクトリは、プロセスの今のディレクトリである

## extern の境界

標準ライブラリの extern は小さい核にとどめ、便利な関数は eml で書く。extern の境界では、スカラー (`Int`、`Float`、`Char`、`Byte`、`Bool`、`Unit`)、文字列、バイト列、`Foreign` だけを受け渡し、`List` と `Option` は eml の側で組む。`eml_rt` がデータの配置を知らないで済むようにするためである ([Core IR](core-ir.md) の「translate の配置」)。結果が `Char` の extern の行はスカラー値を、結果が `Byte` の extern の行は 0..=255 を返す ([LIR](lir.md) の「値の表現」)。そのため、`pub` でない extern は番兵 (見つからない位置の -1、終わりの空の文字列など) を返すことがある。番兵は標準ライブラリの中に閉じ込め、利用者には `Option` で見せる。

extern の行は、どの位置の引数も借用にできる ([LIR](lir.md) の「extern の借用」)。`String` を読む行 (`byte_length`、`byte_at`、`char_at`、`slice`、`find_from`、`String` の `==`、`!=`、`compare`、`show`、`Float` の `can_parse` と `parse_raw`) は文字列を借りる。`Bytes` を読む行 (`length`、`get`、`slice`、`find_from`、`find_byte_from`、`append_slice` の `src`、`Bytes` の `==`、`!=`、`compare`) もバイト列を借りる。借りた値は、行が失敗しても呼ぶ側が持ったままである。

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

捕まえる失敗と `kind` は次のとおりである。`message` は、[LIR](lir.md) の「実行時エラー」の文言と1文字も違わない。

| `Fault` | 起こす関数 | `kind` |
|---|---|---|
| `Io { reason, .. }` | `FileIn.open`、`FileIn.read_all`、`FileIn.read_line`、`Fs.read_text`、`FileIn.read_all_bytes`、`FileIn.read_line_bytes`、`FileIn.read_chunk`、`FileOut.create`、`FileOut.append`、`FileOut.write`、`FileOut.write_bytes`、`FileOut.write_line`、`FileOut.flush`、`FileOut.close`、`Fs.write_text`、`Proc.run` | `reason` が `NotFound` なら `NotFound`、`PermissionDenied` なら `PermissionDenied`、`NotAvailable` なら `NotAvailable`、`Other` なら `Other` |
| `Stdin(reason)` | `Stdin.read_text` と、`Stdin` のハンドルの読み (`read_all`、`read_line`、`read_all_bytes`、`read_line_bytes`、`read_chunk`) | 同上 |
| `NotUtf8(_)` | `FileIn.read_all`、`FileIn.read_line`、`Fs.read_text`、`Stdin.read_text`、`Stdin.read_all`、`Stdin.read_line`、`Env.args`、`Env.get` | `NotUtf8` |

書き出しの失敗 (`Output`) を捕まえないのは、標準出力をまとめて書くためである ([オブジェクトモデル](object-model.md) の「Host」)。書けなかったことが分かるのは、後でバッファがあふれた `println` か、読みや起動の前の書き出しか、プログラムの終わりの書き出しになる。そのため、`try_io (fn () -> println s)` は書けなくても `Ok` を返しうるし、関係のない別の `try_io` が失敗を受け取りうる。どこで捕まるかが決まらないので、実行時エラーのままにする。

`try_io` で捕まえなかった IO の失敗は、実行時エラーである。位置を付けて表示し、終了コード 1 で止まる。このとき eml の後始末 (handler の節と利用者のコード) は走らないが、ヒープを落とすときの破棄処理が `FileOut` の buffer を書き出す (下の「`FileOut`」)。プロセスが終わるので、OS が開いたファイルを閉じる。

## `try_panic`

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

## `exit`

```haskell
pub extern exit : Int -> <IO> a
```

- `exit n` は、途中のフレームが持っていた値を後始末し、標準出力を書き出してから、終了コード `n & 255` で終わる。シェルが終了コードを 8 ビットで扱うのと同じである
- `try_io` も、利用者の handler も、`exit` を捕まえない。どの handler の `return` の節も呼ばない
- 書き出しが失敗したら、プログラムの終わりの書き出しの失敗と同じく、位置のない実行時エラー (終了コード 1) にする
- `exit` は `<IO>` を持つので、型の上では `IO` の関数としてどこからでも呼べる。`<IO>` を持たせるのは、`par` の子から呼べないようにするためである。`par` の子の row は `IO` を含められない ([マルチコア対応の設計](../future/multicore.md) の「row の Kind」)
- `exit 1`、`exit 2`、`exit 130` の終了コードは、CLI の実行時エラー (1)、使い方の誤り (2)、割り込み (130) と区別できない。スクリプトが選んだことなので、そのまま受け入れる
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

- 位置はどれも UTF-8 のバイトの位置の `Int` で、位置専用の型はない。文字の境界でない位置は、`char_at` と `slice` が `Panic` で断る
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

`String` と同じく、モジュールで修飾して使う (`Float.sqrt x`)。Prelude の名前を増やさないため、Prelude には入れない。

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
pub data Map k v =
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
- コンストラクタは公開される。抽象型がまだなく、コンストラクタを隠せないためである。API は上の関数の表だけである。利用者が `Empty` などで直接作った木に、関数が何を返すかは決めない。そのような木でも、`size` 以外の関数は実行時エラーを起こさない
- `import Map (Map(..))` と書くと、`Empty` が利用者のコンストラクタと重なりうる。そのため、使い方は `Map.empty` と関数に限る

## `Env`

| 関数 | 型 |
|---|---|
| `args` | `Unit -> <IO, Panic> List String` |
| `get` | `String -> <IO> Option String` |

- `args ()` は、上の「起動」の引数である。REPL では `[]` を返す
- `get name` は環境変数の値である。設定されていなければ `None`、空の値なら `Some ""` を返す。空の名前と、`=` か NUL を含む名前は、設定されていないとみなす
- UTF-8 でない引数や値は、読んだときに実行時エラーにする

## ハンドル

`Stdin.Stdin` と `FileIn.FileIn` は読むためのハンドルで、`Stdout.Stdout`、`Stderr.Stderr`、`FileOut.FileOut` は書くためのハンドルである。ハンドルの型ごとに1つのモジュールを置き、どのモジュールも、開く関数 (`open` か `create`)、`close`、次の規則の名前の読みか書きを持つ。読みと書きを型で分けるので、取り違えは型の誤り (E2001) になる。

関数の名前は、std 全体で次の規則に従う。文字列を読み書きする関数が短い名前を持ち、バイト列の版は `_bytes` を足す。読みは `read_all` と `read_all_bytes`、`read_line` と `read_line_bytes` で、書きは `write` と `write_bytes`、文字列だけの `write_line` である。バイト列にしかない読み (`read_chunk`) は、その名前のままである。スクリプトの大半は文字列を読み、文字列の読みが UTF-8 を確かめることが「`String` はいつも妥当な UTF-8」を支えるので、文字列の版を既定にする。

- ハンドルは線形な型 (`Lin`) で、ちょうど1回使う ([線形性](linearity.md))。`Stdin`、`Stdout`、`Stderr`、`FileIn` の `close h` は `drop h` と同じで、ハンドルを手放して閉じる。`FileOut` の `close` は書き出してから閉じ、書き出せなければ誤りを返す (下の「`FileOut`」)
- 読む関数はハンドルを借りて読み、結果を先に、同じハンドルを後に置いた組で返す。`Stdin` と `FileIn` は同じ名前と同じ形の読む関数を持つ (下の「`Stdin`」と「`FileIn`」)
- 書く関数は書くデータを借り、ハンドルを受け取って同じハンドルを返す。`Stdout`、`Stderr`、`FileOut` は同じ名前と同じ形の書く関数を持つ (下の「`Stdout` と `Stderr`」と「`FileOut`」)
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
| 文字列の読み | UTF-8 でない (`NotUtf8`)。バイト列の読みはこれで失敗しない | `IO` (`kind` は `NotUtf8`) |
| どの読みも | 割り込み、確保量の上限 | 位置を付けた実行時エラー |
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
- `Stdout.flush h` は、標準出力の buffer を書き出す。ほかに Host が buffer を書き出すのは、`main` の終わり、`exit`、実行時エラーを出す前、割り込みか上限で止めたとき、REPL の入力の区切り、`Proc.run` の前、標準入力の読み手の補充の前である
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

- `create path` は、ファイルを作るか切り詰めて開く。`append path` は、ファイルを作るか、末尾に足す形で開く。相対パスの扱いは `FileIn.open` と同じである ([エフェクトと handler](effects.md) の「組み込みの `IO`」)
- 書く関数は、`Stdout` の同じ名前の関数と同じ意味である (上の「`Stdout` と `Stderr`」)。`write s f` は文字列 `s` を、`write_bytes b f` はバイト列 `b` をそのまま書き、`write_line s f` は `s` と `\n` を書く。どれもデータを借りて、受け取った `FileOut` を返す
- `FileOut` は自分の書き込みの buffer (64 KiB) を持つ。ファイルに書き出すのは、buffer があふれたとき、`flush` を呼んだとき、`close` を呼んだときの3つだけである。`fsync` はしない。`main` の終わりや実行時エラーの前の書き出しは、標準出力の buffer だけを書き出し、`FileOut` の buffer に触れない
- `close f` は、書き出してから閉じる。std の中だけの extern `close_raw : FileOut -> <IO> Unit` の上に、`close h = close_raw (flush h)` と書いてある。書き出せなければ `close_raw` に進まず、`flush` の誤りを返す
- 閉じずに手放したときも、破棄処理が buffer を書き出し、誤りを捨てる。巻き戻し (`try_io`、`try_panic`、`never` の操作)、`exit`、捕まえなかった実行時エラーのどれで手放しても、書いた中身は書き出される。失うのは、2回目の Ctrl-C と abort のときだけである ([オブジェクトモデル](object-model.md) の「出力のバッファ」)

`write`、`write_bytes`、`write_line`、`flush`、`close` が書けなかったときは、`Io { op: Write, .. }` (``cannot write `<path>`: <reason>``) を返す。標準出力と違って書き込みの順で誤りが決まるので、`try_io` で捕まえる。誤りを返したときの約束は次の4つである。

1. ハンドルは閉じている
2. ファイルには、そのハンドルに渡したバイトの先頭の部分が残る。どこまで書けたかは約束しない
3. 書いていない残りの buffer は捨て、それ以上書かない。そのため、誤りを返した後にファイルが伸びない
4. 誤りは、この呼び出しか、同じハンドルのそれより前の書き込みの失敗を表す

## `Fs`

| 関数 | 型 |
|---|---|
| `read_text` | `String -> <IO> String` |
| `write_text` | `String -> String -> <IO> Unit` |

パスで1回に済む関数である。ハンドルを使う読みは `FileIn` に、書きは `FileOut` にある。`Fs` は extern を持たず、どの関数も `FileIn` と `FileOut` の上に eml で書いてある。

- `read_text path` は、ファイル全体を文字列として読む。`FileIn.open`、`FileIn.read_all`、`FileIn.close` で書いた関数である
- `write_text path text` は、ファイルを作るか切り詰めて、`text` を書く。`write_text path text = FileOut.close (FileOut.write text (FileOut.create path))` である。開けないときと書けないときの誤りは、`FileOut` の関数の誤りである

## `Proc`

| 関数 | 型 |
|---|---|
| `run` | `String -> List String -> <IO, Panic> Int` |

- `run program args` は、`PATH` から `program` を探して子のプロセスを起動し、終わるのを待って終了コードを返す。子はシェルを介さず、標準入出力と環境を受け継ぐ。シグナルで終わった子の終了コードは、シェルと同じく 128 + シグナルの番号にする
- 起動の前に標準出力を書き出す。子と eml の出力が順に並ぶようにするためである。eml が標準入力から先読みしてバッファに持っているバイトは、子には届かない
- NUL を含む引数は OS に渡せないので、起動の誤り (`I/O error`) にする
- Ctrl-C は同じプロセスグループの子にも届く。子が終わった後、割り込みが来ていれば、`run` はその場で止まる (終了コード 130)
- 起動に使う extern は、引数の数と各引数を、それぞれ NUL で終えて並べた1つの文字列を受ける。OS の引数は NUL を含めないので、NUL を含む引数があれば数が合わなくなり、起動の誤りとして見つかる

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
pub data Iter <e> a = | Iter (once Unit -> <e> Option (a, Iter <e> a))
```

| 関数 | 型 |
|---|---|
| `next` | `Iter <e> a -> <e> Option (a, Iter <e> a)` |
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
- コンストラクタ `Iter` も公開する。コンストラクタを隠す `pub data` (抽象型) がないためである
- `next it` は、次の要素と残りの `Iter` の組を返し、終わりなら `None` を返す
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

## 実行時エラー

標準ライブラリの extern の誤りの文言は次のとおりである。I/O の失敗の理由は `not found`、`permission denied`、`not available`、`I/O error` のどれかである。位置は extern を呼んだ所で、標準ライブラリの関数の中の extern なら `<std>/…` の位置になる。

I/O の失敗の一部は `try_io` で捕まえられる (上の「`try_io`」)。`byte_at`、`char_at`、`slice` の誤りと、`Bytes` の範囲の誤りと、`decode` の妥当でない UTF-8 と、`Std.Float` の変換と `to_fixed` の誤りと、配列の添字と長さの誤りと、標準のストリームの2つ目のハンドルは `Panic` で、`try_panic` で捕まえられる (上の「`try_panic`」)。捕まえなかった失敗は、この表の文言の実行時エラーになる。

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
| `FileOut.write`、`write_bytes`、`write_line`、`flush`、`close` | ``cannot write `out.txt`: I/O error`` |
| `Proc.run` の起動 | ``cannot run `rsync`: not found`` |
| 標準入力の2つ目のハンドル | ``the standard input is already open`` |
| 標準出力の2つ目のハンドル | ``the standard output is already open`` |
| 標準エラーの2つ目のハンドル | ``the standard error is already open`` |
| 標準入力の読み | ``cannot read the standard input: not available`` |
| UTF-8 でない標準入力 | ``the standard input is not valid UTF-8`` |
| UTF-8 でない引数 | ``argument 2 is not valid UTF-8`` (`Env.args ()` のリストの 0 から数えた位置) |
| UTF-8 でない環境変数 | ``environment variable `LANG` is not valid UTF-8`` |

標準ライブラリの中だけで起きる誤り (`String.find_from`、`Bytes.find_from`、`Bytes.find_byte_from` の開始位置、`Env.arg` の位置) の文言は、[LIR](lir.md) の「実行時エラー」にある。
