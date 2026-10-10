# S6b 文字列

位置づけ: 作業の設計。ロードマップの S6b を定める。S6b を終えたら、決まったことを `docs/spec/` と `docs/implementation/` に移し、この文書を削除する。

## 目的

文字列の補間 `\{式}`、複数行の文字列 `"""`、raw 文字列 `r"…"` / `r#"…"#` を使えるようにする。そのために lexer にモードのスタックを入れる。S9 (`eml_rt`) で文字列のランタイム表現を決めるときに、文字列を組み立てる実際のプログラムで詰められるようにするためである。

ロードマップの S6b の論点は、次のとおり決める。

- lexer のモード: `Code`、`String`、`Command` のスタックにし、補間のトークンと一緒に設計する (下の「字句」)
- 補間の CST: `STRING_LIT` と `INTERP` の節点にする (下の「CST とパーサ」)
- 未対応のリテラルの E0004: コマンドリテラルも同じモードの仕組みで字句に分け、浮動小数、文字、コマンドリテラルの E0004 をすべて HIR が出す

文字列を扱う関数 (`length`、`split` など) は入れない。関数の API は、S12 の標準ライブラリの spec で決める。

## 前提として確かめたこと

- 設計のレビューで、リポジトリの複製の `std/Prelude.em` に下の「Prelude」の `display` を足し、全テストを流した。失敗したテストはなかった。UI テストの出力と、`bench/` の `RunStats` も変わらない
- そのとき、次の振る舞いも確かめた。`display "hi"` は `hi`、`Show a => a -> String` の関数の中で `display` を `"hi"` に使うと `hi`、`[1, 2]` は `[1, 2]`、`Some "x"` は `Some "x"`、導出した `Show` の `Red` は `Red` になる。ユーザーの instance で `display` を上書きでき、`show` はそのまま残る
- 設計のレビューで、次のことも確かめた。E2006 の文言は「`display` requires `Show Foo`」になる。`display` を通る多相再帰は E2012 になる。導出した instance とタプルの instance は既定の `display` を使う (`translate/instances.rs` の `method_target` が `default_target` に進む)。`multi` の継続を2回再開しても、組み立て中の文字列のその場での連結は正しく、`debug_heap` も通る (補間を手で `++` に書き換えたプログラムで確かめた)
- 今の UI テストは、補間、複数行の文字列、raw 文字列、コマンドリテラルを使っていない (コメントの中のバッククォートだけである)。そのため、UI テストの期待値は変わらない

## Prelude

`std/Prelude.em` の `Show` に `display` を足し、`String` の instance で上書きする。

```haskell
pub class Show a where
  show : a -> String
  show_prec : Int -> a -> String
  show_prec _ x = show x
  display : a -> String
  display x = show x

instance Show String where
  extern show
  display s = s
```

- 補間の穴は `display` を呼ぶ (下の「HIR」)。`show` は読み戻せる形、`display` は人が読む形という分担になる。Haskell の `Show` が `String` の表示のために `showList` を持つのと同じく、クラスのメソッドで `String` だけを特別に扱う
- `display` はクラスの普通のメソッドなので、ユーザーが呼ぶことも、instance で上書きすることもできる。導出した instance と、`List` などの手書きの instance は既定の `display` を使う。そのため `"\{Some "x"}"` は `Some "x"` になり、引用符を外すのは穴の値そのものが `String` のときだけである
- 多相の穴も instance で決まる。`wrap : Show a => a -> String` の本体の `"<\{x}>"` は、`wrap "hi"` で `<hi>` になる。穴の型を見て `show` を呼ぶかを決める方式では、同じ値でも書いた位置の型で表示が変わる。この方式では、そうならない
- `String` の `display` を extern にせず等式で書くのは、恒等関数で足りるためである

## 字句

### トークン

文字列は、穴がなくてもいつも細かいトークンに分ける。[文法](../../spec/grammar.md) の `string` の規則のとおりである。

| トークン | 中身 |
|---|---|
| `STRING_START` | `"` か `"""` |
| `STRING_TEXT` | エスケープでない本文の並び (複数行の文字列では改行を含みうる) |
| `ESCAPE` | エスケープ1つ (`\n`、`\u{…}` など)。不正なエスケープも1つの `ESCAPE` にする |
| `INTERP_START` | 補間の開き `\{` |
| (穴の中) | ふつうのトークン |
| `INTERP_END` | 穴を閉じる `}`。閉じていない穴では幅0 (下の「改行での回復」) |
| `STRING_END` | `"` か `"""` |

