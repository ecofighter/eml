# S11c-2 `:load` / `:reload` / `:type` の設計

## 目的と範囲

S11c-1 の端末の REPL に、前端に触れる3つのコマンドを足す。

- `:load path`: セッションを捨てて新しく始め、ファイルを最初の入力として読む
- `:reload`: 覚えたファイルで `:load` をやり直す
- `:type e`: 式の型を、実行せずに表示する

あわせて、`eml repl [--debug-heap] [file]` でファイルを読んで始められるようにする。

範囲に入れないもの: `Show` を持たない型の表示器 (S11c-3)。`:load` で複数のファイルを読むこと。`:type` で式を評価したときのエフェクトの row を見せること。`:cd`、`:browse`、`:info` などのほかのコマンド。

決めたことは、`docs/spec/repl.md` (「REPL の端末」) と関係する仕様の文書に移す (下の「文書」)。

## 利用者から見た動き

### コマンドの表

コマンドの表を `quit`、`help`、`load`、`reload`、`type` の順にする。接頭辞は今と同じく表の順に引くので、`:l` は `:load`、`:r` は `:reload`、`:t` は `:type` になる。GHCi と同じである。

コマンドの引数は、名前の後の空白を除いた行の残りすべてである (後ろの空白も除く)。空白を含むパスもそのまま書ける。

| コマンド | 引数 | 引数の誤り |
|---|---|---|
| `:quit` | なし | 引数があれば `':quit' takes no argument` (今と同じ) |
| `:help` | なし | 同上 |
| `:load` | パス (省けば空のセッションに戻す) | なし |
| `:reload` | なし | 引数があれば `':reload' takes no argument` |
| `:type` | 式 | 引数がなければ `':type' needs an expression` |

`:help` の一覧に3つのコマンドを足す。

### `:load path`

1. パスの綴りを決める。`eml run` と同じく、大文字小文字を区別しないファイルシステムでは、ディレクトリの一覧にある綴りに直す (`main.rs` の `exact_spelling` を使う)。この綴りを表示のパスにする
2. ファイルを読む。相対パスはカレントディレクトリから読む。読めなければ、標準エラーに ``error: cannot read `path`: {error}`` を書く。セッションは変えず、覚えたファイルも変えない
3. 読めたら、今のセッションを閉じる (`Repl::close`)。検査モードのリークか根を手放すときの誤りがあれば、標準エラーに `runtime error: {error}` を書いて続ける。その誤りは覚えておき、REPL が終わるときの終了コードを 1 にする (「終わり方」の `close` の誤りと同じ扱い)
4. 新しいセッションを作り、ファイルをその最初の入力 (入力 1) として評価する (`Repl::with_file`)。ファイルの文法で読み、診断と実行時エラーの位置は表示のパスで見せる。`main` は実行しない
5. import の根は、ファイルのあるディレクトリにする。`eml run` が入口のファイルのディレクトリを根にするのと同じである。Host の相対パスの基準はカレントディレクトリのままにする (`eml run` と同じ)
6. 結果は普通の入力と同じく表示する。受け付けたときは何も書かない。受け付けた入力が何も書かないのと同じである
7. 結果によらず、そのファイルを覚えて `:reload` の対象にする。ファイルに誤りがあるときや、前端の検査の間に割り込まれたときは、診断か `interrupted` を見せ、セッションは空のままにする。直してから `:reload` で読み直せる。GHCi と同じ扱いである。空かコメントだけのファイルも、セッションは空のままである (`Outcome::Empty`)

`:load` だけ (引数なし) は、今のセッションを 3. と同じく閉じて空のセッションを作り、覚えたファイルを忘れる。import の根はカレントディレクトリに戻す。

ファイルのトップレベルの item は、後の入力からすべて見える。`pub` は見ない。既存の「REPL のセッション」の規則 (前の入力のトップレベルを `pub` を見ずに引く) がそのまま当たるためで、GHCi の `*Main` のスコープと同じである。後の入力は、ファイルの item を普通の前の入力の item と同じく定義し直せる。古い版は `Shape#1` と表示する。ファイルの instance は、前の入力の instance と同じく、後の入力で定義し直せない (E1035)。

