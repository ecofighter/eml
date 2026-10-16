# S12a 起動と標準ライブラリの核の設計

## 目的と範囲

S12 (スクリプトの MVP) を S12a〜S12d の4つの作業に分け、その最初の S12a で、スクリプトを書くのに要る起動と標準ライブラリの核を入れる。

| 作業 | 中身 |
|---|---|
| S12a | 起動と標準ライブラリの核: `eml file.em args...`、`Env`、`Stdin`、`Fs` の読み書き、`Proc.run`、`eprintln`、`String` の核の extern と eml で書く `String` と `List` の関数、使う std のモジュールだけを読み込むこと |
| S12b | `try_io` と `exit`: 中断の経路、`IOError` の値、戻らない extern |
| S12c | ローカルの再帰関数: 構文、HIR、単相の推論、lambda lifting |
| S12d | 5本のスクリプト (wc、grep、ログの集計、CSV の変換、デプロイ手順) を UI テストとして書く |

S12a の方針は次の3つである。

- extern は最小の核にとどめる。便利な関数のうち、どのスクリプトも使う少数だけを `std/` に eml で書く。ほかはスクリプトの側で書き、その負担を S13 の材料にする
- extern の境界では、スカラー (`Int`、`Bool`、`Unit`)、文字列、`Foreign` だけを受け渡す。`List` と `Option` への包みは `std/` の eml が書く。`eml_rt` はデータの配置を知らないままで済む
- 文字の型がまだないので、文字列の位置はバイトの位置 (`Int`) で表す

範囲に入れないもの: `try_io` と `exit` (S12b)。I/O の失敗は今と同じく実行時エラーで止まる。ローカルの再帰関数 (S12c)。`replace`、並べ替え、`Option` と `Result` の補助関数、`Map`。コマンドリテラルと `Cmd` 型 (コマンドリテラルの段)。Unicode の空白や大文字小文字の表。速い部分文字列の照合。

ロードマップの S12 の「決めたこと」のうち、次の2つを変える。

- std の extern が名前的なデータ (`List`、`Option`、`Result`) を作るときは、extern の行がその型の正規の名前を持ち、translate がその配置を表に入れる、としていた。S12a では extern がデータを作らず、std の eml が包むので、この仕組みは要らない。extern の行は今と同じく配置を持たない
- Rust で実装した extern の標準ライブラリとして、`String` の `lines`、`split`、`trim`、`contains`、`replace`、`join`、`to_int` を入れる、としていた。S12a では、Rust の extern は `String` の核の4つにとどめ、ほかは eml で書く。`replace` は入れない

決めたことは、段の終わりに下の「文書」の場所へ移す。

## 利用者から見た動き

### 起動

```sh
eml wc.em -l input.txt          # eml run wc.em -l input.txt と同じ
eml run --debug-heap wc.em -l input.txt
./wc.em -l input.txt            # 1行目が #!/usr/bin/env eml で、実行できるファイル
```

- 最初の引数が `-` で始まらず、サブコマンドの名前 (`check`、`run`、`repl`、`help`) でもなければ、`run` として扱う。`run` という名前のファイルを動かすときは `eml run run` と書く。`-` で始まる最初の引数は今と同じく clap が読むので、`eml --verison` は使い方の誤り (終了コード 2) のままである
- ファイルより後ろの引数は、`-` で始まるものも含めてそのままスクリプトに渡す。`--debug-heap` はファイルより前にだけ書ける
- `Env.args ()` は、ファイルより後ろの引数を返す。`eml` とスクリプトのパスは含めない
- shebang は `#!/usr/bin/env eml` と書く。Linux の `env` は `#!` の行の残りを1つの引数として渡すので、`#!/usr/bin/env eml run` は動かない
- 終了コードは今と同じである (成功 0、診断か実行時エラー 1、使い方の誤り 2、割り込み 130)。スクリプトが終了コードを選ぶ `exit` は S12b で入れる

### 公開する API

引数の順は、対象を最後に置く。`line |> String.split ","` と部分適用で使えるようにするためである。