- 今の1トークンの `STRING` と `MULTILINE_STRING` は消す。`RAW_STRING` は、中に構造がないので1トークンのまま残す
- `ESCAPE` を分けるのは、値の取り出しと E0008 の検査が同じトークンを見るためと、後の LSP でエスケープを色分けできるようにするためである
- 穴の閉じを `R_BRACE` でなく専用の `INTERP_END` にするのは、ほかの括弧と取り違えないためである。`R_BRACE` にすると、`"\{(x}"` の `}` をレイアウト段とパーサが `(` の閉じと読み、穴がいつまでも閉じない。`"\{x)}"` では、`)` が穴の `Bracket` を取り除いてしまう
- コマンドリテラルも同じ形で分ける。`CMD_START` (バッククォート)、`CMD_TEXT`、`ESCAPE` (文字列のエスケープと `` \` ``)、`INTERP_START`、`INTERP_END`、`CMD_END` である。穴の中の `..` は今の `DOT2` のトークンとして出す。今の1トークンの `COMMAND` は消す。コマンドリテラルの中の `"` は本文である
- `\u{` のエスケープの `}` を探す範囲は、今の行末と `"` に加えて、`\` の手前までにする。`"\u{4\{x}}"` の穴を、不正なエスケープに飲み込まないためである
- 単一行の文字列とコマンドリテラルの、行末かファイルの終わりにある `\` は、長さ1の `ESCAPE` にし、E0008 は出さない。文字列は閉じていない扱いになり、E0002 だけを出す (今と同じ)。複数行の文字列の中の `\` と改行の組は、`\` だけを `ESCAPE` にして E0008 を出す (下の「複数行の文字列」)

### モードのスタック

lexer は、`Code { braces }`、`String { multiline }`、`Command` からなるスタックを持つ。底は `Code { braces: 0 }` である。

- `Code` のモードで `"` か `"""` を読むと、`STRING_START` を出して `String` を積む。バッククォートなら `CMD_START` を出して `Command` を積む
- `String` か `Command` のモードで `\{` を読むと、`INTERP_START` を出して `Code { braces: 0 }` を積む
- `Code` のモードの `{` は `braces` を1増やし、`}` は1減らす。穴の中で `braces` が 0 のときの `}` は、`INTERP_END` を出してモードを1つ戻す。底の `Code` では、`}` はいつも `R_BRACE` で、モードは変えない
- 閉じの `"`、`"""`、バッククォートで `STRING_END` か `CMD_END` を出し、モードを1つ戻す
- 穴の中の `"` は新しい `String` を積むので、`"a \{f "b \{x}"}"` のような入れ子も読める
- lexer は、どの `INTERP_START` にも対応する `INTERP_END` を必ず1つ出す (幅0のものを含む)。パーサとレイアウト段は、これを頼りに穴の終わりを決める

### 穴の中の改行

穴の中には改行を書けない。単一行の文字列でも、複数行の文字列でも同じである。

- 理由は回復のためである。`"a \{x` のように `}` を忘れたとき、改行を許す規則では、ファイルの残りを穴として読んでしまう
- 穴の中のトークンは改行 (`\r\n` か `\n`) の手前で切る。改行を含みうるトークン (空白、ブロックコメント、raw 文字列) も、穴の中では改行の手前で終わる
- `if` や `match` を、1行で穴の中に書くことはできる

### 改行での回復

単一行の文字列、コマンドリテラル、穴の中で改行かファイルの終わりに着いたら、スタックの上からモードを取り除く。止まるのは、穴の外にある複数行の文字列か、底の `Code` に着いたときである。