ファイルは、`eml run` の入口のファイルと同じく import できない。ファイルの依存先が import しても、後の入力が import しても E1030 になる。ファイルを受け付けなかったセッション (誤り、割り込み、空のファイル) でも同じである。診断の文言は `eml run` の入口のファイルのもの (`the entry module cannot be imported`) をそのまま使う。`:load` のファイルは REPL の入口のファイルだからである。

### `:reload`

覚えたファイルで `:load` の 1. から 7. をやり直す。ディスクからファイルを読み直し、新しいセッションの import のキャッシュは空なので、import したファイルも読み直す。

- 覚えたファイルがなければ、標準エラーに `no file to reload; use ':load'` を書き、セッションを変えない。GHCi はこのときもセッションを空にするが、打ち間違えた `:r` で `let` の束縛が消えないようにする
- 読めなければ (ファイルを消したときなど)、`:load` と同じく誤りを書き、セッションと覚えたファイルを変えない

### `:type e`

- 引数を1つの式として読む。入力の読み分け (item か `let` か式か) はしないので、`f x = 1` を渡すと構文の誤り (`':type' takes one expression`) になる
- 今のセッションに対して型を検査する。lowering も実行もせず、入力を受け付けず、入力の番号も進めない
- 誤りがなければ、標準出力に `e : T` を書く。`e` は引数の前後の空白を除いたテキストである。診断は普通の入力と同じく標準エラーに書く (警告は、受け付けた入力までに読んでいないファイルのものだけ)。誤りが1つでもあれば型を書かない
- 前端の検査の間に来た割り込みは、ほかの入力と同じく扱う。型を書かずに `interrupted` を書き、同じ読みで届いた残りの行を捨てる (「割り込み」)。`:type` も、`eval` と同じく始めに `Control::clear` を呼ぶので、入力を待つ間に来た割り込みでは止まらない

型 `T` は次のように書く。

- 式の入力と違い、型に現れる型変数を `Unit` に決めない (defaulting しない)
- 型に現れる型変数は、書いた型を左から読んで最初に現れた順に `a`、`b`、`c`、`d`、`f`、…、`z`、`a1`、`b1`、… と名付ける。`e` で始まる名前は、番号が付いても型変数に使わない。row 変数の名前と重ねないためである
- row 変数は、書いた型 (文脈を含む) の中に2回以上現れるものだけを、左から最初に現れた順に `e`、`e1`、`e2`、… と名付ける。1回だけ現れる row 変数は閉じた row として書く (row がラベルを持たなければ省く)。入力の row の defaulting と同じ扱いで、`fn x -> x` を `a -> <e> a` でなく `a -> a` と見せるためである。2つの矢印が同じ row 変数を持つ `(>>)` は `(a -> <e> b) -> (b -> <e> c) -> a -> <e> c` になる。数えるのは書いた型の木の上で、同じ部分の型が2か所に現れれば、その中の row 変数も2回と数える
- 型に現れる型変数に残った制約は文脈にする。1つなら `Show a => …`、2つ以上なら `(Eq a, Show b) => …` と書く。制約は、型変数が現れた順に並べ、同じ型変数の中ではクラスの表示の名前の順 (同じなら定義の順) に並べる。同じ制約は1つにし、ほかの制約の上位クラスでたどれる制約は書かない (`Ord a` があれば `Eq a` を書かない)
- 型に現れない型変数に残った制約は、式の入力と同じく、その型変数を `Unit` に決めて解く。表示する型を変えないので、上の「defaulting しない」とは食い違わない。`Unit` で解けなければ E2009 にする。そのため `:type show []` は `String` を返し、`:type name (make ())` (`Unit` の instance のないクラス) は E2009 になる。型変数でない型に残った制約は、今と同じく E2006 にする
- Kind (矢印の線形性) は書かない。表面の型の構文に Kind はないためである
- 型と制約の名前は、診断と同じ表示 (`DisplayNames`) で書く。定義し直した item の古い版は `Shape#1` になる