| モジュール | 関数 | 実装 |
|---|---|---|
| `Prelude` | `eprintln : String -> <IO> Unit` | extern |
| `List` | `map : (a -> <e> b) -> List a -> <e> List b` | eml |
| | `filter : (a -> <e> Bool) -> List a -> <e> List a` | eml |
| | `filter_map : (a -> <e> Option b) -> List a -> <e> List b` | eml |
| | `fold : (b -> a -> <e> b) -> b -> List a -> <e> b` | eml |
| | `each : (a -> <e> Unit) -> List a -> <e> Unit` | eml |
| | `length : List a -> Int` | eml |
| | `reverse : List a -> List a` | eml |
| | `append : List a -> List a -> List a` | eml |
| `String` | `byte_length : String -> Int` | extern |
| | `byte_at : Int -> String -> Int` | extern |
| | `slice : Int -> Int -> String -> String` | extern |
| | `find : String -> String -> Option Int` | eml |
| | `contains`、`starts_with`、`ends_with` : `String -> String -> Bool` | eml |
| | `split : String -> String -> List String` | eml |
| | `lines : String -> List String` | eml |
| | `join : String -> List String -> String` | eml |
| | `trim : String -> String` | eml |
| | `to_int : String -> Option Int` | eml |
| `Env` | `args : Unit -> <IO> List String` | eml (非公開の extern を包む) |
| | `get : String -> <IO> Option String` | eml (非公開の extern を包む) |
| `Stdin` | `read_all : Unit -> <IO> String` | extern |
| | `read_line : Unit -> <IO> Option String` | eml (非公開の extern を包む) |
| `Fs` | `read_text : String -> <IO> String` | eml (`open`、`read_all`、`close`) |
| | `write_text : String -> String -> <IO> Unit` (パス、内容) | extern |
| `Proc` | `run : String -> List String -> <IO> Int` (プログラム、引数) | eml (非公開の extern を包む) |

各関数の意味は次のとおりである。

- `List`
  - `map`、`filter`、`filter_map`、`each` は、要素の順に関数を呼ぶ。`fold f init xs` は左からたたむ (`f (f init x1) x2`)
  - `append xs ys` は `xs` の後ろに `ys` を続ける
  - どの関数も末尾再帰で書く (逆順に積んで最後に `reverse` する)。長いリストでも VM の深さの上限に当たらないようにするためである
- `String` の核
  - `byte_length s` は UTF-8 のバイトの数である
  - `byte_at i s` は位置 `i` のバイト (0〜255) である。`i` が `0 <= i < byte_length s` でなければ実行時エラーにする
  - `slice start end s` は、バイトの位置 `start` から `end` の手前までの部分の文字列である。`0 <= start <= end <= byte_length s` でないときと、`start` か `end` が文字の境界でないときは実行時エラーにする。文字の境界は、文字列の端か、UTF-8 の続きのバイト (`0x80`〜`0xBF`) でない位置である
- eml で書く `String` の関数
  - `find needle s` は、`needle` が最初に現れるバイトの位置である。空の `needle` は `Some 0` である
  - `contains needle s`、`starts_with prefix s`、`ends_with suffix s` は、名前のとおりの判定である
  - `split sep s` は、`sep` で区切った部分を順に返す。Rust の `str::split` と同じく、続いた区切りは空の部分を作り、`split "," ""` は `[""]` である。空の `sep` は区切らず `[s]` を返す
  - `lines s` は、今の Rust の `str::lines` と同じである。行の終わりは `\n` か `\r\n` で、最後の行の終わりは省ける。`lines "a\r\nb\n"` は `["a", "b"]`、`lines "a\r"` は `["a\r"]`、`lines ""` は `[]` である
  - `join sep xs` は、要素の間に `sep` を挟んでつなぐ
  - `trim s` は、両端の ASCII の空白 (空白、`\t`、`\n`、`\r`、`\x0B`、`\x0C`) を除く
  - `to_int s` は、Rust の `i64::from_str` と同じ形を読む。省略できる符号 `+` か `-` の後に、1つ以上の ASCII の数字が続く形である。空白は許さない。範囲を超えれば `None` で、`"-9223372036854775808"` は読める
- `Env`
  - `args ()` は上の「起動」の引数である。REPL では `[]` を返す
  - `get name` は環境変数の値である。設定されていなければ `None`、空の値なら `Some ""` を返す。空の名前と、`=` か NUL を含む名前は、設定されていないとみなす
  - UTF-8 でない引数や値は、読んだときに実行時エラーにする