- 取り除いた穴には、改行の手前 (ファイルの終わりではその位置) に幅0の `INTERP_END` を出す。パーサは穴を普通に閉じ、次の行のトークンを穴の中として読まない。レイアウト段の穴の文脈も、この `INTERP_END` が取り除く
- 報告するのは、一番内側の層の1件だけである。外側の層は黙って閉じる。1つの書き忘れに、層の数だけ誤りを並べないためである
  - 文字列: E0002「unterminated string literal」(今と同じ)
  - コマンドリテラル: E0002「unterminated command literal」(今と同じ)
  - 穴: E0002「unterminated string interpolation」。範囲は `\{` で、ラベルは「missing closing `}`」にする
  - 穴の中で改行に着いたブロックコメント: E0005「unterminated block comment」(今と同じ文言)
  - 穴の中で改行に着いた raw 文字列: E0002「unterminated raw string」(今と同じ文言)
  - 穴の中の `"""`: E0014「multi-line strings are not allowed inside an interpolation」。範囲は `"""` である。`"""` は `STRING_START` として出し、`String { multiline: false }` を積む。穴の中の `"""` は開きの行で必ず改行に着くので、その行の終わりで閉じるときは、E0002 の代わりにこの E0014 を一番内側の層の報告にする
- 複数行の文字列は改行では閉じない。ファイルの終わりで閉じていなければ、今のとおり開きの `"""` に E0002 を出す。raw 文字列も、穴の外では同じである
- ファイルの終わりでは、複数行の文字列も含めて、開いているモードをすべて取り除く。報告は上と同じく一番内側の層の1件だけにする

### 複数行の文字列

形は Swift と同じで、[字句](../../spec/lexical.md) の今の規則を細かくする。

