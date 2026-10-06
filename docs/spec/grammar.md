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

item        ::= 'pub'? decl
              | equation
              | import_item
decl        ::= signature | data_item | type_item | effect_item | fixity_item

signature   ::= var ':' type
var         ::= LIDENT | '(' OP ')'
equation    ::= LIDENT apat* '=' body
              | apat OP apat '=' body                    -- 演算子の定義
body        ::= block(stmt) | expr

data_item   ::= 'data' UIDENT LIDENT* ('=' alts)?
alts        ::= block(alt) | alt+
alt         ::= '|' UIDENT type_atom*
              | '|' btype CONOP btype                     -- 中置のコンストラクタ
type_item   ::= 'type' UIDENT LIDENT* '=' type
effect_item ::= 'effect' UIDENT LIDENT* 'where' block(op_decl)
op_decl     ::= ('never' | 'once' | 'multi')? LIDENT ':' type
fixity_item ::= ('infixl' | 'infixr' | 'infix') INT OP (',' OP)*
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
              | 'let' pat (':' type)? '=' expr 'in' expr
              | op_expr
arms        ::= block(arm) | arm+
arm         ::= '|' pat '->' body
clauses     ::= block(clause) | clause+
clause      ::= '|' LIDENT apat* '->' body                -- 操作の節 (引数、k、状態)
              | '|' 'return' apat+ '->' body              -- return x [状態]

op_expr     ::= operand (OP operand)*                     -- CST では平たい列
operand     ::= '-' operand | app
app         ::= ('resume' | 'drop')? postfix+ lambda?
              | lambda
lambda      ::= 'fn' apat+ '->' body                      -- 最後の引数のラムダ
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

- 型の位置では、`<` で始まる演算子のトークン (`<>` など) と `>` で始まる演算子のトークン (`>->` など) を、parser が分割して読む (Rust が `>>` を分割するのと同じ)。`->` の直後に row を空白なしで書いた `-><` も、`->` と `<` に分割して読む (`Int -><IO> Int`)
- 型の位置での `<` は row の開始だけを意味する
- `postfix` の `.INT` で、lexer が `t.0.1` の `0.1` を浮動小数のトークンにした場合、parser がフィールドアクセスの位置で分割する (Rust と同じ)
- `qvar` / `qcon` の `.` と、`postfix` の `.` は、前後に空白を置かない (置くと E0010)。`Upper.lower` は修飾された名前、`lower.lower` はフィールドアクセスである
- `match`、`handle`、`if`、`fn` (最後の引数の位置を除く)、`let ... in` は atom ではない。関数の引数や演算の項にするときは括弧で囲む。これにより、`match e with` の `e` は `with` の手前で終わる
- トップレベルでパターンによる束縛 (`(a, b) = ...`) は書けない。トップレベルの値は、関数と同じく `LIDENT apat* '='` (引数 0 個) で定義する
- 等式が `LIDENT OP` で始まる場合は演算子の定義、`LIDENT` の後に `=` か `apat` が続く場合は関数の定義である。2トークンの先読みで区別する
- `A -> <E> B -> C` の row `<E>` は最初の `->` に付き、`A -> <E> (B -> C)` と読む。内側の `B -> C` の row は省略扱いとする
- `handle` の節の引数の個数と `resume` の引数の個数は、文法では制限せず HIR で検査する ([式](expressions.md) の「handler」と「パラメータ付き handler」)
- セクションの被演算子は演算子の列でもよい。優先順位による可否は HIR で検査する ([式](expressions.md) の「セクション」)
- 式・パターン・型の入れ子の深さは 256 までとする。深さは parser の再帰 (式、演算子の列、パターン、型) の段数で数える。超えたら E0013 を出し、今の括弧かブロックの中身を読み飛ばして、その項目の中では連鎖する診断を出さない。後の段階の再帰がスタックを溢れさせないよう、parser で止める

## 実装の段階

文法はすべての段階の構文を含む。マイルストーン1 の範囲外の機能 (レコード、モジュールなど) も構文の置き場所を先に決めてあるのは、後から入れても文法を作り直さずに済むようにするためである。S1 で実装したのはマイルストーン1 の機能の文法で、次のとおり。

- 宣言: シグネチャ、等式、`data`、`effect`、fixity
- 式: `let`、`let ... in`、`if`、`match`、`fn`、`handle ... with` / `from`、`use`、`resume`、`drop`、演算子の列、セクション、最後の引数のラムダ、タプル、型の明示
- パターン、型と row、エスケープだけの通常の文字列

名前付きのレコードと `type`、モジュールと import / `pub`、リストのリテラル、補間、複数行の文字列、raw 文字列は S2 で、コマンドリテラルは S3 で実装する。段階の全体は [実装の現在地](../implementation/status.md) にまとめてある。

S2 と S3 の構文は、パーサが CST まで組み、HIR が E0004 (未対応) を出す。意味を与える最初の段階が未対応を判定するという方針である。HIR が E0004 を出すのは、import、`pub`、`type` の宣言、修飾名 (式、型、パターン、row のエフェクト)、`::` (式とパターン)、フィールドの参照と `(.x)`、浮動小数、文字、複数行の文字列、raw 文字列である。例外は2つあり、どちらもパーサか lexer が E0004 を出す。

- 補間とコマンドリテラル: 穴を読むには lexer のモードが要るので、S2 と S3 で lexer と一緒に作り直す。補間は lexer が、コマンドリテラルはパーサが E0004 を出す
- `{…}` と `[…]` で書くレコードとリスト (式、パターン、型): レイアウト規則3とレコードの `with` の衝突を S2 で解くまで、CST を組まず、パーサが E0004 を出す
