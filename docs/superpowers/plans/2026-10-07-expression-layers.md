# 式の層の整理の実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 最後の引数のラムダの特例をなくし、式を `expr`・`operand`・`postfix` の3つの層に整理して、層を外れた形をすべて E0012 で報告する。

**Architecture:** 変更するコードは `crates/eml_syntax/src/grammar/expressions.rs` だけである。`fn` を `expr` の層に移し (Task 1)、`resume` と `drop` を引数の位置で E0012 にする (Task 2)。続けて UI テストを足し (Task 3)、仕様の文書を新しい文法に合わせる (Task 4)。CST のノードの種類と形は変えないので、HIR から先の crate は変えない。

**Tech Stack:** Rust (edition 2024)、rowan の CST、`insta` の inline snapshot、`cargo test`、Markdown (日本語)

**Spec:** `docs/superpowers/specs/2026-10-07-expression-layers-design.md`

## Global Constraints

- 開発環境は Nix flake の devShell である (`direnv`)。`cargo`、`cargo insta`、`clippy`、`rustfmt` はそこから使う
- コードのコメントと docs は日本語で、である調で書く。日本語を書く前に `yomiyasu:yomiyasu` のスキルを読み、その規則に従う。英単語やインラインコードの前後に半角空白を入れる今の docs の書き方は保つ
- コメントは理由だけを書き、規則を指すときは `docs/` のパスを書く
- CST のノードの種類と形は変えない。HIR、型検査、Core IR、インタプリタの crate は変えない
- E0012 のメッセージは `` `<キーワード>` expression must be parenthesized here ``、help は `wrap it in parentheses` のままにする
- 他の言語の書き方を前提にした help や Warning は足さない (`docs/spec/diagnostics.md`)
- 既存のテストの変更はすべて種類1 (振る舞いの変更) で、設計の承認が合意である。コミットメッセージに理由を書く。理由は「括弧なしで引数や演算の項に書いた `fn` が E0012 になるため」である
- テストの期待値を通すためだけに設計を曲げない。期待と違う結果が出たら、作業を止めて報告する
- コミットメッセージは英語の命令形で書き、末尾に次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01VUzBkYor3qv3avLwvBSKvK
  ```

## Review Focus

- 古い書き方の回復: `each items fn item ->` の後に本体のブロックが続く入力は、E0012 が1件だけで、後に連鎖する診断が出ないこと (Task 1 の `old_trailing_lambda_reports_one_error`)
- 括弧の中のラムダ: タプルの要素と型の明示の中の `fn` は、今までどおり診断なしで読めること (Task 1 の `lambda_in_a_tuple_and_an_annotation`)
- 閉じ括弧だけの行: `(fn item ->` の本体の後で、`)` を囲むブロックと同じ列の行の先頭に置いても読めること (Task 1 の `lambda_closed_on_its_own_line`)
- `if` の枝のラムダ: `if c then fn x -> x else fn y -> y` の `then` の枝の本体が `else` の手前で終わること (Task 1 の `lambdas_in_if_branches`)
- 演算の項の `resume`: `resume k 1 + 2` と `again k + resume k False (st + 2)` は今までどおり通ること。既存の `resume_is_an_operand` (`crates/eml_syntax/tests/handlers.rs`) と `crates/eml_types/tests/effects.rs` のテストが守る

---

### Task 1: `fn` を `expr` の層に移す

**Files:**
- Modify: `crates/eml_syntax/src/grammar/expressions.rs` (`NEEDS_PARENS` の定義、`expr_inner`、`operand`、`app`、`paren_expr`、`needs_parens`)
- Modify: `crates/eml_syntax/tests/expressions.rs` (`trailing_lambda_with_a_block_body`、`lambda_as_an_operand`)
- Modify: `crates/eml_syntax/tests/corpus/s1.em:76-77`
- Modify: `crates/eml_hir/tests/lower.rs` (`a_lambda_can_be_the_last_argument`)
- Modify: `tests/ui/run/functions/closures.em`

**Interfaces:**
- Consumes: なし
- Produces: `const EXPR_FORMS: TokenSet` (`IF_KW`、`MATCH_KW`、`HANDLE_KW`、`FN_KW`、`LET_KW`)、`fn misplaced(p: &mut Parser)` (E0012 を出し、回復のために読む)。Task 2 がどちらも使う

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_syntax/tests/expressions.rs` の `lambda_as_an_operand` のテストを丸ごと消し、同じ場所に次のテストを置く。

