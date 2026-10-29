# 文法

位置づけ: 規範。

parser が受け付ける文法と、文法だけでは表しきれない補足の規則を定める。各構文の意味と脱糖は [宣言](declarations.md)、[式](expressions.md)、[直積型とレコード](records.md)、[モジュールと名前解決](modules.md) で扱う。

## 記法

- `OPEN` `SEP` `CLOSE` は [レイアウト規則](layout.md) の仮想トークンである。`;` は `SEP` として扱う
- `block(x) ::= OPEN x (SEP x)* CLOSE`
- `list(x) ::= (x (',' x)* ','?)?`
- `CONOP` は `:` で始まる演算子、`OP` はそれ以外の演算子である

## 文法

```
file        ::= (item (SEP item)*)?

item        ::= 'pub'? 'abstract'? ('extern' extern_decl | decl)
              | equation
              | import_item
              | instance_item
decl        ::= signature | data_item | type_item | effect_item | fixity_item | class_item

extern_decl ::= signature | 'data' UIDENT LIDENT* | 'effect' UIDENT ('where' block(op_decl))?
signature   ::= var ':' context? type
var         ::= LIDENT | '(' OP ')'
equation    ::= LIDENT apat* '=' body
              | apat OP apat '=' body                    -- 演算子の定義
body        ::= block(stmt) | expr
context     ::= (btype | '(' btype (',' btype)* ')') '=>'

data_item   ::= 'data' UIDENT tparam* (':' UIDENT)? ('=' alts)? deriving?
tparam      ::= LIDENT | '<' LIDENT '>'                   -- 後者は row の型引数
alts        ::= block(alt) | alt+
alt         ::= '|' UIDENT type_atom*
              | '|' UIDENT '{' list(ftype) '}'            -- 名前付きのフィールド
              | '|' btype CONOP btype                     -- 中置のコンストラクタ
ftype       ::= LIDENT ':' type
deriving    ::= 'deriving' (qcon | '(' qcon (',' qcon)* ')')
type_item   ::= 'type' UIDENT LIDENT* '=' type
effect_item ::= 'effect' UIDENT LIDENT* 'where' block(op_decl)
op_decl     ::= ('never' | 'once' | 'multi')? LIDENT ':' context? type
fixity_item ::= ('infixl' | 'infixr' | 'infix') INT (OP | CONOP) (',' (OP | CONOP))*
class_item  ::= 'class' context? UIDENT LIDENT ('where' block(class_member))?
class_member ::= signature | equation
instance_item ::= 'instance' context? qcon type_atom ('where' block(inst_member))?
inst_member ::= equation | 'extern' var
import_item ::= 'import' modpath ('as' UIDENT)? ('(' list(import_name) ')')?
modpath     ::= UIDENT ('.' UIDENT)*
import_name ::= LIDENT | '(' OP ')' | UIDENT ('(' '..' ')')?

stmt        ::= 'let' pat (':' type)? '=' body
              | 'let' local_fn ('and' local_fn)*           -- ローカル関数の組
              | 'use' (pat '<-')? expr
              | 'var' LIDENT (':' type)? '=' body
              | expr

expr        ::= 'if' expr 'then' body ('else' body)?
              | 'match' expr 'with' arms
              | 'handle' expr ('from' expr)? 'with' clauses
              | 'fn' apat+ '->' body
              | 'let' pat (':' type)? '=' body 'in' expr
              | 'let' local_fn ('and' local_fn)* 'in' expr -- ローカル関数の組
              | op_expr '<-' expr                         -- 代入。右辺の続きに '<-' は書けない
              | op_expr
local_fn    ::= LIDENT apat+ '=' body
arms        ::= block(arm) | arm+
arm         ::= '|' pat '->' body
clauses     ::= block(clause) | clause+
clause      ::= '|' ('fn' | 'ctl') qvar apat* '->' body   -- 操作の節 (引数、ctl の k、状態)
              | '|' 'return' apat+ '->' body              -- return x [状態]

op_expr     ::= operand ((OP | CONOP) operand)*           -- CST では平たい列
operand     ::= '-' operand | app
app         ::= 'drop'? postfix+
postfix     ::= atom ('.' (LIDENT | INT) | '[' expr ']')*  -- '.' の前後に空白を置かない。'[' は下の補足

atom        ::= INT | FLOAT | BYTE | CHAR | string | RAW_STRING | command
              | qvar | qcon
              | '(' ')' | '(' expr ')' | '(' expr ':' type ')'
              | '(' expr (',' expr)+ ','? ')'
              | '[' list(expr) ']'
              | qcon '{' list(field) '}'                  -- 作る式
              | '{' op_expr '|' field (',' field)* ','? '}'   -- 更新
              | '(' (OP | CONOP) ')' | '(' (OP | CONOP) op_expr ')' | '(' op_expr (OP | CONOP) ')'
              | '(' '.' (LIDENT | INT) ')'
qvar        ::= (UIDENT '.')* LIDENT
qcon        ::= (UIDENT '.')* UIDENT
field       ::= LIDENT ('=' expr)?

pat         ::= cpat (CONOP pat)?                         -- 右結合 (例: x :: rest)
cpat        ::= qcon apat+ | apat
apat        ::= '_' | LIDENT | qcon | literal | '-' (INT | FLOAT | BYTE)
              | '(' ')' | '(' pat ')' | '(' pat (',' pat)+ ','? ')'
              | '(' pat ':' type ')'
              | '[' list(pat) ']'
              | qcon '{' list(fpat) '}'
fpat        ::= LIDENT ('=' pat)?
literal     ::= INT | FLOAT | BYTE | string | RAW_STRING | CHAR

type        ::= 'once'? btype ('->' row? type)?           -- 'once' の後ろは関数型
btype       ::= qcon targ* | type_atom
targ        ::= type_atom | row                           -- row は row の型引数
type_atom   ::= qcon | LIDENT
              | '(' type ')' | '(' type (',' type)+ ')'
row         ::= '<' '>' | '<' row_var '>' | '<' effect (',' effect)* ('|' row_var)? '>'
row_var     ::= 'never'? LIDENT
effect      ::= qcon type_atom*

string      ::= STRING_START (STRING_TEXT | ESCAPE | '\{' expr INTERP_END)* STRING_END
command     ::= CMD_START (CMD_TEXT | CMD_QUOTE | ESCAPE | '\{' '..'? expr INTERP_END)* CMD_END
```