式の入力の規則のうち、次の3つは `:type` に当てない。

- row が `<IO>` に収まらなければ E2015 にする規則。`:type ask ()` は `Int` を返す
- 型が `Unr` でなければ E2016 にする規則。`:type Fs.open "x"` は `File` を返す
- 型に現れる型変数を `Unit` に決める規則 (上のとおり名前を付け、制約は文脈にする)

本体の線形性の検査、網羅性の検査、ほかの型の誤りは、普通の式と同じく報告する。

例:

```
> :type (>>)
(>>) : (a -> <e> b) -> (b -> <e> c) -> a -> <e> c
> :type fn x -> x
fn x -> x : a -> a
> :type []
[] : List a
> :type show []
show [] : String
> :type fn x y -> x < y && x == y
fn x y -> x < y && x == y : Ord a => a -> a -> Bool
> :type println "hi"
println "hi" : Unit
```

### コマンドライン

`eml repl [--debug-heap] [file]` にする。`file` を渡すと、最初に `:load file` と同じことをする。ただし `file` を読めなければ、`eml run` と同じく使い方の誤り (``error: cannot read `file`: …``、終了コード 2) にする。ファイルに型の誤りなどがあるときは、診断を見せて空のセッションで続ける。

## 設計

### lib API (`eml_cli::Repl`)

```rust
impl Repl {
    /// ファイルを最初の入力とする新しいセッションを作り、ファイルを評価する。`path` は表示のパスで、import の根は
    /// その最後の `/` までである。`source` はそのディレクトリを読む。
    pub fn with_file(
        config: &RunConfig,
        path: &str,
        text: &str,
        source: Box<dyn ModuleSource + Send>,
        host: HostRef,
    ) -> (Repl, Evaluated);

    /// `text` を問い合わせの入力としてセッションに対して検査し、型を返す。lowering も実行もせず、入力を受け付けず、
    /// 入力の番号も進めない。
    pub fn type_of(&self, text: &str) -> Typed;
}

#[derive(Debug)]
pub struct Typed {
    /// 誤りと、受け付けた入力までに読んでいないファイルの警告。
    pub diagnostics: Vec<Diagnostic>,
    pub files: SourceFiles,
    /// 誤りがなければ `Show a => a -> String` の形の型。
    pub ty: Option<String>,
}
```

- `with_file` は、`Repl::new` で作ったセッションの最初の入力としてファイルを評価する。`eval` と同じ道 (`Session::load_session`、検査、E2018、lowering、止める語、実行) を通り、違いは入力の読み方と表示のパスだけである。誤りのないファイルは定義だけの入力で、入力の関数を持たないので、lowering も実行もせず、止める語だけを読む。ファイルを途中の入力として足す API は作らない。足すと、前の入力が import したモジュールとファイルが同じになる場合や、根とファイルのディレクトリが食い違う場合が出るためである
- 根の文字列と `Loader` の入口のファイル名は、どちらも `path` を最後の `/` で分けて作る (`eml_hir::load_with_std` と同じ)。`Repl` はファイルの表示のパスを、ファイルを受け付けたかによらず持ち、以後の `eval` と `type_of` でも入口のファイル名として `load_session` に渡す。受け付けなかったときにも、後の入力がファイルを普通のモジュールとして import できないようにするためである
- 受け付けた入力 (`Accepted`) は、表示のパス (`Option<String>`) も覚える。以後の `eval` と `type_of` は、セッションを読み直すたびに入力 1 をファイルとして読む。覚えないと、2つ目の入力からファイルを `parse_input` で `<input 1>` として読み直し、診断と実行時エラーの位置、E1030、警告の絞り込みがすべて変わる
- `type_of` は、続けられない誤りの後のセッションでも型を検査する。前端だけを使い、ヒープに触れないためである。端末は `Broken` で終わるので、この場合に `type_of` を呼ばない
- `type_of` は `eval` と同じく、始めに `Control::clear` を呼び、`Session::load_session` と `check` を通る。lowering しない。番号は次の入力の番号を使い、進めない。問い合わせの入力は `CodeBase` に入らないので、同じ番号の `input$N` と食い違わない。警告は `eval` と同じく、受け付けた入力までに読んでいないファイルのものだけを返す。import のキャッシュには足さない。`has_errors(&diagnostics)` なら `ty` を `None` にする。線形性の誤り、網羅性の誤り、E2012 は、`eml_types` が型の文字列を作った後で報告するので、`TypedProgram::query` があっても誤りがありうるためである
- セッションを空にするメソッドは足さない。新しい `Repl` を作り、古い `Repl` を閉じればよい。`RunConfig` と `HostRef` (`Clone`) は呼び出し側が持っている
- `Control` は新しい `Repl` にも同じ `Arc` を渡す。SIGINT のハンドラが持つ `Control` を変えずに済む