```rust
#[test]
fn lambda_must_be_parenthesized_as_an_argument_or_an_operand() {
    assert_eq!(
        diagnostics("f = g fn x -> x"),
        ["E0012 1:7 `fn` expression must be parenthesized here"]
    );
    assert_eq!(
        diagnostics("h = xs |> each fn l -> println l"),
        ["E0012 1:16 `fn` expression must be parenthesized here"]
    );
    assert_eq!(
        diagnostics("f = 1 + fn x -> x"),
        ["E0012 1:9 `fn` expression must be parenthesized here"]
    );
    assert_eq!(
        diagnostics("f = - fn x -> x"),
        ["E0012 1:7 `fn` expression must be parenthesized here"]
    );
}

#[test]
fn old_trailing_lambda_reports_one_error() {
    let text = lines(&["f = each items fn item ->", "  println item"]);
    assert_eq!(
        diagnostics(&text),
        ["E0012 1:16 `fn` expression must be parenthesized here"]
    );
}

#[test]
fn lambda_in_a_tuple_and_an_annotation() {
    assert_eq!(
        diagnostics("f = ((fn x -> x, 1), (fn y -> y : Int -> Int))"),
        Vec::<String>::new()
    );
}

#[test]
fn lambda_closed_on_its_own_line() {
    let text = lines(&["f =", "  each items (fn item ->", "    println item", "  )"]);
    assert_eq!(diagnostics(&text), Vec::<String>::new());
}

#[test]
fn lambdas_in_if_branches() {
    assert_eq!(
        diagnostics("f c = if c then fn x -> x else fn y -> y"),
        Vec::<String>::new()
    );
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_syntax --test expressions lambda`
Expected: `lambda_must_be_parenthesized_as_an_argument_or_an_operand` と `old_trailing_lambda_reports_one_error` が FAIL する (診断が空で、E0012 が出ない)。`lambda_in_a_tuple_and_an_annotation`、`lambda_closed_on_its_own_line`、`lambdas_in_if_branches` は今も PASS する。これらは変更の後も通り続けることを守るテストである

- [ ] **Step 3: 括弧なしのラムダを使っている既存のテストの入力を書き換える**

今のパーサは括弧で囲んだラムダも読むので、この Step の後もすべてのテストが通る。

`crates/eml_syntax/tests/expressions.rs` の `trailing_lambda_with_a_block_body` を、次のテストに置き換える (改名し、入力と snapshot を変える)。

```rust
#[test]
fn lambda_in_parentheses_with_a_block_body() {
    let text = lines(&["f = each items (fn item ->", "  println item)"]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "f"
        EQ "="
        APP_EXPR
          PATH_EXPR
            PATH
              NAME_REF
                LIDENT "each"
          PATH_EXPR
            PATH
              NAME_REF
                LIDENT "items"
          PAREN_EXPR
            L_PAREN "("
            LAMBDA_EXPR
              FN_KW "fn"
              BIND_PAT
                NAME
                  LIDENT "item"
              THIN_ARROW "->"
              BLOCK
                EXPR_STMT
                  APP_EXPR
                    PATH_EXPR
                      PATH
                        NAME_REF
                          LIDENT "println"
                    PATH_EXPR
                      PATH
                        NAME_REF
                          LIDENT "item"
            R_PAREN ")"
    "#);
}
```

`crates/eml_syntax/tests/corpus/s1.em` の76〜77行目を

