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

標準ライブラリの extern は小さい核にとどめ、便利な関数は eml で書く。extern の境界では、スカラー (`Int`、`Float`、`Char`、`Byte`、`Bool`、`Unit`)、文字列、`Foreign` だけを受け渡し、`List` と `Option` は eml の側で組む。`eml_rt` がデータの配置を知らないで済むようにするためである ([Core IR](core-ir.md) の「translate の配置」)。結果が `Char` の extern の行はスカラー値を、結果が `Byte` の extern の行は 0..=255 を返す ([LIR](lir.md) の「値の表現」)。そのため、`pub` でない extern は番兵 (見つからない位置の -1、終わりの空の文字列など) を返すことがある。番兵は標準ライブラリの中に閉じ込め、利用者には `Option` で見せる。

文字列の位置は UTF-8 のバイトの位置 (`Int`) で表す。引数の順は、対象を最後に置く。`line |> String.split ","` と部分適用で使えるようにするためである。

## `Prelude`

- `eprintln : String -> <IO> Unit` は、標準エラーに1行を書く。C の stdio と同じく、標準出力は書き出さない。標準出力をまとめて書いているとき (端末でないとき) は、`eprintln` の行が先に出ることがある
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
| `Io { reason, .. }` | `Fs.open`、`Fs.read_all`、`Fs.read_line`、`Fs.read_text`、`Fs.write_text`、`Proc.run` | `reason` が `NotFound` なら `NotFound`、`PermissionDenied` なら `PermissionDenied`、`NotAvailable` なら `NotAvailable`、`Other` なら `Other` |
| `Stdin(reason)` | `Stdin.read_all`、`Stdin.read_line` | 同上 |
| `NotUtf8(_)` | `Fs.read_all`、`Fs.read_line`、`Fs.read_text`、`Stdin.read_all`、`Stdin.read_line`、`Env.args`、`Env.get` | `NotUtf8` |

書き出しの失敗 (`Output`) を捕まえないのは、標準出力をまとめて書くためである ([オブジェクトモデル](object-model.md) の「Host」)。書けなかったことが分かるのは、後でバッファがあふれた `println` か、読みや起動の前の書き出しか、プログラムの終わりの書き出しになる。そのため、`try_io (fn () -> println s)` は書けなくても `Ok` を返しうるし、関係のない別の `try_io` が失敗を受け取りうる。どこで捕まるかが決まらないので、実行時エラーのままにする。

`try_io` で捕まえなかった IO の失敗は、実行時エラーである。位置を付けて表示し、終了コード 1 で止まる。このときは後始末をしない。プロセスが終わるので、OS が開いたファイルを閉じる。

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
| `Index(_)` | `String.byte_at`、`String.slice`、`String.find_from` と、それらを使う std の関数。`Array.get`、`Array.set`、`Array.make`、`MutArray.read`、`MutArray.write`、`MutArray.make` (添字の範囲と負の長さ) と、添字の式が使う `check_index`。`Env.arg` の範囲の外の位置も `Index` だが、`Env.arg` は std の中だけで使うので、利用者のプログラムでは起きない |

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

- `map`、`filter`、`filter_map`、`each` は、要素の順に関数を呼ぶ。`fold f init xs` は左からたたむ (`f (f init x1) x2`)
- `append xs ys` は `xs` の後ろに `ys` を続ける
- どの関数も末尾再帰で書き、逆順に積んで最後に `reverse` する。長いリストでも VM の深さの上限に当たらないようにするためである

## `String`

| 関数 | 型 | 実装 |
|---|---|---|
| `byte_length` | `String -> Int` | extern |
| `byte_at` | `Int -> String -> <Panic> Int` | extern |
| `slice` | `Int -> Int -> String -> <Panic> String` | extern |
| `find` | `String -> String -> <Panic> Option Int` | eml |
| `contains`、`starts_with`、`ends_with` | `String -> String -> <Panic> Bool` | eml |
| `split` | `String -> String -> <Panic> List String` | eml |
| `lines` | `String -> <Panic> List String` | eml |
| `join` | `String -> List String -> String` | eml |
| `trim` | `String -> <Panic> String` | eml |
| `trim_line_end` | `String -> <Panic> String` | eml |
| `to_int` | `String -> <Panic> Option Int` | eml |