## 文法上の補足

- `import_item` は、ファイルの先頭で宣言より前に書く。宣言の後の `import_item` は E0011 にする。`import_name` の `'(' OP ')'` は `:` で始まらない演算子なので、`((:+))` も E0011 にする。CST はどちらも組む ([モジュールと名前解決](modules.md) の「import」)
- `extern` は宣言の修飾子で、`pub` の後に書く。後ろに続けられるのは、等式のないシグネチャ、`=` と Kind の宣言のない `data` (型引数は型変数の名前だけ)、型引数なしの `effect` だけである。`effect` には、普通の `effect` と同じく `where` と操作の並びを書ける。ほかの形 (`extern type`、`extern infixl`、`extern data T = …`、`extern effect E = …`、型引数のある `effect`、`extern pub`) は E0011 にする。`extern` を書けるのは標準ライブラリのモジュールだけで、ユーザーのモジュールに書くと HIR で E1033 になる ([宣言](declarations.md) の「`extern`」)
- `abstract` は `pub` の後、`extern` の前に書く修飾子で、`data` と `effect` にだけ付く ([宣言](declarations.md) の「`pub`」)。ほかの宣言に付けたとき、`pub` がないとき、`extern` の宣言に付けたときは、`abstract` のトークンに E0011 を1つ出す (``` `abstract` is only allowed on `pub data` and `pub effect` ```)。`abstract pub` の順は、`extern pub` と同じく E0011 にして読み直す。クラスと instance のメンバーに書いた `abstract` は、`pub` と同じく E0011 にして読み飛ばす
- 文脈 `context` は、型として読める形 (`Eq a`、`(Eq a, Show b)`) の後に `=>` を書いたものである。parser は、`signature` と `op_decl` の `:` の後と、`class` と `instance` のキーワードの後で、括弧の入れ子の外の `=>` が型の終わりより前にあるかを先読みし、あれば文脈として読む。括弧の外の `->`、`=`、`where` とブロックの始まりは文脈の後ろにしか現れないので、そこで先読みをやめる (中置のコンストラクタの `alt` の先読みと同じ形)。CST は `CONTEXT` の節点の下に、制約ごとの `CONSTRAINT` を置く。制約の形 (クラスの名前と1つの型変数) は HIR で検査する (E1040)
- 文脈の構文があるのは、`signature` (トップレベルの関数、extern の関数、クラスのメソッド)、`op_decl`、`class` と `instance` の頭である。extern のシグネチャと操作のシグネチャの文脈は、構文として読んだうえで HIR が E1040 にする ([宣言](declarations.md) の「宣言の検査」)。本体の型の明示 `(e : T)`、ラムダの引数の型、`data` のフィールドの型には文脈の構文がなく、そこに書いた `=>` は E0011 になる
- `pub` は `class` に付けられる。`pub instance` は E0011 にする。instance は名前を持たず、いつもプログラム全体で見えるためである ([モジュールと名前解決](modules.md) の「instance の一貫性」)。`class` と `instance` のブロックの中の `pub` も E0011 にする。メソッドはクラスと一緒に公開するためである
- `class` の `where` のブロックには、メソッドのシグネチャと既定のメソッドの等式を並べる。`instance` の `where` のブロックには、メソッドの等式と `extern` の行だけを書ける。メソッドの型はクラスが決めるので、`instance` のブロックのシグネチャは E0011 にする (Haskell 98 と同じ)。CST には組んで回復する。`where` のない `class` と `instance` も書ける
- instance の頭は `type_atom` として読む。頭の形 (型コンストラクタに互いに異なる型変数を適用したもの) は HIR で検査する。そのため `instance Eq a`、`instance Eq (Int, Int)`、`instance Eq (Option Int)` は、構文の誤りでなく E1039 になる
- `deriving` は、`data` の最後の選択肢の後に置く。最後の選択肢と同じ行、選択肢のブロックの項目 (`|` と同じ列)、最後の選択肢の続きの行 (`|` より深い字下げ)、ブロックを閉じた後の続きの行 (`data` より深く `|` より浅い字下げ) のどれに書いてもよい。parser は、最後の選択肢の型を読む途中でも、ブロックの項目の始まりでも、ブロックを閉じた後でも `deriving` を受け付ける。`deriving` の後に選択肢が続いたら E0011 にする。選択肢のブロックの `deriving` は選択肢に数えないので、`=` の後のブロックに `deriving` しかなければ、`=` の後に選択肢がない誤り (E0011) にする。そのときは `deriving` がいくつあっても1件だけ報告する。`deriving` の句は1つの `data` に1つだけ書ける。2つ目からの句は E0011 にするが、クラスは読み、すべての句のクラスを導出する。使う位置に instance がない誤り (E2006) を連鎖させないためである

  ```haskell
  data Color = | Red | Green deriving (Eq, Show)

  data Option a =
    | None
    | Some a
    deriving (Eq, Ord, Show)
  ```