### `eml_syntax`

- `parse_query(file, text) -> (Parse, Vec<Diagnostic>)` を足す。根は新しい `REPL_QUERY` で、式をちょうど1つ読む。式の文法は `parse_input` の式の分かれ道と同じである。式の後に何か残れば E0011 (`':type' takes one expression`) にする。`parse_input` の先読みを通さないので、`f x = 1` も式として読み始めて `=` で誤りになる

### `eml_hir`

`SessionInput` に、テキストの読み方を足す。

```rust
pub struct SessionInput<'a> {
    pub number: u32,
    pub text: &'a str,
    pub form: InputForm<'a>,
}

pub enum InputForm<'a> {
    /// 端末で打った入力。`eml_syntax::parse_input` で読む。
    Typed,
    /// `:load` のファイル。`eml_syntax::parse` で読み、表示のパスは `path` である。
    File { path: &'a str },
    /// `:type` の問い合わせ。`eml_syntax::parse_query` で読む。
    Query,
}
```

- 3つとも `ModuleOrigin::Session(number)` を持つ。名前解決、orphan 規則、版の表示は変えない
- `File` のモジュールの名前は、ほかの入力と同じく `<input N>` である。表示のパス (診断と `SourceFiles`) だけを `path` にする
- `load_session` は、入口のファイル名 (`entry_file: &str`。ファイルがなければ空) を新しい引数で受け、`Loader` に渡す。依存先か後の入力がそのファイルを import すると、`eml run` と同じく E1030 (`Entry`) になる。`File` の形は、読み方と表示のパスだけを決める。ファイルを受け付けなかったセッションでは `File` の入力はないが、入口のファイル名は渡る。`load_session` の `root` は、呼び出し側がファイルのディレクトリ (`/` で終わるか空) を渡す。`File` の入力が入力 1 でないか2つ以上あれば、内部の誤り (`debug_assert!`) にする
- `item_tree` は `REPL_QUERY` の根を新しい `InputItemKind::Query` にし、`lower_input` はそれを `FunctionKind::Input(InputKind::Query)` の入力の関数にする。`InputKind::Query` を新しく足す

### `eml_types`

`check_input` が `InputKind::Query` を受ける。`Expr` との違いは次のとおりである。