- `Stdin`
  - `read_all ()` は、標準入力の残りをすべて読む
  - `read_line ()` は1行を読み、行の終わり (`\n` か `\r\n`) を除いて返す。EOF なら `None` を返す。行の終わりのない最後の行も1行として返す
  - どちらも、読む前に標準出力を flush する。UTF-8 でない入力は実行時エラーにする
  - REPL では、標準入力を REPL 自身が読むので、どちらも I/O の失敗 (`cannot read the standard input: not available`) にする。S12b からは `try_io` で捕まえられる
- `Fs`
  - `read_text path` は、ファイル全体を文字列として読む
  - `write_text path text` は、ファイルを作るか切り詰めて、`text` を書く
- `Proc`
  - `run program args` は、`PATH` から `program` を探して子のプロセスを起動し、終わるのを待って終了コードを返す。子はシェルを介さない。シグナルで終わった子の終了コードは、シェルと同じく 128 + シグナルの番号にする
  - 子の作業ディレクトリは、ファイルを開くときの根 (`eml run` ではプロセスの今のディレクトリ) である。子は標準入出力と環境を受け継ぐ。eml が標準入力から先読みしてバッファに持っているバイトは、子には届かない
  - 起動の前に標準出力を flush する。子と eml の出力が順に並ぶようにするためである
  - Ctrl-C は同じプロセスグループの子にも届く。子が終わった後、止める語が立っていれば、`Proc.run` はその場で割り込みとして止まる (終了コード 130)
- `eprintln` は標準エラーに1行を書く。C の stdio と同じく、標準出力を flush しない

### 実行時エラーの文言

新しい失敗は、今の `Fault` の文言の形に合わせる。I/O の失敗の理由は、今の `IoReason` (`not found`、`permission denied`、`I/O error`) に `not available` を足したものである。`not available` は、その実行で使えない入出力 (REPL の標準入力) を表す。S12b で `IOError` の値に写すので、ここで理由として足す。

| 失敗 | 文言 |
|---|---|
| `byte_at` の範囲の外 | ``string index 5 is out of range for length 3`` |
| `slice` の範囲の外 | ``string slice 2..9 is out of range for length 3`` |
| `slice` の境界 | ``string slice 1..3 is not on a character boundary`` |
| `find_from` の開始位置 (std の中だけで起きる) | ``string index 5 is out of range for length 3`` |
| `Env.arg` の範囲の外 (std の中だけで起きる) | ``argument index 3 is out of range for 2 arguments`` |
| `Fs.write_text` | ``cannot write `out.txt`: permission denied`` |
| `Proc.run` の起動 | ``cannot run `rsync`: not found`` |
| 標準入力の読み | ``cannot read the standard input: not available`` |
| UTF-8 でない標準入力 | ``the standard input is not valid UTF-8`` |
| UTF-8 でない引数 | ``argument 2 is not valid UTF-8`` (`Env.args ()` のリストの 0 から数えた位置) |
| UTF-8 でない環境変数 | ``environment variable `LANG` is not valid UTF-8`` |

### std の読み込み

今は、`std/` のすべてのモジュールを毎回読み込んで検査している。`bench/empty.em` の命令の数 (約2900万) のほとんどは、Prelude (約150行) の読み込みと検査である。S12a で std に数百行を足すと、すべての実行、UI テスト、REPL の入力ごとに、その分が増える。そのため、使うモジュールだけを読み込む。

- Prelude はいつも読み込む
- そのほかの std のモジュール `X` は、次のどれかのときだけ読み込む
  - 読み込んだモジュールが、`X` を import する (`import String`、`import Std.String as S`)
  - 読み込んだユーザーのモジュールが、パスが `X` の import を書く。ユーザーの根の `X.em` が import の先になる場合も読む。`import Fs as G` の後の `G.nope` に「`import Std.Fs as F` で標準ライブラリに届く」という help を今のまま付けるためである
  - 読み込んだユーザーのモジュールの構文に、`X` か `Std.X` が修飾子として現れる。位置によらず、構文木の修飾した名前のすべてを数える (式、型、パターン、row のエフェクト、handler の節の頭、クラスの制約、instance の頭、`deriving`、レコードを作る式、補間の穴の中など)。`Std.X` を数えるのは、E1031 の help (`write `Fs.open`, or `import Std.Fs as F``) を今のまま出すためである