- 型の位置では、`<` で始まる演算子のトークン (`<>` など) と `>` で始まる演算子のトークン (`>->` など) を、parser が分割して読む (Rust が `>>` を分割するのと同じ)。`->` の直後に row を空白なしで書いた `-><` は、lexer が `->` と `<` に分ける ([字句](lexical.md) の「演算子」)
- 型の位置での `<` は row の開始だけを意味する。レイアウト段は、`->` の直後の `<` を row の始まりとして括弧に数える ([レイアウト規則](layout.md) の規則 4)
- `once` は、後ろに続く関数型の一番外側の矢印に付く。`once a -> b -> c` の `once` は `a -> b -> c` の最初の矢印に付き、返す `b -> c` には付かない。`a -> once b -> c` の `once` は、右の関数型 `once b -> c` に付く。引数の位置の関数型は括弧で包む (`(once a -> b) -> c`)。`once` の後ろに `->` が続かなければ E0011 にする ([型と Kind](types.md) の「1回だけ呼べる関数の型」)
- `data` の型引数の `<e>` は row の型引数で、`<` と `>` の間に書けるのは row 変数の名前1つだけである (それ以外は E0011)。型の中の型引数の位置に書く `<…>` は row である ([型と Kind](types.md) の「row の型引数」)。`:` 以降の `UIDENT` は Kind の宣言で、名前の検査は HIR が行う ([型と Kind](types.md) の「`Lin` の宣言」)。`extern data` の後ろに `:` を書いたら E0011 にする
- row 変数の前には、多重度の上限 `never` を書ける (`<never e>`、`<Panic | never e>`)。上限は row 変数に付くもので、row 全体には付かない。`never` の後ろが row 変数でない形 (`<never Panic | e>`、`<never>`) と、`never` 以外の多重度 (`<once e>`、`<multi e>`) は E0011 にし、row の残りは読んで回復する。`data` の型引数の `<e>` には上限を書けない (E0011)。上限の意味と書ける場所は [型と Kind](types.md) の「row 変数の多重度の上限」で定める
- リストの型の構文は `List a` だけで、`[a]` の形はない。型の位置の `[` は E0011 にする
- 式の `qcon '{'` は、`{` の後が `}`、`,`、`=` か、`LIDENT` の後に `=`、`,`、`}` が続くときだけ、作る式として1つの `atom` に読む。そのほかの `{` は、引数としての更新の始まりである。更新の元の値は `,` や `=` で始まらないので、`C { , }` や `C { = 1 }` は作る式として読み、フィールドの並びの誤りにする。そのため `Some { p | age = 1 }` は、`Some` に更新を渡す式として読める。作る式は適用より強く結びつくので、`Some Person { … }` は `Some (Person { … })` と読む (Haskell と同じ)
- 更新の元の値は `op_expr` として読む。`if`、`match`、`handle`、`fn`、`let ... in` の形は、演算の項と同じく括弧が要る (E0012)。`match` の枝の `|` と、更新の `|` を取り違えないためである。`{ p | }` のようにフィールドがなければ、フィールドを期待する E0011 にする
- パターンでは、`qcon` の後の `{` をいつも分解のパターンとして読む。`cpat` は、`qcon '{'` を `qcon apat+` より先に見る。演算子の等式の先読み (`apat_len`) は、`qcon '{' … '}'` を1つの `apat` として数える。そのため `P { a } <+> q = …` は演算子の等式として読める。トップレベルの `P { a } = x` は、`(a, b) = …` と同じく、`=` を指すトップレベルのパターンによる束縛の E0011 になる
- コンストラクタのない `{` は、型、パターン、式のどの位置でも E0011 にし、対応する `}` まで読み飛ばす。中のフィールドの名前から E1001 や型の誤りを連鎖させないためである。型の位置では型を、パターンの位置ではパターンを期待する誤りにする。式の位置の `{` は更新として読み始め、元の値の後に `|` がなければ `|` を期待する誤りにする。無名のレコード (`{ name = "a" }`)、レコードの型 (`{ name : String }`)、レコードの拡張は、どれもこの誤りになる。そのため、parser が E0004 を出す構文はない
- レコードの CST は次の節点で組む。typed AST は、`ast::RecordExpr` と `ast::UpdateExpr` (`ast::Expr` の選択肢)、`ast::RecordPat` (`ast::Pat` の選択肢)、`ast::Field`、`ast::FieldPat`、`ast::RecordFields`、`ast::FieldDecl` である

  | 節点 | 子 |
  |---|---|
  | `RECORD_EXPR` | `PATH`、`{`、`FIELD` と `,` の並び、`}` |
  | `FIELD` | `NAME_REF`、省略しなければ `=` と式 |
  | `UPDATE_EXPR` | `{`、式、`\|`、`FIELD` と `,` の並び、`}` |
  | `RECORD_PAT` | `PATH`、`{`、`FIELD_PAT` と `,` の並び、`}` |
  | `FIELD_PAT` | `NAME_REF`、省略しなければ `=` とパターン |
  | `RECORD_FIELDS` (`ALT` の子) | `{`、`FIELD_DECL` と `,` の並び、`}` |
  | `FIELD_DECL` | `NAME`、`:`、型 |