- 入力の関数の型を `Unit -> <ρ> T` にする。`ρ` は新しい row 変数で、`<IO>` に閉じない。E2015 は起きない。本体の row と末尾を共有した `ρ` は、`T` のほかの row と同じく数え、名前を付けた後で閉じる
- 制約を解くときの defaulting を、今の `bool` から3つの値にする (`Unit`、`None`、`Query`)。`Query` では、`solve` は型変数の葉で失敗せず、`(クラス, 型変数, 参照)` を残りの制約として記録する。instance がない誤り (E2006) は今と同じく優先し、本体に誤りがあれば報告しない扱いも今と同じである。残りの制約は参照の解いた型引数とともに返す。型引数に `≤ Unr` を足すのは、下の分け方の後で E2009 にならなかった参照だけである。今と同じく、誤りを出した参照の型引数には足さず、線形性の診断を重ねない
- 解いた後、`check_input` は残りの制約を分ける。型変数が `T` に現れれば文脈にする。現れなければ、その型変数を `Unit` に決めて制約を解き直し、解けなければその参照に E2009 を出す (参照ごとに1件、今と同じ)。この E2009 は `usage::reliable` を呼ぶ前に足す。ほかの本体の誤りと同じく、線形性の診断を止めるためである
- その後、型の文字列を作る。推論の表 (`Table`) の上で、`T` の木を左から読んで (矢印は引数、row のラベルの型引数、row の変数、結果の順) row 変数を数え、名前を付ける。文字列は、名前を付けた変数を `TypeKind::Rigid(name)` と `RowTail::Rigid(name)` に、1回だけ現れる row 変数を閉じた row にして書き出す `Exporter` のモードで、別の `TypeStore` に書き出してから `TypeStore::display` で作る。型の表示の処理をもう1つ作らずに済む。文脈も同じ名前で書く
- 文字列を作った後で、`default_unsolved(.., false)` を今と同じく呼ぶ。本体から届く row 変数を閉じ、後の `closed_shape` が開いた row で `unreachable!` にならないようにするためである。型変数は決めない
- 文字列は `PendingBody` の新しい欄で `check_module` へ運び、新しい `TypedProgram::query: Option<String>` に入れる。問い合わせの検査が文字列まで届かなければ `None` である。後で報告する誤りがあっても `Some` になりうるので、誤りの判断は `Repl::type_of` が診断で行う
- `finish_input` は `Query` のとき、E2016 の検査と表示の仕方の計算を飛ばし、`TypedProgram::inputs` に入れない。入力の関数の型 (`closed_shape`) は今と同じく `signatures` に入れる。`typed_program` がすべての関数に型を求めるためである

translate は問い合わせを見ない (`Repl` が lowering しない)。`InputKind::Query` の入力の関数を translate に渡すと内部の誤りにする (`unreachable!`)。

### `eml_cli::lines`

`Command` を次のようにする。値を持つので `Copy` を外す。

```rust
pub enum Command {
    Quit,
    Help,
    /// `None` は空のセッションに戻す。
    Load(Option<String>),
    Reload,
    Type(String),
}
```

引数の検査は上の「コマンドの表」のとおりである。

### 端末 (`terminal.rs`、`main.rs`)

- `terminal.rs` は `RunConfig`、`HostRef`、今の `Repl`、覚えたファイルのパス、閉じたセッションに誤りがあったか、を持つ。`HostRef` は `Repl::new` に渡す前に複製して持つ
- `:load` と `:reload` は、綴りを直したパスの文字列を表示のパスにし、`fs::read_to_string` で読む。読めたら、今の `Repl` を取り出して閉じ (誤りを書く)、その後で `Repl::with_file` で新しいセッションを作る。閉じる誤りがファイルの診断より前に出るようにするためである。`source` は `FsProvider::new(dir)` で、`dir` は `Path::new(path).parent()` (空ならカレントディレクトリ) である
- `:type` は `render` で診断を標準エラーに書き、止める語を読む。割り込みが立っていれば `interrupted` を書いて `Flow::Interrupted` を返す。立っていなければ、`Typed::ty` があれば `{e} : {ty}` を標準出力に書く
- `main.rs` の `repl` のサブコマンドに、省ける位置引数 `file` を足す。読めなければ使い方の誤り (2) にする

`:load` の評価は普通の入力と同じく `Flow` を返す。`Broken` なら今と同じく終了コード 1 で終わり、`Interrupted` なら同じ読みで届いた残りの行を捨てる。

## テスト

TDD で進める。