- `byte_length s` は UTF-8 のバイトの数である
- `byte_at i s` は位置 `i` のバイト (0〜255) である。`0 <= i < byte_length s` でなければ `Panic` を起こす
- `slice start end s` は、位置 `start` から `end` の手前までの部分の文字列である。`0 <= start <= end <= byte_length s` でないときと、`start` か `end` が文字の境界でないときは `Panic` を起こす。文字の境界は、文字列の端か、UTF-8 の続きのバイト (`0x80`〜`0xBF`) でない位置である
- `find needle s` は、`needle` が最初に現れる位置である。空の `needle` は `Some 0` である
- `contains needle s`、`starts_with prefix s`、`ends_with suffix s` は、名前のとおりの判定である。バイトで比べるので、文字の境界でない位置で比べても誤りにならない
- `split sep s` は、`sep` で区切った部分を順に返す。Rust の `str::split` と同じく、続いた区切りは空の部分を作り、`split "," ""` は `[""]` である。空の `sep` は区切らず `[s]` を返す
- `lines s` は、Rust の `str::lines` と同じである。行の終わりは `\n` か `\r\n` で、最後の行の終わりは省ける。`lines "a\r\nb\n"` は `["a", "b"]`、`lines "a\r"` は `["a\r"]`、`lines ""` は `[]` である
- `join sep xs` は、要素の間に `sep` を挟んでつなぐ
- `trim s` は、両端の ASCII の空白 (空白、`\t`、`\n`、`\x0B`、`\x0C`、`\r`) を除く。Unicode の空白は除かない
- `trim_line_end s` は、最後の行の終わり (`\n` か `\r\n`) を1つ除く。行の終わりで終わらなければ `s` をそのまま返し、`trim_line_end ""` は `""` である。`Stdin.read_line` と `Fs.read_line` が、読んだ行の終わりを除くのに使う
- `to_int s` は、Rust の `i64::from_str` と同じ形を読む。省略できる符号 `+` か `-` の後に、1つ以上の ASCII の数字が続く形である。空白は許さない。範囲を超えれば `None` で、`"-9223372036854775808"` は読める
- `byte_length` と `join` のほかの関数は、`byte_at`、`slice`、std の中だけの `find_from` か算術を使うので、型の上では `Panic` を起こしうる
- 部分文字列の照合は素朴な方法で、`needle` の長さと探す範囲の長さの積に比例する時間がかかる

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
- `is_ascii_space` は `' '` と、`'\t'` から `'\r'` (U+0009 から U+000D) で成り立つ。`Std.String` の空白の判定と同じ範囲である
- どの判定も、ASCII 以外の文字では偽である

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
- `Char` と `Byte` を直接変える関数はない。`Int` を経由する

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

## `Env`

| 関数 | 型 |
|---|---|
| `args` | `Unit -> <IO, Panic> List String` |
| `get` | `String -> <IO> Option String` |

- `args ()` は、上の「起動」の引数である。REPL では `[]` を返す
- `get name` は環境変数の値である。設定されていなければ `None`、空の値なら `Some ""` を返す。空の名前と、`=` か NUL を含む名前は、設定されていないとみなす
- UTF-8 でない引数や値は、読んだときに実行時エラーにする

## `Stdin`

| 関数 | 型 |
|---|---|
| `read_all` | `Unit -> <IO> String` |
| `read_line` | `Unit -> <IO, Panic> Option String` |

- `read_all ()` は、標準入力の残りをすべて読む
- `read_line ()` は1行を読み、行の終わり (`\n` か `\r\n`) を `String.trim_line_end` で除いて返す。EOF なら `None` を返す。行の終わりのない最後の行も1行として返す
- どちらも、読む前に標準出力を書き出す。読みは `Fs.read_all` と同じく、64 KiB ずつ読みながら割り込みと確保量の上限を見る。UTF-8 でない入力は実行時エラーにする
- REPL では、標準入力を REPL 自身が読むので、どちらも I/O の失敗 (`cannot read the standard input: not available`) にする

## `Fs`

| 関数 | 型 |
|---|---|
| `read_text` | `String -> <IO> String` |
| `write_text` | `String -> String -> <IO> Unit` |
| `read_line` | `File -> <IO, Panic> (File, Option String)` |

`open`、`read_all`、`close` と `File` は [エフェクトと handler](effects.md) の「組み込みの `IO`」にある。