- std のモジュール同士は import で明示するので、読み込む集合は import をたどって閉じる
- 修飾子は名前解決より前に構文木から拾う。ユーザーが自分の `String` を import していると、使わない std の `String` を読むことがあるが、名前解決は import を先に見るので結果は変わらない
- REPL のセッションでは、すべての入力 (今の入力と `:type` の問い合わせを含む) から拾う。そのため、ある入力で初めて使った std のモジュールは、その入力から読み込まれる
- モジュールの番号は、読み込んだ順にする。Prelude が 0、入口が 1 で、その後は import と修飾子をたどって見つけた順に、std のモジュールとユーザーのモジュールが混ざって並ぶ。今の「std のモジュールを埋め込みの順に並べてから、import したモジュール」という規則を置き換える。`ModuleId` はファイルを足したときに決まり、構文の診断がもう番号を持っているので、後から並べ替えられないためである
- 型の表示で同じ名前の型を修飾するかどうかは、読み込んだモジュールの型だけで決まる。そのため、利用者の `data File` は、`Fs` を読み込んだプログラムでだけ `Main.File` と書く。REPL では、後の入力で `Fs` を初めて使うと、前の入力の型の書き方が変わることがある。std の `pub` でない型 (`Proc.Cmd`) は、利用者が書けないので、名前の衝突に数えない

## 設計

### `std/`

- 新しいファイル: `List.em`、`String.em`、`Env.em`、`Stdin.em`、`Proc.em`。`eml_hir::STD` の表に足す (Prelude が先頭の規則は変えない)
- `Prelude.em` に `pub extern eprintln : String -> <IO> Unit` を足す
- `Fs.em` に `read_text` (eml) と `pub extern write_text` を足す
- 非公開の extern (`pub` を付けない。`Prelude` の `negate` と同じく、今の検査で書ける)
  - `String.find_from : String -> Int -> String -> Int` (探す文字列、開始位置、対象。なければ -1)。`find`、`contains`、`split` などは、これと `slice` で書く
  - `Env.arg_count : Unit -> <IO> Int`、`Env.arg : Int -> <IO> String`、`Env.has : String -> <IO> Bool`、`Env.var : String -> <IO> String`
  - `Stdin.read_line_raw : Unit -> <IO> String` (行の終わりを含む1行。EOF で `""`)
  - `Proc.command : String -> Cmd`、`Proc.add_arg : String -> Cmd -> Cmd`、`Proc.status : Cmd -> <IO> Int`。`Cmd` は `extern data Cmd` (`pub` を付けない) で、`Lin` の `Foreign` である
- 番兵 (-1、`""`、`Env.has` と `Env.var` の組) は std の中に閉じ込め、利用者には `Option` で見せる

### `eml_extern`

- 型の行: `Std.Proc.Cmd` (`Lin`、`Obj`)
- 関数の行 (`Repr` と Kind と純粋さ)

| 行 | params | ret | 純粋さ |
|---|---|---|---|
| `Prelude.eprintln` | `obj` | `unit` | `Effectful` |
| `Std.String.byte_length` | `obj` | `int` | `Pure` |
| `Std.String.byte_at` | `int`、`obj` | `int` | `MayFail` |
| `Std.String.slice` | `int`、`int`、`obj` | `obj` | `MayFail` |
| `Std.String.find_from` | `obj`、`int`、`obj` | `int` | `MayFail` |
| `Std.Env.arg_count` | `unit` | `int` | `Effectful` |
| `Std.Env.arg` | `int` | `obj` | `Effectful` |
| `Std.Env.has` | `obj` | `enum` | `Effectful` |
| `Std.Env.var` | `obj` | `obj` | `Effectful` |
| `Std.Stdin.read_all` | `unit` | `obj` | `Effectful` |
| `Std.Stdin.read_line_raw` | `unit` | `obj` | `Effectful` |
| `Std.Fs.write_text` | `obj`、`obj` | `unit` | `Effectful` |
| `Std.Proc.command` | `obj` | `obj` (`Lin`) | `Pure` |
| `Std.Proc.add_arg` | `obj`、`obj` (`Lin`) | `obj` (`Lin`) | `Pure` |
| `Std.Proc.status` | `obj` (`Lin`) | `int` | `Effectful` |

- `find_from` が `MayFail` なのは、開始位置が範囲の外で失敗するためである (下の `eml_rt`)
- `command` と `add_arg` は I/O をしないので `Pure` である。`Pure` を見るのは使われない `let` を消す縮約だけで、`Lin` の結果は必ず使われるので消えない

### `eml_hir`

#### 読み込み

