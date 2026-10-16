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

標準ライブラリの extern は小さい核にとどめ、便利な関数は eml で書く。extern の境界では、スカラー (`Int`、`Bool`、`Unit`)、文字列、`Foreign` だけを受け渡し、`List` と `Option` は eml の側で組む。`eml_rt` がデータの配置を知らないで済むようにするためである ([Core IR](core-ir.md) の「translate の配置」)。そのため、`pub` でない extern は番兵 (見つからない位置の -1、終わりの空の文字列など) を返すことがある。番兵は標準ライブラリの中に閉じ込め、利用者には `Option` で見せる。

文字の型がまだないので、文字列の位置は UTF-8 のバイトの位置 (`Int`) で表す。引数の順は、対象を最後に置く。`line |> String.split ","` と部分適用で使えるようにするためである。

## `Prelude`

- `eprintln : String -> <IO> Unit` は、標準エラーに1行を書く。C の stdio と同じく、標準出力は書き出さない。標準出力をまとめて書いているとき (端末でないとき) は、`eprintln` の行が先に出ることがある

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
    | io_failed kind message -> Err (IOError { kind = kind, message = message })
    | return x -> Ok x
```

`IOError` の `show` は、導出した `Show` がレコードに使う形と同じにする。導出では `display` を上書きできないので、手で書く。

- `try_io action` は `action ()` を呼び、IO の失敗がなければ `Ok` に包んで返す。IO の失敗があれば、そこで `action` の実行を止め、途中のフレームが持っていた値を後始末して `Err` を返す
- 捕まえるのは、いちばん内側の `try_io` である。`IO` は `mask` に入らないので、row の上の位置でなく、実行の時点で外側にある最も近い `try_io` が捕まえる。例外と同じ動きである。たとえば `wrap : (Unit -> <e> a) -> <IO | e> Result IOError a` が、受け取ったコールバックを `try_io` の中で呼ぶと、コールバックの IO の失敗も `wrap` の `try_io` が捕まえる。ほかのエフェクトなら `mask` がコールバックの操作を外側へ通すが、`IO` ではそうしない
- handler の節は handle の外側の `evv` で動く ([エフェクトと handler](effects.md) の「健全性」)。そのため、handle の本体の中にある `try_io` は、その handler の節の中で起きた IO の失敗を捕まえない。handle より外側の `try_io` が捕まえる
- `try_io` は、`exit`、書き出しの失敗、算術の誤り、位置の範囲の誤り、割り込み、確保量と深さの上限、検査モードのヒープの誤りを捕まえない

捕まえる失敗と `kind` は次のとおりである。`message` は、[LIR](lir.md) の「実行時エラー」の文言と1文字も違わない。

| `Fault` | 起こす関数 | `kind` |
|---|---|---|
| `Io { reason, .. }` | `Fs.open`、`Fs.read_all`、`Fs.read_text`、`Fs.write_text`、`Proc.run` | `reason` が `NotFound` なら `NotFound`、`PermissionDenied` なら `PermissionDenied`、`NotAvailable` なら `NotAvailable`、`Other` なら `Other` |
| `Stdin(reason)` | `Stdin.read_all`、`Stdin.read_line` | 同上 |
| `NotUtf8(_)` | `Fs.read_all`、`Fs.read_text`、`Stdin.read_all`、`Stdin.read_line`、`Env.args`、`Env.get` | `NotUtf8` |

書き出しの失敗 (`Output`) を捕まえないのは、標準出力をまとめて書くためである ([オブジェクトモデル](object-model.md) の「Host」)。書けなかったことが分かるのは、後でバッファがあふれた `println` か、読みや起動の前の書き出しか、プログラムの終わりの書き出しになる。そのため、`try_io (fn () -> println s)` は書けなくても `Ok` を返しうるし、関係のない別の `try_io` が失敗を受け取りうる。どこで捕まるかが決まらないので、実行時エラーのままにする。

`try_io` で捕まえなかった IO の失敗は、実行時エラーである。位置を付けて表示し、終了コード 1 で止まる。このときは後始末をしない。プロセスが終わるので、OS が開いたファイルを閉じる。

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

## `List`

| 関数 | 型 |
|---|---|
| `map` | `(a -> <e> b) -> List a -> <e> List b` |
| `filter` | `(a -> <e> Bool) -> List a -> <e> List a` |
| `filter_map` | `(a -> <e> Option b) -> List a -> <e> List b` |
| `fold` | `(b -> a -> <e> b) -> b -> List a -> <e> b` |
| `each` | `(a -> <e> Unit) -> List a -> <e> Unit` |
| `length` | `List a -> Int` |
| `reverse` | `List a -> List a` |
| `append` | `List a -> List a -> List a` |

- `map`、`filter`、`filter_map`、`each` は、要素の順に関数を呼ぶ。`fold f init xs` は左からたたむ (`f (f init x1) x2`)
- `append xs ys` は `xs` の後ろに `ys` を続ける
- どの関数も末尾再帰で書き、逆順に積んで最後に `reverse` する。長いリストでも VM の深さの上限に当たらないようにするためである

## `String`

| 関数 | 型 | 実装 |
|---|---|---|
| `byte_length` | `String -> Int` | extern |
| `byte_at` | `Int -> String -> Int` | extern |
| `slice` | `Int -> Int -> String -> String` | extern |
| `find` | `String -> String -> Option Int` | eml |
| `contains`、`starts_with`、`ends_with` | `String -> String -> Bool` | eml |
| `split` | `String -> String -> List String` | eml |
| `lines` | `String -> List String` | eml |
| `join` | `String -> List String -> String` | eml |
| `trim` | `String -> String` | eml |
| `to_int` | `String -> Option Int` | eml |

- `byte_length s` は UTF-8 のバイトの数である
- `byte_at i s` は位置 `i` のバイト (0〜255) である。`0 <= i < byte_length s` でなければ実行時エラーにする
- `slice start end s` は、位置 `start` から `end` の手前までの部分の文字列である。`0 <= start <= end <= byte_length s` でないときと、`start` か `end` が文字の境界でないときは実行時エラーにする。文字の境界は、文字列の端か、UTF-8 の続きのバイト (`0x80`〜`0xBF`) でない位置である
- `find needle s` は、`needle` が最初に現れる位置である。空の `needle` は `Some 0` である
- `contains needle s`、`starts_with prefix s`、`ends_with suffix s` は、名前のとおりの判定である。バイトで比べるので、文字の境界でない位置で比べても誤りにならない
- `split sep s` は、`sep` で区切った部分を順に返す。Rust の `str::split` と同じく、続いた区切りは空の部分を作り、`split "," ""` は `[""]` である。空の `sep` は区切らず `[s]` を返す
- `lines s` は、Rust の `str::lines` と同じである。行の終わりは `\n` か `\r\n` で、最後の行の終わりは省ける。`lines "a\r\nb\n"` は `["a", "b"]`、`lines "a\r"` は `["a\r"]`、`lines ""` は `[]` である
- `join sep xs` は、要素の間に `sep` を挟んでつなぐ
- `trim s` は、両端の ASCII の空白 (空白、`\t`、`\n`、`\x0B`、`\x0C`、`\r`) を除く。Unicode の空白は除かない
- `to_int s` は、Rust の `i64::from_str` と同じ形を読む。省略できる符号 `+` か `-` の後に、1つ以上の ASCII の数字が続く形である。空白は許さない。範囲を超えれば `None` で、`"-9223372036854775808"` は読める
- 部分文字列の照合は素朴な方法で、`needle` の長さと探す範囲の長さの積に比例する時間がかかる

## `Env`

| 関数 | 型 |
|---|---|
| `args` | `Unit -> <IO> List String` |
| `get` | `String -> <IO> Option String` |

- `args ()` は、上の「起動」の引数である。REPL では `[]` を返す
- `get name` は環境変数の値である。設定されていなければ `None`、空の値なら `Some ""` を返す。空の名前と、`=` か NUL を含む名前は、設定されていないとみなす
- UTF-8 でない引数や値は、読んだときに実行時エラーにする

## `Stdin`

| 関数 | 型 |
|---|---|
| `read_all` | `Unit -> <IO> String` |
| `read_line` | `Unit -> <IO> Option String` |

- `read_all ()` は、標準入力の残りをすべて読む
- `read_line ()` は1行を読み、行の終わり (`\n` か `\r\n`) を除いて返す。EOF なら `None` を返す。行の終わりのない最後の行も1行として返す
- どちらも、読む前に標準出力を書き出す。読みは `Fs.read_all` と同じく、64 KiB ずつ読みながら割り込みと確保量の上限を見る。UTF-8 でない入力は実行時エラーにする
- REPL では、標準入力を REPL 自身が読むので、どちらも I/O の失敗 (`cannot read the standard input: not available`) にする

## `Fs`

| 関数 | 型 |
|---|---|
| `read_text` | `String -> <IO> String` |
| `write_text` | `String -> String -> <IO> Unit` |

- `read_text path` は、ファイル全体を文字列として読む。`open`、`read_all`、`close` で書いた関数である
- `write_text path text` は、ファイルを作るか切り詰めて、`text` を書く

## `Proc`

| 関数 | 型 |
|---|---|
| `run` | `String -> List String -> <IO> Int` |

- `run program args` は、`PATH` から `program` を探して子のプロセスを起動し、終わるのを待って終了コードを返す。子はシェルを介さず、標準入出力と環境を受け継ぐ。シグナルで終わった子の終了コードは、シェルと同じく 128 + シグナルの番号にする
- 起動の前に標準出力を書き出す。子と eml の出力が順に並ぶようにするためである。eml が標準入力から先読みしてバッファに持っているバイトは、子には届かない
- NUL を含む引数は OS に渡せないので、起動の誤り (`I/O error`) にする
- Ctrl-C は同じプロセスグループの子にも届く。子が終わった後、割り込みが来ていれば、`run` はその場で止まる (終了コード 130)
- 起動に使う extern は、引数の数と各引数を、それぞれ NUL で終えて並べた1つの文字列を受ける。OS の引数は NUL を含めないので、NUL を含む引数があれば数が合わなくなり、起動の誤りとして見つかる

## 実行時エラー

標準ライブラリの extern の誤りの文言は次のとおりである。I/O の失敗の理由は `not found`、`permission denied`、`not available`、`I/O error` のどれかである。位置は extern を呼んだ所で、標準ライブラリの関数の中の extern なら `<std>/…` の位置になる。

I/O の失敗の一部は `try_io` で捕まえられる (上の「`try_io`」)。捕まえなかった失敗は、この表の文言の実行時エラーになる。

| 失敗 | 文言 |
|---|---|
| `byte_at` の範囲の外 | ``string index 5 is out of range for length 3`` |
| `slice` の範囲の外 | ``string slice 2..9 is out of range for length 3`` |
| `slice` の境界 | ``string slice 1..3 is not on a character boundary`` |
| `Fs.write_text` | ``cannot write `out.txt`: permission denied`` |
| `Proc.run` の起動 | ``cannot run `rsync`: not found`` |
| 標準入力の読み | ``cannot read the standard input: not available`` |
| UTF-8 でない標準入力 | ``the standard input is not valid UTF-8`` |
| UTF-8 でない引数 | ``argument 2 is not valid UTF-8`` (`Env.args ()` のリストの 0 から数えた位置) |
| UTF-8 でない環境変数 | ``environment variable `LANG` is not valid UTF-8`` |

標準ライブラリの中だけで起きる誤り (`String.find_from` の開始位置、`Env.arg` の位置) の文言は、[LIR](lir.md) の「実行時エラー」にある。