- `eml_syntax`: `parse_query` が式をちょうど1つ読み、根が `REPL_QUERY` になる。`f x = 1` と2つの式は E0011 (`':type' takes one expression`) になる。`let x = 1` (`in` がない) と `data …` (式で始まらない) は、式の文法のままの E0011 (``expected `in` ``、`expected an expression`) になる
- `eml_hir` (`tests/session.rs`)
  - `File` の入力をファイルの文法で読み、`SourceFiles` の表示のパスが `path` になる
  - 後の入力が、ファイルの `pub` でない item を引ける
  - ファイルの依存先がファイルを import すると E1030 になる。後の入力が import しても E1030 になる
  - `Query` の入力が `InputKind::Query` の入力の関数になる
- `eml_types` (`check/input.rs` の単体テスト。セッションの最後に問い合わせを置く新しい補助 `test_query` を足す)
  - `(>>)` が `(a -> <e> b) -> (b -> <e> c) -> a -> <e> c`、`[]` が `List a`、`fn x -> x` が `a -> a` になる
  - 型変数の制約: `fn x -> show x` が `Show a => a -> String`、`fn x y -> x < y && x == y` が `Ord a => a -> a -> Bool` (上位クラスの制約を書かない)、2つの型変数の文脈が `(Eq a, Show b) => …` になる
  - 型変数が5つ以上の型で `e` を飛ばす
  - row: 1回だけ現れるラベル付きの row が `<Ask>`、2回現れるものが `<Ask | e>` になる
  - `Lin` の矢印 (`File` を捕まえたクロージャ) を Kind なしで書く
  - `ask ()` が `Int`、`Fs.open "x"` が `File` になり、誤りがない
  - `let` の束縛の名前の型を書く
  - `show []` が `String` になる。`show (fn x -> x)` は E2006、`Unit` の instance のないクラスの、型に現れない型変数の制約は E2009 になる
  - 定義し直した型の古い版が `Shape#1` と表示される
- `eml_cli::lines` (`tests/lines.rs`)
  - `:load path`、`:load` (引数なし)、`:reload`、`:type x` と、接頭辞 `:l`、`:r`、`:t`
  - `:reload x` と、引数のない `:type` の誤り
  - 前後に空白のある引数と、空白を含むパス
- トランスクリプト (`tests/repl.rs`)
  - harness は `> :` で始まる入力を `Lines::push` でコマンドとして読む。接頭辞と引数の誤りもそこで決まる。コマンドの行には `input_status` の接頭辞の検査をしない
  - `:type` は `Repl::type_of` を呼び、`e : T` か診断を記録する
  - `:load` と `:reload` は `<name>/main.repl` の形のトランスクリプトでだけ使え、ほかの形で使うと harness が失敗する。パスはトランスクリプトのディレクトリからの相対パスで、表示のパスも書いたとおりである。古い `Repl` の `close` の結果は、最後の `--- close ---` と同じく検査モードの記録にだけ書き、速いモードとの比較に入れない
  - `:quit` と `:help` をトランスクリプトに書くと harness が失敗する
  - 読めない `:load` は、端末と同じ ``error: cannot read `path`: …`` を記録し、セッションを変えない。引数のない `:load` は、根を `""`、読み込み先をトランスクリプトのディレクトリにした `Repl::new` に替える
  - 閉じたセッションの `close` の結果は、`Transcript::inputs` に入れず、検査モードだけの記録として、コマンドの位置に `--- close ---` と同じ形で書く
  - 新しいトランスクリプト: `:type` の例 (上の例と、定義し直した型、警告の出る式)。ファイルを読み、item を定義し直し、`:reload` で `let` の束縛が消えるもの。警告のあるファイルを読み、後の入力でファイルの関数の実行時エラーと診断を起こして、表示のパスが残り、警告を繰り返さないもの
