# eml 構文設計 spec (本番の構文)

- 日付: 2026-10-03
- 状態: レビュー待ち
- 関連: [言語設計 spec](2026-10-03-eml-language-design.md) (以下「言語 spec」)、[マルチコア対応の設計 spec](2026-10-03-eml-multicore-design.md) (以下「マルチコア spec」)

## 1. 目的と範囲

### 目的

言語 spec §7 の暫定構文を置き換える、本番の構文を定める。プログラム例を書きながら議論して決めた。

- 主目的 (言語 spec §1) に合わせ、「線形型 × 代数的エフェクト」のプログラムが自然に読み書きできること
- 副目的に合わせ、日常のスクリプト (テキスト・ファイル処理、シェル自動化、データ変換、ネットワーク・並行) が短く安全に書けること
- 文法に曖昧さがなく、rowan とイベント方式のパーサ (言語 spec §3) で素直に実装できること

### 範囲

- 字句、レイアウト規則、文法、各構文の意味 (脱糖) を定める
- 構文の決定に伴って必要になった意味論の変更 (直積型の構成など) も、この spec で定め、言語 spec への変更点として §11 にまとめる
- M1 の範囲外の機能 (レコード、モジュールなど) も、構文の置き場所は今決める。後から入れても文法が壊れないようにするため
- 標準ライブラリの API の設計は対象外とし、別の spec で扱う。この spec では、構文から標準ライブラリへの申し送りだけを §14 に書く

## 2. 確定した設計判断

| 領域 | 決定 |
|---|---|
| 全体の方針 | Haskell 流を土台にする。F# から、逐次実行 (`do` なしで行を並べる)、`\|>` のパイプライン、`match` / `handle` の `with` と `\|` の枝、`List.map` 形式のモジュール修飾を借りる |
| レイアウト | オフサイドルール。開始トークン (`=` `->` `with` `then` `else` `where`) で行が終わり、次の行が深いときだけブロックを開く (§4) |
| トップレベルのシグネチャ | Haskell / Idris と同じく、別の行に書く。必須であることは HIR で検査する。複数の等式で定義できる |
| ラムダ | `fn x y -> e` |
| `if` | then 節が `Unit` なら `else` を省略できる |
| `match` / handler の枝 | `\|` を必須にし、字下げする。1行でも書ける。直和型の宣言も同じ形 |
| 型の宣言 | `data` は直和型、`type` は型の別名 |
| 直積型 | SML 方式。タプルは数字ラベルのレコード、Unit は `{}`。構造的な row 多相のレコード。Kind はフィールドの Kind の join で推論する (§8) |
| エフェクトの宣言 | `effect Name a where` の後に操作を並べる。操作のカリー化を許す |
| `use` | ブロックの残りを、最後の引数のラムダとして渡す糖衣構文 |
| パラメータ付き handler | `handle e from init with`。状態を節の最後の引数で受ける糖衣構文 |
| 線形値の受け渡し | 糖衣構文は入れない。再束縛、`use`、パラメータ付き handler で吸収する |
| 文字列 | 補間は `"\{x}"`。複数行の `"""`、raw の `r"..."` |
| コマンドリテラル | バッククォート。シェルを介さず、引数のリストを組む (Julia 方式) |
| fixity | ユーザーが宣言する。優先順位は整数 0〜9。CST では演算子の列を平たいまま持ち、HIR で組み直す |
| モジュール | 1ファイル = 1モジュール。`pub` で公開する。import は既定で修飾付き。標準ライブラリは import なしで修飾付きで使える |
| 並行処理 | 新しい構文は追加しない。エフェクトにより直接の形で書ける |
| コメント | `--` と、入れ子にできる `{- -}` |

## 3. 字句

### 文字コードと先頭行

- ソースは UTF-8。識別子は ASCII に限る。文字列とコメントには任意の文字を書ける
- ファイルの先頭行が `#!` で始まる場合、その行全体を `SHEBANG` トークンとし、trivia として扱う

### 空白とコメント

- 空白はスペースと改行 (`\n`、`\r\n`)。**インデントにタブを使うことはエラー**とする (E0xxx。番号は実装計画で割り当てる。以下同じ)
- 行コメントは `--` から行末まで。ただし、`--` の後ろに演算子の文字が続く場合 (例: `-->`) は演算子とする (Haskell と同じ規則)
- ブロックコメントは `{-` から `-}` まで。入れ子にできる
- ドキュメントコメントは `-- |` で始まる行コメント (Haddock と同じ)。字句としては普通のコメントで、ドキュメント生成と LSP の hover が解釈する

### 識別子

| 種類 | 規則 | 用途 |
|---|---|---|
| 小文字識別子 | `[a-z_][A-Za-z0-9_']*` (`_` 単独を除く) | 変数、関数、型変数、row 変数、操作名、フィールド名 |
| 大文字識別子 | `[A-Z][A-Za-z0-9_']*` | 型名、コンストラクタ、エフェクト名、モジュール名 |
| `_` 単独 | 専用のトークン | ワイルドカード |

- `x'` のように `'` を含められる (Haskell と同じ)
- 「大文字 = 具体的なもの、小文字 = 変数」の区別は暫定構文から変えない

### キーワード

```
data type effect where pub import as infixl infixr infix
let in if then else match with handle from resume drop return
never once multi use fn
```

将来のために予約する: `forall` `class` `instance`

暫定構文から、`true` / `false` を削除した (§11)。

### 演算子