- `read_text path` は、ファイル全体を文字列として読む。`open`、`read_all`、`close` で書いた関数である
- `write_text path text` は、ファイルを作るか切り詰めて、`text` を書く
- `read_line file` は、今の位置から1行を読み、行の終わり (`\n` か `\r\n`) を `String.trim_line_end` で除いて、同じ `File` と組にして返す。終わりなら `None` を返す。行の終わりのない最後の行も1行として返す。`Stdin.read_line` と同じである
- `read_line` は、`File` を持ったまま1行ずつ読む生産者を書くための関数で、後のバイト列とハンドルの項目でハンドルの API に置き換わりうる ([ロードマップ](../future/roadmap.md) の「バイト列とハンドル」)。std の中だけの extern `read_line_raw : File -> <IO> (File, String)` の上に書いてある。`read_line_raw` は行の終わりを含む1行を返し、終わりなら空の文字列を返す (上の「extern の境界」の番兵)。読み方は `read_all` と同じく `File` の `BufRead` から読み、64 KiB を超える行も、割り込みと確保量の上限を見ながら読む ([オブジェクトモデル](object-model.md) の「`File`」)
- 読めないときと UTF-8 でない行の誤りは、文言も `try_io` での捕まえ方も `read_all` と同じである ([LIR](lir.md) の「実行時エラー」)。誤りのときは `read_all` と同じく、`File` をそこで閉じる

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

`Yield`、`Fail`、`State` は、エフェクトとその handler を持つモジュールで、`Iter` は `Yield` の上に書いた引いて使うジェネレータである。エフェクトの型と操作を修飾せずに書くには、`import Yield (Yield(..))` のように import の一覧に入れる。一覧のない `import Yield` や import なしでは、`Yield.Yield Int`、`Yield.yield x` と修飾して書く ([モジュールと名前解決](modules.md) の「import」)。

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
- `to_option action` は、誤りを捨てて `None` を返す。捨てるので、誤りの型は `Unr` でなければならない。節で誤りを `_` に束縛するので、型検査が `Unr` の制約を付け、`Fail File` のような誤りの型は E3001 になる。新しい規則は要らない
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

- `Iter` は `once` の関数を持つので `Lin` である ([型と Kind](types.md) の「1回だけ呼べる関数の型」)。途中でやめるときは `drop` で捨てる。生産者が `File` を持っていれば、そこで閉じる
- コンストラクタ `Iter` も公開する。コンストラクタを隠す `pub data` (抽象型) がないためである
- `next it` は、次の要素と残りの `Iter` の組を返し、終わりなら `None` を返す
- `generate`、`from_list` と変換 (`map`、`filter`、`filter_map`、`take`、`zip`、`enumerate`) は、`next` を呼ぶまで何も動かさない。そのため、どれもエフェクトを起こさず、矢印に row が要らない。Python のジェネレータが `next` まで本体を動かさないのと同じである
- `generate producer` は、生産者を `Yield` の handler で包む。名前を `from` にしないのは、`from` が `handle … from …` のキーワードだからである。`yield` の節は、受け取った値と、継続 `k` を包んだ次の `Iter` の組を返し、`return` の節は `None` を返す。`yield_all` は逆向きの変換で、`Iter` の要素を順に `yield` する
- `take n it` は、n 個を渡した後の `next` で残りを `drop` して `None` を返す。n が 0 以下なら、最初の `next` で何も取り出さずに捨てる。`take` の `Iter` そのものを `drop` したときも、中の `Iter` は一緒に捨てられる。そのため、生産者の `File` が閉じるのは、渡し終えた次の `next` か `drop` の時点である
- `filter p it` は、要素を `p` に渡したうえで、その要素を返す。要素を2回使うので、要素の型は `Unr` でなければならない。型検査がこの制約を付け、`File` の `Iter` に `filter` を使うと E3001 になる。`map`、`filter_map`、`take`、`zip`、`enumerate` は要素を1回だけ使うので、`Lin` の要素にも使える
- `zip` は、どちらかが終わったら、もう一方の残りを `drop` する。右が先に終わったときは、左から引いた要素も `drop` する。`enumerate start` は、番号を `start` から数える。行番号のように1から数える場面が多いためである
- `each`、`fold`、`to_list` は、終わりまで `next` を呼ぶ。`fold` は左からたたみ、`to_list` は要素の順のリストを返す
- `next` の結果を専用の型にせず `Option` の組にするのは、利用者が覚える型を `Iter` だけにするためである (Rust の `Iterator::next`、OCaml の `Seq.uncons` と同じ形)。先頭の値を先に求める形 (`| Done | More a (once Unit -> <e> Iter <e> a)`) では、作っただけで生産者が最初の `yield` まで動く
- `Std.Iter` は `Std.Yield` を import し、`Std.Yield` は `Std.Iter` を知らない。両方向の変換 (`generate` と `yield_all`) を `Std.Iter` に置くので、依存は一方向で、push の形だけを使うプログラムは `Std.Iter` を読み込まない
- 多回の再開ができるジェネレータ (`multi` の `yield`) は用意しない。生産者が `yield` をまたいで `Lin` の値を持てなくなり、`File` を持つ生産者を書けないためである。要る例が出たら、`Yield` の意味を変えずに別のエフェクトとして足せる