- `postfix` とセクションの `.INT` の番号は、先頭に 0 のない10進数 (`0`、`1`、`12`) で、`u32` に収まるものに限る。`t.01`、`t.1_0`、`t.0x1` と範囲外の番号は、フィールドの番号を期待する E0011 にし、そのトークンを読んで先へ進む。`t.0.1` は2回の射影である。空白のない `.` の直後の `0.1` は、lexer が `INT` `.` `INT` に分ける ([字句](lexical.md) の「数値と文字」)
- `var` の文はブロックの中にだけ書ける。トップレベルの項目と REPL の入力の形にはない。`var` の後には名前を1つだけ書く。ほかのトークン (`var (a, b) = p` の `(` など) が続けば、変数の名前を期待する E0011 にし、文の終わりまでを `ERROR` に入れる
- 代入 `lhs <- e` は、優先順位が最も低い `expr` の層の形である。文、ラムダの本体、`if` の枝、`match` の腕に括弧なしで書ける (`for 0 n (fn i -> a[i] <- i)`)。左辺は `op_expr` として読み、`var` の名前か、その名前の添字かは HIR が確かめる。右辺は式で、`<-` はブロックの開始トークンではない。長い右辺は、深く字下げした次の行に式の続きとして書く ([レイアウト規則](layout.md) の規則 1)
- `<-` は、代入でも `use p <- e` でも、左辺が右辺から値を受け取ることだけを表す。そのため、どちらの右辺でも式の続きに `<-` を書くと (`x <- y <- 1`、`use p <- f <- 1`)、E0011 にし、2つ目の `<-` から後ろを `ERROR` に入れる。`use` の文は、括弧とブロックの外に `<-` があればパターンのある形として読むので、パターンのない `use` の式に括弧なしの代入は書けない。括弧の中の要素、リストの要素、添字の中は `op_expr` か `expr` の形として読むので、`if` の枝などの中を除いて `<-` を書けない (`(x <- 1)` は `)` を期待する E0011)
- 後置の添字 `e[i]` は射影と同じく `postfix` の連鎖で、関数適用より強く結びつく (`f xs[i]` は `f (xs[i])`、`a[i][j]` も書ける)。`[` の直前に空白がなく、直前のトークンが本物のトークンで、名前 (`LIDENT`、`UIDENT`。修飾した名前の最後の部分も含む)、閉じ括弧 (`)`、`]`、`}`)、射影の番号 (`t.0[i]` の `0`) のどれかであるときだけ、添字として読む。字句は変えず、parser が前のトークンとの間を見る。空白があれば (`f [i]`)、リストを渡す関数適用である。レイアウト段の仮想トークンは幅 0 で次のトークンの先頭に置かれるので、前のトークンに数えない。そのため、行頭の `[` は前の行の続きの添字にならない。整数のリテラルの後の `[` (`1[0]`) も添字ではない。添字の中はリストの要素と同じく読む
- `var` の文、代入、添字の CST は次の節点で組む。typed AST は `ast::VarStmt` (`ast::Stmt` の選択肢。`name`、`ty`、`init`)、`ast::AssignExpr` (`target`、`value`)、`ast::IndexExpr` (`base`、`index`) である。後の2つは `ast::Expr` の選択肢である

  | 節点 | 子 |
  |---|---|
  | `VAR_STMT` | `var`、`NAME`、型があれば `:` と型、`=`、本体 |
  | `ASSIGN_EXPR` | 左辺の式、`<-`、右辺の式 |
  | `INDEX_EXPR` | 添字をとる式、`[`、添字の式、`]` |