- `Loader` が std の表を持ち、std のモジュールを要るときに読む。`finish` の幅優先のたどりの中で、モジュールを処理するときに、上の「std の読み込み」の規則で要る std のモジュールを見つけて、まだ読んでいなければ足す
- import の行き先の規則 (`user_target` と `std_target`) は今と同じで、std のモジュールを前もって読んでおく代わりに、その場で読む。ユーザーの根に `Fs.em` がない `import Fs` は、今と同じく std の `Fs` を指す
- 修飾子の名前は、各ユーザーのモジュールの構文木の修飾した名前の節点をすべて歩いて集める。def_map の `std_short_names` と同じく、`Std.` を除いた短い名前で std のモジュールを引く
- `load_with_std` の前もってすべての std のモジュールを足す処理 (`add_std_modules`) と、それに伴う `load.rs` の注記を消す。`Loaded` の番号の順の説明 (`load.rs` の `Loaded` の doc) と modules.md を、読み込んだ順の規則に直す

#### extern の索引

今の `extern_index` は、`ExternType::ALL` のすべての行を読み込んだ std から引き、見つからなければ panic する。使うモジュールだけを読むと、`Fs` を使わないプログラムに `Std.Fs.File` がない。

- `ExternIndex` の型の索引は、読み込んだモジュールが宣言する行だけを持つ。`ExternIndex::ty` は `Option<TypeDefId>` を返す
- 行の正式な名前が `Prelude.` で始まる型 (`Int`、`String`、`Unit`) と、`io`、`negate` は、Prelude がいつも読まれるので、見つからなければ今と同じく panic する
- 処理系の中で `ExternType::File` と `ExternType::Cmd` を `ty` で引く所はない。テストが引く所は、`Fs` を使うプログラムで引く (下の「テストの変更」)

#### 型の表示

- `display_names` の名前の衝突の数えから、std のモジュールの `pub` でない item を除く

### `eml_rt`

#### 文字列の核

`heap/ctx/strings.rs` に `Ctx` のメソッドを4つ足し、`verus!` の中で結果まで証明する。引数の文字列は今の extern と同じく消費する。

| メソッド | 証明すること |
|---|---|
| `string_byte_length(s) -> i64` | 結果がゴーストのバイト列の長さと等しい |
| `string_byte_at(i, s) -> Result<i64, Fault>` | 範囲の中なら結果がその位置のゴーストのバイトと等しく、外なら `Fault::Index` |
| `string_slice(start, end, s) -> Result<Word, Fault>` | 結果のゴーストのバイト列が部分の列と等しい。境界の条件は vstd の `is_char_boundary` で書き、結果の UTF-8 の妥当性を保つ |
| `string_find_from(needle, from, s) -> Result<i64, Fault>` | `0 <= from <= len` でなければ `Fault::Index`。結果は `from` 以上で `needle` と一致する最小の位置で、なければ -1。空の `needle` は `from` |

- `string_slice` は、`String.show` と同じ道 (`alloc_string_room` で場所を取り、`append_part` で足す) で新しい文字列を作る。ヒープの文字列から借りた `&str` を持ったまま `&mut` の `alloc_string` は呼べないためである。部分の UTF-8 の妥当性は、今ある `lemma_boundary_in_prefix` の考え方で示す。一意な文字列の切り詰めなど、その場の書き換えはしない
- `string_find_from` は素朴な照合 (O(nm)) である
- 期待値の表 (`heap/tests/expected.txt`) に、4つの結果の行を足す

#### `Fault`

- `Fault` は今と同じく 24 バイトに収める (`heap/tests/externs.rs` が確かめ、[オブジェクトモデル](../../spec/object-model.md) にも書いてある)。入りきらない中身は `Box` に入れる
- `Fault::Index` を足す。中身の種類は4つある: 文字列の位置と長さ (`byte_at`、`find_from`)、切り出しの範囲と長さ (`slice` の範囲の外)、切り出しの範囲 (`slice` の境界)、引数の位置と数 (`Env.arg`)。中身は `Box` に入れる
- `Fault::Io` の `path` を、何の入出力かを表す値に替える: ファイルのパス、プログラムの名前、標準入力。`IoOp` に `Write`、`Run` を足す。文言は上の「実行時エラーの文言」である
- `Fault::NotUtf8` の `path` も、ファイル、標準入力、引数の位置、環境変数の名前を表す値に替える
- `IoReason` に `NotAvailable` を足す。Host の `io::ErrorKind::Unsupported` をこれに写す

#### Host

`Host` の trait (`verus!` の外の安全な glue) にメソッドを足す。

