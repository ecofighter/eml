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

item        ::= 'pub'? ('extern' extern_decl | decl)
              | equation
              | import_item
              | instance_item
decl        ::= signature | data_item | type_item | effect_item | fixity_item | class_item

extern_decl ::= signature | 'data' UIDENT | 'effect' UIDENT
signature   ::= var ':' context? type
var         ::= LIDENT | '(' OP ')'
equation    ::= LIDENT apat* '=' body
              | apat OP apat '=' body                    -- 演算子の定義
body        ::= block(stmt) | expr
context     ::= (btype | '(' btype (',' btype)* ')') '=>'

data_item   ::= 'data' UIDENT LIDENT* ('=' alts)? deriving?
alts        ::= block(alt) | alt+
alt         ::= '|' UIDENT type_atom*
              | '|' btype CONOP btype                     -- 中置のコンストラクタ
deriving    ::= 'deriving' (qcon | '(' qcon (',' qcon)* ')')
type_item   ::= 'type' UIDENT LIDENT* '=' type
effect_item ::= 'effect' UIDENT LIDENT* 'where' block(op_decl)
op_decl     ::= ('never' | 'once' | 'multi')? LIDENT ':' context? type
fixity_item ::= ('infixl' | 'infixr' | 'infix') INT OP (',' OP)*
class_item  ::= 'class' context? UIDENT LIDENT ('where' block(class_member))?
class_member ::= signature | equation
instance_item ::= 'instance' context? qcon type_atom ('where' block(inst_member))?
inst_member ::= equation | 'extern' var
import_item ::= 'import' modpath ('as' UIDENT)? ('(' list(import_name) ')')?
modpath     ::= UIDENT ('.' UIDENT)*
import_name ::= LIDENT | '(' OP ')' | UIDENT ('(' '..' ')')?

stmt        ::= 'let' pat (':' type)? '=' body
              | 'use' (pat '<-')? expr
              | expr

expr        ::= 'if' expr 'then' body ('else' body)?
              | 'match' expr 'with' arms
              | 'handle' expr ('from' expr)? 'with' clauses
              | 'fn' apat+ '->' body
              | 'let' pat (':' type)? '=' body 'in' expr
              | op_expr
arms        ::= block(arm) | arm+
arm         ::= '|' pat '->' body
clauses     ::= block(clause) | clause+
clause      ::= '|' qvar apat* '->' body                  -- 操作の節 (引数、k、状態)
              | '|' 'return' apat+ '->' body              -- return x [状態]

op_expr     ::= operand (OP operand)*                     -- CST では平たい列
operand     ::= '-' operand | app
app         ::= 'drop'? postfix+
postfix     ::= atom ('.' (LIDENT | INT))*                -- '.' の前後に空白を置かない

atom        ::= INT | FLOAT | CHAR | string | RAW_STRING | command
              | qvar | qcon
              | '(' ')' | '(' expr ')' | '(' expr ':' type ')'
              | '(' expr (',' expr)+ ','? ')'
              | '[' list(expr) ']'
              | '{' list(field) '}' | '{' expr 'with' list(field) '}'
              | '{' list(field) '|' expr '}'              -- 拡張
              | '(' OP ')' | '(' OP op_expr ')' | '(' op_expr OP ')' | '(' '.' LIDENT ')'
qvar        ::= (UIDENT '.')* LIDENT
qcon        ::= (UIDENT '.')* UIDENT
field       ::= LIDENT ('=' expr)?

pat         ::= cpat (CONOP pat)?                         -- 右結合 (例: x :: rest)
cpat        ::= qcon apat+ | apat
apat        ::= '_' | LIDENT | qcon | literal | '-' INT
              | '(' ')' | '(' pat ')' | '(' pat (',' pat)+ ','? ')'
              | '(' pat ':' type ')'
              | '[' list(pat) ']'
              | '{' list(fpat) ('|' LIDENT)? '}'