- `qvar` / `qcon` の `.` と、`postfix` の `.` は、前後に空白を置かない (置くと E0010)。`Upper.lower` は修飾された名前、`lower.lower` は射影である
- 式の形は `expr`、`operand`、`postfix` の3つの層に分かれる。上の層の形を下の層の位置に括弧なしで書くと E0012 にする
  - `if`、`match`、`handle`、`fn`、`let ... in` は `expr` の層の形で、末尾の本体が右へできるだけ伸びる。関数の引数や演算の項にするときは括弧で囲む。これにより、`match e with` の `e` は `with` の手前で終わる
  - `drop` の適用は `operand` の層の形で、引数が atom なので右へ伸びない。演算の項には書けるが、関数の引数にするときは括弧で囲む
- `let` が関数の形かどうかは、`let` の後の2つのトークンで決める。`let` の次が `LIDENT` で、その次が `LIDENT`、`_`、`UIDENT`、整数、浮動小数、`Byte`、文字列の始まり (`STRING_START`、`RAW_STRING`)、文字、`(`、`[`、`{` のどれかなら、ローカル関数である。浮動小数は `apat` の `literal` で、HIR が E1052 にする。`Byte` の負のリテラル `'-' BYTE` は E1054 にする。`{` は、コンストラクタのない `{` をパターンの位置ではパターンを期待する誤りにする規則に合わせて数える。`-` は含めない。`apat` は `'-' INT` を含むが、トップレベルで `LIDENT OP` を演算子の定義として先に読むのと揃えるためである。次の形はパターンの束縛のまま読む
  - `let x = e` と `let x : T = e` (名前の次が `=` か `:`)
  - `let (f, text) = e`、`let Some x = e`、`let P { name = n } = e` (最初がパターン)
  - `let x :: rest = e` (名前の次が演算子)