- 開きの `"""` の後ろは、行末まで空白しか書けない。その空白は値に入れない。閉じの `"""` の前も、その行の先頭から空白しか書けない
- 中身は `\r\n` か `\n` で行に分ける。閉じの `"""` の列 (前に空白しかないので、文字の数とバイトの数は同じ) を N とする。中身の各行は、先頭の N 個の空白を取り除く。空白だけの行は N より短くてもよく、空の行になる。N より長い空白だけの行は、N 個を除いた残りの空白を保つ。それ以外の行で、先頭の N 文字に空白でない文字 (タブ、エスケープの `\`、穴の `\{` を含む) があれば誤りにする
- 開きの直後の改行と、閉じの直前の改行は値に入れない。`"""` の次の行が閉じの `"""` なら、2つの改行は同じもので、値は空の文字列である
- エスケープと補間は、通常の文字列と同じに書ける。`"` と `""` はそのまま書け、`"""` を中身に入れるときは `\"""` と書く。行末の `\` で行をつなぐ書き方は入れない。複数行の文字列の中の `\` と改行の組は E0008 (不正なエスケープ) にし、範囲は `\` だけにする
- 字下げを取り除くのはソースの本文だけで、穴の値は変えない。行頭の穴の前の空白は、ほかの行と同じく字下げとして扱う
- 形の誤りは、新しいコード E0014 (`INVALID_MULTILINE_STRING`) にする。範囲はいつも文字列のトークンの中に収め、文字列の外のコードを含めない
  - 開きと閉じが同じ行にある (`"""m"""`): 文字列全体に1件出す。文言は「a multi-line string must start on a new line after `\"\"\"`」にする。字下げの検査はしない
  - 開きの行に文字がある: 開きの `"""` の後ろの最初の空白でない文字から、その行の改行の手前までに1件出す
  - 閉じの前に文字がある: 閉じの行の、字下げの後の最初の空白でない文字から、閉じの `"""` の手前までに1件出す。N が決まらないので、字下げの検査はしない
  - 字下げが足りない: その行の先頭から、最初の空白でない文字まで (その文字を含む) に、行ごとに1件出す
  - 閉じていない複数行の文字列は、E0002 を出し、字下げの検査はしない。開きの行の検査はする
- 穴の中の `"""` の E0014 は、上の「改行での回復」にある
- E0014 があっても、トークンと CST は同じ形に組む。値は取り出さず、HIR はその文字列を `Missing` にする

```haskell
main : Unit -> <IO> Unit
main () =
  let name = "eml"
  println """
    Hello, \{name}!
      indented line
    """
```

この例は `Hello, eml!` と `  indented line` の2行を出す。

### raw 文字列

- `r"…"`、`r#"…"#` (`#` は何個でもよい) と書く。エスケープも補間もなく、書いたとおりの値になる
- Rust と同じく、改行を含められる。字下げは取り除かない。字下げ付きで複数行のテキストを書くときは、複数行の文字列を使う
- 閉じていなければ、今のとおりファイルの終わりまでを E0002 にする

### 改行の正規化

複数行の文字列と raw 文字列のどちらも、ソースの `\r\n` は値では `\n` にする。ファイルの改行の種類で値が変わらないようにするためである。

### レイアウト

- レイアウト段が行の先頭とみなすのは、改行を含む trivia の直後のトークンだけである (今の `scan_lines` のとおり)。`STRING_TEXT` と `RAW_STRING` の中の改行は trivia でないので、その後ろのトークンは行の先頭にならない。そのため、複数行の文字列の中の行と、閉じの `"""` の後ろのトークンは、レイアウトに影響しない。レイアウト規則7を、この言い方に直す
- `INTERP_START` は、新しい文脈 `Interp` を積む。`INTERP_END` は、一番内側の `Interp` より上の `Bracket` と `Block` をすべて取り除き (`Block` には `CLOSE` を挿入する)、その `Interp` を取り除く
- 閉じ括弧 `)` `]` `}` は、一番内側の `Interp` より上にある `Bracket` だけを閉じる。そうした `Bracket` がなければ何もしない (今の「対応する開き括弧がなければ何もしない」と同じ扱い)。穴の外の括弧を、穴の中の閉じ括弧で閉じないためである
- 穴の中に改行はないので、`Interp` が規則1と規則2に出会うことはない
- レイアウト規則4を、この扱いに直す

## CST とパーサ

### 節点

- 文字列は `STRING_LIT` の節点にする。子は `STRING_START`、`STRING_TEXT`、`ESCAPE`、穴の `INTERP` の節点、`STRING_END` である。閉じていない文字列には `STRING_END` がない
- `INTERP` の子は、`INTERP_START`、式、`INTERP_END` である。コマンドリテラルの穴では、式の前に `DOT2` を置ける
- コマンドリテラルは `COMMAND_LIT` の節点にし、子の形は `STRING_LIT` と同じにする
- 式の位置では、`STRING_LIT` と `COMMAND_LIT` を `LITERAL` で包まずに `atom` として置く。typed AST は `Expr::String(StringLit)` と `Expr::Command(CommandLit)` にする。`RAW_STRING` は、今のとおり `LITERAL` の中のトークンにする
- [文法](../../spec/grammar.md) の `string` と `command` の規則の閉じの `'}'` を、`INTERP_END` に直す

### typed AST の値

- `StringLit::parts()` は、`Option<Vec<StringPart>>` を返す。`StringPart` は `Text(String)` か `Hole(Option<Expr>)` である
- `Text` は値に直した後の文字列である。エスケープ、複数行の字下げ、改行の正規化を済ませてある。隣り合う `STRING_TEXT` と `ESCAPE` は1つの `Text` にまとめ、空の `Text` は返さない
- 閉じていない文字列、不正なエスケープ、E0014 を含む文字列は `None` を返す。どれも lexer が報告済みである
- `Literal::value()` は、`RAW_STRING` の値も返す。改行の正規化だけをした中身である
- `LiteralPat::value()` は、`STRING_LIT` の子と `RAW_STRING` のトークンを見る。穴のある文字列と、`parts()` が `None` の文字列には `None` を返す
- 値に直す規則は `literal.rs` に置き、lexer の検査 (E0008、E0014) と同じ表を使う。S6a までと同じ作りである

### パターン

- `LITERAL_PAT` は、`INT`、`CHAR`、`RAW_STRING` のトークンか、`STRING_LIT` の節点を持つ。複数行の文字列と raw 文字列も、リテラルのパターンに書ける
- パターンの文字列に穴があれば、パーサが穴ごとに E0011「string interpolation is not allowed in a pattern」を `INTERP` の範囲に出す。CST には組む
- パターンのコマンドリテラルは、今と同じく E0011 になる (`apat` の始まりでない)

### 穴の読み方

- `INTERP_START` の後に式を1つ読み、`INTERP_END` を期待する。lexer が `INTERP_END` を必ず出すので、穴はいつもそこで終わる
- `"\{}"` のように式がなければ、式を期待する E0011 にする。式の後に `INTERP_END` 以外が続けば E0011 を出し、`INTERP_END` まで読み飛ばす。幅0の `INTERP_END` (閉じていない穴) の手前では、lexer が報告済みなので E0011 を出さない
- 文字列の穴に `..` を書くと、式を期待する E0011 になる。`..` を読むのはコマンドリテラルの穴だけである
- 読み飛ばし (`skip_to_closing` の `Nesting`) は、`INTERP_START` と `INTERP_END` を入れ子の対として数え、穴の外の `INTERP_END` で止まる。`close_bracket` は `INTERP_END` を閉じ括弧として読まない。`"\{(x}"` は、`)` がない E0011 を1件出し、穴は `INTERP_END` で閉じる

### 直すパーサの箇所

文字列が1トークンでなくなるので、次の箇所を直す。

- 式の始まりの集合 (`ATOM_START`) と `atom` に、`STRING_START` と `CMD_START` を足す。今の `STRING`、`MULTILINE_STRING`、`COMMAND` の分岐は消す
- パターンの始まりの判定 (`at_apat_start_at`) と `apat` に、`STRING_START` と `RAW_STRING` を足す。`INTERP_START` と `INTERP_END` は開き括弧と閉じ括弧 (`is_opening_bracket`、`is_closing_bracket`) に入れない。入れると、`at_apat_start` が `INTERP_START` を始まりとみなし、`apat` を読まないまま回り続ける
- 演算子の等式の先読みが使う `apat_len` は、文字列を `STRING_START` から `STRING_END` (なければ文字列の最後のトークン) までの長さで数える。穴の中の入れ子の文字列も、`INTERP_START` と `INTERP_END` の対で数えて飛ばす
- 診断のトークンの名前 (`token_name` と `unexpected`) は、`STRING_START` を「a string」、`CMD_START` を「a command literal」と書く。今のように1トークンの文字列全体を引用すると、`"` だけが出てしまうためである

### 深さ

- 穴は、括弧と同じく1段に数える。そのため、`paren_expr` とリストの要素と同じく、`expr` の形 (`if`、`fn` など) で始まらない穴の式は `op_expr` で直接読み、`expr` の段を重ねない
- 1つの文字列の穴の数は、深さに数えない。HIR が穴を平らな列で持つためで (下の「HIR」)、S6a のリストの式と同じ扱いである

## HIR

- `LangItems` に `display` (`Show` のメソッド) を足す。リストの構文と同じく、名前を引かずに Prelude を指す。ユーザーが自分の `display` を定義しても、補間の意味は変わらない。`++` は extern なので `LangItems` に入れない。translate が extern の行を直接呼ぶ (下の「translate」)
- 穴のない文字列は、今までどおり `Literal::String` にする。複数行の文字列と raw 文字列も同じで、`unsupported_literal` の2つの分岐を消す
- 穴のある文字列は、新しい `ExprKind::Interpolation(Vec<Segment>)` にする。`Segment` は `Text(String)` か `Hole(ExprId)` である。穴が1つ以上あるので、列は空でない
- `Hole` の `ExprId` は、HIR が組んだ `display e` の呼び出しである (`ExprKind::Call` と、`lang.display` を指す `ExprKind::Path`)。`Call` と `Path` の範囲は `\{…}` 全体にする。こうすると、型検査はメソッドの参照として普通に制約を作り、translate は普通に instance を解決する。補間を `++` の呼び出しにまで脱糖しないのは、連結を translate がまとめて組むためである (下の「translate」)
- 穴の式が欠けていれば (`"\{}"`)、`Hole` の中身は `Missing` の式を引数にした呼び出しにする。型検査は `Error` の型として連鎖を抑える
- `StringLit::parts()` が `None` なら、文字列全体を `Missing` にし、穴の中の式は lower しない。lexer が報告済みの誤りのある文字列では、穴の中の名前や型の誤りも出さないことにする。穴だけを lower して本文を `Missing` にする方法もあるが、誤りのある文字列の中の誤りを重ねて報告しないことを選ぶ
- パターン: `LiteralPat::value()` が `None` のパターン (穴のある文字列、`parts()` が `None` の文字列) は `PatKind::Missing` にする。どれもパーサか lexer が報告済みである
- コマンドリテラルは、リテラル全体の範囲に E0004「command literals are not supported yet」を出して `Missing` にする。穴の中の式は lower しない。誤りを E0004 の1件にするためである
- HIR のダンプ (`pretty`) は、`Interpolation` を `"` で囲み、`Text` を今の文字列のリテラルと同じ規則 (Rust の `{:?}` のエスケープ) で引用符なしに書き、`Hole` を `\{` と `}` の間に式のダンプで書く。例えば `"a\{(@Prelude.display x)}b"` になる

## 型検査

- `Interpolation` の型は `String` である。各 `Hole` を `String` に対して検査する。`display` の型が `a -> String` なので、穴の式の型 `a` に `Show a` の制約がつく
- 穴の型に instance がなければ E2006、型が決まらなければ E2009 になる。どちらも今の文言のままで、`display` の名前が出る (E2006 は「`display` requires `Show Foo`」、E2009 は「cannot decide which instance of `Show` to use for `display`」)。`display` は補間の意味として spec に書くメソッドなので、補間のための文言は足さない
- 線形性 (`usage.rs`) は、穴を左から順にたどる。穴の値は `display` に渡して消費される
- 持ち越し (`carry.rs`) は、穴を左から順に評価する部分として扱い、今の生きている値の集合を穴から穴へ渡す。組み立て中の文字列は持っている値に足さない。`String` は `Unr` なので、持っていても制約が増えないためである。そのため、穴の数に比例する時間で済む

## translate

- `Interpolation` は左から1つずつ処理する。最初の部分 (文字列の定数か、穴の結果) を `acc` に入れる。それ以降の部分は、評価するたびに `acc = acc ++ 部分` とする。`++` は、導出した `Show` の translate (`derive.rs`) と同じく、`Rhs::Extern { ext: Extern::StrConcat, at: None }` を直接作る。ユーザーが自分の `++` を定義しても、補間の意味は変わらない。部分が1つだけなら (`"\{x}"`)、連結はしない
- 評価の途中でつなぐので、穴の呼び出しをまたいで生きているのは `acc` だけである。リストのリテラルのように、`save` の大きさが要素の数の2乗になることはない
- 最初の `++` は、左辺が不死のリテラルか共有された文字列なら、写して新しい文字列を作る。2つ目からは一意な左辺をその場で伸ばす ([ランタイム](../../spec/runtime.md) の「文字列の連結」)。そのため、全体の長さに比例する時間で済む
- 穴の評価の順は左から右である。[式](../../spec/expressions.md) の「関数適用」の評価の順と同じ方針である
- boxing、contract、Perceus、verifier、インタプリタ、ランタイムは変えない

## 診断

- 新しい診断のコードは E0014 (`INVALID_MULTILINE_STRING`) だけである。[診断](../../spec/diagnostics.md) の表に足す
- 閉じていない穴は E0002 にする (上の「改行での回復」)。E0002 の説明に補間を足す
- パターンの補間と、空の穴は E0011 にする
- 未対応のリテラルの E0004 は、浮動小数、文字、コマンドリテラルの3つになる。どれも HIR が出す。パーサが E0004 を出すのは `{…}` のレコードだけになる
- [診断](../../spec/diagnostics.md) の「番号を割り当てていない診断」の「E0xxx 閉じていない補間」の行は消す。閉じていない穴は E0002 にするためである
- [宣言](../../spec/declarations.md) の `String` の `show` の節の「S6b で補間が入ったら `{` も `\{` にする」は消す。`\{` は補間の開きなので、`show` が `{` を `\{` と書くと読み戻せない。`\` は `\\` と書くので、`{` はそのままで読み戻せる

## テストの変更

### 成否の変更

- `eml_syntax` の `tests/lexer.rs` の `interpolation_is_skipped_with_not_yet_supported` を消す。補間の E0004 を示すテストで、補間は未対応でなくなるためである。補間の字句は新しいテストで見る

### 期待値の変更

- `eml_syntax` のテストのうち、文字列のトークン (`STRING`、`MULTILINE_STRING`、`RAW_STRING`、`COMMAND`) や CST を写したもの、補間、複数行の文字列、raw 文字列、コマンドリテラルの E0004 を期待するものを、新しいトークン、CST、診断に書き直す。今わかっているのは次のテストである
  - `tests/lexer.rs`: `literals_and_comments`、`unterminated_string_becomes_string_with_error`、`backslash_at_end_of_line_gives_only_the_unterminated_error`、`unterminated_string_does_not_swallow_carriage_return`、`unicode_escape_does_not_run_past_the_string`、`later_stage_literals_are_single_tokens`、`unterminated_later_stage_literals_are_errors`。名前が意味を失うもの (`later_stage_literals_are_single_tokens` など) は、見ているものに合わせて名前を変える。`"""abc` は、閉じていない E0002 に加えて E0014 も出す
  - `tests/expressions.rs`: `parameter_patterns` と `application_field_access_and_qualified_names` の CST、`later_stage_literals_are_parsed` (パーサの E0004 がなくなる。1行の `"""m"""` は E0014 になるので、入力から外す)
  - `tests/corpus.rs`: `later_stage_corpus_reports_only_not_yet_supported` の E0004 の文言の一覧が「records are not supported yet」だけになる。注釈も直す
  - `tests/ast.rs`: `tuple_and_literal_pattern_parts` の要素の種類 `[PATH_EXPR, LITERAL]` が `[PATH_EXPR, STRING_LIT]` になる。パターンの `"t"` の値は今のとおり `String` である
  - `tests/literals.rs`: 文字列の値を `Literal` でなく `StringLit::parts()` で取り出すように補助関数を直す。`strings_reported_by_the_lexer_have_no_value` から `"\{x}"` を外す (穴のある文字列は値を持つようになる)
- `eml_syntax` の `tests/parser.rs` の `stray_unterminated_string_reports_both_problems` は、期待値を変えない (E0002 と E0003 を1件ずつ)。トップレベルの読み飛ばしが文字列の各トークンに E0003 を出すなら、実装を直す
- `eml_hir` の `tests/lower.rs` の `later_stage_literals_are_not_supported_yet` は、E0004 を浮動小数、文字、コマンドリテラルの3つにする。raw 文字列と複数行の文字列は入力から外す。注釈の「コマンドリテラルは、パーサが E0004 を出す例外である」も消す
- UI テストと `bench/` の `RunStats` は変わらない (複製で確かめた)。`bench/run.sh` の命令の数は、Prelude と lexer が変わるので変わる。記録するときに、その理由を書く

### 新しいテスト

テストを先に書く (TDD)。

- UI `tests/ui/run/strings/`
  - 補間: `Int`、引用符なしの `String`、リスト、`Option`、穴の中の文字列の入れ子、`\\{` (補間にならない)、穴の評価の順 (`println` を起こす関数を穴に書く)
  - `display`: ユーザーの instance での上書き、多相の穴 (`wrap : Show a => a -> String`)、`display` を直接呼ぶ
  - 複数行: 字下げを取り除くこと、深い行、空白だけの行、穴、`\"""`、エスケープ
  - raw: `r"\n"`、`r#"a"b"#`、改行を含む raw 文字列
  - パターン: 複数行の文字列と raw 文字列のリテラルのパターン
  - 穴の値の中の文字列は引用符付きのまま: `"\{[Some "x"]}"` は `[Some "x"]`、`"\{(1, "a")}"` は `(1, "a")`
  - `multi` の操作を穴の中で起こし、handler が2回再開する。組み立て中の文字列のその場での連結と、継続の写しを `debug_heap` 付きで通す
- UI `tests/ui/check-fail/`
  - `syntax/multiline_string_indent.em`: E0014 の形
  - `syntax/unterminated_interpolation.em`: 閉じていない穴の E0002 と、その後の行が読めること
  - `syntax/interpolation_in_pattern.em`: パターンの補間の E0011
  - `classes/interpolation_without_show.em`: `Show` のない型の穴 (E2006)
  - `classes/ambiguous_interpolation.em`: `"\{[]}"` (E2009)
  - `not-yet-supported/command_literal.em`: コマンドリテラルの E0004 (HIR が出す)
- `eml_syntax`
  - トークン: モードの入れ子、`{` と `}` の対応、`INTERP_END`、穴の中の文字列、コマンドリテラルの `\{..xs}`、`"\u{4\{x}}"`、行末とファイルの終わりの `\`
  - 回復: 改行で穴と文字列を閉じ、幅0の `INTERP_END` を出し、一番内側の層だけを報告すること (`"a \{f "b` が1件)、複数行の文字列の中の穴の改行、穴の中のブロックコメント、raw 文字列、`"""`、ファイルの終わり、CRLF の行末
  - パーサ: `"\{(x}"` と `"\{x)}"` で穴が閉じ、文字列の外の括弧に影響しないこと、閉じていない穴の後の行を穴として読まないこと、演算子の等式の先読みが文字列のパターンを読み飛ばすこと
  - CST: `STRING_LIT`、`INTERP`、`COMMAND_LIT`、`LITERAL_PAT` の中の `STRING_LIT`
  - 値: `parts()` (エスケープ、字下げ、空白だけの行、`"""` の直後に閉じる空の文字列、開きの行の後ろの空白、CRLF)、raw 文字列の値と CRLF
  - 診断: E0014 の形 (開きと閉じが同じ行、開きの行の文字、閉じの前の文字、字下げ)、範囲が文字列の外に出ないこと、空の穴、パターンの補間、文字列の穴の `..`
  - 深さ: 穴を1段に数え、穴の数は数えないこと
  - レイアウト: 行頭に `"""` や穴がある複数行の文字列がブロックを区切らないこと
- `eml_hir`: `Interpolation` への lower と範囲、ユーザーの `display` があっても Prelude の `display` を指すこと、穴のある文字列のパターンが `Missing` になること、コマンドリテラルの E0004、`pretty` の出力
- `eml_types`: 穴の `Show` の制約、多相の穴、穴の線形性。`Lin` の局所変数が `multi` の操作を起こす穴をまたいで生きていれば E3006 になり、`once` の操作なら誤りにならないこと
- `eml_core_ir`: `"a\{x}b\{y}"` の translate が、穴を評価するたびに `++` でつなぐ IR になること。Perceus の後のダンプ (`core_until`) で、2つ目の穴の呼び出しをまたいで退避するのが `acc` だけであること。ユーザーが自分の `++` を定義しても、IR が `StrConcat` の extern を呼ぶこと
- `eml_interp` の `scaling.rs`: 穴の多い文字列 (穴が n 個と 2n 個) で、`string_bytes_copied` が2倍程度 (比が 2.5 以下) になること。右から畳む誤った組み立てを見分けるためのテストで、`save` の大きさは上の `eml_core_ir` のダンプで見る

## 文書の更新

S6b の終わりに1回で直す。

- [字句](../../spec/lexical.md): 文字列の節を、トークン、モード、穴の中の改行、回復、複数行の文字列の規則、raw 文字列の改行、改行の正規化で書き直す。「当面の間、穴の式の型は `String` に限る」を、`display` の規則に置き換える
- [文法](../../spec/grammar.md): `string` と `command` の閉じを `INTERP_END` にする。`literal` に `RAW_STRING` を足す。パターンの補間の誤りと、穴の深さの数え方を補足に書く
- [レイアウト規則](../../spec/layout.md): 規則4 (`Interp` の文脈と、`INTERP_END`、閉じ括弧の扱い) と規則7 (行の先頭の決め方)
- [宣言](../../spec/declarations.md): Prelude の `Show` に `display` を足す。`String` の `show` の節の `{` の注記を消す
- [式](../../spec/expressions.md): 補間の意味 (`display` を呼び、左から評価してつなぐこと)。「HIR で行う脱糖」の「補間の `++` の連結への変換」を、「HIR は穴に `display` の呼び出しを入れ、連結は translate が組む」に直す。値の一覧に、穴のある文字列は値でない (`display` を呼ぶ) ことと、穴のない複数行の文字列と raw 文字列はリテラルであることを書く
- [診断](../../spec/diagnostics.md): E0014 の行。E0002 の説明 (閉じていない穴)。E0004 の説明から S6b の構文を消す。「E0xxx 閉じていない補間」の行を消す
- [Core IR とインタプリタ](../../spec/core-ir.md): 変換の規則に、補間の組み立ての順を足す
- [プログラムの例](../../spec/examples.md): 補間と複数行の文字列を「まだ実装していない (S6b)」とする注記を消す
- [実装の現在地](../../implementation/status.md): 今の言語の範囲、「未対応の構文と E0004」(未対応のリテラルの E0004 はすべて HIR が出す。パーサの例外はレコードだけ)
- [コンパイラの構成](../../implementation/architecture.md): lexer のモード、`LangItems` の `display`、`ExprKind::Interpolation`。S6b の穴を将来の形で書いた所を、今の形に直す
- [標準ライブラリへの申し送り](../../future/stdlib.md): 穴を `String` に限るという記述を直す
- [ロードマップ](../../future/roadmap.md): S6b の節を削る
