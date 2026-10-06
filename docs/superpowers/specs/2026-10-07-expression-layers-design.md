# 式の層の整理の設計

位置づけ: 作業用の設計文書。作業を終えたら削除する。

## 目的と範囲

最後の引数のラムダを括弧なしで書ける特例をなくし、式の文法とパーサを一貫した簡潔な形にする。

今の文法とパーサには、一貫しない点が4つある。

1. `fn` だけが特例を持つ。文法は `app ::= ... postfix+ lambda?` で、関数適用の最後の引数に括弧なしの `fn` を書ける。パーサは `fn` を `operand` として読むので、`x + fn y -> y` のように演算の項にも書ける。同じキーワードで始まる `if`、`match`、`handle`、`let ... in` は、引数や演算の項の位置に書くと E0012 になる。
2. `resume` と `drop` を引数の位置に書くと、`g resume k 1` は E0011 `` unexpected `resume` `` になる。`g match x with ...` は E0012 で括弧で囲むよう案内するので、エラーの出方がそろっていない。
3. `let ... in` の右辺は、文法では `'=' expr` だが、パーサは `body` を読んでブロックも受け付ける。文の `let` も `body` を読むので、文法の記述のほうが実装と食い違っている。
4. 括弧が要るかどうかの判定が、パーサの `operand`、`app`、`paren_expr` の3か所に別々にある。文法にも、`expr` の `'fn' ...` と `lambda` 規則が重なって載っている。

範囲は式の文法とパーサ、それに合わせた仕様、例、テストである。CST のノードの種類と形は変えないので、HIR から先の crate と AST のラッパーは変えない。

## 式の3つの層

式を次の3つの層に分ける。上の層の形を下の層の位置に書くと E0012 にする。

| 層 | 中身 | 書ける位置 |
|---|---|---|
| 式 (`expr`) | `if`、`match`、`handle`、`fn`、`let ... in` と演算子の列 | 本体、括弧の中、`if` の条件、`with` の手前など、`expr` を期待する位置 |
| 項 (`operand`) | 前置の `-`、関数適用、`resume` と `drop` の適用 | 演算子の列の項 |
| 引数 (`postfix`) | atom とフィールドアクセス | 関数適用の引数 |

`if`、`match`、`handle`、`fn`、`let ... in` は、末尾の本体が右へできるだけ伸びるので、式の層に置く。`resume` と `drop` は、引数が atom なので右へ伸びない。演算の項に書いても読み方は1つに決まるので、項の層に置く。そのため `again k + resume k False (st + 2)` は今のまま書ける。

コールバックを渡すときは `(fn x -> ...)` か `use` で書く。括弧の中で `->` で行を終えるとブロックが開き、閉じ括弧で閉じる (レイアウト規則4)。

```haskell
each items (fn item ->
  println item)

use item <- each items
println item
```

## 仕様の変更

### 文法 (`docs/spec/grammar.md`)

```
expr        ::= 'if' expr 'then' body ('else' body)?
              | 'match' expr 'with' arms
              | 'handle' expr ('from' expr)? 'with' clauses
              | 'fn' apat+ '->' body
              | 'let' pat (':' type)? '=' body 'in' expr
              | op_expr
op_expr     ::= operand (OP operand)*                     -- CST では平たい列
operand     ::= '-' operand | app
app         ::= ('resume' | 'drop')? postfix+
postfix     ::= atom ('.' (LIDENT | INT))*                -- '.' の前後に空白を置かない
```

- `lambda` 規則と、`app` の `lambda?` と `| lambda` を消す
- `let ... in` の右辺を `expr` から `body` にする
- 補足の「`match`、`handle`、`if`、`fn` (最後の引数の位置を除く)、`let ... in` は atom ではない」の項を、3つの層の規則に書き換える。`if`、`match`、`handle`、`fn`、`let ... in` は、演算の項や関数の引数にするときに括弧で囲む。`resume` と `drop` の適用は、演算の項には書けるが、関数の引数にするときは括弧で囲む。どちらの違反も E0012 にする。`match e with` の `e` が `with` の手前で終わるという理由づけは残す
- 「実装の段階」の M1 の一覧から「最後の引数のラムダ」を消す

### ほかの仕様と文書

- `docs/spec/expressions.md` の「ラムダ」: BlockArguments の項目を消し、引数や演算の項にするときは括弧で囲む ([文法](grammar.md)) と書く。例の `each items fn item ->` を、上の括弧の形と `use` の形に置き換える
- `docs/spec/expressions.md` の「並行処理」: `Async.start fn () -> Http.status url` を `Async.start (fn () -> Http.status url)` にする
- `docs/spec/examples.md` の grep: `|> each fn line -> ...` を `|> each (fn line -> ...)` にする
- `docs/spec/diagnostics.md` の E0012: 対象に `fn` を足し、`resume` と `drop` は引数の位置だけが対象だと書く
- `docs/implementation/status.md`: 実装済みの一覧から「最後の引数のラムダ」を消す
- `docs/overview.md` の `use` の説明 (「最後の引数のラムダとして渡す」) は、`use` の意味の説明なので変えない

E0012 のメッセージは今の形式のまま、`` `fn` expression must be parenthesized here ``、`` `resume` expression must be parenthesized here `` とする。help も今と同じ `wrap it in parentheses` である。

## パーサの変更