| メソッド | 使う extern |
|---|---|
| `args(&self) -> &[OsString]` | `Env.arg_count`、`Env.arg` |
| `env_var(&self, name: &str) -> Option<OsString>` | `Env.has`、`Env.var` |
| `stdin(&self) -> io::Result<StdinLock<'_>>` | `Stdin.read_all`、`Stdin.read_line_raw` |
| `write_file(&self, path: &str, text: &str) -> io::Result<()>` | `Fs.write_text` |
| `run(&self, program: &str, args: &[Box<str>]) -> io::Result<i32>` | `Proc.status` |

- `StdinLock<'_>` は、標準入力の読み手 (`Box<dyn BufRead + Send>`) を守る `MutexGuard` の別名である。読み手を Host が持ち、Host のメソッドは `&self` を受けるため、読む間だけ排他で貸す。標準入力を使えない Host は `Unsupported` の誤りを返す
- 引数は Host を作るときに渡す。`StdHost` は本物の環境を読み、空の名前と `=` か NUL を含む名前には `None` を返す (`std::env::var_os` が panic しうるため)
- `run` は、起動の前に標準出力を flush し、子の作業ディレクトリを Host のファイルの根にする。シグナルで終わった子は 128 + 番号を返す
- `StopOnInterrupt` に `BufRead` を足す。止める語が立った後の `EINTR` を止める誤りに変えるのは、今の `Read` と同じである。`read_line_raw` は std の `read_until` を使わず、`fill_buf` と `consume` で `\n` まで読む。std の `read_until` は `Interrupted` を自分で読み直すので、止める語を見られないためである
- 標準入力の読みは、`Fs.read_all` の読み (`read_text`) と同じく、64 KiB ずつ読みながら `Control` を見て、確保量に数える。`read_line_raw` も1行が長いときに同じく数える
- 読む前の flush は、今の出力のバッファの規則 ([オブジェクトモデル](../../spec/object-model.md) の「出力のバッファ」にある予定) を実装する
- `proc_status` は、Host の `run` が戻った後に止める語を見て、立っていれば `Fault::Stopped(Interrupted)` を返す

#### 信頼の一覧 (T11)

T11 の行は、`verus!` の中で Host を呼ぶ `external_body` の橋と、中身の見えない型である。次の行を足し、`trust.rs` の一覧に載せる。

- 引数を読む橋、環境変数を読む橋、標準入力の全体を読む橋、標準入力の1行を読む橋、ファイルを書く橋、子を起動する橋
- `Cmd` の `Foreign` の値 (`{ program: Box<str>, args: Vec<Box<str>> }`) の型と、それを作る処理と引数を足す処理

`proc_command` が lin ビット付きで `Cmd` を確保し、`proc_add_arg` が `foreign_unique` で引数を足して同じ語を返し、`proc_status` が子を起動する橋を呼んで `Cmd` を解放する。新しい行ごとに、`heap/tests/trusted.rs` に Miri のテストを足す。メモリの Host (`heap/tests/host.rs`) は、引数、環境、標準入力、書いたファイルの表、決まった終了コードを返す `run` を持つ形に広げる。

### `eml_vm`

- `extern_call` に新しい行を足し、`compile/emit.rs` の算術でない extern の一覧に足す
- テスト用の Host (`capture_host`) を組み立て式にする。`CaptureHost::new(file_root)` は、引数なし、空の標準入力、空の環境で始まる。引数 (`with_args`)、標準入力の内容 (`with_stdin`)、標準入力なし (`without_stdin`。読むと `not available`)、環境変数の表 (`with_env`) を渡せる
  - 環境は与えた表だけで、本物の環境は読まない
  - `write_file` はディスクに書かず、メモリの表に書く。同じパスを後で `open` すると、表の内容を読む。書いたファイルの一覧と内容は `Capture` から読める
  - `run` は本物の子を起動する。子の作業ディレクトリはファイルの根、環境は与えた表に本物の `PATH` を足したもの、標準入力は空 (`Stdio::null`) である。子の標準出力はパイプで受けて、捕まえた出力に足す。子の標準エラーは、今の eml の標準エラーと同じく捨てる
- `RunConfig` は変えない。引数と標準入力は Host が持つ

### `eml_cli`