- 演算子の文字は `! $ % & * + - . / < = > ? @ ^ | ~ :` で、これらを1文字以上並べたものが演算子のトークンになる (最長一致)
- 次の並びは予約記号で、演算子にはならない: `=` `|` `:` `.` `->` `<-` `..`
- `:` で始まる演算子はコンストラクタの演算子とする (例: `::`)。「大文字の名前 = コンストラクタ」の規則の、演算子版である
- 区切り記号: `( ) [ ] { } , ; \` とバッククォート

### 数値と文字

| リテラル | 形 | M1 での扱い |
|---|---|---|
| 整数 | `123`、`1_000_000`、`0xff`、`0o17`、`0b1010` | 使える |
| 浮動小数 | `1.5`、`1e9`、`2.5e-3` | 字句として予約する。使うと E0004 (未対応) |
| 文字 | `'a'`、`'\n'` | 字句として予約する。使うと E0004 (未対応) |

- 負の数は単項の `-` で書く (§7)

### 文字列

| 種類 | 形 | エスケープ | 補間 |
|---|---|---|---|
| 通常 | `"..."` | あり | あり |
| 複数行 | `"""` + 改行 ... `"""` | あり | あり |
| raw | `r"..."`、`r#"..."#` (`#` は何個でも) | なし | なし |

- エスケープは `\n` `\t` `\r` `\\` `\"` `\0` `\u{XXXX}`、および補間の `\{`
- 補間 `\{式}` の中には任意の式を書ける。中に文字列や `{}` を入れ子にしてよい。M1 以降の当面の間、**穴の式の型は `String` に限る** (値の変換は `show_int` などで明示する)。アドホック多相を入れるときに見直す (§13)
- 複数行の文字列では、開きの `"""` の直後の改行と、閉じの `"""` の直前の改行を取り除く。閉じの `"""` の列までの字下げを、各行から取り除く (Swift と同じ)。閉じの `"""` より浅い行があればエラー
- `r` と `"` の間に空白を入れると、識別子 `r` と通常の文字列の並置になる

### コマンドリテラル

```haskell
run `rsync -a --delete \{..files} \{dest}`
```