fpat        ::= LIDENT ('=' pat)?
literal     ::= INT | string | CHAR

type        ::= btype ('->' row? type)?
btype       ::= qcon type_atom* | type_atom
type_atom   ::= qcon | LIDENT
              | '(' type ')' | '(' type (',' type)+ ')'
              | '{' list(ftype) ('|' LIDENT)? '}'
ftype       ::= LIDENT ':' type
row         ::= '<' '>' | '<' LIDENT '>' | '<' effect (',' effect)* ('|' LIDENT)? '>'
effect      ::= qcon type_atom*

string      ::= STRING_START (STRING_TEXT | ESCAPE | '\{' expr '}')* STRING_END
command     ::= CMD_START (CMD_TEXT | ESCAPE | '\{' '..'? expr '}')* CMD_END
```

## 文法上の補足

- `import_item` は、ファイルの先頭で宣言より前に書く。宣言の後の `import_item` は E0011 にする。`import_name` の `'(' OP ')'` は `:` で始まらない演算子なので、`((:+))` も E0011 にする。CST はどちらも組む ([モジュールと名前解決](modules.md) の「import」)
- `extern` は宣言の修飾子で、`pub` の後に書く。後ろに続けられるのは、等式のないシグネチャ、`=` のない型引数なしの `data`、`where` のない型引数なしの `effect` だけである。ほかの形 (`extern type`、`extern infixl`、`extern data T = …`、`extern effect E where …`、型引数のある `data` と `effect`、`extern pub`) は E0011 にする。`extern` を書けるのは標準ライブラリのモジュールだけで、ユーザーのモジュールに書くと HIR で E1033 になる ([宣言](declarations.md) の「`extern`」)
- 文脈 `context` は、型として読める形 (`Eq a`、`(Eq a, Show b)`) の後に `=>` を書いたものである。parser は、`signature` と `op_decl` の `:` の後と、`class` と `instance` のキーワードの後で、括弧の入れ子の外の `=>` が型の終わりより前にあるかを先読みし、あれば文脈として読む。括弧の外の `->`、`=`、`where` とブロックの始まりは文脈の後ろにしか現れないので、そこで先読みをやめる (中置のコンストラクタの `alt` の先読みと同じ形)。CST は `CONTEXT` の節点の下に、制約ごとの `CONSTRAINT` を置く。制約の形 (クラスの名前と1つの型変数) は HIR で検査する (E1040)
- 文脈の構文があるのは、`signature` (トップレベルの関数、extern の関数、クラスのメソッド)、`op_decl`、`class` と `instance` の頭である。extern のシグネチャと操作のシグネチャの文脈は、構文として読んだうえで HIR が E1040 にする ([宣言](declarations.md) の「宣言の検査」)。本体の型の明示 `(e : T)`、ラムダの引数の型、`data` のフィールドの型には文脈の構文がなく、そこに書いた `=>` は E0011 になる
- `pub` は `class` に付けられる。`pub instance` は E0011 にする。instance は名前を持たず、いつもプログラム全体で見えるためである ([モジュールと名前解決](modules.md) の「instance の一貫性」)。`class` と `instance` のブロックの中の `pub` も E0011 にする。メソッドはクラスと一緒に公開するためである
- `class` の `where` のブロックには、メソッドのシグネチャと既定のメソッドの等式を並べる。`instance` の `where` のブロックには、メソッドの等式と `extern` の行だけを書ける。メソッドの型はクラスが決めるので、`instance` のブロックのシグネチャは E0011 にする (Haskell 98 と同じ)。CST には組んで回復する。`where` のない `class` と `instance` も書ける
- instance の頭は `type_atom` として読む。頭の形 (型コンストラクタに互いに異なる型変数を適用したもの) は HIR で検査する。そのため `instance Eq a`、`instance Eq (Int, Int)`、`instance Eq (Option Int)` は、構文の誤りでなく E1039 になる
- `deriving` は、`data` の最後の選択肢の後に置く。最後の選択肢と同じ行、選択肢のブロックの項目 (`|` と同じ列)、最後の選択肢の続きの行 (`|` より深い字下げ)、ブロックを閉じた後の続きの行 (`data` より深く `|` より浅い字下げ) のどれに書いてもよい。parser は、最後の選択肢の型を読む途中でも、ブロックの項目の始まりでも、ブロックを閉じた後でも `deriving` を受け付ける。`deriving` の後に選択肢が続いたら E0011 にする。選択肢のブロックの `deriving` は選択肢に数えないので、`=` の後のブロックに `deriving` しかなければ、`=` の後に選択肢がない誤り (E0011) にする

  ```haskell
  data Color = | Red | Green deriving (Eq, Show)

  data Option a =
    | None
    | Some a
    deriving (Eq, Ord, Show)
  ```
- 型の位置では、`<` で始まる演算子のトークン (`<>` など) と `>` で始まる演算子のトークン (`>->` など) を、parser が分割して読む (Rust が `>>` を分割するのと同じ)。`->` の直後に row を空白なしで書いた `-><` も、`->` と `<` に分割して読む (`Int -><IO> Int`)
- 型の位置での `<` は row の開始だけを意味する
- `postfix` の `.INT` で、lexer が `t.0.1` の `0.1` を浮動小数のトークンにした場合、parser がフィールドアクセスの位置で分割する (Rust と同じ)
- `qvar` / `qcon` の `.` と、`postfix` の `.` は、前後に空白を置かない (置くと E0010)。`Upper.lower` は修飾された名前、`lower.lower` はフィールドアクセスである
- 式の形は `expr`、`operand`、`postfix` の3つの層に分かれる。上の層の形を下の層の位置に括弧なしで書くと E0012 にする
  - `if`、`match`、`handle`、`fn`、`let ... in` は `expr` の層の形で、末尾の本体が右へできるだけ伸びる。関数の引数や演算の項にするときは括弧で囲む。これにより、`match e with` の `e` は `with` の手前で終わる
  - `drop` の適用は `operand` の層の形で、引数が atom なので右へ伸びない。演算の項には書けるが、関数の引数にするときは括弧で囲む
- トップレベルでパターンによる束縛 (`(a, b) = ...`) は書けない。トップレベルの値は、関数と同じく `LIDENT apat* '='` (引数 0 個) で定義する
- 等式が `LIDENT OP` で始まる場合は演算子の定義、`LIDENT` の後に `=` か `apat` が続く場合は関数の定義である。2トークンの先読みで区別する
- `A -> <E> B -> C` の row `<E>` は最初の `->` に付き、`A -> <E> (B -> C)` と読む。内側の `B -> C` の row は省略扱いとする
- `handle` の節の引数の個数と `drop` の引数の個数は、文法では制限せず HIR で検査する ([式](expressions.md) の「handler」と「パラメータ付き handler」)
- セクションの被演算子は演算子の列でもよい。優先順位による可否は HIR で検査する ([式](expressions.md) の「セクション」)
- 式・パターン・型の入れ子の深さは 256 までとする。深さは parser の再帰 (式、演算子の列、パターン、型) の段数で数える。連鎖 (演算子の列の演算子と前置の `-`、フィールドの参照 `.x`、中置のコンストラクタのパターン) も、1つごとに1段と数える。HIR がこれらを fixity に従って1つごとに1段深い木に組み直すためである。組み直した木では、先に読み終えた被演算子が後の段の下に来うるので、後の段は、それまでの被演算子の高さの最大を下に確保して数える。ブロックの `use` の文も、残りの文を1段深く数える。HIR が残りの文をラムダで包むためである ([式](expressions.md) の「`use`」)。超えたら E0013 を出し、今の括弧かブロックの中身を読み飛ばして、その項目の中では連鎖する診断を出さない。後の段階の再帰がスタックを溢れさせないよう、parser で止める