- `main.rs`: 最初の引数が `-` で始まらず、サブコマンドの名前でもなければ、`run` を前に補ってから clap に渡す。`Run` は、ファイルの後ろに残りの引数 (`trailing_var_arg`、`allow_hyphen_values`) を受ける
- `stdio_host` は引数と、標準入力を使うかどうかを受けて `StdHost` を作る
- REPL の Host は、`eml repl` の端末 (`terminal.rs`) でも REPL のトランスクリプトのテスト (`tests/repl.rs`) でも、引数が空で、標準入力なしで作る。Host を作る側がこれを守る。`Repl` は渡された Host をそのまま使う

## テスト

### テストの仕組み

- UI テストの入力は、テストの隣の付属のファイルで与える。単独のファイルのテスト `<name>.em` には `<name>.args`、`<name>.stdin`、`<name>.env`、ディレクトリのテスト `<name>/main.em` には `main.args`、`main.stdin`、`main.env` を置く
  - `.args` は1行に1つの引数、`.stdin` はそのままのバイト列、`.env` は `KEY=VALUE` の行である。`.args` では、改行を含む引数と、最後の空の引数は書けない。要れば CLI のテストで確かめる
  - glob は `.em` だけを拾うので、今の置き場所の検査は付属のファイルを見ない。付属のファイルはどれも省ける。省くと、引数なし、空の標準入力、空の環境である
- UI テストのスナップショットに、`Fs.write_text` で書いたファイルの一覧と内容を足す。書いたファイルがなければ何も足さない。速いモードでも、書いたファイルが検査モードと同じであることを確かめる
- `eml_test_support` の `execute` などは、今と同じく既定の `CaptureHost` を使う。引数などを渡すテストのために、Host を受ける形を足す

### 足すテスト

- `eml_rt`
  - 文字列の核の4つを `both_modes!` で確かめる: 範囲の端、空の文字列、多バイトの文字の中の位置、`find_from` の空の `needle` と開始位置の端
  - 期待値の表の行
  - 新しい T11 の行の Miri テスト (`trusted::`)
- `eml_hir`: std の読み込み。修飾子で読む、`Std.X` の修飾子で読む、型、パターン、row、クラスの制約、補間の穴の中の修飾子で読む、import で読む、ユーザーの `X.em` が勝つ `import X` でも std の `X` を読む、std の中の import で推移的に読む、使わないモジュールは読まない、読み込んだ順の番号、REPL のセッションの前の入力と今の入力の修飾子で読む
- 結合テスト: extern の行と `std/` の宣言の結び付け (`eml_hir/tests/externs.rs`)、行の Repr と std のシグネチャの Repr (`eml_core_ir/tests/externs.rs`)。入口のプログラムが、すべての std のモジュールを修飾子で使うようにする。`Cmd` の型は、利用者が書けないので、`Std.Proc.command` の宣言の結果の型から引く
- UI テスト (`run/std/`)
  - `List` の各関数 (効果のある関数を渡したときの呼ぶ順を含む)、`length` が長いリスト (10万要素) で深さの上限に当たらないこと
  - `String` の各関数 (多バイトの文字、空の文字列、`split` の端の場合、`lines` の `\r\n` と最後の `\r`、`to_int` の範囲の端と不正な形)
  - `Env.args` と `Env.get` (`.args` と `.env`)、`Stdin.read_all` と `Stdin.read_line` (`.stdin`。最後の行に行の終わりがない場合を含む)
  - `Fs.read_text` と `Fs.write_text` (書いた後に読み直す)
  - `Proc.run` (`sh -c 'echo hi; exit 3'` の出力と終了コード、出力の順)
- UI テスト (`run-fail/std/`): `byte_at` と `slice` の範囲の外、`slice` の境界、`Proc.run` の存在しないプログラム、`Fs.write_text` の書けないパス、UTF-8 でない標準入力
- `eml_cli/tests/cli.rs`
  - `eml file.em a -b --c` がすべての引数を渡すこと
  - `eml run --debug-heap file.em a` が引数を渡すこと
  - サブコマンド名とファイル名の読み分け (`eml check x.em` は検査だけ、`eml x.em` は実行、`eml --verison` は終了コード 2)
  - shebang の行を持つ実行できるファイルを直接動かせること (`cfg(unix)`)
  - 標準入力をパイプで渡したスクリプトが `Stdin.read_all` で読めること
  - `eprintln` が標準エラーに書くこと
  - UTF-8 でない引数と環境変数の実行時エラー (`cfg(unix)`)
  - 待っている `Stdin.read_line` と `Proc.run` の子 (`sleep`) を SIGINT で止めると、終了コード 130 で終わること (`cfg(unix)`)