変更は `crates/eml_syntax/src/grammar/expressions.rs` の中だけである。

層ごとにトークンの集合を1つずつ持つ。今の `NEEDS_PARENS` は `EXPR_FORMS` に置き換える。

```rust
const EXPR_FORMS: TokenSet = TokenSet::new(&[IF_KW, MATCH_KW, HANDLE_KW, FN_KW, LET_KW]);
const KEYWORD_APPS: TokenSet = TokenSet::new(&[RESUME_KW, DROP_KW]);
```

- `expr_inner`: 分岐に `FN_KW => lambda(p)` を足す。`fn` を読むのはここだけになる
- `operand`: `ATOM_START` か `KEYWORD_APPS` なら `app`、`EXPR_FORMS` なら `misplaced` を呼び、それ以外は偽を返す。`lambda` の分岐を消す
- `app` の引数のループ: `ATOM_START` なら `postfix` を読む。`EXPR_FORMS` か `KEYWORD_APPS` なら `misplaced` を呼び、引数に数えてループを抜ける。それ以外はループを抜ける。`lambda` の分岐と、最後の引数のラムダについてのコメントを消す
- `paren_expr`: 括弧の直下は `expr` の位置なので、`EXPR_FORMS` なら `expr` を読み、それ以外は `op_expr(p, true)` でセクションを検出する。今と同じ形で、集合の名前だけが変わる

`misplaced` は今の `needs_parens` を改名したもので、E0012 を出したあと、その形を本来の層で読んで回復する。

- `EXPR_FORMS` は `expr` で読む (今と同じ)
- `KEYWORD_APPS` は `app` で読む。`g resume k 1 + 2` の `+ 2` を `resume` の中に取り込まず、`g (resume k 1) + 2` と同じ木にするためである
- `app` から `app` を呼ぶと、`g resume resume …` のような入力で再帰が際限なく深くなる。そのため `nested` を通し、入れ子の深さの上限 (E0013) に数える。`expr` はすでに `nested` を通っている

レイアウト段、`use` の解析 (`has_left_arrow`)、HIR の脱糖は変えない。`let ... in` の右辺は、パーサがすでに `body` を読んでいるので、変わるのは文法の記述だけである。

## テスト

TDD で進める。新しいテストを先に書いて失敗を確かめてから、パーサを直す。既存のテストの変更はすべて種類1 (振る舞いの変更) で、この設計の承認をその合意とする。理由は、括弧なしで引数や演算の項に書いた `fn` が通らなくなり、E0012 になるためである。

### 新しいテスト

`eml_syntax` に足す。最後の1つ以外は、今は失敗する。

| テスト | 入力 | 期待する結果 |
|---|---|---|
| 引数の位置の `fn` | `f = g fn x -> x` | `` E0012 1:7 `fn` expression must be parenthesized here `` |
| 演算の項の `fn` | `f = 1 + fn x -> x`、`f = - fn x -> x` | それぞれ E0012 が1件 |
| 引数の位置の `resume` と `drop` | `f = g resume k 1`、`f = g drop k` | それぞれ E0012 が1件 |
| `resume` の回復 | `f = g resume k 1 + 2` | CST が `OP_SEQ[APP_EXPR[g, RESUME_EXPR[k, 1]], +, 2]` で、診断は E0012 の1件だけ |
| 回復の深さの上限 | `g` の後に `resume` を300個並べる | E0013 が出て、スタックが溢れない |
| `let ... in` の右辺のブロック | 右辺を字下げしたブロックにし、`in` を続ける | 診断なしの CST の snapshot。今も通るが、文法を `body` に直したことの裏付けにする |

UI テスト `tests/ui/check-fail/syntax/unparenthesized_lambda.em` も足す。括弧なしで最後の引数に書いたラムダと、引数の位置の `resume` を1つのファイルに入れ、E0012 の表示を help も含めて snapshot に残す。

### 既存のテストの変更

| テスト | 変更 |
|---|---|
| `crates/eml_syntax/tests/expressions.rs` の `trailing_lambda_with_a_block_body` | `lambda_in_parentheses_with_a_block_body` に改名する。入力を `each items (fn item ->` の後に本体の行を続けて `)` で閉じる形にし、snapshot に `PAREN_EXPR` が加わる |
| 同じファイルの `lambda_as_an_operand` | 消す。`xs \|> each fn l -> ...` が E0012 になることは、新しいテストで確かめる |
| `crates/eml_syntax/tests/corpus/s1.em` の76行目 | `each (Cons 1 Nil) (fn x ->` の後に本体を続けて `)` で閉じる形にする。期待値 (診断なし、item の並び) は変わらない |
| `crates/eml_hir/tests/lower.rs` の `a_lambda_can_be_the_last_argument` | `a_lambda_can_be_an_argument` に改名し、入力を `call n (fn x -> x + 1)` にする。snapshot はバイト単位で同じ |
| `tests/ui/run/functions/closures.em` | `each 3 (fn n ->` の後に本体を続けて `)` で閉じる形にし、冒頭のコメントの "a last-argument lambda" を "a lambda argument with a block body" にする。出力の snapshot は変わらない |

### 完了の条件

- `cargo test`、`cargo clippy --all-targets`、`cargo fmt` がすべて通る
- `docs/` とテストに、括弧なしで引数や演算の項に書いた `fn` が残っていないことを grep で確かめる