- ローカル関数の引数の後に `:` が来たら、``a local function cannot declare its result type`` (help は ``annotate a parameter instead, as in `(x : Int)` ``) の E0011 にする。`and` の束縛でも同じである
- ローカル関数の組は、関数の形の `let` の束縛の後に、`and` で始まる束縛を続けて読む。行の先頭の `and` の直前の `SEP` は1つ読み飛ばす ([レイアウト規則](layout.md) の「`then` / `else` / `and` の前の `SEP`」)。CST では、`and` の束縛ごとに `AND_BINDING` の節点を `LET_STMT` か `LET_EXPR` の子に置く。`AND_BINDING` は、`and`、名前のパターン、`PARAM_LIST`、`=`、本体を持つ。typed AST の `ast::LetStmt` と `ast::LetExpr` は、`and_bindings` で `ast::AndBinding` を並べる。次の形は E0011 にし、束縛として読んで回復する
  - 値の `let` の後の `and` (`` `and` follows a function-form `let`, not a value `let` ``)
  - 引数のない `and` の束縛 (`and x = 1`。`` `and` defines a local function, so it needs parameters ``)
  - `and` の後が名前でない束縛 (名前を期待する誤り)
- 操作の節の `fn` と `ctl` は、文法では省ける形で読む。`return` の節に付いた `fn` / `ctl` (`| fn return x -> e`) も読む。どちらの誤りも HIR が E1051 にする ([式](expressions.md) の「handler」)。`fn` を書けない `never` の操作の節を見分けるには、操作の宣言が要るためである。`OpClause` と `ReturnClause` の `keyword` は、`|` の直後の `fn` か `ctl` を返す
- トップレベルでパターンによる束縛 (`(a, b) = ...`) は書けない。トップレベルの値は、関数と同じく `LIDENT apat* '='` (引数 0 個) で定義する
- 等式が `LIDENT OP` で始まる場合は演算子の定義、`LIDENT` の後に `=` か `apat` が続く場合は関数の定義である。2トークンの先読みで区別する
- `A -> <E> B -> C` の row `<E>` は最初の `->` に付き、`A -> <E> (B -> C)` と読む。内側の `B -> C` の row は省略扱いとする
- `handle` の節の引数の個数と `drop` の引数の個数は、文法では制限せず HIR で検査する ([式](expressions.md) の「handler」と「パラメータ付き handler」)
- セクションの被演算子は演算子の列でもよい。優先順位による可否は HIR で検査する ([式](expressions.md) の「セクション」)
- `string` と `command` のトークンは [字句](lexical.md) の「文字列のトークン」にある。穴は `INTERP_END` でいつも終わる。lexer が、閉じていない穴にも幅 0 の `INTERP_END` を出すためである
- 穴に式がない (`"\{}"`) ときは、式を期待する E0011 にする。文字列の穴に `..` を書いたときも同じである。`..` を読むのはコマンドリテラルの穴だけである。式の後に `INTERP_END` 以外が続けば E0011 を出し、その穴の `INTERP_END` まで読み飛ばす。幅 0 の `INTERP_END` (閉じていない穴) の手前では、lexer が報告済みなので E0011 を出さない
- パターンの浮動小数のリテラル (`1.5`、`-2.5`) は、構文としては読み、HIR が E1052 にする ([網羅性](exhaustiveness.md))
- パターンの文字列には補間を書けない。パーサは穴ごとに、穴の開き `\{` を指す E0011 を出し、CST には組む。穴のない複数行の文字列と raw 文字列は、リテラルのパターンに書ける。コマンドリテラルは `apat` の始まりでないので、パターンに書くと E0011 になる
- 式・パターン・型の入れ子の深さは 256 までとする。深さは parser の再帰 (式、演算子の列、パターン、型) の段数で数える。連鎖 (演算子の列の演算子と前置の `-`、射影 `.x`、添字 `[i]`、中置のコンストラクタのパターン) も、1つごとに1段と数える。HIR がこれらを fixity に従って1つごとに1段深い木に組み直すためである。組み直した木では、先に読み終えた被演算子が後の段の下に来うるので、後の段は、それまでの被演算子の高さの最大を下に確保して数える。添字の連鎖でも、先に読んだ添字の式の高さに1段足したものを、後の段の下に確保する。ブロックの `use` の文も、残りの文を1段深く数える。HIR が残りの文をラムダで包むためである ([式](expressions.md) の「`use`」)。超えたら E0013 を出し、今の括弧かブロックの中身を読み飛ばして、その項目の中では連鎖する診断を出さない。後の段階の再帰がスタックを溢れさせないよう、parser で止める
  - `[` は、括弧と同じく1段に数える。パターンのリストでは、k 番目 (1 始まり) の要素を、`[` の中の深さに k - 1 を足した深さで読む。HIR がパターンのリストを右に入れ子の `::` の木に組むためである ([式](expressions.md) の「リスト」)。組んだ木では前の要素がいつも浅い位置に来るので、中置のコンストラクタのパターンと違い、前の要素の高さを下に確保しない
  - 式のリストは、要素の数を深さに数えない。HIR が平らな節点で持ち、後の段階もループでたどるためである
  - レコードの `{` (作る式、更新、分解のパターン、宣言のフィールド) は、括弧と同じく1段に数える。フィールドの数は深さに数えない
  - 補間の穴は、括弧と同じく1段に数える。1つの文字列の穴の数は、深さに数えない。HIR が穴を平らな列で持つためで、式のリストと同じ扱いである