- REPL のトランスクリプト
  - `Env.args ()` が `[]` を返すこと、`Stdin.read_all ()` が `not available` の実行時エラーになり、セッションが続くこと
  - 後の入力で初めて std のモジュールを使うこと (番号が変わっても、前の入力の定義と値がそのまま使えること)

## テストの変更

- 成否の変更: なし
- 期待値の変更
  - 使わない std のモジュールを読まなくなり、番号が読み込んだ順になることによる、読み込んだモジュールの一覧、モジュールの番号、段階のダンプの差分。`eml_hir` の `load.rs`、`def_map.rs`、`session.rs` などのスナップショットが当たる
  - `eml_hir/tests/lower.rs` の `operator_definitions_and_qualified_names`: `List.length` が `Std.List` の関数に届くので、今の `unknown module qualifier `List`` (E1031) の診断が変わる。修飾子を、std にない名前に替えて今の意図 (未知の修飾子の診断) を保つ
  - 入口が `Fs` を使わないと意図が成り立たなくなるテストの入力を、`Fs` を使う形に書き直す: `load.rs` の `a_std_module_reaches_another_only_through_an_import`、`def_map.rs` の `Main.Tag` と `Std.Fs.Tag` の衝突のテスト。`eml_types` の `#[cfg(test)]` のテストで `ExternType::File` を引くもの (`store.rs`、`kind/mod.rs`) も、`Fs` を使うプログラムにする
  - extern の行を足すことで値が増える表: `trust.rs` の `TRUSTED`、`heap/tests/expected.txt`、`eml_core_ir/tests/externs.rs` の Kind の表
  - UI テストの診断の文言は変わらない (E1031 の help は `Std.Fs` の修飾子で `Fs` を読むので今のまま)
- 機械的な追随: extern の表に行を足すことによる、行を数えるだけのテストの追随

## 文書

段の終わりに、決めたことを次の場所へ移す。

- [モジュールと名前解決](../../spec/modules.md) の「標準ライブラリ」と「REPL のセッション」: std のモジュールの一覧、使うモジュールだけを読み込む規則、読み込んだ順の番号
- 新しい文書 `docs/spec/stdlib.md`: 上の「公開する API」の関数と意味。`docs/future/stdlib.md` (申し送り) の S12 の項目 (`eprintln`、文字列の関数、`Fs` の読み書き、分割と検索と反復を Rust の extern で入れること) を、この文書への参照に置き換える
- [オブジェクトモデル](../../spec/object-model.md): 文字列の核の extern、Host のメソッド、`Cmd` の `Foreign`、T11 の行、`Fault` の形 (`Index`、入出力の対象、`NotAvailable`)、標準入力の前の flush、止める語を見る extern に `Stdin` と `Proc.status` を足すこと
- [LIR](../../spec/lir.md) の「実行時エラー」: 新しい文言 (`Fault` の表示と同じ文字列)
- [VM](../../spec/vm.md) の「上限と割り込み」: extern の中で止まるのが `Fs.read_all` だけでなくなること
- [Core IR](../../spec/core-ir.md) の「translate の配置」: extern がデータを作らない方針 (S12 で行が型の正規の名前を持つ、という記述を消す)
- [REPL の端末](../../spec/repl.md): REPL の `Env.args` と標準入力
- [テスト戦略](../../implementation/testing.md) の「UI テスト」: 付属のファイルと、書いたファイルのスナップショット
- [プログラム例](../../spec/examples.md): shebang を `#!/usr/bin/env eml` に直す
- [ロードマップ](../../future/roadmap.md) の S12: S12a〜S12d の表、S12a で変えた「決めたこと」の2つ、S12a で閉じた論点 (`Stdin` は入力全体と行を読む関数にとどめる。extern が閉包を呼び戻す形は S12a では要らない。多相な extern の行も要らない)
- [実装の現在地](../../implementation/status.md) と [コンパイラの構成](../../implementation/architecture.md) は、S12 の終わりに1回で直す。S12a の終わりには、`CLAUDE.md` の該当する記述 (std の読み込みと番号、extern の数、Host、CLI) を直す
- `bench/run.sh` の結果を [計測の基準](../../implementation/benchmarks.md) に記録する。`empty` の命令の数は、`Fs` を読まなくなる分だけ減る見込みである