```
  each (Cons 1 Nil) fn x ->
    println (show_int (x <+> 1)) -- 行末のコメント
```

から次に変える。

```
  each (Cons 1 Nil) (fn x ->
    println (show_int (x <+> 1))) -- 行末のコメント
```

`crates/eml_hir/tests/lower.rs` の `a_lambda_can_be_the_last_argument` を次に変える。snapshot はバイト単位で同じである。

```rust
#[test]
fn a_lambda_can_be_an_argument() {
    let text = "call : Int -> (Int -> Int) -> Int\ncall n f = f n\n\ng : Int -> Int\ng n = call n (fn x -> x + 1)";
    insta::assert_snapshot!(lower_text(text), @r"
    call : Int -> (Int -> Int) -> Int
    call n#0 f#1 = (f#1 n#0)
    g : Int -> Int
    g n#0 = (@call n#0 (fn x#1 -> (+ x#1 1)))
    ");
}
```

`tests/ui/run/functions/closures.em` の先頭のコメントの1行目を

```
-- Lambdas, closures that capture strings, a closure called twice, a last-argument lambda,
```

から次に変える。

```
-- Lambdas, closures that capture strings, a closure called twice, a lambda argument with a block body,
```

同じファイルの

```
  each 3 fn n ->
    println ("n = " ++ show_int n)
```

を次に変える。

```
  each 3 (fn n ->
    println ("n = " ++ show_int n))
```

- [ ] **Step 4: 書き換えた入力が今のパーサで通ることを確かめる**

Run: `cargo test -p eml_syntax --test expressions lambda_in_parentheses_with_a_block_body && cargo test -p eml_syntax --test corpus && cargo test -p eml_hir --test lower a_lambda_can_be_an_argument && cargo test -p eml_cli --test ui`
Expected: すべて PASS。UI テストの出力の snapshot は変わらない (`cargo insta` の未承認の snapshot が増えていないこと)

- [ ] **Step 5: パーサを直す**

`crates/eml_syntax/src/grammar/expressions.rs` を次のとおり変える。

`NEEDS_PARENS` の定義とそのコメントを、次に置き換える。

```rust
/// 本体が右へできるだけ伸びる形なので、引数や演算の項の位置では括弧が要る (docs/spec/grammar.md の「文法上の補足」)。
/// これにより、`match e with` の `e` が `with` の手前で終わる。
const EXPR_FORMS: TokenSet = TokenSet::new(&[IF_KW, MATCH_KW, HANDLE_KW, FN_KW, LET_KW]);
```

`expr_inner` に `fn` の分岐を足す。

```rust
fn expr_inner(p: &mut Parser) -> bool {
    match p.current() {
        IF_KW => if_expr(p),
        MATCH_KW => match_expr(p),
        HANDLE_KW => handle_expr(p),
        FN_KW => lambda(p),
        LET_KW => let_expr(p),
        _ => return op_expr(p, false) != OpExpr::Nothing,
    }
    true
}
```

`operand` から `lambda` の分岐を消す。

```rust
fn operand(p: &mut Parser) -> bool {
    if p.at_ts(ATOM_START) || p.at(RESUME_KW) || p.at(DROP_KW) {
        app(p);
    } else if p.at_ts(EXPR_FORMS) {
        misplaced(p);
    } else {
        return false;
    }
    true
}
```

`app` の引数のループから `lambda` の分岐とそのコメントを消す。

```rust
    let mut args = 0;
    loop {
        if p.at_ts(ATOM_START) {
            postfix(p);
            args += 1;
        } else if p.at_ts(EXPR_FORMS) {
            misplaced(p);
            args += 1;
            break;
        } else {
            break;
        }
    }
```

`paren_expr` の `let inner = if p.at_ts(NEEDS_PARENS) {` を `let inner = if p.at_ts(EXPR_FORMS) {` に変える。

`needs_parens` を `misplaced` に改名する。本体とコメントは変えない。