## REPL の入力

REPL の入力は、ファイルの文法でなく、入力のエントリ (`eml_syntax::parse_input`) で読む。入力は、item の並び、`let` の束縛、式のどれかで、構文木の根の種類 (`REPL_ITEMS`、`REPL_LET`、`REPL_EXPR`) で区別する。

- 最初の項目を、ファイルと同じ先読みで item と判定できれば、item の並びとして読む。並びの規則はファイルと同じである。ただし等式の形 (`f x …`、`a <+> b …`) は、最初の項目の中で、括弧の外にあり、かつレイアウトのブロックが開く前に `=` があるときだけ item とみなす。そのため、`f x` と `1 + 2` は式である。演算子のシグネチャは、`(` 演算子 `)` の後に `:` が続く形である。ファイルでも同じで、`(+) 1 2` の形はシグネチャとして読まない
- `let` で始まり、束縛の後ろに `in` が続かなければ、`let` の束縛である。局所の `let` の文と同じ文法で読むので、型の明示 (`let x : T = e`) と、行末の `=` の後のブロックを書ける。名前でないパターンによる束縛は E0011 にする。関数の形 (`let f x = 1`) は、``a `let` input cannot define a function`` (help は ``write a signature and an equation``) の E0011 を1件だけ出す。ローカル関数は、入力の中のブロックと `let … in` の式なら書ける
- どれでもなければ式である。`let … in …` も式である
- `let` の束縛と式は、ちょうど1つの項目でなければならない。列 0 の `SEP` か `;` で2つ目の項目を続けたら、構文の誤り (E0011) にする
- trivia だけの入力 (空、コメントだけ) は、要素のない `REPL_ITEMS` になる。REPL は何もせず、入力の番号も進めない
- `:type` の問い合わせ ([REPL の端末](repl.md) の「型の表示」) は、入力のエントリでなく、問い合わせのエントリ (`eml_syntax::parse_query`) で読む。根は `REPL_QUERY` で、式をちょうど1つ持つ。入力の読み分けの先読みはしないので、`f x = 1` も式として読み始め、`=` で E0011 (`':type' takes one expression`) にする。`let x = 1` と `data …` は、式の文法のままの誤り (``expected `in` ``、`expected an expression`) になる
- `:load` のファイル ([REPL の端末](repl.md) の「ファイルの読み込み」) は、セッションの入力だが、ファイルの文法 (`eml_syntax::parse`) で読む