- 端末 (`tests/terminal.rs`、標準入力はパイプ、一時ディレクトリ)
  - サブディレクトリのファイルが隣のファイルを import する (import の根がファイルのディレクトリになる)
  - ファイルを書き換えてから `:reload` すると、新しい定義が見える
  - 誤りのあるファイルを `:load` し、直してから `:reload` すると受け付ける
  - 読めないファイルの `:load` は誤りを書き、前のセッションの `let` の束縛が残る
  - ファイルを覚えていない `:reload` は誤りになる
  - `eml repl file.em` がファイルを読んで始まる。`eml repl missing.em` は終了コード 2 で終わる
  - `:type` の出力が標準出力に出る
- lib API (`tests/api.rs`)
  - `type_of` は入力を受け付けず、番号を進めない (後で定義し直した item の古い版の番号が、受け付けた入力の番号のままである)
  - `with_file` の診断がファイルのパスを指す。空のファイルは `Outcome::Empty` になる
  - import を読む間に割り込む読み込み先 (`tests/api.rs` の `InterruptingSource`) を渡した `with_file` は `Outcome::Interrupted` になり、後の入力からファイルの item が見えない
  - ファイルを受け付けなかったセッションでも、後の入力がファイルを import すると E1030 になる
  - 誤りが線形性の誤りだけの問い合わせでは、`ty` が `None` である

### 既存のテストの変更

- 機械的な追従 (種類 3): テストの中で `SessionInput { number, text }` を作る3か所 (`eml_types` の `test_session`、`eml_hir/tests/session.rs`、`eml_core_ir/tests/common/mod.rs`) に `form: InputForm::Typed` を足す。期待値は変わらない。`eml_cli::repl` の2か所はテストでなく、`Accepted` のパスから `form` を作る本体の変更である
- `:load` が閉じたセッションの誤りで終了コードを 1 にする道は、誤りを起こせる入力がない (コンパイラか実行時の不具合でだけ起きる) ので、テストせずにレビューで確かめる
- 合否の変更 (種類 1) と期待値の変更 (種類 2) はない。今のトランスクリプトに `:` で始まる入力はなく、`:help` の出力のテストは一部を含むかだけを見る

## 文書

- `docs/spec/repl.md`
  - 「コマンドライン」に `file` の引数
  - 「コマンド」の表に3つのコマンドと引数の規則
  - 新しい節「ファイルの読み込み」(`:load` / `:reload`) と「型の表示」(`:type`)
  - 「終わり方」に、`:load` が閉じたセッションの誤りで終了コードが 1 になること
  - 「割り込み」に `:type` の扱い
  - 「残る制限」の 2. に、`:load` のファイルが FIFO か遅いファイルのときも抜けられないこと
- `docs/spec/modules.md` の「REPL のセッション」: ファイルの入力と問い合わせの入力、ファイルのディレクトリを根にすること、ファイルの表示のパス、ファイルを import すると E1030 になること (今の「E1030 は当てない」と「表示のパスは `<input N>`」を直す)
- `docs/spec/types.md` の「REPL の入力」: 問い合わせの入力の規則と、型の書き方
- `docs/spec/grammar.md` の「REPL の入力」: ファイルの入力はファイルの文法、問い合わせの入力は `REPL_QUERY` で式1つを読むこと
- `docs/spec/core-ir.md` の「REPL の入力」: 問い合わせの入力は translate しないこと
- `docs/spec/diagnostics.md` と `docs/implementation/diagnostics.md`: E1030 の行 (`:load` のファイルも入口のファイル)、E2009 と E2015 と E2016 の行 (問い合わせでの扱い)、E0011 の `':type' takes one expression`
- `docs/future/roadmap.md`: S11c-2 を終えた行にし、決めたことの移し先を書く。「論点」の `:reload` の行 (編集したモジュールの読み直しと、`:load` で型が変わるときの値の扱い) を、セッションを作り直すことで閉じる
- `docs/implementation/status.md`、`architecture.md` (「CLI と lib API」)、`testing.md` (「REPL のトランスクリプト」のコマンドの行、「CLI のテスト」)、`CLAUDE.md`