```rust
/// E0012 を出した後も、回復のためにそのまま式として読む。
fn misplaced(p: &mut Parser) {
    p.error(
        codes::NEEDS_PARENS,
        format!(
            "`{}` expression must be parenthesized here",
            p.current_text()
        ),
        "wrap it in parentheses",
    );
    expr(p);
}
```

`lambda` 関数そのものは変えない。

- [ ] **Step 6: テストが通ることを確かめる**

Run: `cargo test -p eml_syntax`
Expected: すべて PASS (Step 1 の5つのテストを含む)

Run: `cargo test`
Expected: すべて PASS

- [ ] **Step 7: コミットする**

```bash
git add crates/eml_syntax/src/grammar/expressions.rs crates/eml_syntax/tests/expressions.rs crates/eml_syntax/tests/corpus/s1.em crates/eml_hir/tests/lower.rs tests/ui/run/functions/closures.em
git commit -F - <<'EOF'
Require parentheses around a lambda used as an argument or an operand

A lambda now belongs to the expression layer like if, match, handle and
let ... in, so writing it unparenthesized as an argument or an operand
reports E0012. Tests that wrote a trailing lambda now parenthesize it,
because the old form is a syntax error (behavior change).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01VUzBkYor3qv3avLwvBSKvK
EOF
```

---

### Task 2: `resume` と `drop` を引数の位置で E0012 にする

**Files:**
- Modify: `crates/eml_syntax/src/grammar/expressions.rs` (`EXPR_FORMS` の後に定数を足す、`operand`、`app`、`misplaced`)
- Test: `crates/eml_syntax/tests/handlers.rs`
- Test: `crates/eml_syntax/tests/nesting.rs`

**Interfaces:**
- Consumes: Task 1 の `EXPR_FORMS` と `misplaced(p: &mut Parser)`、`grammar/mod.rs` の `nested<T>(p: &mut Parser, on_too_deep: T, parse: impl FnOnce(&mut Parser) -> T) -> T`
- Produces: `const KEYWORD_APPS: TokenSet` (`RESUME_KW`、`DROP_KW`)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_syntax/tests/handlers.rs` の `resume_is_an_operand` の後に、次の2つのテストを足す。

```rust
#[test]
fn resume_and_drop_must_be_parenthesized_as_arguments() {
    assert_eq!(
        diagnostics("f = g resume k 1"),
        ["E0012 1:7 `resume` expression must be parenthesized here"]
    );
    assert_eq!(
        diagnostics("f = g drop k"),
        ["E0012 1:7 `drop` expression must be parenthesized here"]
    );
}