- バッククォートで囲む。結果は `Cmd` 型の値 (§14)
- 本文は空白で引数に分ける。シェルは介さない。`'...'` で囲んだ部分は、空白を含んでも1つの引数になる
- `\{x}` (`x : String`) は、中身に空白があっても、常に1つの引数 (または引数の一部) になる
- `\{..xs}` (`xs : List String`) は、要素ごとに別の引数として展開する。単独の引数の位置にだけ書ける
- パイプ、リダイレクト、`$VAR`、glob は解釈しない。標準ライブラリの関数で組み立てる
- エスケープは通常の文字列と同じで、さらに `` \` `` を使える
- Haskell のバッククォートによる中置 (`` x `div` y ``) は採用しない。中置で使いたい関数は演算子として定義する

## 4. レイアウト規則

### 基本

レイアウトは、lexer と parser の間に置くレイアウト段で処理する。レイアウト段は、trivia を除いたトークン列に、幅 0 の仮想トークン `OPEN` (ブロックの開始)、`SEP` (項目の区切り)、`CLOSE` (ブロックの終了) を挿入する。

- 仮想トークンは parser の入力にだけ現れ、イベントには出さない (rowan の木には入らない)。CST は lossless のまま
- レイアウト段は、行の先頭のトークンの列 (0 始まり) を、元のテキストから計算する

### 文脈のスタック

レイアウト段は、`Block(n)` (基準列 n のブロック) と `Bracket` (括弧の内側) からなるスタックを持つ。ファイル全体は `Block(0)` から始まる。

1. **行の先頭のトークン** (列 c) について、スタックの一番上が `Block(n)` なら:
   - `c == n`: そのトークンの前に `SEP` を挿入する
   - `c < n`: `CLOSE` を挿入して `Block(n)` を取り除き、規則 1 をやり直す
   - `c > n`: 前の行の続き (継続行) とみなし、何も挿入しない
2. スタックの一番上が `Bracket` なら、改行は意味を持たない (何も挿入しない)
3. **ブロックの開始**: 開始トークン `=` `->` `with` `then` `else` `where` が行の最後のトークンで (後ろにコメントがあってもよい)、次の行の先頭のトークンの列 m が、囲んでいる `Block(n)` の n より大きければ、`OPEN` を挿入して `Block(m)` を積む。m ≤ n ならエラー (字下げしたブロックが必要)
4. **開き括弧** `(` `[` `{` と補間の `\{` は `Bracket` を積む。対応する閉じ括弧は、その `Bracket` より上にあるすべての `Block` を `CLOSE` で閉じてから、`Bracket` を取り除く
5. **`;`** は、今のブロックの中での明示的な `SEP` として扱う。`Bracket` の直下で使うとエラー
6. ファイルの終わりで、残ったすべての `Block` を閉じる
7. 文字列とコマンドリテラルの本文のトークン (`STRING_TEXT` など) は、行の先頭のトークンとみなさない。複数行の文字列の中の行は、レイアウトに影響しない (補間 `\{ }` の中は規則 4 の `Bracket` になる)

### 規則の帰結

- 行の性質は「直前の行の末尾」と「深さ」だけで決まる。開始トークンで終わらない行の後に深い行が続けば、それは式の続きになる

  ```haskell
  main () =
    let total =
      lines (read_text "data.txt")
        |> map parse_int
        |> sum
    println (show_int total)
  ```

- 開始トークンが行の途中にある場合は、ブロックを開かない。`let x = 1` や `if c then a else b` は1行の式になる
- 括弧の中では改行が自由。ただし、括弧の中でも開始トークンで行が終われば、ブロックを開ける (`map (fn x ->` の後に本体を続ける、など)。そのブロックは、閉じ括弧で閉じる
- 名前を変えても、ブロックの基準列は変わらない (Haskell の列規則と違い、開始トークンの右側の長さに依存しない)

### `then` / `else` の前の `SEP`

ブロックの中で、`if` の後の `else` (と `then`) を `if` と同じ列に書くと、規則 1 により `SEP` が挿入される。

```haskell
  if c then
    a
  else
    b
```

parser は、`if` の解析中に、`then` / `else` の直前の `SEP` を1つ読み飛ばす (Haskell の DoAndIfThenElse と同じ扱い)。`else` で始まる文は存在しないので、曖昧さはない。

### エラー回復

- 列 0 の `SEP` (トップレベルの項目の区切り) を、最も強い回復の同期点にする。暫定構文の同期点 (セミコロン、波括弧、`fn` / `type` / `effect`) を置き換える
- ブロックの中では、`SEP` と `CLOSE` を同期点にする

## 5. 文法

`OPEN` `SEP` `CLOSE` はレイアウト段の仮想トークン (`;` は `SEP` として扱う)。`block(x) ::= OPEN x (SEP x)* CLOSE`、`list(x) ::= (x (',' x)* ','?)?`。

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

data_item   ::= 'data' UIDENT LIDENT* '=' alts
alts        ::= block(alt) | alt+
alt         ::= '|' UIDENT type_atom*
              | '|' type_atom CONOP type_atom             -- 中置のコンストラクタ
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
              | 'fn' param+ '->' body
              | 'let' pat (':' type)? '=' expr 'in' expr
              | op_expr
arms        ::= block(arm) | arm+
arm         ::= '|' pat '->' body
clauses     ::= block(clause) | clause+
clause      ::= '|' LIDENT apat* '->' body                -- 操作の節 (引数、k、状態)
              | '|' 'return' apat+ '->' body              -- return x [状態]

op_expr     ::= operand (OP operand)*                     -- CST では平たい列 (§7)
operand     ::= '-' operand | app
app         ::= ('resume' | 'drop')? postfix+ lambda?
              | lambda
lambda      ::= 'fn' param+ '->' body                     -- 最後の引数のラムダ
param       ::= apat | '(' pat ':' type ')'
postfix     ::= atom ('.' (LIDENT | INT))*                -- '.' の前後に空白を置かない

atom        ::= INT | FLOAT | CHAR | string | RAW_STRING | command
              | qvar | qcon
              | '(' ')' | '(' expr ')' | '(' expr ':' type ')'
              | '(' expr (',' expr)+ ','? ')'
              | '[' list(expr) ']'
              | '{' list(field) '}' | '{' expr 'with' list(field) '}'
              | '{' list(field) '|' expr '}'              -- 拡張
              | '(' OP ')' | '(' OP operand ')' | '(' operand OP ')' | '(' '.' LIDENT ')'
qvar        ::= (UIDENT '.')* LIDENT
qcon        ::= (UIDENT '.')* UIDENT
field       ::= LIDENT ('=' expr)?

pat         ::= cpat (CONOP pat)?                         -- 右結合 (例: x :: rest)
cpat        ::= qcon apat+ | apat
apat        ::= '_' | LIDENT | qcon | literal | '-' INT
              | '(' ')' | '(' pat ')' | '(' pat (',' pat)+ ','? ')'
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

### 文法上の補足

- `CONOP` は `:` で始まる演算子、`OP` はそれ以外の演算子
- 型の位置では、`<` で始まる演算子のトークン (`<>` など) と `>` で始まる演算子のトークン (`>->` など) を、parser が分割して読む (Rust が `>>` を分割するのと同じ)
- `postfix` の `.INT` で、lexer が `t.0.1` の `0.1` を浮動小数のトークンにした場合、parser がフィールドアクセスの位置で分割する (Rust と同じ)
- `qvar` / `qcon` の `.` と、`postfix` の `.` は、前後に空白を置かない。`Upper.lower` は修飾された名前、`lower.lower` はフィールドアクセスである
- `match`、`handle`、`if`、`fn` (最後の引数の位置を除く)、`let ... in` は atom ではない。関数の引数や演算の項にするときは括弧で囲む。これにより、`match e with` の `e` は `with` の手前で終わる
- トップレベルでパターンによる束縛 (`(a, b) = ...`) は書けない。トップレベルの値は、関数と同じく `LIDENT apat* '='` (引数 0 個) で定義する
- 等式が `LIDENT OP` で始まる場合は演算子の定義、`LIDENT` の後に `=` か `apat` が続く場合は関数の定義と、2トークンの先読みで区別する
- `handle` の節の引数の個数は HIR で検査する (§7)

## 6. 宣言

### シグネチャと等式

```haskell
len : List a -> Int
len Nil = 0
len (Cons _ rest) = 1 + len rest
```

- トップレベルの関数と値は、シグネチャが必須 (言語 spec §4 の決定のまま)。構文上は別の item なので、HIR で次を検査する (E1xxx)
  - シグネチャのない等式
  - 等式のないシグネチャ
  - 同じ名前の等式が連続していない (間に別の item がある)
  - シグネチャと等式が隣り合っていない
  - 等式ごとに引数の個数が違う
- 診断には、シグネチャの追加を提案する help を付ける
- 複数の等式は、HIR で「引数のタプルに対する `match`」1つに脱糖する。網羅性と線形性の検査は、その `match` に対して行う
- ローカルの `let` は、注釈なしで推論する (言語 spec §4 のまま)
- 引数のない値 (`pi : Int`、`pi = 3`) も書ける。言語 spec の「引数のない関数は `()` を取る」という規約は、エフェクトを持つ計算に使う

### `data` と `type`

```haskell
data Option a =
  | None
  | Some a

data Color = | Red | Green | Blue

data List a =
  | Nil
  | a :: List a

type Person = { name : String, age : Int }
type Unit = {}
```

- `data` は名前的な直和型。各選択肢は `|` で始める
- 中置のコンストラクタは、`:` で始まる演算子で宣言できる
- `type` は型の別名で、構造的なレコードに名前を付けるのに使う。別名は展開して扱う

### `effect`

```haskell
effect State s where
  get : Unit -> s
  put : s -> Unit

effect Log where
  log : Level -> String -> Unit

effect Fail where
  never fail : String -> a
```

- 多重度は `never` / `once` / `multi` を前に付ける。省略したら `once` (言語 spec のまま)
- **操作のカリー化を許す。** 操作の引数の個数は、シグネチャの一番外側の `->` の数とする。言語 spec の「一番外側の `->` に row を書けない」規則は、外側のすべての `->` に適用する

### fixity

```haskell
pub infixr 5 </>
pub (</>) : Path -> String -> Path
dir </> name = join_path dir name
```

- トップレベルに `infixl` / `infixr` / `infix` + 優先順位 (0〜9) + 演算子 (カンマ区切りで複数) を書く
- fixity の宣言がない演算子は `infixl 9` とする (Haskell と同じ)
- `pub` を付けた fixity は、演算子と一緒に公開される。fixity は演算子の名前に付属し、import した先でも同じ fixity を使う
- 同じ演算子に2回宣言する、定義されていない演算子に宣言する、はエラー (E1xxx)

標準の演算子の表 (Prelude で宣言する):

| 優先順位 | 演算子 | 結合 |
|---|---|---|
| 0 | `<\|` | 右 |
| 1 | `\|>` | 左 |
| 2 | `\|\|` | 右 |
| 3 | `&&` | 右 |
| 4 | `==` `!=` `<` `<=` `>` `>=` | 結合しない |
| 5 | `++` `::` | 右 |
| 6 | `+` `-` | 左 |
| 7 | `*` `/` `%` | 左 |
| 9 | `>>` `<<` (関数合成) | 右 |
| (10) | 関数適用、フィールドアクセス | 左 |

- 合成演算子 `.` は採用しない (`.` はフィールドアクセスとモジュール修飾に使うため)
- 暫定構文の単項の `!` は廃止し、`not` 関数にする

### `pub`

- シグネチャ、`data`、`type`、`effect`、fixity に `pub` を付けると公開される。付けないものはモジュールの中だけで見える
- `pub data` は、型とコンストラクタを公開する。コンストラクタを隠した抽象型は、将来の課題とする (§13)

## 7. 式

### ブロックと `let`

```haskell
copy : String -> String -> <IO> Unit
copy src dst =
  let f = open src
  let (f, text) = read_all f
  close f
  write_text dst text
```

- ブロックは文を並べたもの。最後の文が式ならその値がブロックの値で、そうでなければ `()`
- 最後でない式文は `Unit` 型でなければならない (言語 spec の「式文 `e;` は `Unit` を要求する」のまま)
- `let` による同じ名前の再束縛を許す (言語 spec のまま)
- 1行で書くための式の形 `let p = e in e2` を用意する。ブロックの `let` と同じ意味である

### ラムダ

```haskell
people |> filter (fn p -> p.age >= 18)
each items fn item ->
  println item
```

- `fn x y -> e`、`fn (a, b) -> e`、`fn () -> e`、`fn (x : Int) -> e`
- 関数適用の最後の引数がラムダなら、括弧なしで書ける (Haskell の BlockArguments)
- `->` で行が終われば、本体はブロックになる

### `if`

```haskell
if errors > 0 then
  eprintln "\{show_int errors} errors"
if s >= 1 then println line
else if verbose () then println "skipped"
```

- **then 節の型が `Unit` の場合、`else` を省略できる。** HIR で `else ()` を補う。正格評価なので、Haskell の `when c (println x)` は `println x` を必ず評価してしまう。そのため構文で用意する
- `else if` を連ねられる。1行の中に入れ子にした場合、`else` は一番近い `if` に付く

### `match`

```haskell
match first_word line with
  | "ERROR" -> 2
  | "WARN"  -> 1
  | _ ->
      log "other"
      0

match b with | True -> 1 | False -> 0
```

- 枝は `|` で始める。`with` で行が終われば字下げしたブロック、そうでなければ同じ行に枝を並べる
- 1行の形では、枝の本体の式は次の `|` の手前で終わる (`|` 単独は演算子ではないため)。入れ子の `match` を1行に書いた場合、後ろの枝は内側の `match` に付く

### 演算子の列と単項マイナス

- CST では、`a op1 b op2 c` を平たい列 (`OP_SEQ` ノード) のまま持つ
- HIR への変換で、名前解決の後に fixity の表を引いて木に組み直す (GHC の renamer と同じ方式)。結合しない演算子同士の並び (`a < b < c`) や、優先順位が同じで結合の向きが違う演算子の並びは、エラー (E1xxx)
- 単項の `-` は Haskell と同じ扱いにする。`negate` として優先順位 6 で組み直す。`f -1` は `f - 1` (引き算) と読むので、負の数を渡すときは `f (-1)` と書く

### セクション

| 書き方 | 意味 |
|---|---|
| `(+)` | `fn a b -> a + b` |
| `(+ 1)` | `fn x -> x + 1` |
| `(1 +)` | `fn x -> 1 + x` |
| `(.name)` | `fn x -> x.name` |
| `(- 1)` | 負の数 -1 (セクションではない。Haskell と同じ) |

### `use`

```haskell
main () =
  use with_env
  use tmp <- with_temp_dir
  use with_log "\{tmp}/build.log"
  build tmp
  deploy ()
```

| 書き方 | 脱糖後 |
|---|---|
| `use f a b` の後にブロックの残り | `f a b (fn () -> 残り)` |
| `use p <- f a b` の後にブロックの残り | `f a b (fn p -> 残り)` |

- 純粋な糖衣構文で、型、線形性、エフェクトの規則は増えない
- ブロックの最後の文が `use` の場合はエラー (包む残りがない) (E1xxx)
- `use` の後ろのコードは、すべてラムダの中に入る。そのラムダが `multi` で複数回再開されれば、`use` の後ろのコードも複数回実行される。この点は LSP の hover とドキュメントで補う

### handler

```haskell
try : (Unit -> <Fail | e> a) -> <e> Option a
try action =
  handle action () with
    | fail _   -> None
    | return x -> Some x
```

- 節は `| op 引数... k -> 本体` と `| return x -> 本体`
- 節の引数の個数は、`never` の操作なら「操作の引数の個数」、それ以外なら「操作の引数の個数 + 1 (`k`)」。個数は HIR で検査する (言語 spec §4 の「1 または 2」を、カリー化に合わせて一般化する)
- `return` の節は省略でき、省略したら `| return x -> x` とみなす (言語 spec のまま)
- 継続は `resume k v` で再開し、`drop k` で捨てる (言語 spec のまま)

### パラメータ付き handler

```haskell
with_log : String -> (Unit -> <Log, IO | e> a) -> <IO | e> a
with_log path action =
  handle action () from open path with
    | log lv msg k f ->
        let f = write f "[\{show_level lv}] \{msg}"
        resume k () f
    | return x f ->
        close f
        x
```

- `from 初期値` で handler の状態の初期値を与える。各節 (`return` を含む) は、最後の引数として状態を受け取る。`resume k v s` で、次の状態を渡して再開する
- 次のように、関数を返す handler に初期値を適用する形へ脱糖する。言語 spec の意味論 (deep handler、中断時の後始末) は変わらない

  ```haskell
  (handle action () with
     | log lv msg k -> fn f -> (resume k ()) (... f ...)
     | return x -> fn f -> ...) (open path)
  ```

- 状態は普通の引数なので、線形性の検査がそのまま効く。状態が `Lin` なら、各節で `resume` に渡すか、`close` / `drop` しなければならない。`drop k` する節でも、状態の後始末を忘れればエラーになる
- `resume` の引数の個数 (2 または 3) は HIR で検査する
- `Ref` には `Unr` の値しか入れられない (マルチコア spec §4)。そのため、`Lin` なリソースを handler に持たせる方法は、この構文が基本になる

### 直接扱うリソース

`File` などのリソースは、明示的に `open` / `close` する。Python の `with` や Haskell の `bracket` に当たる構文は用意しない。`close` を忘れれば線形性の検査が検出し、中断時は言語 spec のクリーンアップが破棄処理を呼ぶからである。

### 並行処理

並行処理のための構文は追加しない。マルチコア spec §6 の `par` / `spawn` / `run_scheduler` は、ラムダと `use` で書ける。非同期の処理は、row に `Async` を持つ普通の関数として直接の形で書け、`async` / `await` も関数の色分けも要らない。

```haskell
fetch_all : List String -> <IO> List Int
fetch_all urls =
  use run_scheduler
  let tasks = map (fn url -> Async.start fn () -> Http.status url) urls
  map Async.await tasks
```

## 8. 直積型とレコード

### 構成

- 直積型は、**ラベル付きの row を持つ構造的なレコード** 1種類とする
- タプルは、0 から始まる数字ラベルのレコードの糖衣構文: `(a, b)` は `{ 0 = a, 1 = b }`、型 `(A, B)` は `{ 0 : A, 1 : B }`
- Unit は空のレコード: `()` は `{}`、`type Unit = {}`
- `{ }` の中には名前ラベルだけを書ける。数字ラベルはタプルの構文でだけ書ける
- 診断と hover では、ラベルが 0 から連番の閉じたレコードを `(A, B)` と表示する

### 構文

| 用途 | 書き方 |
|---|---|
| 型 (閉じた) | `{ name : String, age : Int }` |
| 型 (開いた) | `{ age : Int \| r }` |
| 値 | `{ name = "a", age = 3 }`、フィールド名の省略 `{ name, age }` |
| 射影 | `p.name`、`t.0` |
| 更新 | `{ p with age = 31 }` (ラベルが存在しなければ型エラー) |
| 拡張 | `{ age = 31 \| rest }` (`rest` にラベルを加える。分解パターンと対になる形) |
| 分解パターン | `{ name, age }`、`{ name = n }`、残りの束縛 `{ log \| rest }` |
| フィールドのセクション | `(.name)` |

### Kind

- レコードの row の Kind を `RecRow<m>` とする。`m` はフィールドの Kind の join
- `{ ρ } : Type<m>` (ρ : `RecRow<m>`)。`Lin` なフィールドが1つでもあれば、レコード全体が `Lin` になる
- 型の中の row 変数 (`{ age : Int | r }` の `r`) は、線形性の Kind 変数を持つ。言語 spec §4 の型変数と同じく、使い方から制約を推論する
- タプルの Kind が要素の Kind の join になること (`(File, String) : Type<Lin>`) は、この規則の特別な場合である

### 線形性の規則

| 操作 | 規則 |
|---|---|
| 射影 `e.l` | `e` を1回消費する。**取り出さない残りのフィールドは、すべて `Unr` でなければならない**。取り出すフィールド自体は `Lin` でもよい |
| 更新 `{ e with l = v }` | `e` を1回消費する。**上書きされる古い値は `Unr` でなければならない** |
| 拡張 `{ l = v \| e }` | `e` と `v` を1回ずつ消費する。何も捨てないので制約はない |
| 分解パターン | 各フィールドを別の変数に束縛する。`Lin` なレコードは、これで分解する |

```haskell
type Job = { name : String, log : File }

log_of : Job -> File
log_of j = j.log                  -- OK: 残りの { name : String } は Unr

name_of : Job -> String
name_of j = j.name                -- エラー: 残りの { log : File } が Lin
                                  -- help: `let { name, log } = j` で分解する

let n = (read_all f).1            -- エラー: 残りの .0 (File) が Lin
let (f, text) = read_all f        -- OK
```

- 違反は線形性の診断 (E3xxx) とし、分解パターンへの書き換えを help と fix で提案する
- `{ p with age = p.age + 1 }` は `p` を2回使うので、`p` の row 変数に `Unr` の制約が付く。`Lin` なレコードにも使える形は次のとおり

  ```haskell
  birthday : { age : Int | r } -> { age : Int | r }
  birthday p =
    let { age | rest } = p
    { age = age + 1 | rest }
  ```

  分解パターン `{ age | rest }` で取り出し、拡張の式 `{ age = ... | rest }` で組み直す。どちらも何も捨てないので、`r` に制約は付かない

### 実行時の表現

- M1 のインタプリタでは、フィールドを名前で引く
- ネイティブ化では、フィールドの位置が多相になる箇所に、オフセットを証拠として渡す (evidence passing) か、単相化する (§13)

## 9. モジュールと名前解決

### モジュール

- 1ファイル = 1モジュール。モジュール名はパスから決まる (`report/Csv.em` は `Report.Csv`)。`module` ヘッダは書かない
- `Prelude` (`Option`、`Result`、`List`、`Bool`、`println` など) を暗黙に取り込む
- エントリポイントは `main : Unit -> <IO> Unit` (言語 spec のまま)。終了コードは標準ライブラリの `exit` で扱う (§14)

### import

| 書き方 | 効果 |
|---|---|
| `import Report.Csv` | `Csv.parse` のように、最後のセグメントで修飾して使える |
| `import Report.Csv as C` | `C.parse` |
| `import Report.Csv (parse, Row)` | 修飾付きに加えて、列挙した名前を修飾なしで使える |
| `import Report.Format (Style(..))` | 型とそのコンストラクタを、修飾なしで使える |

- **標準ライブラリのモジュール** (`Fs`、`Json`、`String`、`Proc` など) は、import しなくても `Fs.read_text` のように修飾付きで使える
- 名前の解決の順序: ローカルの束縛 → import で修飾なしにした名前 → `Prelude` → (修飾付きの場合) import したモジュール → 標準ライブラリのモジュール。ユーザーのモジュールが標準ライブラリのモジュールと同じ名前なら、ユーザーのモジュールを優先する
- 修飾なしの名前の衝突 (2つの import が同じ名前を出す) は、使った箇所でエラー (E1xxx)
- エフェクトの操作は値の名前空間に置き、関数と同じく import する。コンストラクタと型名、エフェクト名は型の名前空間に置く

## 10. 実装への影響

### `eml_syntax`

- **lexer**: logos を土台にしたまま、モードのスタックを持つ手書きの層をかぶせる
  - 文字列の中、補間 `\{` の中 (括弧の深さを数える)、コマンドリテラルの中、入れ子のブロックコメントの各モード
  - 文字列は、`STRING_START` / `STRING_TEXT` / `ESCAPE` / `INTERP_START` / `INTERP_END` / `STRING_END` のトークン列に分ける。コマンドリテラルも同様
- **レイアウト段** (新規): §4 の規則で、trivia を除いたトークン列に仮想トークンを挿入する。`Parser::new` が受け取る列をこの段の出力に置き換える。仮想トークンを読んでも `Event::Token` を出さないので、`sink::build_tree` は変更しない
- **grammar/**: §5 の文法に全面的に書き換える。演算子の列は `OP_SEQ` ノードに平たく並べる
- **`SyntaxKind`**: キーワード、演算子、文字列の部品、仮想トークン (木には入らないが parser が使う)、ノードの種類を追加・変更する
- **型付き AST ラッパ**: 新しいノードに合わせて作り直す
- 言語 spec §3 の方針どおり、構文の差し替えは `grammar/`、`SyntaxKind`、lexer、AST ラッパ (と新しいレイアウト段) に閉じる

### `eml_hir` (実装時)

HIR への変換で、次の脱糖と検査を行う。

- シグネチャと等式の対応の検査、複数の等式の `match` への脱糖
- 演算子の列の組み直し (fixity の表を引く)、単項マイナス、セクション
- `use`、パラメータ付き handler、`if` の `else` の補完、`let ... in`
- タプルのレコードへの変換、補間の `++` の連結への変換、コマンドリテラルの `Cmd` の構築
- handler の節の引数の個数、`resume` の引数の個数

### 診断コード

この spec で増える診断の例 (番号は実装計画で割り当てる):

| 範囲 | 例 |
|---|---|
| E0xxx | インデントのタブ、字下げしたブロックが必要、閉じていない補間・コマンドリテラル・ブロックコメント、`.` の前後の空白、浮動小数と文字のリテラル (E0004 未対応) |
| E1xxx | シグネチャのない等式、等式のないシグネチャ、等式が連続していない、fixity の衝突と重複、ブロックの最後の `use`、名前の衝突 |
| E3xxx | 射影で `Lin` な残りを捨てる、更新で `Lin` な古い値を捨てる |

### 実装の段階

| 段階 | 内容 |
|---|---|
| **S1 (次の実装計画)** | lexer (新しいトークン、`--` と `{- -}`、shebang、`'` を含む識別子、数値の形、浮動小数と文字は E0004)、レイアウト段、M1 の機能の文法: 宣言 (シグネチャ、等式、`data`、`effect`、fixity)、式 (`let`、`let ... in`、`if`、`match`、`fn`、`handle ... with` / `from`、`use`、`resume`、`drop`、演算子の列、セクション、最後の引数のラムダ、タプル、型の明示)、パターン、型と row、エスケープだけの通常の文字列 |
| S2 | 名前付きのレコードと `type`、モジュールと import / `pub`、`Prelude`、リストのリテラル、補間、複数行の文字列、raw 文字列 |
| S3 | コマンドリテラル (標準ライブラリの `Proc` と一緒に) |

- S1 は `eml_syntax` だけの作業になる (HIR 以降は M1 のスケルトンではまだスタブ)。暫定構文の grammar は実装せずに捨てる
- 型検査器を実装するときは、S2 の名前付きレコードを待たずに、M1 のタプルを最初から「数字ラベルの閉じたレコード」として表現することを勧める。後の作り直しを避けるため
- 既存のテストソースは、言語 spec §8 の合意済みの例外として、新しい構文に機械的に書き換える (期待値は変えない)。`$` と `@` は演算子の文字になるので、「認識できない文字」のテスト (`unexpected_character.em`、`multiple_errors.em`) は、新しい構文でも認識できない文字 (例: `€`) に置き換える

## 11. 言語 spec への変更点

| 言語 spec の箇所 | 変更 |
|---|---|
| §2 構文 | 「本番の構文は作者が別途設計する」を、この spec への参照に置き換える |
| §3 エラー回復 | 回復の同期点を、レイアウトの `SEP` / `CLOSE` に置き換える (§4) |
| §4 型システム | 直積型を構造的なレコードに統一する。`RecRow<m>`、射影・更新の線形性の規則、Unit = `{}` を追加する (§8) |
| §4 handler | 節の引数の個数を、操作のカリー化に合わせて一般化する。パラメータ付き handler の脱糖を追加する (§7) |
| §7 含めるもの | `Bool` を組み込みの `data Bool = \| False \| True` にする (キーワード `true` / `false` を廃止)。単項の `!` を廃止し、`not` にする |
| §7 含めるもの | `if` の `else` を、then 節が `Unit` なら省略できるようにする |
| §7 含めないもの | レコード、モジュール、ブロックコメント、16進のリテラルは、この spec で構文を定めた。実装は §10 の段階に従う |
| §7 暫定構文 | この spec の §3〜§9 で置き換える |

## 12. プログラム例

### ログの集計 (全体)

```haskell
#!/usr/bin/env eml run
-- | ログを集計して、レポートを書く
import Report.Format (render, Style(..))

data Level =
  | Info
  | Warn
  | Error

type Entry = { level : Level, msg : String }

effect Fail where
  never fail : String -> a

parse_entry : String -> Option Entry
parse_entry line =
  match String.split_once " " line with
    | Some ("INFO", msg)  -> Some { level = Info, msg }
    | Some ("WARN", msg)  -> Some { level = Warn, msg }
    | Some ("ERROR", msg) -> Some { level = Error, msg }
    | _ -> None

is_error : Level -> Bool
is_error Error = True
is_error _ = False

try : (Unit -> <Fail | e> a) -> <IO | e> Option a
try action =
  handle action () with
    | fail msg ->
        eprintln "error: \{msg}"
        None
    | return x -> Some x

summarize : String -> <IO, Fail> Unit
summarize path =
  let entries = Fs.read_lines path |> filter_map parse_entry
  let errors = entries |> filter (fn e -> is_error e.level) |> map (.msg)
  if length errors > 10 then fail "too many errors"
  let commit = read `git rev-parse --short HEAD`
  Fs.write_text "report.md" """
    # Report for \{commit}
    \{render Markdown errors}
    """

main : Unit -> <IO> Unit
main () =
  match Env.args () with
    | [path] ->
        match try (fn () -> summarize path) with
          | Some () -> println "done"
          | None -> exit 1
    | _ -> eprintln "usage: summarize LOG"
```

### grep (線形なファイル)

```haskell
grep : String -> String -> <IO> Unit
grep pat path =
  let f = open path
  let (f, text) = read_all f
  close f
  lines text
    |> filter (String.contains pat)
    |> each fn line -> println "match: \{line}"
```

### 状態 (パラメータ付き handler)

```haskell
effect State s where
  get : Unit -> s
  put : s -> Unit

run_state : s -> (Unit -> <State s | e> a) -> <e> (a, s)
run_state init action =
  handle action () from init with
    | get () k st -> resume k st st
    | put st2 k _ -> resume k () st2
    | return x st -> (x, st)

counter : Unit -> <State Int> Int
counter () =
  let n = get ()
  put (n + 1)
  n
```

`put` の節で `_` により古い状態を捨てるので、`s` に `Unr` の制約が付く (言語 spec §4)。

### デプロイ (エフェクト、`use`、コマンドリテラル)

`Fail` と `try` は、上のログの集計の例と同じものを使う。

```haskell
effect Ask where
  ask : String -> String

with_env : (Unit -> <Ask, IO | e> a) -> <IO | e> a
with_env action =
  handle action () with
    | ask key k -> resume k (Option.default "" (Env.get key))

deploy : List String -> <Ask, Fail, IO> Unit
deploy files =
  let host = ask "HOST"
  if host == "" then fail "HOST is not set"
  let dest = "\{host}:/srv/app"
  let code = run `rsync -a --delete \{..files} \{dest}`
  if code != 0 then fail "rsync failed"

main : Unit -> <IO> Unit
main () =
  use with_env
  match try (fn () -> deploy ["dist/app", "dist/assets"]) with
    | Some () -> println "deployed"
    | None -> exit 1
```

### 子プロセスとパイプ (線形なハンドルの束)

```haskell
type Child = { proc : Process, stdin : Stdin, stdout : Stdout }

sort_lines : List String -> <IO> String
sort_lines xs =
  let { proc, stdin, stdout } = Proc.spawn `sort`
  let stdin = Proc.write stdin (String.join "\n" xs)
  Proc.close_stdin stdin
  let (stdout, out) = Proc.read_all stdout
  Proc.close_stdout stdout
  Proc.wait proc
  out
```

### データ変換 (構造的レコード)

```haskell
type Person = { name : String, age : Int, email : Option String }

parse_row : List String -> Option Person
parse_row cols =
  match cols with
    | [name, age, email] -> Some { name, age = parse_int age, email = non_empty email }
    | _ -> None

adults : List Person -> List String
adults people =
  people
    |> filter (fn p -> p.age >= 18)
    |> map (fn p -> "\{p.name} <\{Option.default "-" p.email}>")
```

## 13. 将来の論点

- **線形値の受け渡しの糖衣**: `read_all &input` を `let (input, r) = read_all input` に脱糖する、Swift の `inout` 風の構文。API の返り値の形を縛ることと、巻き上げの規則が課題。標準ライブラリを書いてみて、受け渡しが辛ければ再検討する
- **借用**: Rust / Austral 方式。意味論の大きな追加
- **フィールドの変換の糖衣**: `{ p with age = _ + 1 }` のような、関数でフィールドを書き換える構文。`Lin` なレコードでも、分解せずに書けるようにする
- **補間の穴の型**: アドホック多相 (型クラスなど) を入れたら、`Show` 相当で任意の型を穴に入れられるようにする
- **レコードのネイティブな表現**: evidence passing か単相化か
- **接頭辞付きリテラルの一般化**: `re"..."` のように、ユーザーが接頭辞付きのリテラルを定義できるようにする (穴の型付けにアドホック多相が要る)
- **サンクの糖衣**: `fn () ->` が多い場合の短縮形。現時点では `use` とセクションで十分と判断した
- **抽象型**: コンストラクタを公開しない `pub data`
- **再エクスポート**: `pub import`
- **優先順位グループ**: 整数の優先順位で、ライブラリ同士の演算子の衝突が問題になった場合の代替案 (Swift 方式)

## 14. 標準ライブラリ spec への申し送り

- **handler でリソースを持つ API を中心にする**: `with_log`、`with_output`、`with_temp_dir`、`with_cwd`、`with_env` など。`use` で並べて使う形を想定する
- **`Cmd` 型**: コマンドリテラルの結果。`run : Cmd -> <IO> Int`、`read : Cmd -> <IO> String`、`spawn : Cmd -> <IO> Child`、パイプ (`.|` などの演算子)、リダイレクト、環境変数、作業ディレクトリの設定
- **同じ型のハンドルの取り違え**: `Stdin` / `Stdout` / `Stderr` は別の型にする
- **`Task a` を `Lin` にする**: `Async.start` が返すタスクを、必ず `await` するか `drop` (キャンセル) させる。構造化された並行処理にする
- **`exit`**: `never` の操作として、`IO` 側に用意する
- **`Prelude` の範囲**: `Option`、`Result`、`List`、`Bool`、`println` / `eprintln`、`show_int`、`not`、`|>` などの標準の演算子と fixity
- **モジュールの候補**: `Fs`、`Path`、`Proc`、`Env`、`String`、`List`、`Map`、`Json`、`Csv`、`Toml`、`Regex`、`Http`、`Async`
- **補間の穴は当面 `String` のみ**: 各型に `show_*` 関数を揃える