`Iter` を使うときは、次の3つに注意する。

- 速さ: 末尾再帰の生産者を `generate` で引く形は、handler への特殊化 ([Core IR](core-ir.md) の「handler への特殊化」) で yield せず、融合 ([Core IR](core-ir.md) の「`con` の値の形についての特殊化」) で、`fold` で引く形と `map` を重ねた形は1要素ごとに何も確保しないループになる。`filter` を重ねた形では、残した要素ごとに4つの確保 (`Some`、組、`Iter`、閉包) が残る。末尾でない再帰の生産者 (木の走査など) は、1要素ごとに yield する ([実装の現在地](../implementation/status.md) の「最適化が扱わない形」)
- エフェクトの届く先: 生産者のエフェクトは、`Iter.generate` を呼んだ場所ではなく、`next` を呼んだ場所の handler に届く。そのため、`try_io` は `Iter.generate` ではなく、消費するループを包む (`try_io (fn () -> Iter.each f (Iter.generate producer))`)
- `multi` の操作: `yield` は `once` の操作なので、`Yield.for_each` の要素ごとの関数の中で `multi` の操作を起こすと E3006 になる。`yield` の継続を持ったまま、`multi` の操作をまたぐためである ([エフェクトと handler](effects.md) の「継続の多重度と持ち越し規則」)。多回の再開ができるジェネレータを用意しないことの帰結である

## 実行時エラー

標準ライブラリの extern の誤りの文言は次のとおりである。I/O の失敗の理由は `not found`、`permission denied`、`not available`、`I/O error` のどれかである。位置は extern を呼んだ所で、標準ライブラリの関数の中の extern なら `<std>/…` の位置になる。

I/O の失敗の一部は `try_io` で捕まえられる (上の「`try_io`」)。`byte_at` と `slice` の誤りと、`Std.Float` の変換と `to_fixed` の誤りと、配列の添字と長さの誤りは `Panic` で、`try_panic` で捕まえられる (上の「`try_panic`」)。捕まえなかった失敗は、この表の文言の実行時エラーになる。

| 失敗 | 文言 |
|---|---|
| `byte_at` の範囲の外 | ``string index 5 is out of range for length 3`` |
| `slice` の範囲の外 | ``string slice 2..9 is out of range for length 3`` |
| `slice` の境界 | ``string slice 1..3 is not on a character boundary`` |
| `Float.truncate`、`floor`、`ceil`、`round` の範囲の外 | `` float `1e+19` does not fit in `Int` `` (値は `show` の形) |
| `Float.to_fixed` の桁数 | `` `to_fixed` takes 0 to 100 digits, not 101 `` |
| `Char.of_int` の引数 | ``int `55296` is not a Unicode scalar value`` |
| `Byte.of_int` の引数 | ``int `256` does not fit in `Byte` `` |
| 配列の添字の範囲の外 (`get`、`set`、`read`、`write`) | ``array index 3 is out of range for length 3`` |
| 配列の負の長さ (`make`、`tabulate`) | ``array length -1 is negative`` |
| `Fs.write_text` | ``cannot write `out.txt`: permission denied`` |
| `Proc.run` の起動 | ``cannot run `rsync`: not found`` |
| 標準入力の読み | ``cannot read the standard input: not available`` |
| UTF-8 でない標準入力 | ``the standard input is not valid UTF-8`` |
| UTF-8 でない引数 | ``argument 2 is not valid UTF-8`` (`Env.args ()` のリストの 0 から数えた位置) |
| UTF-8 でない環境変数 | ``environment variable `LANG` is not valid UTF-8`` |

標準ライブラリの中だけで起きる誤り (`String.find_from` の開始位置、`Env.arg` の位置) の文言は、[LIR](lir.md) の「実行時エラー」にある。