#[test]
fn resume_as_an_argument_is_read_as_an_operand() {
    insta::assert_snapshot!(shape("f = g resume k 1 + 2"), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "f"
        EQ "="
        OP_SEQ
          APP_EXPR
            PATH_EXPR
              PATH
                NAME_REF
                  LIDENT "g"
            RESUME_EXPR
              RESUME_KW "resume"
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "k"
              LITERAL
                INT "1"
          OP "+"
          LITERAL
            INT "2"
    ---
    E0012 1:7 `resume` expression must be parenthesized here
    "#);
}
```

`crates/eml_syntax/tests/nesting.rs` の `deep_if_chain_reports_one_error` の後に、次のテストを足す。

```rust
#[test]
fn deep_chain_of_misplaced_resumes_does_not_overflow_the_stack() {
    let found = diagnostics(&format!("x = g {}k", "resume k ".repeat(10_000)));
    let too_deep = found.iter().filter(|d| d.starts_with("E0013 ")).count();
    assert_eq!(too_deep, 1, "{found:?}");
}
```

この入力では、`resume` の適用の引数の位置に次の `resume` が並ぶ。回復が `resume` の適用を読むたびに入れ子が1段深くなるので、E0013 で止まることを確かめる。

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_syntax --test handlers resume_ && cargo test -p eml_syntax --test nesting deep_chain_of_misplaced_resumes`
Expected: 3つとも FAIL する。今は E0012 ではなく `` E0011 1:7 unexpected `resume` `` が出て、E0013 は出ない

- [ ] **Step 3: パーサを直す**

`crates/eml_syntax/src/grammar/expressions.rs` の `EXPR_FORMS` の定義の後に、次の定数を足す。

```rust
/// 引数が atom なので右へ伸びず、演算の項には書けるが、引数の位置では括弧が要る (docs/spec/grammar.md の「文法上の補足」)。
const KEYWORD_APPS: TokenSet = TokenSet::new(&[RESUME_KW, DROP_KW]);
```

`operand` の条件を `KEYWORD_APPS` で書く。

```rust
fn operand(p: &mut Parser) -> bool {
    if p.at_ts(ATOM_START) || p.at_ts(KEYWORD_APPS) {
        app(p);
    } else if p.at_ts(EXPR_FORMS) {
        misplaced(p);
    } else {
        return false;
    }
    true
}
```

`app` の引数のループで、`KEYWORD_APPS` も `misplaced` に渡す。

```rust
        } else if p.at_ts(EXPR_FORMS) || p.at_ts(KEYWORD_APPS) {
            misplaced(p);
            args += 1;
            break;
        } else {
```

`misplaced` を、形を本来の層で読むように変える。

```rust
/// E0012 を出した後も、回復のためにその形を本来の層で読む。`resume` と `drop` を `app` で読むのは、
/// `g resume k 1 + 2` を `g (resume k 1) + 2` と同じ木にして、`+ 2` を取り込まないため。
fn misplaced(p: &mut Parser) {
    p.error(
        codes::NEEDS_PARENS,
        format!(
            "`{}` expression must be parenthesized here",
            p.current_text()
        ),
        "wrap it in parentheses",
    );
    if p.at_ts(KEYWORD_APPS) {
        // `app` は自分では深さを数えないので、`g resume k resume k …` の再帰をここで数える。
        nested(p, (), app);
    } else {
        expr(p);
    }
}
```

`app` の先頭の `match p.current() { RESUME_KW => ..., DROP_KW => ..., _ => None }` は変えない。

- [ ] **Step 4: テストが通ることを確かめる**

Run: `cargo test -p eml_syntax`
Expected: すべて PASS (Step 1 の3つのテストと、既存の `resume_is_an_operand` と `drop_needs_an_argument` を含む)

Run: `cargo test`
Expected: すべて PASS

- [ ] **Step 5: コミットする**

```bash
git add crates/eml_syntax/src/grammar/expressions.rs crates/eml_syntax/tests/handlers.rs crates/eml_syntax/tests/nesting.rs
git commit -F - <<'EOF'
Report E0012 for resume and drop used as arguments

resume and drop applications stay operands, but as arguments they now
report E0012 like the other forms that need parentheses, instead of a
generic syntax error. Recovery reads them as an application so that a
following operator stays outside, and counts the recursion toward the
nesting limit.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01VUzBkYor3qv3avLwvBSKvK
EOF
```

---

### Task 3: E0012 の表示の UI テストを足す

**Files:**
- Create: `tests/ui/check-fail/syntax/unparenthesized_lambda.em`
- Create (insta が作る): `crates/eml_cli/tests/snapshots/ui__check_fail@syntax__unparenthesized_lambda.em.snap`

**Interfaces:**
- Consumes: Task 1 と Task 2 のパーサ
- Produces: なし

- [ ] **Step 1: UI テストのソースを書く**

`tests/ui/check-fail/syntax/unparenthesized_lambda.em`:

```
-- A lambda or a `resume` used as an argument must be parenthesized; each reports E0012 once.
each : Int -> (Int -> <e> Unit) -> <e> Unit
each n f =
  if n > 0 then
    f n
    each (n - 1) f

inc : Int -> Int
inc n = n + 1

effect Ask where
  ask : Unit -> Int

main : Unit -> <IO> Unit
main () =
  each 3 fn n ->
    println (show_int n)
  let v = handle ask () + 1 with
    | ask () k -> inc resume k 1
  println (show_int v)
```

回復した木は `each 3 (fn n -> ...)` と `inc (resume k 1)` なので、後の段階の診断は出ない見込みである。

- [ ] **Step 2: UI テストを走らせて snapshot を確かめる**

Run: `cargo test -p eml_cli --test ui`
Expected: 新しい snapshot が未承認のため FAIL する

Run: `cargo insta review` (または `cargo insta pending-snapshots` で中身を読む)

新しい snapshot に、次の2件の E0012 だけがあり、ほかの診断がないことを確かめてから承認する。

- 16行目の `fn` を指す `` [E0012] Error: `fn` expression must be parenthesized here `` と help `wrap it in parentheses`
- 19行目の `resume` を指す `` [E0012] Error: `resume` expression must be parenthesized here `` と help `wrap it in parentheses`

ほかの診断が出たら、承認せずに原因を調べる。ソースの書き方の誤り (型が合わないなど) ならソースを直す。パーサの回復が原因なら、作業を止めて報告する。

- [ ] **Step 3: テストが通ることを確かめる**

Run: `cargo test -p eml_cli --test ui`
Expected: PASS

- [ ] **Step 4: コミットする**

```bash
git add tests/ui/check-fail/syntax/unparenthesized_lambda.em crates/eml_cli/tests/snapshots/ui__check_fail@syntax__unparenthesized_lambda.em.snap
git commit -F - <<'EOF'
Add a UI test for unparenthesized lambdas and resume arguments

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01VUzBkYor3qv3avLwvBSKvK
EOF
```

---

### Task 4: 仕様の文書を新しい文法に合わせる

**Files:**
- Modify: `docs/spec/grammar.md` (文法の `expr` と `app`、「文法上の補足」、「実装の段階」)
- Modify: `docs/spec/expressions.md` (「ラムダ」、「並行処理」)
- Modify: `docs/spec/examples.md` (grep の例)
- Modify: `docs/spec/diagnostics.md` (E0012 の行)
- Modify: `docs/implementation/status.md` (実装済みの一覧)
- Test: `crates/eml_syntax/tests/expressions.rs`

**Interfaces:**
- Consumes: なし
- Produces: なし

- [ ] **Step 1: `let ... in` の右辺のブロックのテストを書く**

文法の `let ... in` の右辺を `body` に直すので、その裏付けのテストを `crates/eml_syntax/tests/expressions.rs` の `let_in_inside_parentheses` の後に足す。パーサはすでに `body` を読むので、このテストは最初から通る。

```rust
#[test]
fn let_in_with_a_block_on_the_right() {
    let text = lines(&["f x =", "  let y =", "      x + 1", "    in y"]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        NAME
          LIDENT "f"
        BIND_PAT
          NAME
            LIDENT "x"
        EQ "="
        BLOCK
          EXPR_STMT
            LET_EXPR
              LET_KW "let"
              BIND_PAT
                NAME
                  LIDENT "y"
              EQ "="
              BLOCK
                EXPR_STMT
                  OP_SEQ
                    PATH_EXPR
                      PATH
                        NAME_REF
                          LIDENT "x"
                    OP "+"
                    LITERAL
                      INT "1"
              IN_KW "in"
              PATH_EXPR
                PATH
                  NAME_REF
                    LIDENT "y"
    "#);
}
```

Run: `cargo test -p eml_syntax --test expressions let_in_with_a_block_on_the_right`
Expected: PASS

- [ ] **Step 2: yomiyasu のスキルを読む**

日本語を書く前に `yomiyasu:yomiyasu` のスキルを呼ぶ。

- [ ] **Step 3: `docs/spec/grammar.md` を直す**

文法のブロックの `expr` の `let` の行を

```
              | 'let' pat (':' type)? '=' expr 'in' expr
```

から次に変える。

```
              | 'let' pat (':' type)? '=' body 'in' expr
```

同じブロックの

```
app         ::= ('resume' | 'drop')? postfix+ lambda?
              | lambda
lambda      ::= 'fn' apat+ '->' body                      -- 最後の引数のラムダ
postfix     ::= atom ('.' (LIDENT | INT))*                -- '.' の前後に空白を置かない
```

を次に変える。

```
app         ::= ('resume' | 'drop')? postfix+
postfix     ::= atom ('.' (LIDENT | INT))*                -- '.' の前後に空白を置かない
```

「文法上の補足」の

```
- `match`、`handle`、`if`、`fn` (最後の引数の位置を除く)、`let ... in` は atom ではない。関数の引数や演算の項にするときは括弧で囲む。これにより、`match e with` の `e` は `with` の手前で終わる
```

を次に変える。

```
- 式の形は `expr`、`operand`、`postfix` の3つの層に分かれる。上の層の形を下の層の位置に括弧なしで書くと E0012 にする
  - `if`、`match`、`handle`、`fn`、`let ... in` は `expr` の層の形で、末尾の本体が右へできるだけ伸びる。関数の引数や演算の項にするときは括弧で囲む。これにより、`match e with` の `e` は `with` の手前で終わる
  - `resume` と `drop` の適用は `operand` の層の形で、引数が atom なので右へ伸びない。演算の項には書けるが、関数の引数にするときは括弧で囲む
```

「実装の段階」の M1 の一覧の

```
- 式: `let`、`let ... in`、`if`、`match`、`fn`、`handle ... with` / `from`、`use`、`resume`、`drop`、演算子の列、セクション、最後の引数のラムダ、タプル、型の明示
```

を次に変える。

```
- 式: `let`、`let ... in`、`if`、`match`、`fn`、`handle ... with` / `from`、`use`、`resume`、`drop`、演算子の列、セクション、タプル、型の明示
```

- [ ] **Step 4: `docs/spec/expressions.md` を直す**

「ラムダ」の節の例を

````
```haskell
people |> filter (fn p -> p.age >= 18)
each items fn item ->
  println item
```
````

から次に変える。

````
```haskell
people |> filter (fn p -> p.age >= 18)
each items (fn item ->
  println item)
```
````

同じ節の

```
- 関数適用の最後の引数がラムダなら、括弧なしで書ける (Haskell の BlockArguments)
- `->` で行が終われば、本体はブロックになる
```

を次に変える。

```
- 関数の引数や演算の項にするときは、括弧で囲む ([文法](grammar.md) の「文法上の補足」)。ブロックの残りを渡すなら、`use item <- each items` とも書ける (「`use`」)
- `->` で行が終われば、本体はブロックになる。括弧の中でも同じで、そのブロックは閉じ括弧で閉じる ([レイアウト規則](layout.md))
```

「並行処理」の節の例の

```
  let tasks = map (fn url -> Async.start fn () -> Http.status url) urls
```

を次に変える。

```
  let tasks = map (fn url -> Async.start (fn () -> Http.status url)) urls
```

- [ ] **Step 5: `docs/spec/examples.md` を直す**

grep の例の

```
    |> each fn line -> println "match: \{line}"
```

を次に変える。

```
    |> each (fn line -> println "match: \{line}")
```

- [ ] **Step 6: `docs/spec/diagnostics.md` を直す**

E0012 の行を

```
| E0012 | `NEEDS_PARENS` | 括弧の要る式 (`if`、`match`、`handle`、`let`) を、引数や演算の項の位置に括弧なしで書いた |
```

から次に変える。

```
| E0012 | `NEEDS_PARENS` | 括弧の要る形を括弧なしで書いた。`if`、`match`、`handle`、`fn`、`let ... in` を引数や演算の項の位置に書いた場合と、`resume`、`drop` を引数の位置に書いた場合である ([文法](grammar.md) の「文法上の補足」) |
```

- [ ] **Step 7: `docs/implementation/status.md` を直す**

```
- fixity の宣言とユーザー定義の演算子、演算子のセクション、`use`、最後の引数のラムダ
```

を次に変える。

```
- fixity の宣言とユーザー定義の演算子、演算子のセクション、`use`
```

- [ ] **Step 8: 文書の日本語と残りを確かめる**

Run: `python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py docs/spec/grammar.md docs/spec/expressions.md docs/spec/diagnostics.md`
Expected: 文末のコロン、ダッシュ記号、絵文字の警告がない。英単語の前後の半角空白と箇条書きの比率の警告は、今の docs の書き方なので直さない

Run: `grep -rn "最後の引数のラムダ\|BlockArguments" docs/spec docs/implementation`
Expected: 出力なし

- [ ] **Step 9: コミットする**

```bash
git add docs/spec/grammar.md docs/spec/expressions.md docs/spec/examples.md docs/spec/diagnostics.md docs/implementation/status.md crates/eml_syntax/tests/expressions.rs
git commit -F - <<'EOF'
Describe the three expression layers in the spec

Drop the trailing lambda from the grammar, state which forms need
parentheses in which positions, and write the right side of let ... in
as a body, which is what the parser already reads.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01VUzBkYor3qv3avLwvBSKvK
EOF
```

---

### Task 5: 全体を確かめる

**Files:**
- なし (直すものが見つかったときだけ、その原因のファイルを直す)

**Interfaces:**
- Consumes: Task 1〜4 の成果
- Produces: なし

- [ ] **Step 1: テスト、clippy、fmt を走らせる**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: すべて成功する。`cargo fmt --check` が差分を出したら `cargo fmt` で直す

- [ ] **Step 2: 括弧なしのラムダが残っていないことを確かめる**

Run: `grep -rnE "[A-Za-z0-9_)\"] fn [a-z_(]|(\|>|<\||[-+*]) *fn " docs/spec docs/implementation docs/future docs/overview.md tests crates --include='*.md' --include='*.em' --include='*.rs' | grep -v "/src/" | grep -v "pub fn "`
Expected: 出るのは次のものだけである。それ以外が出たら、括弧で囲む
- E0012 を確かめるテストの入力 (`crates/eml_syntax/tests/expressions.rs` と `tests/ui/check-fail/syntax/unparenthesized_lambda.em`)
- ソースではない文字列。`crates/eml_hir/tests/eval.rs` の評価のトレース `"eval fn y -> y"` と、`crates/eml_syntax/tests/lexer.rs` のキーワードの一覧

Run: `grep -rn "NEEDS_PARENS\|needs_parens" crates/eml_syntax/src`
Expected: `crates/eml_syntax/src/lib.rs` の `pub const NEEDS_PARENS: ErrorCode` と、`misplaced` の中の `codes::NEEDS_PARENS` だけが出る

- [ ] **Step 3: 直したものがあればコミットする**

Step 1 と Step 2 で直したものがあるときだけ、そのファイルを `git add` してコミットする。

```bash
git commit -F - <<'EOF'
Fix the leftovers found in the final check

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01VUzBkYor3qv3avLwvBSKvK
EOF
```

---

### Task 6: 作業用の文書を消す

ブランチ全体のレビューを終え、指摘を直した後に行う。

**Files:**
- Delete: `docs/superpowers/specs/2026-10-07-expression-layers-design.md`
- Delete: `docs/superpowers/plans/2026-10-07-expression-layers.md`

**Interfaces:**
- Consumes: なし
- Produces: なし

- [ ] **Step 1: 文書を消してコミットする**

```bash
git rm docs/superpowers/specs/2026-10-07-expression-layers-design.md docs/superpowers/plans/2026-10-07-expression-layers.md
git commit -F - <<'EOF'
Delete the expression layers design and plan

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01VUzBkYor3qv3avLwvBSKvK
EOF
```
