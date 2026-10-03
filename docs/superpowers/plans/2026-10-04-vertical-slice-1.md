# 縦の貫通 段階1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 名前解決以降の段階 (HIR、型検査、Core IR、ランタイム、インタプリタ) に段階1の中身を入れ、`eml run` で関数、`Int` / `String` / `Bool`、`if`、`let`、標準の演算子、`println` を使うプログラムを実行できるようにする。

**Architecture:** 各段階を下から順に実装する。`eml_syntax` の小さな修正 (Task 1〜3) の上に、HIR (Task 4〜5)、型検査器 (Task 6〜7)、ランタイムのヒープ (Task 8)、Core IR と Perceus (Task 9)、CEK インタプリタ (Task 10) を積み、UI テスト (Task 11) と文書 (Task 12) で締める。各段階は `fn stage(&In) -> (Out, Vec<Diagnostic>)` の純粋関数のままにし、Core IR はエラーのないプログラムだけを受け取る。

**Tech Stack:** Rust (edition 2024)、rowan 0.16、la-arena 0.3 (新規)、insta 1.49。

**Spec:** `docs/superpowers/specs/2026-10-04-vertical-slice-1-design.md` (段階1の内部設計)。規範は `docs/spec/` の `types.md` (省略した row は `<>`、`main`)、`declarations.md` (標準の演算子の型と意味)、`lexical.md` (整数リテラルの上限)、`diagnostics.md` (E1001〜E1006、E2001〜E2005、E0004 の扱い)、`core-ir.md`、`runtime.md`、および `docs/implementation/status.md` の「名前解決以降の実装段階」と `testing.md`。

## Global Constraints

- 作業は `main` から切ったブランチ `vertical-slice-1` で行う。タスクごとにコミットする
- 外部 crate は `la-arena = "0.3"` だけを足す (workspace の依存に置く)。ほかは増やさない
- コードのコメントは日本語で、何をするかではなく理由を書く。規則を指すときは `docs/` のパスを書く (CLAUDE.md)
- 各段階は `fn stage(&In) -> (Out, Vec<Diagnostic>)` の純粋関数にする。グローバルな可変状態を持たない。例外は `eml_core_ir::lower(&Module, &TypedModule) -> Program` で、エラーのないプログラムだけを受け取るので診断を返さない (Task 9)
- 診断の help / note は eml 自身の規則の説明に限る。他の言語の書き方を前提にしたヒントは入れない
- インタプリタとランタイムの値とフレームに `Rc` と `RefCell` を使わない。`unsafe` を書かない
- 既存のテストは、このプランで名前を挙げたものだけを変える。`tests/ui/check-fail/` のスナップショットは、HIR 以降の段階が新しい診断を足す場合に限って更新してよい (既存の診断が消えたり変わったりしたら実装の誤り)
- 各タスクの最後に `cargo test`、`cargo clippy --all-targets`、`cargo fmt` を通す。clippy の警告を残さない
- インラインスナップショットの期待値は、このプランのコードが出す形をそのまま書いてある。食い違ったら、まず実装がプランのコードと一致しているかを確かめる。プランの期待値の誤り (位置の数え違いなど) だと判断した場合は、理由をコミットメッセージに書いてから直す
- 診断の位置の表記は、1 始まりの行と、文字数で数えた列 (`2:7`) である

## Review Focus

- 10 万段の再帰。Rust のスタックを溢れさせず、`debug_heap` のリーク検出も通る → Task 10 のテスト `deep_recursion_does_not_overflow_the_stack`、Task 11 の `run/deep_recursion.em`
- 文字列を再束縛する、2回使う、`_` で捨てる、使わない引数で受ける。どれもリークも解放済みアクセスも起こさない → Task 9 のテスト `strings_are_dupped_and_decreffed`、`shadowed_and_discarded_strings`、Task 10 の `strings_are_freed`
- 分岐の後でも使う文字列を、`if` の片方の枝だけが使う (`let t = if b then s else "none"` の後に `s` を使う) → Task 9 のテスト `a_non_tail_if_keeps_strings_used_later`、Task 10 の `strings_are_freed`
- 未定義の名前や結合しない演算子の並びの後に、型エラーが連鎖しない → Task 7 のテスト `name_errors_do_not_cascade`
- `&&` と `||` の右辺が、短絡したときに評価されない (`println` が出ない) → Task 10 のテスト `and_and_or_short_circuit`、Task 11 の `run/operators.em`

---

### Task 0: ブランチを切り、既存のテストに `main` を足す

`main` を入口とする spec ([型と Kind](../../spec/types.md) の「推論」) に合わせて、`main` を持たないプログラムを実行していたテストを直す。これは brainstorming で合意した変更である (testing.md に記録する)。この時点の HIR 以降は仮実装なので、テストの結果は変わらない。

**Files:**
- Modify: `tests/ui/run/empty.em`、`tests/ui/run/comments_only.em`
- Modify: `crates/eml_cli/tests/api.rs`
- Modify: `docs/implementation/testing.md` (「テストの変更に関する合意済みの例外」)

**Interfaces:**
- Produces: `main` を持つ `run/` のテスト。以降のタスクで `main` の検査 (Task 7) と実行 (Task 10) が入っても通る

- [ ] **Step 1: ブランチを作る**

```bash
git switch -c vertical-slice-1
```

- [ ] **Step 2: UI テストのソースに `main` を足す**

`tests/ui/run/empty.em` を次の内容にする (元は空のファイル)。

```haskell
main : Unit -> <IO> Unit
main () = ()
```

`tests/ui/run/comments_only.em` の末尾 (既存のコメントの後) に、空行と次の2行を足す。

```haskell

main : Unit -> <IO> Unit
main () = ()
```

- [ ] **Step 3: lib API のテストのソースに `main` を足す**

`crates/eml_cli/tests/api.rs` の `compile_returns_a_program_without_errors` と `execute_runs_a_compiled_program` で、`files.add("a.em", "")` を次に変える。

```rust
    let file = files.add("a.em", "main : Unit -> <IO> Unit\nmain () = ()");
```

- [ ] **Step 4: 合意済みの例外を testing.md に記録する**

`docs/implementation/testing.md` の「テストの変更に関する合意済みの例外」にある、`run/empty.em` と `comments_only.em` についての項目を次に置き換える。

```markdown
- `tests/ui/run/empty.em`、`comments_only.em` と、`crates/eml_cli/tests/api.rs` の `compile_returns_a_program_without_errors`、`execute_runs_a_compiled_program` は、`main` を持たない「空のプログラムが実行できる」ことを前提にしていた。`main` を入口とする spec と合わないので、縦の貫通の段階1で `main : Unit -> <IO> Unit` と `main () = ()` を足した。コメントを読み飛ばすことと lib API の流れを確かめる目的は変わらない
```

- [ ] **Step 5: テストを走らせる**

Run: `cargo test`
Expected: PASS (スナップショットの変更なし)

- [ ] **Step 6: コミットする**

```bash
git add tests/ui/run crates/eml_cli/tests/api.rs docs/implementation/testing.md
git commit -m "Give the run tests a main function"
```

---

### Task 1: 整数リテラルの上限と、リテラルの値を読む関数

整数リテラルが `Int` の最大値を超えたら E0007 にする ([字句](../../spec/lexical.md))。HIR が同じ規則でリテラルの値を読めるように、`int_value` と `decode_string` を `eml_syntax` から公開する。

**Files:**
- Modify: `crates/eml_syntax/src/lexer.rs` (`simple` の `Raw::Number`、新しい関数 `int_value` と `decode_string`)
- Modify: `crates/eml_syntax/src/lib.rs` (再公開)
- Create: `crates/eml_syntax/tests/literals.rs`

**Interfaces:**
- Produces: `eml_syntax::int_value(text: &str) -> Option<i64>` (`INT` トークンの値。範囲外や不正な形なら `None`)、`eml_syntax::decode_string(text: &str) -> Option<String>` (`STRING` トークンの値。補間、不正なエスケープ、閉じていないものは `None`)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_syntax/tests/literals.rs` を作る。

```rust
mod common;

use common::diagnostics;
use eml_syntax::{decode_string, int_value};

#[test]
fn integer_values() {
    assert_eq!(int_value("1_000"), Some(1000));
    assert_eq!(int_value("0xff"), Some(255));
    assert_eq!(int_value("0o17"), Some(15));
    assert_eq!(int_value("0b1010"), Some(10));
    assert_eq!(int_value("9223372036854775807"), Some(i64::MAX));
    assert_eq!(int_value("9223372036854775808"), None);
}

#[test]
fn too_large_integer_literals_are_reported() {
    assert_eq!(
        diagnostics("x = 9223372036854775808"),
        ["E0007 1:5 integer literal `9223372036854775808` is too large"]
    );
    assert!(diagnostics("x = 0x7fff_ffff_ffff_ffff").is_empty());
}

#[test]
fn string_values() {
    assert_eq!(decode_string(r#""a\nb""#).as_deref(), Some("a\nb"));
    assert_eq!(decode_string(r#""\t\r\\\"\0""#).as_deref(), Some("\t\r\\\"\0"));
    assert_eq!(decode_string(r#""\u{1F600}!""#).as_deref(), Some("😀!"));
    assert_eq!(decode_string(r#""""#).as_deref(), Some(""));
}

#[test]
fn strings_reported_by_the_lexer_have_no_value() {
    assert_eq!(decode_string(r#""\{x}""#), None);
    assert_eq!(decode_string(r#""\q""#), None);
    assert_eq!(decode_string(r#""abc"#), None);
    assert_eq!(decode_string(r#""abc\""#), None);
    assert_eq!(decode_string(r#"""#), None);
}
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_syntax --test literals`
Expected: コンパイルエラー (`int_value` と `decode_string` がない)

- [ ] **Step 3: `int_value` と `decode_string` を実装する**

`crates/eml_syntax/src/lexer.rs` の `number_kind` の直前に足す。

```rust
/// `INT` トークンの値。`Int` の範囲を超えるものは `None` で、字句解析が E0007 を報告している
/// (docs/spec/lexical.md)。HIR も同じ関数で値を読む。
pub fn int_value(text: &str) -> Option<i64> {
    let digits = text.replace('_', "");
    let (body, radix) = match digits.get(..2) {
        Some("0x") => (&digits[2..], 16),
        Some("0o") => (&digits[2..], 8),
        Some("0b") => (&digits[2..], 2),
        _ => (&digits[..], 10),
    };
    i64::from_str_radix(body, radix).ok()
}

/// 通常の文字列リテラル `"..."` の値。補間を含むもの (S2)、不正なエスケープを含むもの、閉じていないものは `None`
/// で、どれも字句解析が報告している。
pub fn decode_string(text: &str) -> Option<String> {
    if text.len() < 2 {
        return None;
    }
    let inner = text.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            '0' => out.push('\0'),
            'u' => {
                let rest = chars.as_str();
                let close = rest.strip_prefix('{')?.find('}')?;
                let code = u32::from_str_radix(&rest[1..close + 1], 16).ok()?;
                out.push(char::from_u32(code)?);
                chars = rest[close + 2..].chars();
            }
            _ => return None,
        }
    }
    Some(out)
}
```

`"abc\"` (閉じていない) は、最後の `"` がエスケープなので `inner` が `abc\` になり、`\` の次の文字がなくて `None` になる。

- [ ] **Step 4: 範囲外の整数リテラルを報告する**

`crates/eml_syntax/src/lexer.rs` の `simple` で、`Ok(Raw::Number) => number_kind(slice).unwrap_or_else(|| { ... })` の腕を次に置き換える。

```rust
            Ok(Raw::Number) => match number_kind(slice) {
                Some(INT) if int_value(slice).is_none() => {
                    self.error(
                        codes::INVALID_NUMBER,
                        format!("integer literal `{slice}` is too large"),
                        self.pos,
                        self.pos + len,
                        "the largest `Int` is 9223372036854775807",
                    );
                    INT
                }
                Some(kind) => kind,
                None => {
                    self.error(
                        codes::INVALID_NUMBER,
                        format!("invalid number literal `{slice}`"),
                        self.pos,
                        self.pos + len,
                        "not a valid number",
                    );
                    INT
                }
            },
```

- [ ] **Step 5: 再公開する**

`crates/eml_syntax/src/lib.rs` の `pub use lexer::{Token, lex};` を次に変える。

```rust
pub use lexer::{Token, decode_string, int_value, lex};
```

- [ ] **Step 6: テストが通ることを確かめる**

Run: `cargo test -p eml_syntax`
Expected: PASS

- [ ] **Step 7: 全体の確認とコミット**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`

```bash
git add crates/eml_syntax
git commit -m "Reject integer literals beyond Int and expose literal values"
```

---

### Task 2: フィールドアクセスの連鎖を入れ子の深さに数える

`a.b.c…` の連鎖は parser の再帰を使わずに深い木を作るので、約2万段で木の解放がスタックを溢れさせる。連鎖の各段で `enter` し、E0013 の上限の対象にする (`docs/implementation/status.md`)。

**Files:**
- Modify: `crates/eml_syntax/src/grammar/expressions.rs` (`postfix`)
- Test: `crates/eml_syntax/tests/nesting.rs`

**Interfaces:**
- Consumes: `Parser::enter` / `Parser::leave`、`grammar::too_deep` (どちらも既存)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_syntax/tests/nesting.rs` の末尾に足す。

```rust
#[test]
fn long_field_access_chain_reports_one_error() {
    assert_one_nesting_error(&format!("x = a{}", ".b".repeat(30_000)));
}

#[test]
fn moderate_field_access_chain_is_fine() {
    assert!(diagnostics(&format!("x = a{}", ".b".repeat(200))).is_empty());
}
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_syntax --test nesting long_field_access_chain`
Expected: FAIL (スタックの溢れで異常終了するか、E0013 が出ない)

- [ ] **Step 3: 実装する**

`crates/eml_syntax/src/grammar/expressions.rs` の `postfix` を次に置き換える。

```rust
/// 連鎖は再帰せずに深い木を作るので、各段を入れ子の深さに数える。数えないと、長い連鎖の木の解放がスタックを
/// 溢れさせる (docs/implementation/status.md)。
fn postfix(p: &mut Parser) -> bool {
    let Some(mut lhs) = atom(p) else {
        return false;
    };
    let mut entered = 0;
    while p.at(DOT) && matches!(p.nth(1), LIDENT | INT) {
        if !p.enter() {
            too_deep(p);
            break;
        }
        entered += 1;
        let m = lhs.precede(p);
        dot(p);
        p.bump_any();
        lhs = m.complete(p, FIELD_EXPR);
    }
    for _ in 0..entered {
        p.leave();
    }
    true
}
```

- [ ] **Step 4: テストが通ることを確かめる**

Run: `cargo test -p eml_syntax --test nesting`
Expected: PASS

- [ ] **Step 5: 全体の確認とコミット**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`

```bash
git add crates/eml_syntax
git commit -m "Count field access chains toward the nesting limit"
```

---

### Task 3: HIR への変換で使う AST のアクセサ

HIR への変換で読む部分のアクセサを型付き AST ラッパに足す。欠けた部分があっても位置がずれないように、`if` と関数型は区切りのトークンの間で子を探す。`NOT_YET_SUPPORTED_LABEL` を後の段階から使えるように公開する。

**Files:**
- Modify: `crates/eml_syntax/src/ast.rs`
- Modify: `crates/eml_syntax/src/lib.rs` (`NOT_YET_SUPPORTED_LABEL` を `pub` にする)
- Test: `crates/eml_syntax/tests/ast.rs`

**Interfaces:**
- Produces (すべて `eml_syntax::ast` の型のメソッド):
  - `LetStmt::pat() -> Option<Pat>`、`LetStmt::ty() -> Option<Type>`、`LetStmt::body() -> Option<Expr>`
  - `ExprStmt::expr() -> Option<Expr>`
  - `IfExpr::condition() / then_branch() / else_branch() -> Option<Expr>`
  - `AppExpr::callee() -> Option<Expr>`、`AppExpr::args() -> impl Iterator<Item = Expr>`
  - `ParenExpr::expr() -> Option<Expr>`、`AnnotExpr::expr() -> Option<Expr>`、`AnnotExpr::ty() -> Option<Type>`
  - `BindPat::name() -> Option<SyntaxToken>`、`ParenPat::pat() -> Option<Pat>`
  - `PathType::segments() -> impl Iterator<Item = SyntaxToken>`、`ParenType::ty() -> Option<Type>`
  - `FnType::param() / ret() -> Option<Type>`、`FnType::row() -> Option<EffectRow>`
  - `EffectRow::effects() -> AstChildren<Effect>`、`EffectRow::tail() -> Option<SyntaxToken>`、`Effect::name() -> Option<SyntaxToken>`
  - `eml_syntax::NOT_YET_SUPPORTED_LABEL: &str` (公開)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_syntax/tests/ast.rs` の末尾に足す。

```rust
#[test]
fn let_and_expression_statement_parts() {
    let file = source("main () =\n  let x : Int = 1\n  x");
    let Some(Expr::Block(block)) = first_equation(&file).body() else {
        panic!("expected a block body");
    };
    let stmts: Vec<Stmt> = block.stmts().collect();
    let Stmt::LetStmt(stmt) = &stmts[0] else {
        panic!("expected a let statement");
    };
    assert!(matches!(stmt.pat(), Some(Pat::BindPat(_))));
    assert!(matches!(stmt.ty(), Some(Type::PathType(_))));
    assert!(matches!(stmt.body(), Some(Expr::Literal(_))));
    let Stmt::ExprStmt(stmt) = &stmts[1] else {
        panic!("expected an expression statement");
    };
    assert!(matches!(stmt.expr(), Some(Expr::PathExpr(_))));
}

#[test]
fn if_branches() {
    let text = |expr: Option<Expr>| expr.unwrap().syntax().text().to_string();
    let file = source("f = if a then b else c");
    let Some(Expr::IfExpr(e)) = first_equation(&file).body() else {
        panic!("expected an if expression");
    };
    assert_eq!(text(e.condition()), "a");
    assert_eq!(text(e.then_branch()), "b");
    assert_eq!(text(e.else_branch()), "c");
    let file = source("f = if a then b");
    let Some(Expr::IfExpr(e)) = first_equation(&file).body() else {
        panic!("expected an if expression");
    };
    assert_eq!(text(e.then_branch()), "b");
    assert!(e.else_branch().is_none());
}

#[test]
fn application_parts() {
    let file = source("f = g x (h y)");
    let Some(Expr::AppExpr(app)) = first_equation(&file).body() else {
        panic!("expected an application");
    };
    assert_eq!(app.callee().unwrap().syntax().text().to_string(), "g");
    let args: Vec<String> = app.args().map(|a| a.syntax().text().to_string()).collect();
    assert_eq!(args, ["x", "(h y)"]);
}

#[test]
fn function_type_parts() {
    let file = source("f : Int -> <IO | e> String");
    let Some(Item::Signature(signature)) = file.items().next() else {
        panic!("expected a signature");
    };
    let Some(Type::FnType(ty)) = signature.ty() else {
        panic!("expected a function type");
    };
    assert_eq!(ty.param().unwrap().syntax().text().to_string(), "Int");
    assert_eq!(ty.ret().unwrap().syntax().text().to_string(), "String");
    let row = ty.row().unwrap();
    let effects: Vec<String> = row
        .effects()
        .map(|e| e.name().unwrap().text().to_string())
        .collect();
    assert_eq!(effects, ["IO"]);
    assert_eq!(row.tail().unwrap().text(), "e");
}

#[test]
fn parenthesized_and_annotated_parts() {
    let file = source("f (x) = (x : (Int))");
    let equation = first_equation(&file);
    let Some(Pat::ParenPat(paren)) = equation.params().next() else {
        panic!("expected a parenthesized pattern");
    };
    let Some(Pat::BindPat(bind)) = paren.pat() else {
        panic!("expected a variable pattern");
    };
    assert_eq!(bind.name().unwrap().text(), "x");
    let Some(Expr::AnnotExpr(annot)) = equation.body() else {
        panic!("expected an annotation");
    };
    assert!(matches!(annot.expr(), Some(Expr::PathExpr(_))));
    let Some(Type::ParenType(paren)) = annot.ty() else {
        panic!("expected a parenthesized type");
    };
    let Some(Type::PathType(path)) = paren.ty() else {
        panic!("expected a type name");
    };
    let segments: Vec<String> = path.segments().map(|s| s.text().to_string()).collect();
    assert_eq!(segments, ["Int"]);
}

#[test]
fn parenthesized_expression() {
    let file = source("f = (g)");
    let Some(Expr::ParenExpr(paren)) = first_equation(&file).body() else {
        panic!("expected a parenthesized expression");
    };
    assert!(matches!(paren.expr(), Some(Expr::PathExpr(_))));
}
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_syntax --test ast`
Expected: コンパイルエラー (アクセサがない)

- [ ] **Step 3: アクセサを実装する**

`crates/eml_syntax/src/ast.rs` の `impl PathExpr` を、共通の関数を使う形に変え、その後ろに次を足す (既存の `impl PathExpr` は置き換える)。

```rust
impl PathExpr {
    pub fn segments(&self) -> impl Iterator<Item = SyntaxToken> {
        path_segments(&self.syntax)
    }
}

impl LetStmt {
    pub fn pat(&self) -> Option<Pat> {
        support::child(&self.syntax)
    }

    pub fn ty(&self) -> Option<Type> {
        support::child(&self.syntax)
    }

    pub fn body(&self) -> Option<Expr> {
        support::child(&self.syntax)
    }
}

impl ExprStmt {
    pub fn expr(&self) -> Option<Expr> {
        support::child(&self.syntax)
    }
}

impl IfExpr {
    pub fn condition(&self) -> Option<Expr> {
        child_between(&self.syntax, Some(SyntaxKind::IF_KW), Some(SyntaxKind::THEN_KW))
    }

    pub fn then_branch(&self) -> Option<Expr> {
        child_between(&self.syntax, Some(SyntaxKind::THEN_KW), Some(SyntaxKind::ELSE_KW))
    }

    pub fn else_branch(&self) -> Option<Expr> {
        child_between(&self.syntax, Some(SyntaxKind::ELSE_KW), None)
    }
}

impl AppExpr {
    pub fn callee(&self) -> Option<Expr> {
        support::child(&self.syntax)
    }

    pub fn args(&self) -> impl Iterator<Item = Expr> {
        support::children::<Expr>(&self.syntax).skip(1)
    }
}

impl ParenExpr {
    pub fn expr(&self) -> Option<Expr> {
        support::child(&self.syntax)
    }
}

impl AnnotExpr {
    pub fn expr(&self) -> Option<Expr> {
        support::child(&self.syntax)
    }

    pub fn ty(&self) -> Option<Type> {
        support::child(&self.syntax)
    }
}

impl BindPat {
    pub fn name(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::LIDENT)
    }
}

impl ParenPat {
    pub fn pat(&self) -> Option<Pat> {
        support::child(&self.syntax)
    }
}

impl PathType {
    pub fn segments(&self) -> impl Iterator<Item = SyntaxToken> {
        path_segments(&self.syntax)
    }
}

impl ParenType {
    pub fn ty(&self) -> Option<Type> {
        support::child(&self.syntax)
    }
}

impl FnType {
    pub fn param(&self) -> Option<Type> {
        child_between(&self.syntax, None, Some(SyntaxKind::THIN_ARROW))
    }

    pub fn row(&self) -> Option<EffectRow> {
        support::child(&self.syntax)
    }

    pub fn ret(&self) -> Option<Type> {
        child_between(&self.syntax, Some(SyntaxKind::THIN_ARROW), None)
    }
}

impl EffectRow {
    pub fn effects(&self) -> AstChildren<Effect> {
        support::children(&self.syntax)
    }

    /// `<IO | e>` と `<e>` の row 変数。
    pub fn tail(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::LIDENT)
    }
}

impl Effect {
    pub fn name(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::UIDENT)
    }
}

fn path_segments(node: &SyntaxNode) -> impl Iterator<Item = SyntaxToken> {
    node.children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .filter(|token| matches!(token.kind(), SyntaxKind::UIDENT | SyntaxKind::LIDENT))
}

/// 欠けた部分があっても前後の部分を取り違えないように、区切りのトークンの間で子を探す。
fn child_between<N: AstNode<Language = EmlLanguage>>(
    node: &SyntaxNode,
    after: Option<SyntaxKind>,
    before: Option<SyntaxKind>,
) -> Option<N> {
    let mut started = after.is_none();
    for element in node.children_with_tokens() {
        let kind = element.kind();
        if !started {
            started = Some(kind) == after;
            continue;
        }
        if Some(kind) == before {
            return None;
        }
        if let Some(found) = element.into_node().and_then(N::cast) {
            return Some(found);
        }
    }
    None
}
```

`crates/eml_syntax/src/lib.rs` の `pub(crate) const NOT_YET_SUPPORTED_LABEL` を `pub const NOT_YET_SUPPORTED_LABEL` にし、直前に次のコメントを置く。

```rust
/// E0004 のラベル。HIR 以降の段階も、まだ扱えない構文に同じ文言を使う (docs/spec/diagnostics.md)。
```

- [ ] **Step 4: テストが通ることを確かめる**

Run: `cargo test -p eml_syntax --test ast`
Expected: PASS

- [ ] **Step 5: 全体の確認とコミット**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`

```bash
git add crates/eml_syntax
git commit -m "Add the AST accessors that HIR lowering needs"
```

---
### Task 4: HIR のデータ構造、宣言の対応づけ、名前解決 (演算子の列を除く)

`eml_hir` に段階1の HIR を作る。宣言 (シグネチャと等式) を名前で対応づけ、型の注釈と本体を変換し、名前を解決する。演算子の列 (`OP_SEQ`) は Task 5 で扱うので、このタスクでは `Missing` にしておく (Task 5 で置き換える一時的な扱い。このタスクのテストは演算子を使わない)。

HIR のノードは `SyntaxNodePtr` ではなく `TextRange` を持つ。演算子の列を組み直した部分式 (`a + b * c` の `b * c`) のように、対応する構文ノードのない式があるため (Task 12 で architecture.md に反映する)。

**Files:**
- Modify: `Cargo.toml` (workspace の依存に `la-arena = "0.3"`)
- Modify: `crates/eml_hir/Cargo.toml`
- Modify: `crates/eml_hir/src/lib.rs`
- Create: `crates/eml_hir/src/hir.rs`、`crates/eml_hir/src/builtin.rs`、`crates/eml_hir/src/pretty.rs`
- Create: `crates/eml_hir/src/lower/mod.rs`、`crates/eml_hir/src/lower/types.rs`、`crates/eml_hir/src/lower/expr.rs`
- Create: `crates/eml_hir/tests/common/mod.rs`、`crates/eml_hir/tests/lower.rs`
- Snapshots: `crates/eml_cli/tests/snapshots/ui__check_fail@*.snap` (E1004 などが加わる)

**Interfaces:**
- Consumes: Task 1 の `int_value` / `decode_string`、Task 3 のアクセサと `NOT_YET_SUPPORTED_LABEL`
- Produces:
  - `eml_hir::lower(file: FileId, source: &ast::SourceFile) -> (Module, Vec<Diagnostic>)` (シグネチャは既存と同じ)
  - `eml_hir::pretty(module: &Module) -> String` (テスト用の表示)
  - `eml_hir::not_yet_supported(file: FileId, range: TextRange, message: impl Into<String>) -> Diagnostic` (E0004。`eml_types` も使う)
  - `eml_hir::codes::{UNDEFINED_NAME, UNDEFINED_TYPE, DUPLICATE_DEFINITION, MISSING_SIGNATURE, MISSING_EQUATION, NON_ASSOCIATIVE_OPERATORS}` (E1001〜E1006)
  - 型: `Module { file, functions: Arena<Function> }`、`Function { name, name_range, signature: Option<Signature>, body: Option<Body>, types: Arena<TypeRef> }`、`Signature { ty: TypeRefId, range }`、`Body { params: Vec<PatId>, root: ExprId, exprs, pats, locals }`、`Expr { kind: ExprKind, range }`、`ExprKind::{Missing, Literal, Path, Call, If, Block, Annot}`、`Literal::{Int(i64), String(String), Unit}`、`Res::{Local, Function, Builtin}`、`Stmt::{Let { pat, ty, init }, Expr}`、`Pat { kind: PatKind, range }`、`PatKind::{Missing, Bind, Wildcard, Unit}`、`Local { name, range }`、`TypeRef { kind: TypeRefKind, range }`、`TypeRefKind::{Error, Builtin(BuiltinType), Fn { param, row: RowRef, ret }}`、`RowRef::{Omitted, Closed { effects: Vec<EffectRef>, range }, Error}`、`EffectRef::Io`、ID 型 `FunctionId`、`ExprId`、`PatId`、`LocalId`、`TypeRefId`
  - `eml_hir::builtin::{Builtin, BuiltinType, Assoc, fixity}`。`Builtin` は `Println, ShowInt, Not, True, False, IntAdd, IntSub, IntMul, IntDiv, IntMod, IntNeg, IntEq, IntNe, IntLt, IntLe, IntGt, IntGe, StrConcat` で、`Builtin::from_name(&str) -> Option<Builtin>` と `Builtin::name(self) -> &'static str` を持つ

- [ ] **Step 1: 依存を足す**

`Cargo.toml` の `[workspace.dependencies]` に足す (アルファベット順で `insta` の後)。

```toml
la-arena = "0.3"
```

`crates/eml_hir/Cargo.toml` を次にする。

```toml
[package]
name = "eml_hir"
version.workspace = true
edition.workspace = true

[dependencies]
eml_diagnostics.workspace = true
eml_syntax.workspace = true
la-arena.workspace = true
rowan.workspace = true

[dev-dependencies]
insta.workspace = true
```

- [ ] **Step 2: テストの補助を書く**

`crates/eml_hir/tests/common/mod.rs` を作る。

```rust
use eml_diagnostics::{Diagnostic, SourceFiles};

/// HIR の表示と、構文と HIR の診断を位置の順に並べたもの。
pub fn lower_text(text: &str) -> String {
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (parse, mut diagnostics) = eml_syntax::parse(file, text);
    let (module, hir_diagnostics) = eml_hir::lower(file, &parse.tree());
    diagnostics.extend(hir_diagnostics);
    diagnostics.sort_by_key(|d| d.primary.range.start());
    let mut out = eml_hir::pretty(&module);
    if !diagnostics.is_empty() {
        out.push_str("---\n");
        for diagnostic in &diagnostics {
            out.push_str(&format_diagnostic(text, diagnostic));
            out.push('\n');
        }
    }
    out
}

fn format_diagnostic(text: &str, diagnostic: &Diagnostic) -> String {
    let offset = u32::from(diagnostic.primary.range.start()) as usize;
    let before = &text[..offset];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    format!("{} {line}:{column} {}", diagnostic.code, diagnostic.message)
}
```

- [ ] **Step 3: 失敗するテストを書く**

`crates/eml_hir/tests/lower.rs` を作る。表示の約束: 局所変数は `名前#番号`、トップレベルの関数は `@名前`、組み込みは名前そのまま、呼び出しは `(関数 引数...)`、ブロックは `{` と `}` の間に1行1文。

```rust
mod common;

use common::lower_text;

#[test]
fn a_signature_and_an_equation_become_a_function() {
    insta::assert_snapshot!(lower_text("f : Int -> <IO> Unit\nf x = println (show_int x)"), @r"
    f : Int -> <IO> Unit
    f x#0 = (println (show_int x#0))
    ");
}

#[test]
fn later_lets_shadow_earlier_names() {
    let text = "f : Int -> Int\nf x =\n  let y = x\n  let x = y\n  x";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> Int
    f x#0 = {
      let y#1 = x#0
      let x#2 = y#1
      x#2
    }
    ");
}

#[test]
fn functions_resolve_in_any_order_and_shadow_builtins() {
    let text = "a : Int -> Int\na n = b n\n\nb : Int -> Int\nb n = not n\n\nnot : Int -> Int\nnot n = n";
    insta::assert_snapshot!(lower_text(text), @r"
    a : Int -> Int
    a n#0 = (@b n#0)
    b : Int -> Int
    b n#0 = (@not n#0)
    not : Int -> Int
    not n#0 = n#0
    ");
}

#[test]
fn if_without_else_and_annotations() {
    let text = "f : Bool -> Unit\nf b =\n  if b then println \"yes\"\n  (() : Unit)";
    insta::assert_snapshot!(lower_text(text), @r#"
    f : Bool -> Unit
    f b#0 = {
      (if b#0 (println "yes"))
      (() : Unit)
    }
    "#);
}

#[test]
fn unit_and_wildcard_parameters_and_literals() {
    let text = "g : Unit -> Bool -> String\ng () _ = if True then \"a\\n\" else \"b\"";
    insta::assert_snapshot!(lower_text(text), @r#"
    g : Unit -> Bool -> String
    g () _ = (if True "a\n" "b")
    "#);
}

#[test]
fn undefined_names_are_reported() {
    insta::assert_snapshot!(lower_text("f : Int -> Strin\nf x = g y Foo"), @r"
    f : Int -> <error>
    f x#0 = (<missing> <missing> <missing>)
    ---
    E1002 1:12 cannot find type `Strin`
    E1001 2:7 cannot find value `g`
    E1001 2:9 cannot find value `y`
    E1001 2:11 cannot find constructor `Foo`
    ");
}

#[test]
fn signatures_and_equations_are_paired_by_name() {
    let text = "a : Int\nb = 1\nc : Int\nc = 2\nc : Int\nd : Int\nd = 3\nd = 4";
    insta::assert_snapshot!(lower_text(text), @r"
    a : Int
    a = <no equation>
    b : <no signature>
    b = 1
    c : Int
    c = 2
    d : Int
    d = 3
    ---
    E1005 1:1 `a` has a signature but no equation
    E1004 2:1 `b` has no type signature
    E1003 5:1 `c` is defined more than once
    E0004 8:1 defining a function with several equations is not supported yet
    ");
}

#[test]
fn constructs_of_later_stages_are_not_yet_supported() {
    let text = "data Color = | Red\nf : Int -> Int\nf x =\n  let g = fn y -> y\n  match x with | _ -> x";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> Int
    f x#0 = {
      let g#1 = <missing>
      <missing>
    }
    ---
    E0004 1:1 `data` declarations are not supported yet
    E0004 4:11 lambdas are not supported yet
    E0004 5:3 `match` is not supported yet
    ");
}

#[test]
fn rows_and_types_of_later_stages() {
    let text = "f : Int -> <e> Int\nf x = x\ng : a -> <State> Int\ng x = 1";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> <error> Int
    f x#0 = x#0
    g : <error> -> <error> Int
    g x#0 = 1
    ---
    E0004 1:13 row variables are not supported yet
    E0004 3:5 type variables are not supported yet
    E1002 3:11 cannot find effect `State`
    ");
}

#[test]
fn operator_definitions_and_qualified_names() {
    let text = "(<+>) : Int\na <+> b = a\nf : Int -> Int\nf (x) = List.length x";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> Int
    f x#0 = (<missing> x#0)
    ---
    E0004 1:2 defining operators is not supported yet
    E0004 2:3 defining operators is not supported yet
    E0004 4:9 qualified names are not supported yet
    ");
}
```

- [ ] **Step 4: 失敗を確かめる**

Run: `cargo test -p eml_hir --test lower`
Expected: コンパイルエラー (`pretty` などがない)

- [ ] **Step 5: 組み込みの表を書く**

`crates/eml_hir/src/builtin.rs` を作る。

```rust
//! 組み込みの名前と型と演算子。S2 で `Prelude` モジュールに移すまで、名前解決の最も外側のスコープとして扱う。

/// 組み込みの値と関数。型は `eml_types` が、実装は `eml_interp` が、この enum の `match` で与える。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Builtin {
    Println,
    ShowInt,
    Not,
    True,
    False,
    IntAdd,
    IntSub,
    IntMul,
    IntDiv,
    IntMod,
    IntNeg,
    IntEq,
    IntNe,
    IntLt,
    IntLe,
    IntGt,
    IntGe,
    StrConcat,
}

impl Builtin {
    /// 名前で引ける組み込み。演算子は `binary_operator` で引く。
    pub fn from_name(name: &str) -> Option<Builtin> {
        Some(match name {
            "println" => Builtin::Println,
            "show_int" => Builtin::ShowInt,
            "not" => Builtin::Not,
            "True" => Builtin::True,
            "False" => Builtin::False,
            _ => return None,
        })
    }

    /// 二項演算子の組み込み。`&&`、`||`、`|>`、`<|` は HIR で脱糖するので含まない (docs/spec/declarations.md)。
    pub fn binary_operator(op: &str) -> Option<Builtin> {
        Some(match op {
            "+" => Builtin::IntAdd,
            "-" => Builtin::IntSub,
            "*" => Builtin::IntMul,
            "/" => Builtin::IntDiv,
            "%" => Builtin::IntMod,
            "==" => Builtin::IntEq,
            "!=" => Builtin::IntNe,
            "<" => Builtin::IntLt,
            "<=" => Builtin::IntLe,
            ">" => Builtin::IntGt,
            ">=" => Builtin::IntGe,
            "++" => Builtin::StrConcat,
            _ => return None,
        })
    }

    /// ソースでの書き方。診断と表示で使う。
    pub fn name(self) -> &'static str {
        match self {
            Builtin::Println => "println",
            Builtin::ShowInt => "show_int",
            Builtin::Not => "not",
            Builtin::True => "True",
            Builtin::False => "False",
            Builtin::IntAdd => "+",
            Builtin::IntSub => "-",
            Builtin::IntMul => "*",
            Builtin::IntDiv => "/",
            Builtin::IntMod => "%",
            Builtin::IntNeg => "negate",
            Builtin::IntEq => "==",
            Builtin::IntNe => "!=",
            Builtin::IntLt => "<",
            Builtin::IntLe => "<=",
            Builtin::IntGt => ">",
            Builtin::IntGe => ">=",
            Builtin::StrConcat => "++",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinType {
    Int,
    String,
    Bool,
    Unit,
}

impl BuiltinType {
    pub fn from_name(name: &str) -> Option<BuiltinType> {
        Some(match name {
            "Int" => BuiltinType::Int,
            "String" => BuiltinType::String,
            "Bool" => BuiltinType::Bool,
            "Unit" => BuiltinType::Unit,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            BuiltinType::Int => "Int",
            BuiltinType::String => "String",
            BuiltinType::Bool => "Bool",
            BuiltinType::Unit => "Unit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    Left,
    Right,
    None,
}

/// 標準の演算子の表 (docs/spec/declarations.md)。fixity の宣言は段階6で読む。
pub fn fixity(op: &str) -> Option<(u8, Assoc)> {
    Some(match op {
        "<|" => (0, Assoc::Right),
        "|>" => (1, Assoc::Left),
        "||" => (2, Assoc::Right),
        "&&" => (3, Assoc::Right),
        "==" | "!=" | "<" | "<=" | ">" | ">=" => (4, Assoc::None),
        "++" | "::" => (5, Assoc::Right),
        "+" | "-" => (6, Assoc::Left),
        "*" | "/" | "%" => (7, Assoc::Left),
        ">>" | "<<" => (9, Assoc::Right),
        _ => return None,
    })
}
```

- [ ] **Step 6: HIR のデータ構造を書く**

`crates/eml_hir/src/hir.rs` を作る。

```rust
use eml_diagnostics::{FileId, TextRange};
use la_arena::{Arena, Idx};

use crate::builtin::{Builtin, BuiltinType};

pub type FunctionId = Idx<Function>;
pub type ExprId = Idx<Expr>;
pub type PatId = Idx<Pat>;
pub type LocalId = Idx<Local>;
pub type TypeRefId = Idx<TypeRef>;

/// HIR のノードは `SyntaxNodePtr` ではなく範囲を持つ。演算子の列を組み直した部分式のように、対応する構文ノードの
/// ない式があるため。
#[derive(Debug)]
pub struct Module {
    pub file: FileId,
    pub functions: Arena<Function>,
}

#[derive(Debug)]
pub struct Function {
    pub name: String,
    /// 最初の等式の名前の位置。等式がなければシグネチャの名前の位置。
    pub name_range: TextRange,
    /// なければ `None` で、E1004 は報告済み。
    pub signature: Option<Signature>,
    /// 等式がなければ `None` で、E1005 は報告済み。
    pub body: Option<Body>,
    /// シグネチャと、本体の中の型の注釈。
    pub types: Arena<TypeRef>,
}

#[derive(Debug)]
pub struct Signature {
    pub ty: TypeRefId,
    /// シグネチャの型の範囲。
    pub range: TextRange,
}

/// 本体を関数ごとに持つのは、後でクエリ化したときに関数単位で再計算できるようにするため (rust-analyzer と同じ)。
#[derive(Debug)]
pub struct Body {
    pub params: Vec<PatId>,
    pub root: ExprId,
    pub exprs: Arena<Expr>,
    pub pats: Arena<Pat>,
    pub locals: Arena<Local>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expr {
    pub kind: ExprKind,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprKind {
    /// 構文エラーや未対応の構文、未定義の名前の跡。診断は報告済みなので、後の段階は何も言わない。
    Missing,
    Literal(Literal),
    Path(Res),
    /// `(f a) b` と `x |> f a` は、引数を並べた1つの呼び出しにしてある。
    Call { callee: ExprId, args: Vec<ExprId> },
    /// `else` を省略したら `None`。型検査が「`else` のない `if`」として診断できるように、`()` を補わずに残す。
    If {
        condition: ExprId,
        then_branch: ExprId,
        else_branch: Option<ExprId>,
    },
    /// 最後の文が `let` なら `tail` は `None` で、値は `()` である (docs/spec/expressions.md)。
    Block { stmts: Vec<Stmt>, tail: Option<ExprId> },
    Annot { expr: ExprId, ty: TypeRefId },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Literal {
    Int(i64),
    String(String),
    Unit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Res {
    Local(LocalId),
    Function(FunctionId),
    Builtin(Builtin),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    Let {
        pat: PatId,
        ty: Option<TypeRefId>,
        init: ExprId,
    },
    Expr(ExprId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pat {
    pub kind: PatKind,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatKind {
    Missing,
    Bind(LocalId),
    Wildcard,
    Unit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Local {
    pub name: String,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRef {
    pub kind: TypeRefKind,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeRefKind {
    Error,
    Builtin(BuiltinType),
    Fn {
        param: TypeRefId,
        row: RowRef,
        ret: TypeRefId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowRef {
    /// 省略した row。空の row `<>` である (docs/spec/types.md の「関数型」)。
    Omitted,
    Closed {
        effects: Vec<EffectRef>,
        range: TextRange,
    },
    /// 未対応の row 変数や未定義のエフェクトの跡。型検査はどのエフェクトも受け入れ、診断を連鎖させない。
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectRef {
    Io,
}
```

- [ ] **Step 7: lib.rs を書く**

`crates/eml_hir/src/lib.rs` を次の内容に置き換える。

```rust
//! CST から HIR への変換と名前解決 (docs/implementation/architecture.md の「`eml_hir` で行う脱糖と検査」)。

pub mod builtin;
mod hir;
mod lower;
mod pretty;

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};

pub use hir::*;
pub use lower::lower;
pub use pretty::pretty;

pub mod codes {
    use eml_diagnostics::ErrorCode;

    pub const UNDEFINED_NAME: ErrorCode = ErrorCode(1001);
    pub const UNDEFINED_TYPE: ErrorCode = ErrorCode(1002);
    pub const DUPLICATE_DEFINITION: ErrorCode = ErrorCode(1003);
    pub const MISSING_SIGNATURE: ErrorCode = ErrorCode(1004);
    pub const MISSING_EQUATION: ErrorCode = ErrorCode(1005);
    pub const NON_ASSOCIATIVE_OPERATORS: ErrorCode = ErrorCode(1006);
}

/// まだ扱えない構文。E0004 はどの段階でも「後で実装する」という同じ意味で使う (docs/spec/diagnostics.md)。
pub fn not_yet_supported(file: FileId, range: TextRange, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(
        eml_syntax::codes::NOT_YET_SUPPORTED,
        message,
        Label::new(file, range, eml_syntax::NOT_YET_SUPPORTED_LABEL),
    )
}
```

- [ ] **Step 8: 型の注釈の変換を書く**

`crates/eml_hir/src/lower/types.rs` を作る。

```rust
use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_syntax::{SyntaxToken, ast};
use la_arena::Arena;
use rowan::ast::AstNode;

use crate::builtin::BuiltinType;
use crate::hir::{EffectRef, RowRef, TypeRef, TypeRefId, TypeRefKind};
use crate::{codes, not_yet_supported};

pub(super) struct TypeLowering<'a> {
    pub file: FileId,
    pub types: &'a mut Arena<TypeRef>,
    pub diagnostics: &'a mut Vec<Diagnostic>,
}

impl TypeLowering<'_> {
    pub fn lower(&mut self, ty: Option<ast::Type>, fallback: TextRange) -> TypeRefId {
        let Some(ty) = ty else {
            return self.alloc(TypeRefKind::Error, fallback);
        };
        let range = ty.syntax().text_range();
        let kind = match ty {
            ast::Type::PathType(path) => self.path(&path, range),
            ast::Type::ParenType(paren) => return self.lower(paren.ty(), range),
            ast::Type::FnType(function) => {
                let param = self.lower(function.param(), range);
                let row = match function.row() {
                    Some(row) => self.row(&row),
                    None => RowRef::Omitted,
                };
                let ret = self.lower(function.ret(), range);
                TypeRefKind::Fn { param, row, ret }
            }
            ast::Type::VarType(_) => self.unsupported(range, "type variables are not supported yet"),
            ast::Type::AppType(_) => {
                self.unsupported(range, "type applications are not supported yet")
            }
            ast::Type::TupleType(_) => self.unsupported(range, "tuple types are not supported yet"),
        };
        self.alloc(kind, range)
    }

    fn path(&mut self, path: &ast::PathType, range: TextRange) -> TypeRefKind {
        let segments: Vec<SyntaxToken> = path.segments().collect();
        let [name] = segments.as_slice() else {
            return self.unsupported(range, "qualified names are not supported yet");
        };
        match BuiltinType::from_name(name.text()) {
            Some(builtin) => TypeRefKind::Builtin(builtin),
            None => {
                self.diagnostics.push(Diagnostic::error(
                    codes::UNDEFINED_TYPE,
                    format!("cannot find type `{}`", name.text()),
                    Label::new(self.file, range, "not found in this scope"),
                ));
                TypeRefKind::Error
            }
        }
    }

    fn row(&mut self, row: &ast::EffectRow) -> RowRef {
        let mut valid = true;
        if let Some(tail) = row.tail() {
            self.diagnostics.push(not_yet_supported(
                self.file,
                tail.text_range(),
                "row variables are not supported yet",
            ));
            valid = false;
        }
        let mut effects = Vec::new();
        for effect in row.effects() {
            let Some(name) = effect.name() else {
                continue;
            };
            if name.text() == "IO" {
                effects.push(EffectRef::Io);
            } else {
                // ユーザー定義のエフェクトは段階3で入れる。宣言も E0004 になるので、ここでは未定義として扱う
                self.diagnostics.push(Diagnostic::error(
                    codes::UNDEFINED_TYPE,
                    format!("cannot find effect `{}`", name.text()),
                    Label::new(self.file, effect.syntax().text_range(), "not found in this scope"),
                ));
                valid = false;
            }
        }
        if valid {
            RowRef::Closed {
                effects,
                range: row.syntax().text_range(),
            }
        } else {
            RowRef::Error
        }
    }

    fn unsupported(&mut self, range: TextRange, message: &str) -> TypeRefKind {
        self.diagnostics.push(not_yet_supported(self.file, range, message));
        TypeRefKind::Error
    }

    fn alloc(&mut self, kind: TypeRefKind, range: TextRange) -> TypeRefId {
        self.types.alloc(TypeRef { kind, range })
    }
}
```

- [ ] **Step 9: 本体の変換と名前解決を書く**

`crates/eml_hir/src/lower/expr.rs` を作る。

```rust
use std::collections::HashMap;

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_syntax::{SyntaxKind, SyntaxNode, SyntaxToken, ast, decode_string, int_value};
use la_arena::Arena;
use rowan::ast::AstNode;

use super::types::TypeLowering;
use crate::builtin::Builtin;
use crate::hir::*;
use crate::{codes, not_yet_supported};

pub(super) struct BodyLowering<'a> {
    pub(super) file: FileId,
    functions: &'a HashMap<String, FunctionId>,
    types: &'a mut Arena<TypeRef>,
    pub(super) diagnostics: &'a mut Vec<Diagnostic>,
    pub(super) exprs: Arena<Expr>,
    pats: Arena<Pat>,
    locals: Arena<Local>,
    /// 内側の束縛ほど後ろにある。後の `let` が前の同じ名前を隠す (docs/spec/expressions.md)。
    scope: Vec<(String, LocalId)>,
}

impl<'a> BodyLowering<'a> {
    pub(super) fn new(
        file: FileId,
        functions: &'a HashMap<String, FunctionId>,
        types: &'a mut Arena<TypeRef>,
        diagnostics: &'a mut Vec<Diagnostic>,
    ) -> Self {
        BodyLowering {
            file,
            functions,
            types,
            diagnostics,
            exprs: Arena::new(),
            pats: Arena::new(),
            locals: Arena::new(),
            scope: Vec::new(),
        }
    }

    pub(super) fn lower_equation(mut self, equation: &ast::Equation) -> Body {
        let range = equation.syntax().text_range();
        let params = equation
            .params()
            .map(|pat| self.lower_pat(Some(pat), range))
            .collect();
        let root = self.lower_expr(equation.body(), range);
        Body {
            params,
            root,
            exprs: self.exprs,
            pats: self.pats,
            locals: self.locals,
        }
    }

    pub(super) fn lower_expr(&mut self, expr: Option<ast::Expr>, fallback: TextRange) -> ExprId {
        let Some(expr) = expr else {
            return self.alloc(ExprKind::Missing, fallback);
        };
        let range = expr.syntax().text_range();
        match expr {
            ast::Expr::Literal(literal) => {
                let value = literal.token().and_then(|token| match token.kind() {
                    SyntaxKind::INT => int_value(token.text()).map(Literal::Int),
                    SyntaxKind::STRING => decode_string(token.text()).map(Literal::String),
                    // 浮動小数などは字句解析と構文解析が E0004 を報告済み
                    _ => None,
                });
                let kind = value.map_or(ExprKind::Missing, ExprKind::Literal);
                self.alloc(kind, range)
            }
            ast::Expr::UnitExpr(_) => self.alloc(ExprKind::Literal(Literal::Unit), range),
            ast::Expr::PathExpr(path) => self.lower_path(&path, range),
            ast::Expr::ParenExpr(paren) => self.lower_expr(paren.expr(), range),
            ast::Expr::AppExpr(app) => {
                let callee = self.lower_expr(app.callee(), range);
                let args = app
                    .args()
                    .map(|arg| {
                        let arg_range = arg.syntax().text_range();
                        self.lower_expr(Some(arg), arg_range)
                    })
                    .collect();
                self.call(callee, args, range)
            }
            ast::Expr::AnnotExpr(annot) => {
                let expr = self.lower_expr(annot.expr(), range);
                let ty = self.lower_type(annot.ty(), range);
                self.alloc(ExprKind::Annot { expr, ty }, range)
            }
            ast::Expr::IfExpr(if_expr) => {
                let condition = self.lower_expr(if_expr.condition(), range);
                let then_branch = self.lower_expr(if_expr.then_branch(), range);
                let else_branch = if_expr
                    .else_branch()
                    .map(|e| self.lower_expr(Some(e), range));
                self.alloc(
                    ExprKind::If {
                        condition,
                        then_branch,
                        else_branch,
                    },
                    range,
                )
            }
            ast::Expr::Block(block) => self.lower_block(&block, range),
            // 演算子の列は Task 5 で組み直す
            ast::Expr::OpSeq(_) => self.alloc(ExprKind::Missing, range),
            ast::Expr::LambdaExpr(e) => {
                self.unsupported(keyword(e.syntax()), "lambdas are not supported yet")
            }
            ast::Expr::MatchExpr(e) => {
                self.unsupported(keyword(e.syntax()), "`match` is not supported yet")
            }
            ast::Expr::HandleExpr(e) => {
                self.unsupported(keyword(e.syntax()), "handlers are not supported yet")
            }
            ast::Expr::LetExpr(e) => {
                self.unsupported(keyword(e.syntax()), "`let ... in` is not supported yet")
            }
            ast::Expr::ResumeExpr(e) => {
                self.unsupported(keyword(e.syntax()), "`resume` is not supported yet")
            }
            ast::Expr::DropExpr(e) => {
                self.unsupported(keyword(e.syntax()), "`drop` is not supported yet")
            }
            ast::Expr::FieldExpr(_) => self.unsupported(range, "field access is not supported yet"),
            ast::Expr::TupleExpr(_) => self.unsupported(range, "tuples are not supported yet"),
            ast::Expr::OpRef(_) => {
                self.unsupported(range, "operator references are not supported yet")
            }
            ast::Expr::LeftSection(_) | ast::Expr::RightSection(_) | ast::Expr::FieldSection(_) => {
                self.unsupported(range, "sections are not supported yet")
            }
        }
    }

    fn lower_path(&mut self, path: &ast::PathExpr, range: TextRange) -> ExprId {
        let segments: Vec<SyntaxToken> = path.segments().collect();
        let [name] = segments.as_slice() else {
            return self.unsupported(range, "qualified names are not supported yet");
        };
        let text = name.text();
        let res = self
            .scope
            .iter()
            .rev()
            .find(|(local, _)| local == text)
            .map(|&(_, local)| Res::Local(local))
            .or_else(|| self.functions.get(text).map(|&f| Res::Function(f)))
            .or_else(|| Builtin::from_name(text).map(Res::Builtin));
        match res {
            Some(res) => self.alloc(ExprKind::Path(res), range),
            None => {
                let what = if name.kind() == SyntaxKind::UIDENT {
                    "constructor"
                } else {
                    "value"
                };
                self.diagnostics.push(Diagnostic::error(
                    codes::UNDEFINED_NAME,
                    format!("cannot find {what} `{text}`"),
                    Label::new(self.file, range, "not found in this scope"),
                ));
                self.alloc(ExprKind::Missing, range)
            }
        }
    }

    /// `(f a) b` と `x |> f a` を、引数の揃った1つの呼び出しとして型検査できるように、入れ子の呼び出しを平たくする。
    pub(super) fn call(&mut self, callee: ExprId, mut args: Vec<ExprId>, range: TextRange) -> ExprId {
        if let ExprKind::Call {
            callee: inner,
            args: inner_args,
        } = &self.exprs[callee].kind
        {
            let inner = *inner;
            let mut all = inner_args.clone();
            all.append(&mut args);
            return self.alloc(ExprKind::Call { callee: inner, args: all }, range);
        }
        self.alloc(ExprKind::Call { callee, args }, range)
    }

    fn lower_block(&mut self, block: &ast::Block, range: TextRange) -> ExprId {
        let mark = self.scope.len();
        let all: Vec<ast::Stmt> = block.stmts().collect();
        let mut stmts = Vec::new();
        let mut tail = None;
        for (index, stmt) in all.iter().enumerate() {
            let stmt_range = stmt.syntax().text_range();
            match stmt {
                ast::Stmt::ExprStmt(stmt) => {
                    let expr = self.lower_expr(stmt.expr(), stmt_range);
                    if index + 1 == all.len() {
                        tail = Some(expr);
                    } else {
                        stmts.push(Stmt::Expr(expr));
                    }
                }
                ast::Stmt::LetStmt(stmt) => {
                    // 右辺を先に変換する。`let x = x + 1` の右辺の `x` は外側の `x` を指すため
                    let init = self.lower_expr(stmt.body(), stmt_range);
                    let ty = stmt.ty().map(|ty| self.lower_type(Some(ty), stmt_range));
                    let pat = self.lower_pat(stmt.pat(), stmt_range);
                    stmts.push(Stmt::Let { pat, ty, init });
                }
                ast::Stmt::UseStmt(stmt) => {
                    self.unsupported(keyword(stmt.syntax()), "`use` is not supported yet");
                }
            }
        }
        self.scope.truncate(mark);
        self.alloc(ExprKind::Block { stmts, tail }, range)
    }

    fn lower_pat(&mut self, pat: Option<ast::Pat>, fallback: TextRange) -> PatId {
        let Some(pat) = pat else {
            return self.pats.alloc(Pat {
                kind: PatKind::Missing,
                range: fallback,
            });
        };
        let range = pat.syntax().text_range();
        let kind = match pat {
            ast::Pat::BindPat(bind) => match bind.name() {
                Some(name) => {
                    let name = name.text().to_string();
                    let local = self.locals.alloc(Local {
                        name: name.clone(),
                        range,
                    });
                    self.scope.push((name, local));
                    PatKind::Bind(local)
                }
                None => PatKind::Missing,
            },
            ast::Pat::WildcardPat(_) => PatKind::Wildcard,
            ast::Pat::UnitPat(_) => PatKind::Unit,
            ast::Pat::ParenPat(paren) => return self.lower_pat(paren.pat(), range),
            ast::Pat::ConPat(_) | ast::Pat::InfixConPat(_) => {
                self.unsupported_pat(range, "constructor patterns are not supported yet")
            }
            ast::Pat::LiteralPat(_) => {
                self.unsupported_pat(range, "literal patterns are not supported yet")
            }
            ast::Pat::TuplePat(_) => {
                self.unsupported_pat(range, "tuple patterns are not supported yet")
            }
            ast::Pat::AnnotPat(_) => {
                self.unsupported_pat(range, "type annotations in patterns are not supported yet")
            }
        };
        self.pats.alloc(Pat { kind, range })
    }

    fn lower_type(&mut self, ty: Option<ast::Type>, fallback: TextRange) -> TypeRefId {
        TypeLowering {
            file: self.file,
            types: &mut *self.types,
            diagnostics: &mut *self.diagnostics,
        }
        .lower(ty, fallback)
    }

    pub(super) fn unsupported(&mut self, range: TextRange, message: &str) -> ExprId {
        self.diagnostics.push(not_yet_supported(self.file, range, message));
        self.alloc(ExprKind::Missing, range)
    }

    fn unsupported_pat(&mut self, range: TextRange, message: &str) -> PatKind {
        self.diagnostics.push(not_yet_supported(self.file, range, message));
        PatKind::Missing
    }

    pub(super) fn alloc(&mut self, kind: ExprKind, range: TextRange) -> ExprId {
        self.exprs.alloc(Expr { kind, range })
    }
}

/// キーワードで始まる構文は、診断でキーワードだけを指す。本体全体を指すと読みにくいため。
fn keyword(node: &SyntaxNode) -> TextRange {
    node.first_token()
        .map_or(node.text_range(), |token| token.text_range())
}
```

- [ ] **Step 10: 宣言の対応づけを書く**

`crates/eml_hir/src/lower/mod.rs` を作る。

```rust
mod expr;
mod types;

use std::collections::HashMap;

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_syntax::{SyntaxKind, SyntaxNode, SyntaxToken, ast};
use la_arena::Arena;
use rowan::ast::AstNode;

use crate::hir::*;
use crate::{codes, not_yet_supported};
use expr::BodyLowering;
use types::TypeLowering;

/// 同じ名前のシグネチャと等式。名前で対応づけてから、並び方を検査する (docs/spec/declarations.md)。
struct Definition {
    name: String,
    first_range: TextRange,
    /// (item の番号, シグネチャ, 名前の位置)
    signature: Option<(usize, ast::Signature, TextRange)>,
    /// (item の番号, 等式, 名前の位置)
    equations: Vec<(usize, ast::Equation, TextRange)>,
}

pub fn lower(file: FileId, source: &ast::SourceFile) -> (Module, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let definitions = collect(file, source, &mut diagnostics);
    let mut functions = Arena::new();
    let mut names = HashMap::new();
    let mut pending = Vec::new();
    for definition in definitions {
        let Definition {
            name,
            first_range,
            signature,
            equations,
        } = definition;
        let mut equations = equations.into_iter();
        let first_equation = equations.next();
        for (_, _, range) in equations {
            // 複数の等式は段階6で `match` に脱糖する
            diagnostics.push(not_yet_supported(
                file,
                range,
                "defining a function with several equations is not supported yet",
            ));
        }
        match (&signature, &first_equation) {
            (Some((_, _, range)), None) => diagnostics.push(Diagnostic::error(
                codes::MISSING_EQUATION,
                format!("`{name}` has a signature but no equation"),
                Label::new(file, *range, format!("add an equation for `{name}` after this signature")),
            )),
            (None, Some((_, _, range))) => diagnostics.push(
                Diagnostic::error(
                    codes::MISSING_SIGNATURE,
                    format!("`{name}` has no type signature"),
                    Label::new(file, *range, "every top-level definition needs a signature"),
                )
                .with_help(format!(
                    "add a signature `{name} : ...` on the line before this equation"
                )),
            ),
            (Some((signature_index, _, _)), Some((equation_index, _, range)))
                if *equation_index != signature_index + 1 =>
            {
                // 離れたシグネチャと等式は段階6で E1xxx の検査にする
                diagnostics.push(not_yet_supported(
                    file,
                    *range,
                    "an equation that does not directly follow its signature is not supported yet",
                ));
            }
            _ => {}
        }
        let mut types = Arena::new();
        let signature = signature.map(|(_, node, _)| {
            let range = node
                .ty()
                .map_or(node.syntax().text_range(), |ty| ty.syntax().text_range());
            let ty = TypeLowering {
                file,
                types: &mut types,
                diagnostics: &mut diagnostics,
            }
            .lower(node.ty(), range);
            Signature { ty, range }
        });
        let name_range = first_equation
            .as_ref()
            .map_or(first_range, |(_, _, range)| *range);
        let id = functions.alloc(Function {
            name: name.clone(),
            name_range,
            signature,
            body: None,
            types,
        });
        names.insert(name, id);
        if let Some((_, equation, _)) = first_equation {
            pending.push((id, equation));
        }
    }
    // 本体は、すべての関数の名前がそろってから変換する。後ろで定義した関数も呼べるようにするため
    for (id, equation) in pending {
        let body = BodyLowering::new(file, &names, &mut functions[id].types, &mut diagnostics)
            .lower_equation(&equation);
        functions[id].body = Some(body);
    }
    diagnostics.sort_by_key(|d| d.primary.range.start());
    (Module { file, functions }, diagnostics)
}

fn collect(
    file: FileId,
    source: &ast::SourceFile,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Definition> {
    let mut definitions: Vec<Definition> = Vec::new();
    let mut by_name: HashMap<String, usize> = HashMap::new();
    for (index, item) in source.items().enumerate() {
        match item {
            ast::Item::Signature(signature) => {
                let Some(name) = value_name(file, signature.name(), diagnostics) else {
                    continue;
                };
                let range = name.text_range();
                let slot = slot(&mut definitions, &mut by_name, name.text(), range);
                match &definitions[slot].signature {
                    Some((_, _, first)) => diagnostics.push(
                        Diagnostic::error(
                            codes::DUPLICATE_DEFINITION,
                            format!("`{}` is defined more than once", name.text()),
                            Label::new(file, range, "defined again here"),
                        )
                        .with_secondary(Label::new(file, *first, "first defined here")),
                    ),
                    None => definitions[slot].signature = Some((index, signature, range)),
                }
            }
            ast::Item::Equation(equation) => {
                let Some(name) = value_name(file, equation.name(), diagnostics) else {
                    continue;
                };
                let range = name.text_range();
                let slot = slot(&mut definitions, &mut by_name, name.text(), range);
                definitions[slot].equations.push((index, equation, range));
            }
            ast::Item::DataItem(item) => diagnostics.push(not_yet_supported(
                file,
                keyword(item.syntax()),
                "`data` declarations are not supported yet",
            )),
            ast::Item::EffectItem(item) => diagnostics.push(not_yet_supported(
                file,
                keyword(item.syntax()),
                "`effect` declarations are not supported yet",
            )),
            ast::Item::FixityItem(item) => diagnostics.push(not_yet_supported(
                file,
                keyword(item.syntax()),
                "fixity declarations are not supported yet",
            )),
            // `type` は構文の段階 S2 の構文で、パーサが E0004 を報告済み
            ast::Item::TypeItem(_) => {}
        }
    }
    definitions
}

fn slot(
    definitions: &mut Vec<Definition>,
    by_name: &mut HashMap<String, usize>,
    name: &str,
    range: TextRange,
) -> usize {
    *by_name.entry(name.to_string()).or_insert_with(|| {
        definitions.push(Definition {
            name: name.to_string(),
            first_range: range,
            signature: None,
            equations: Vec::new(),
        });
        definitions.len() - 1
    })
}

/// 演算子の定義は、fixity の宣言と一緒に段階6で扱う。名前がなければパーサが報告済み。
fn value_name(
    file: FileId,
    token: Option<SyntaxToken>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<SyntaxToken> {
    let token = token?;
    if token.kind() == SyntaxKind::LIDENT {
        return Some(token);
    }
    diagnostics.push(not_yet_supported(
        file,
        token.text_range(),
        "defining operators is not supported yet",
    ));
    None
}

fn keyword(node: &SyntaxNode) -> TextRange {
    node.first_token()
        .map_or(node.text_range(), |token| token.text_range())
}
```

- [ ] **Step 11: 表示を書く**

`crates/eml_hir/src/pretty.rs` を作る。

```rust
//! テストで変換結果を確かめるための表示。

use std::fmt::Write;

use crate::hir::*;

pub fn pretty(module: &Module) -> String {
    let mut out = String::new();
    for (_, function) in module.functions.iter() {
        let printer = Printer { module, function };
        printer.function(&mut out);
    }
    out
}

struct Printer<'a> {
    module: &'a Module,
    function: &'a Function,
}

impl Printer<'_> {
    fn function(&self, out: &mut String) {
        let function = self.function;
        match &function.signature {
            Some(signature) => writeln!(out, "{} : {}", function.name, self.ty(signature.ty)),
            None => writeln!(out, "{} : <no signature>", function.name),
        }
        .unwrap();
        let Some(body) = &function.body else {
            writeln!(out, "{} = <no equation>", function.name).unwrap();
            return;
        };
        out.push_str(&function.name);
        for &param in &body.params {
            write!(out, " {}", self.pat(body, param)).unwrap();
        }
        writeln!(out, " = {}", self.expr(body, body.root, 0)).unwrap();
    }

    fn expr(&self, body: &Body, id: ExprId, indent: usize) -> String {
        match &body.exprs[id].kind {
            ExprKind::Missing => "<missing>".to_string(),
            ExprKind::Literal(Literal::Int(n)) => n.to_string(),
            ExprKind::Literal(Literal::String(s)) => format!("{s:?}"),
            ExprKind::Literal(Literal::Unit) => "()".to_string(),
            ExprKind::Path(res) => self.res(body, *res),
            ExprKind::Call { callee, args } => {
                let mut s = format!("({}", self.expr(body, *callee, indent));
                for &arg in args {
                    write!(s, " {}", self.expr(body, arg, indent)).unwrap();
                }
                s + ")"
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let condition = self.expr(body, *condition, indent);
                let then_branch = self.expr(body, *then_branch, indent);
                match else_branch {
                    Some(e) => format!("(if {condition} {then_branch} {})", self.expr(body, *e, indent)),
                    None => format!("(if {condition} {then_branch})"),
                }
            }
            ExprKind::Block { stmts, tail } => {
                let pad = "  ".repeat(indent + 1);
                let mut s = "{\n".to_string();
                for stmt in stmts {
                    let line = match stmt {
                        Stmt::Let { pat, ty, init } => {
                            let init = self.expr(body, *init, indent + 1);
                            match ty {
                                Some(ty) => format!("let {} : {} = {init}", self.pat(body, *pat), self.ty(*ty)),
                                None => format!("let {} = {init}", self.pat(body, *pat)),
                            }
                        }
                        Stmt::Expr(e) => self.expr(body, *e, indent + 1),
                    };
                    writeln!(s, "{pad}{line}").unwrap();
                }
                if let Some(tail) = tail {
                    writeln!(s, "{pad}{}", self.expr(body, *tail, indent + 1)).unwrap();
                }
                s + &"  ".repeat(indent) + "}"
            }
            ExprKind::Annot { expr, ty } => {
                format!("({} : {})", self.expr(body, *expr, indent), self.ty(*ty))
            }
        }
    }

    fn res(&self, body: &Body, res: Res) -> String {
        match res {
            Res::Local(local) => local_name(body, local),
            Res::Function(function) => format!("@{}", self.module.functions[function].name),
            Res::Builtin(builtin) => builtin.name().to_string(),
        }
    }

    fn pat(&self, body: &Body, id: PatId) -> String {
        match &body.pats[id].kind {
            PatKind::Missing => "<missing>".to_string(),
            PatKind::Bind(local) => local_name(body, *local),
            PatKind::Wildcard => "_".to_string(),
            PatKind::Unit => "()".to_string(),
        }
    }

    fn ty(&self, id: TypeRefId) -> String {
        let types = &self.function.types;
        match &types[id].kind {
            TypeRefKind::Error => "<error>".to_string(),
            TypeRefKind::Builtin(builtin) => builtin.name().to_string(),
            TypeRefKind::Fn { param, row, ret } => {
                let param_text = self.ty(*param);
                let param_text = if matches!(types[*param].kind, TypeRefKind::Fn { .. }) {
                    format!("({param_text})")
                } else {
                    param_text
                };
                let row = match row {
                    RowRef::Omitted => String::new(),
                    RowRef::Closed { effects, .. } => {
                        let names: Vec<&str> = effects.iter().map(|EffectRef::Io| "IO").collect();
                        format!("<{}> ", names.join(", "))
                    }
                    RowRef::Error => "<error> ".to_string(),
                };
                format!("{param_text} -> {row}{}", self.ty(*ret))
            }
        }
    }
}

fn local_name(body: &Body, local: LocalId) -> String {
    format!("{}#{}", body.locals[local].name, u32::from(local.into_raw()))
}
```

- [ ] **Step 12: テストが通ることを確かめる**

Run: `cargo test -p eml_hir`
Expected: PASS

- [ ] **Step 13: UI テストのスナップショットを確かめる**

Run: `cargo test -p eml_cli --test ui`

`check-fail/` のスナップショットが、HIR の新しい診断 (例: `missing_indented_block.em` と `tab_indentation.em` に E1004) の分だけ変わる。`cargo insta review` で差分を見て、既存の診断がそのまま残り、E1xxx が加わっただけであることを確かめてから承認する。`run/` のスナップショットは変わらない。

- [ ] **Step 14: 全体の確認とコミット**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`

```bash
git add Cargo.toml Cargo.lock crates/eml_hir crates/eml_cli/tests/snapshots
git commit -m "Lower declarations and expressions to HIR and resolve names"
```

---

### Task 5: 演算子の列の組み直しと脱糖

`OP_SEQ` を標準の演算子の表で木に組み直す (precedence climbing)。単項の `-` は優先順位 6 の `negate`、`&&` / `||` は `if`、`|>` / `<|` は関数適用に脱糖する ([式](../../spec/expressions.md)、[宣言](../../spec/declarations.md) の標準の演算子の表)。

**Files:**
- Create: `crates/eml_hir/src/lower/ops.rs`
- Modify: `crates/eml_hir/src/lower/mod.rs` (`mod ops;`)、`crates/eml_hir/src/lower/expr.rs` (`OpSeq` の腕)
- Create: `crates/eml_hir/tests/operators.rs`

**Interfaces:**
- Consumes: Task 4 の `BodyLowering::{lower_expr, call, alloc, unsupported}`、`builtin::{fixity, Assoc, Builtin::binary_operator}`
- Produces: 演算子の列を組み直した HIR。`a + b` は `Call { callee: Path(Builtin(IntAdd)), args: [a, b] }`、`-x` は `Call { callee: Path(Builtin(IntNeg)), args: [x] }`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_hir/tests/operators.rs` を作る。

```rust
mod common;

use common::lower_text;

#[test]
fn precedence_and_left_associativity() {
    insta::assert_snapshot!(lower_text("f : Int -> Int\nf x = 1 + 2 * x - 3"), @r"
    f : Int -> Int
    f x#0 = (- (+ 1 (* 2 x#0)) 3)
    ");
}

#[test]
fn right_associativity() {
    insta::assert_snapshot!(lower_text("s : String\ns = \"a\" ++ \"b\" ++ \"c\""), @r#"
    s : String
    s = (++ "a" (++ "b" "c"))
    "#);
}

#[test]
fn prefix_minus_has_precedence_six() {
    let text = "n : Int\nn = - 2 * 3\nm : Int\nm = - 2 + 3\nk : Bool\nk = 1 == -2";
    insta::assert_snapshot!(lower_text(text), @r"
    n : Int
    n = (negate (* 2 3))
    m : Int
    m = (+ (negate 2) 3)
    k : Bool
    k = (== 1 (negate 2))
    ");
}

#[test]
fn and_and_or_become_if() {
    insta::assert_snapshot!(lower_text("b : Bool\nb = True && False || True"), @r"
    b : Bool
    b = (if (if True False False) True True)
    ");
}

#[test]
fn pipes_become_applications() {
    let text = "f : Int -> Int\nf x = x\ng : Int -> Int -> Int\ng a b = a\np : Int\np = 1 |> f |> g 2\nq : Int\nq = g 1 <| f 2";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> Int
    f x#0 = x#0
    g : Int -> Int -> Int
    g a#0 b#1 = a#0
    p : Int
    p = (@g 2 (@f 1))
    q : Int
    q = (@g 1 (@f 2))
    ");
}

#[test]
fn non_associative_operators_cannot_be_chained() {
    insta::assert_snapshot!(lower_text("b : Bool\nb = 1 < 2 < 3"), @r"
    b : Bool
    b = <missing>
    ---
    E1006 2:11 `<` and `<` cannot be combined without parentheses
    ");
}

#[test]
fn prefix_minus_after_a_tighter_operator_needs_parentheses() {
    insta::assert_snapshot!(lower_text("n : Int\nn = 2 * -3"), @r"
    n : Int
    n = (* 2 (negate 3))
    ---
    E1006 2:9 a prefix `-` cannot appear here without parentheses
    ");
}

#[test]
fn unknown_and_unsupported_operators_and_missing_operands() {
    let text = "x : Int\nx = 1 <+> 2\ny : Int\ny = 1 :: 2\nz : Int\nz = 1 +";
    insta::assert_snapshot!(lower_text(text), @r"
    x : Int
    x = (<missing> 1 2)
    y : Int
    y = (<missing> 1 2)
    z : Int
    z = (+ 1 <missing>)
    ---
    E1001 2:7 cannot find operator `<+>`
    E0004 4:7 lists are not supported yet
    E0011 6:8 expected an expression
    ");
}
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_hir --test operators`
Expected: FAIL (演算子の列が `<missing>` になる)

- [ ] **Step 3: 組み直しを実装する**

`crates/eml_hir/src/lower/ops.rs` を作る。

```rust
//! 演算子の列を fixity の表で木に組み直す (docs/spec/expressions.md の「演算子の列と単項マイナス」)。

use eml_diagnostics::{Diagnostic, Label, TextRange, TextSize};
use eml_syntax::ast::{self, OpSeqElement};
use rowan::ast::AstNode;

use super::expr::BodyLowering;
use crate::builtin::{Assoc, Builtin, fixity};
use crate::codes;
use crate::hir::{ExprId, ExprKind, Res};

#[derive(Clone)]
enum Piece {
    Operand(ExprId),
    Operator { text: String, range: TextRange },
}

struct Cursor {
    pieces: Vec<Piece>,
    pos: usize,
    /// 列の終わり。欠けた被演算子の位置に使う。
    end: TextSize,
}

impl Cursor {
    fn peek(&self) -> Option<Piece> {
        self.pieces.get(self.pos).cloned()
    }
}

/// 単項の `-` は Haskell と同じく、優先順位 6 の `negate` として組み直す。
const NEGATE_PRECEDENCE: u8 = 6;

impl BodyLowering<'_> {
    pub(super) fn lower_op_seq(&mut self, seq: &ast::OpSeq) -> ExprId {
        let range = seq.syntax().text_range();
        let mut pieces = Vec::new();
        for element in seq.elements() {
            match element {
                OpSeqElement::Operand(expr) => {
                    let expr_range = expr.syntax().text_range();
                    pieces.push(Piece::Operand(self.lower_expr(Some(expr), expr_range)));
                }
                OpSeqElement::Operator(token) => pieces.push(Piece::Operator {
                    text: token.text().to_string(),
                    range: token.text_range(),
                }),
            }
        }
        let mut cursor = Cursor {
            pieces,
            pos: 0,
            end: range.end(),
        };
        self.climb(&mut cursor, 0)
    }

    fn climb(&mut self, cursor: &mut Cursor, min_precedence: u8) -> ExprId {
        let mut lhs = self.operand(cursor, min_precedence);
        // 直前に組んだ演算子。結合しない並びを見つけるため
        let mut previous: Option<(String, u8, Assoc)> = None;
        while let Some(Piece::Operator { text, range }) = cursor.peek() {
            // fixity の宣言がない演算子は `infixl 9` とする (docs/spec/declarations.md)
            let (precedence, assoc) = fixity(&text).unwrap_or((9, Assoc::Left));
            if precedence < min_precedence {
                break;
            }
            cursor.pos += 1;
            let conflict = previous.as_ref().is_some_and(|(_, p, a)| {
                *p == precedence && (*a != assoc || assoc == Assoc::None)
            });
            let next_min = if assoc == Assoc::Right {
                precedence
            } else {
                precedence + 1
            };
            let rhs = self.climb(cursor, next_min);
            if conflict {
                let (previous_text, _, _) = previous.as_ref().unwrap();
                self.diagnostics.push(Diagnostic::error(
                    codes::NON_ASSOCIATIVE_OPERATORS,
                    format!("`{previous_text}` and `{text}` cannot be combined without parentheses"),
                    Label::new(self.file, range, "use parentheses to group the operators"),
                ));
                // 組み方が決まらないので、型の誤りを連鎖させないように式全体を Missing にする
                let whole = self.exprs[lhs].range.cover(self.exprs[rhs].range);
                lhs = self.alloc(ExprKind::Missing, whole);
            } else {
                lhs = self.binary(&text, range, lhs, rhs);
            }
            previous = Some((text, precedence, assoc));
        }
        lhs
    }

    fn operand(&mut self, cursor: &mut Cursor, min_precedence: u8) -> ExprId {
        match cursor.peek() {
            Some(Piece::Operator { text, range }) if text == "-" => {
                cursor.pos += 1;
                if min_precedence > NEGATE_PRECEDENCE {
                    self.diagnostics.push(Diagnostic::error(
                        codes::NON_ASSOCIATIVE_OPERATORS,
                        "a prefix `-` cannot appear here without parentheses",
                        Label::new(self.file, range, "put the negation in parentheses"),
                    ));
                }
                let operand = self.climb(cursor, NEGATE_PRECEDENCE + 1);
                let callee = self.alloc(ExprKind::Path(Res::Builtin(Builtin::IntNeg)), range);
                let whole = range.cover(self.exprs[operand].range);
                self.alloc(
                    ExprKind::Call {
                        callee,
                        args: vec![operand],
                    },
                    whole,
                )
            }
            Some(Piece::Operand(expr)) => {
                cursor.pos += 1;
                expr
            }
            // 欠けた被演算子はパーサが報告済み
            Some(Piece::Operator { range, .. }) => self.alloc(ExprKind::Missing, range),
            None => self.alloc(ExprKind::Missing, TextRange::empty(cursor.end)),
        }
    }

    fn binary(&mut self, op: &str, op_range: TextRange, lhs: ExprId, rhs: ExprId) -> ExprId {
        let range = self.exprs[lhs].range.cover(self.exprs[rhs].range);
        match op {
            // 短絡評価にするため `if` に脱糖する (docs/spec/declarations.md)
            "&&" => {
                let otherwise = self.alloc(ExprKind::Path(Res::Builtin(Builtin::False)), op_range);
                self.alloc(
                    ExprKind::If {
                        condition: lhs,
                        then_branch: rhs,
                        else_branch: Some(otherwise),
                    },
                    range,
                )
            }
            "||" => {
                let then = self.alloc(ExprKind::Path(Res::Builtin(Builtin::True)), op_range);
                self.alloc(
                    ExprKind::If {
                        condition: lhs,
                        then_branch: then,
                        else_branch: Some(rhs),
                    },
                    range,
                )
            }
            "|>" => self.call(rhs, vec![lhs], range),
            "<|" => self.call(lhs, vec![rhs], range),
            _ => {
                let callee = match Builtin::binary_operator(op) {
                    Some(builtin) => self.alloc(ExprKind::Path(Res::Builtin(builtin)), op_range),
                    None if op == ">>" || op == "<<" => {
                        self.unsupported(op_range, "function composition is not supported yet")
                    }
                    None if op == "::" => self.unsupported(op_range, "lists are not supported yet"),
                    None => {
                        self.diagnostics.push(Diagnostic::error(
                            codes::UNDEFINED_NAME,
                            format!("cannot find operator `{op}`"),
                            Label::new(self.file, op_range, "not found in this scope"),
                        ));
                        self.alloc(ExprKind::Missing, op_range)
                    }
                };
                self.alloc(
                    ExprKind::Call {
                        callee,
                        args: vec![lhs, rhs],
                    },
                    range,
                )
            }
        }
    }
}
```

`crates/eml_hir/src/lower/mod.rs` の先頭の `mod` に `mod ops;` を足す。`crates/eml_hir/src/lower/expr.rs` の `OpSeq` の腕 (Task 4 の一時的な扱い) を次に置き換える。

```rust
            ast::Expr::OpSeq(seq) => self.lower_op_seq(&seq),
```

- [ ] **Step 4: テストが通ることを確かめる**

Run: `cargo test -p eml_hir`
Expected: PASS

- [ ] **Step 5: 全体の確認とコミット**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`

`check-fail/` のスナップショットに差分が出たら、Task 4 の Step 13 と同じ基準で確かめて承認する。

```bash
git add crates/eml_hir crates/eml_cli/tests/snapshots
git commit -m "Reassociate operator sequences and desugar logical and pipe operators"
```

---
### Task 6: 型、row、Kind の表現と単一化

型検査器の土台を作る。型は表に置いて `Ty` (ID) で引き、型変数の束縛を辿って単一化する。row は scoped labels の書き換えで単一化し、Kind の制約 (`≤`) は束の上で最小解を求める ([型と Kind](../../spec/types.md))。段階1では row 変数も Kind 変数もほとんど現れないが、表現と仕組みは最初から本物にする。

**Files:**
- Modify: `crates/eml_types/Cargo.toml`
- Modify: `crates/eml_types/src/lib.rs` (モジュールの宣言と再公開だけ。`check` は仮実装のまま)
- Create: `crates/eml_types/src/ty.rs` (公開する型 `Type` など)
- Create: `crates/eml_types/src/kind.rs` (束の上の制約)
- Create: `crates/eml_types/src/table.rs` (型の表と単一化)

**Interfaces:**
- Produces:
  - 公開: `eml_types::{Type, Effect, Linearity, Multiplicity}`。`Type::{Int, String, Bool, Record(Vec<(String, Type)>), Fn { param: Box<Type>, linearity: Linearity, effects: Vec<Effect>, ret: Box<Type> }, Error}`、`Type::unit()`、`Type::contains_error()`、`impl Display for Type` (`Int -> <IO> Unit` の形。空の row は書かない。`Unit` は空のレコード)。`Effect::Io` と `Effect::name()`
  - crate 内: `kind::{Lattice<T>, Bound<T>, KindVar}`、`table::{Table, Ty, TyKind, TyCon, Row, Mult, UnifyError}`。`Table::new()`、`Table::{int, string, bool, unit, error}` (フィールド)、`Table::function(param, row, ret) -> Ty`、`Table::fresh_var() -> Ty`、`Table::fresh_row_var() -> RowVar`、`Table::kind(ty) -> &TyKind` (束縛を辿った先)、`Table::unify(a, b) -> Result<(), UnifyError>`、`Table::unify_row(&a, &b) -> Result<(), UnifyError>`、`Table::resolve_row(&row) -> Row`、`Table::export(ty) -> Type`、`Row::pure()`、`Row::closed(Vec<Effect>)`、`UnifyError::{Mismatch, Occurs, MissingEffects(Vec<Effect>)}`

- [ ] **Step 1: 依存を足す**

`crates/eml_types/Cargo.toml` を次にする。

```toml
[package]
name = "eml_types"
version.workspace = true
edition.workspace = true

[dependencies]
eml_diagnostics.workspace = true
eml_hir.workspace = true
la-arena.workspace = true

[dev-dependencies]
eml_syntax.workspace = true
insta.workspace = true
```

- [ ] **Step 2: 公開する型を書く**

`crates/eml_types/src/ty.rs` を作る。

```rust
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Linearity {
    Unr,
    Lin,
}

/// row に含まれてよい操作の上限。`Never` はマルチコア対応のために最初から持つ (docs/spec/types.md)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Multiplicity {
    Never,
    Once,
    Multi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Effect {
    Io,
}

impl Effect {
    pub fn name(self) -> &'static str {
        match self {
            Effect::Io => "IO",
        }
    }

    /// `IO` は実行時が必ず1回再開するので、`once` と同じ扱いになる (docs/spec/effects.md)。
    pub(crate) fn multiplicity(self) -> Multiplicity {
        match self {
            Effect::Io => Multiplicity::Once,
        }
    }
}

/// 型検査の結果として後の段階に渡す型。推論用の変数は解決済みで、残った変数は `Error` になる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Int,
    String,
    Bool,
    /// 閉じたレコード。`Unit` は空のレコード、タプルは数字ラベルのレコードである (docs/spec/records.md)。
    Record(Vec<(String, Type)>),
    Fn {
        param: Box<Type>,
        linearity: Linearity,
        effects: Vec<Effect>,
        ret: Box<Type>,
    },
    Error,
}

impl Type {
    pub fn unit() -> Type {
        Type::Record(Vec::new())
    }

    pub fn contains_error(&self) -> bool {
        match self {
            Type::Error => true,
            Type::Record(fields) => fields.iter().any(|(_, ty)| ty.contains_error()),
            Type::Fn { param, ret, .. } => param.contains_error() || ret.contains_error(),
            Type::Int | Type::String | Type::Bool => false,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Int => f.write_str("Int"),
            Type::String => f.write_str("String"),
            Type::Bool => f.write_str("Bool"),
            Type::Record(fields) if fields.is_empty() => f.write_str("Unit"),
            Type::Record(fields) => {
                f.write_str("{ ")?;
                for (index, (label, ty)) in fields.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{label} : {ty}")?;
                }
                f.write_str(" }")
            }
            Type::Fn {
                param,
                effects,
                ret,
                ..
            } => {
                if matches!(**param, Type::Fn { .. }) {
                    write!(f, "({param}) -> ")?;
                } else {
                    write!(f, "{param} -> ")?;
                }
                // 空の row は書かない。省略した row が `<>` だから (docs/spec/types.md)
                if !effects.is_empty() {
                    let names: Vec<&str> = effects.iter().map(|e| e.name()).collect();
                    write!(f, "<{}> ", names.join(", "))?;
                }
                write!(f, "{ret}")
            }
            Type::Error => f.write_str("{error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_types_are_displayed_like_the_surface_syntax() {
        let pure = Type::Fn {
            param: Box::new(Type::Int),
            linearity: Linearity::Unr,
            effects: vec![],
            ret: Box::new(Type::Bool),
        };
        let io = Type::Fn {
            param: Box::new(pure.clone()),
            linearity: Linearity::Unr,
            effects: vec![Effect::Io],
            ret: Box::new(Type::unit()),
        };
        assert_eq!(pure.to_string(), "Int -> Bool");
        assert_eq!(io.to_string(), "(Int -> Bool) -> <IO> Unit");
    }
}
```

- [ ] **Step 3: 束の上の制約を書く (テストを先に)**

`crates/eml_types/src/kind.rs` を作る。

```rust
//! Kind の変数と `下限 ≤ 上限` の制約を集め、束の上で最小解を求める (docs/spec/types.md の「推論」)。
//! 線形性 (`Unr ≤ Lin`) と多重度 (`Never ≤ Once ≤ Multi`) の両方に使う。

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct KindVar(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bound<T> {
    Const(T),
    Var(KindVar),
}

#[derive(Debug)]
pub(crate) struct Lattice<T> {
    bottom: T,
    vars: usize,
    constraints: Vec<(Bound<T>, Bound<T>)>,
}

impl<T: Copy + Ord> Lattice<T> {
    pub fn new(bottom: T) -> Self {
        Lattice {
            bottom,
            vars: 0,
            constraints: Vec::new(),
        }
    }

    pub fn fresh(&mut self) -> KindVar {
        self.vars += 1;
        KindVar(self.vars as u32 - 1)
    }

    pub fn require(&mut self, lower: Bound<T>, upper: Bound<T>) {
        self.constraints.push((lower, upper));
    }

    /// 最小解と、満たせなかった制約 (定数の上限を超えたもの) の番号を返す。束は有限で更新は単調なので止まる。
    pub fn solve(&self) -> (Vec<T>, Vec<usize>) {
        let mut values = vec![self.bottom; self.vars];
        let value = |values: &[T], bound: Bound<T>| match bound {
            Bound::Const(c) => c,
            Bound::Var(KindVar(v)) => values[v as usize],
        };
        loop {
            let mut changed = false;
            for &(lower, upper) in &self.constraints {
                if let Bound::Var(KindVar(v)) = upper {
                    let lower = value(&values, lower);
                    if lower > values[v as usize] {
                        values[v as usize] = lower;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let violated = self
            .constraints
            .iter()
            .enumerate()
            .filter(|(_, (lower, upper))| {
                matches!(upper, Bound::Const(c) if value(&values, *lower) > *c)
            })
            .map(|(index, _)| index)
            .collect();
        (values, violated)
    }

    pub fn value(&self, var: KindVar) -> T {
        self.solve().0[var.0 as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ty::{Linearity, Multiplicity};

    #[test]
    fn unconstrained_variables_take_the_bottom() {
        let mut lattice = Lattice::new(Linearity::Unr);
        let v = lattice.fresh();
        assert_eq!(lattice.value(v), Linearity::Unr);
    }

    #[test]
    fn lower_bounds_propagate_through_chains() {
        let mut lattice = Lattice::new(Linearity::Unr);
        let a = lattice.fresh();
        let b = lattice.fresh();
        lattice.require(Bound::Var(a), Bound::Var(b));
        lattice.require(Bound::Const(Linearity::Lin), Bound::Var(a));
        assert_eq!(lattice.solve(), (vec![Linearity::Lin, Linearity::Lin], vec![]));
    }

    #[test]
    fn an_upper_bound_below_the_solution_is_reported() {
        let mut lattice = Lattice::new(Multiplicity::Never);
        let a = lattice.fresh();
        lattice.require(Bound::Const(Multiplicity::Multi), Bound::Var(a));
        lattice.require(Bound::Var(a), Bound::Const(Multiplicity::Once));
        assert_eq!(lattice.solve().1, vec![1]);
    }
}
```

- [ ] **Step 4: 型の表と単一化のテストを書く**

`crates/eml_types/src/table.rs` を作り、まずテストのモジュールだけを書く (実装は Step 5)。

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_unify_only_with_themselves() {
        let mut table = Table::new();
        assert_eq!(table.unify(table.int, table.int), Ok(()));
        assert_eq!(table.unify(table.int, table.string), Err(UnifyError::Mismatch));
        assert_eq!(table.unify(table.error, table.string), Ok(()));
    }

    #[test]
    fn variables_are_bound_by_unification() {
        let mut table = Table::new();
        let v = table.fresh_var();
        let f = table.function(table.int, Row::pure(), v);
        let g = table.function(table.int, Row::pure(), table.bool);
        assert_eq!(table.unify(f, g), Ok(()));
        assert_eq!(table.export(v), Type::Bool);
    }

    #[test]
    fn the_occurs_check_rejects_infinite_types() {
        let mut table = Table::new();
        let v = table.fresh_var();
        let f = table.function(v, Row::pure(), table.int);
        assert_eq!(table.unify(v, f), Err(UnifyError::Occurs));
    }

    #[test]
    fn closed_rows_unify_regardless_of_order() {
        let mut table = Table::new();
        let a = Row::closed(vec![Effect::Io]);
        assert_eq!(table.unify_row(&a, &a.clone()), Ok(()));
        assert_eq!(
            table.unify_row(&a, &Row::pure()),
            Err(UnifyError::MissingEffects(vec![Effect::Io]))
        );
    }

    #[test]
    fn an_open_row_absorbs_the_missing_labels() {
        let mut table = Table::new();
        let r = table.fresh_row_var();
        let open = Row { labels: vec![], tail: Some(r) };
        assert_eq!(table.unify_row(&open, &Row::closed(vec![Effect::Io])), Ok(()));
        assert_eq!(table.resolve_row(&open), Row::closed(vec![Effect::Io]));
        assert_eq!(table.row_multiplicity(r), Multiplicity::Once);
    }

    #[test]
    fn an_open_row_cannot_add_labels_to_a_closed_row() {
        let mut table = Table::new();
        let r = table.fresh_row_var();
        let open = Row { labels: vec![Effect::Io], tail: Some(r) };
        assert_eq!(
            table.unify_row(&open, &Row::pure()),
            Err(UnifyError::MissingEffects(vec![Effect::Io]))
        );
    }

    #[test]
    fn two_open_rows_share_a_fresh_tail() {
        let mut table = Table::new();
        let r1 = table.fresh_row_var();
        let r2 = table.fresh_row_var();
        let a = Row { labels: vec![Effect::Io], tail: Some(r1) };
        let b = Row { labels: vec![], tail: Some(r2) };
        assert_eq!(table.unify_row(&a, &b), Ok(()));
        let a = table.resolve_row(&a);
        let b = table.resolve_row(&b);
        assert_eq!(a.labels, vec![Effect::Io]);
        assert_eq!(b.labels, vec![Effect::Io]);
        assert_eq!(a.tail, b.tail);
    }
}
```

Run: `cargo test -p eml_types`
Expected: コンパイルエラー (`Table` などがない)

- [ ] **Step 5: 型の表と単一化を実装する**

`crates/eml_types/src/table.rs` のテストのモジュールの前に書く。

```rust
use crate::kind::{Bound, KindVar, Lattice};
use crate::ty::{Effect, Linearity, Multiplicity, Type};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ty(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TyVar(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RowVar(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TyCon {
    Int,
    String,
    Bool,
}

/// 関数型の線形性 `m`。内部では最初から Kind 変数を扱う (docs/spec/types.md)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mult {
    Known(Linearity),
    Var(KindVar),
}

/// エフェクトの row。`tail` が `None` なら閉じた row である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub labels: Vec<Effect>,
    pub tail: Option<RowVar>,
}

impl Row {
    pub fn pure() -> Row {
        Row::closed(Vec::new())
    }

    pub fn closed(labels: Vec<Effect>) -> Row {
        Row { labels, tail: None }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum TyKind {
    Con(TyCon),
    Record(Vec<(String, Ty)>),
    Fn {
        param: Ty,
        lin: Mult,
        row: Row,
        ret: Ty,
    },
    Var(TyVar),
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnifyError {
    Mismatch,
    Occurs,
    /// 閉じた row に含まれないエフェクト。
    MissingEffects(Vec<Effect>),
}

struct TyVarInfo {
    binding: Option<Ty>,
    /// 型変数の Kind `Type<μ>` の `μ`。段階2でシグネチャの型変数とともに制約が付く。
    #[allow(dead_code)]
    linearity: KindVar,
}

struct RowVarInfo {
    binding: Option<Row>,
    /// row 変数の Kind `Row<σ>` の `σ`。
    multiplicity: KindVar,
}

pub(crate) struct Table {
    kinds: Vec<TyKind>,
    ty_vars: Vec<TyVarInfo>,
    row_vars: Vec<RowVarInfo>,
    linearity: Lattice<Linearity>,
    multiplicity: Lattice<Multiplicity>,
    pub int: Ty,
    pub string: Ty,
    pub bool: Ty,
    pub unit: Ty,
    pub error: Ty,
}

impl Table {
    pub fn new() -> Table {
        let mut table = Table {
            kinds: Vec::new(),
            ty_vars: Vec::new(),
            row_vars: Vec::new(),
            linearity: Lattice::new(Linearity::Unr),
            multiplicity: Lattice::new(Multiplicity::Never),
            int: Ty(0),
            string: Ty(0),
            bool: Ty(0),
            unit: Ty(0),
            error: Ty(0),
        };
        table.int = table.alloc(TyKind::Con(TyCon::Int));
        table.string = table.alloc(TyKind::Con(TyCon::String));
        table.bool = table.alloc(TyKind::Con(TyCon::Bool));
        table.unit = table.alloc(TyKind::Record(Vec::new()));
        table.error = table.alloc(TyKind::Error);
        table
    }

    pub fn alloc(&mut self, kind: TyKind) -> Ty {
        self.kinds.push(kind);
        Ty(self.kinds.len() as u32 - 1)
    }

    /// トップレベルの関数と組み込みの関数型。どちらも `Unr` である。
    pub fn function(&mut self, param: Ty, row: Row, ret: Ty) -> Ty {
        self.alloc(TyKind::Fn {
            param,
            lin: Mult::Known(Linearity::Unr),
            row,
            ret,
        })
    }

    pub fn fresh_var(&mut self) -> Ty {
        let linearity = self.linearity.fresh();
        self.ty_vars.push(TyVarInfo {
            binding: None,
            linearity,
        });
        let var = TyVar(self.ty_vars.len() as u32 - 1);
        self.alloc(TyKind::Var(var))
    }

    pub fn fresh_row_var(&mut self) -> RowVar {
        let multiplicity = self.multiplicity.fresh();
        self.row_vars.push(RowVarInfo {
            binding: None,
            multiplicity,
        });
        RowVar(self.row_vars.len() as u32 - 1)
    }

    fn resolve(&self, mut ty: Ty) -> Ty {
        while let TyKind::Var(var) = &self.kinds[ty.0 as usize] {
            match self.ty_vars[var.0 as usize].binding {
                Some(bound) => ty = bound,
                None => break,
            }
        }
        ty
    }

    /// 束縛を辿った先の形。
    pub fn kind(&self, ty: Ty) -> &TyKind {
        &self.kinds[self.resolve(ty).0 as usize]
    }

    /// 束縛済みの row 変数を展開し、ラベルを1つの並びにまとめる。
    pub fn resolve_row(&self, row: &Row) -> Row {
        let mut labels = row.labels.clone();
        let mut tail = row.tail;
        while let Some(var) = tail {
            match &self.row_vars[var.0 as usize].binding {
                Some(bound) => {
                    labels.extend(bound.labels.iter().copied());
                    tail = bound.tail;
                }
                None => break,
            }
        }
        Row { labels, tail }
    }

    pub fn row_multiplicity(&self, var: RowVar) -> Multiplicity {
        self.multiplicity
            .value(self.row_vars[var.0 as usize].multiplicity)
    }

    pub fn unify(&mut self, a: Ty, b: Ty) -> Result<(), UnifyError> {
        let (a, b) = (self.resolve(a), self.resolve(b));
        if a == b {
            return Ok(());
        }
        match (self.kinds[a.0 as usize].clone(), self.kinds[b.0 as usize].clone()) {
            // `Error` が関わる制約からは診断を出さない (docs/spec/types.md の「エラーの扱い」)
            (TyKind::Error, _) | (_, TyKind::Error) => Ok(()),
            (TyKind::Var(var), _) => self.bind_var(var, b),
            (_, TyKind::Var(var)) => self.bind_var(var, a),
            (TyKind::Con(x), TyKind::Con(y)) if x == y => Ok(()),
            (TyKind::Record(xs), TyKind::Record(ys))
                if xs.len() == ys.len() && xs.iter().zip(&ys).all(|((l, _), (m, _))| l == m) =>
            {
                for ((_, x), (_, y)) in xs.iter().zip(&ys) {
                    self.unify(*x, *y)?;
                }
                Ok(())
            }
            (
                TyKind::Fn {
                    param: p1,
                    lin: l1,
                    row: r1,
                    ret: t1,
                },
                TyKind::Fn {
                    param: p2,
                    lin: l2,
                    row: r2,
                    ret: t2,
                },
            ) => {
                self.unify(p1, p2)?;
                self.unify_mult(l1, l2)?;
                self.unify_row(&r1, &r2)?;
                self.unify(t1, t2)
            }
            _ => Err(UnifyError::Mismatch),
        }
    }

    fn bind_var(&mut self, var: TyVar, ty: Ty) -> Result<(), UnifyError> {
        if self.occurs(var, ty) {
            return Err(UnifyError::Occurs);
        }
        self.ty_vars[var.0 as usize].binding = Some(ty);
        Ok(())
    }

    fn occurs(&self, var: TyVar, ty: Ty) -> bool {
        match self.kind(ty) {
            TyKind::Var(other) => *other == var,
            TyKind::Record(fields) => fields.iter().any(|(_, field)| self.occurs(var, *field)),
            TyKind::Fn { param, ret, .. } => self.occurs(var, *param) || self.occurs(var, *ret),
            TyKind::Con(_) | TyKind::Error => false,
        }
    }

    fn unify_mult(&mut self, a: Mult, b: Mult) -> Result<(), UnifyError> {
        match (a, b) {
            (Mult::Known(x), Mult::Known(y)) if x == y => Ok(()),
            (Mult::Known(_), Mult::Known(_)) => Err(UnifyError::Mismatch),
            (Mult::Var(v), other) | (other, Mult::Var(v)) => {
                let other = match other {
                    Mult::Known(l) => Bound::Const(l),
                    Mult::Var(w) => Bound::Var(w),
                };
                self.linearity.require(Bound::Var(v), other);
                self.linearity.require(other, Bound::Var(v));
                Ok(())
            }
        }
    }

    /// Leijen の scoped labels の書き換えで単一化する (docs/spec/types.md)。
    pub fn unify_row(&mut self, a: &Row, b: &Row) -> Result<(), UnifyError> {
        let a = self.resolve_row(a);
        let b = self.resolve_row(b);
        let mut only_b = b.labels.clone();
        let mut only_a = Vec::new();
        for label in &a.labels {
            match only_b.iter().position(|other| other == label) {
                Some(index) => {
                    only_b.remove(index);
                }
                None => only_a.push(*label),
            }
        }
        match (a.tail, b.tail) {
            (None, None) => {
                let mut missing = only_a;
                missing.extend(only_b);
                if missing.is_empty() {
                    Ok(())
                } else {
                    Err(UnifyError::MissingEffects(missing))
                }
            }
            (Some(tail), None) => {
                if !only_a.is_empty() {
                    return Err(UnifyError::MissingEffects(only_a));
                }
                self.bind_row(tail, Row::closed(only_b))
            }
            (None, Some(tail)) => {
                if !only_b.is_empty() {
                    return Err(UnifyError::MissingEffects(only_b));
                }
                self.bind_row(tail, Row::closed(only_a))
            }
            (Some(x), Some(y)) if x == y => {
                if only_a.is_empty() && only_b.is_empty() {
                    Ok(())
                } else {
                    Err(UnifyError::Occurs)
                }
            }
            (Some(x), Some(y)) => {
                let rest = self.fresh_row_var();
                self.bind_row(x, Row { labels: only_b, tail: Some(rest) })?;
                self.bind_row(y, Row { labels: only_a, tail: Some(rest) })
            }
        }
    }

    /// row 変数に入る操作は、その Kind `Row<σ>` の上限 `σ` を超えてはならない (docs/spec/types.md)。
    fn bind_row(&mut self, var: RowVar, row: Row) -> Result<(), UnifyError> {
        if row.tail == Some(var) {
            return Err(UnifyError::Occurs);
        }
        let sigma = self.row_vars[var.0 as usize].multiplicity;
        for label in &row.labels {
            self.multiplicity
                .require(Bound::Const(label.multiplicity()), Bound::Var(sigma));
        }
        if let Some(tail) = row.tail {
            let inner = self.row_vars[tail.0 as usize].multiplicity;
            self.multiplicity.require(Bound::Var(inner), Bound::Var(sigma));
        }
        self.row_vars[var.0 as usize].binding = Some(row);
        Ok(())
    }

    /// 後の段階に渡す形にする。解けていない変数は `Error` にする。
    pub fn export(&self, ty: Ty) -> Type {
        match self.kind(ty).clone() {
            TyKind::Con(TyCon::Int) => Type::Int,
            TyKind::Con(TyCon::String) => Type::String,
            TyKind::Con(TyCon::Bool) => Type::Bool,
            TyKind::Record(fields) => Type::Record(
                fields
                    .into_iter()
                    .map(|(label, field)| (label, self.export(field)))
                    .collect(),
            ),
            TyKind::Fn {
                param,
                lin,
                row,
                ret,
            } => Type::Fn {
                param: Box::new(self.export(param)),
                linearity: match lin {
                    Mult::Known(l) => l,
                    Mult::Var(v) => self.linearity.value(v),
                },
                effects: self.resolve_row(&row).labels,
                ret: Box::new(self.export(ret)),
            },
            TyKind::Var(_) | TyKind::Error => Type::Error,
        }
    }
}
```

`crates/eml_types/src/lib.rs` を次にする (`check` は Task 7 で本物にする)。

```rust
//! 型、row、Kind の検査 (docs/spec/types.md)。

mod kind;
mod table;
mod ty;

use eml_diagnostics::Diagnostic;
use eml_hir::Module;

pub use ty::{Effect, Linearity, Multiplicity, Type};

#[derive(Debug, Default)]
pub struct TypedModule {}

pub fn check(_module: &Module) -> (TypedModule, Vec<Diagnostic>) {
    (TypedModule::default(), Vec::new())
}
```

この時点では `table` と `kind` の多くがまだ使われないので、`cargo clippy` の dead_code 警告を避けるため、`mod kind;` と `mod table;` の前に `#[allow(dead_code)] // Task 7 の検査器が使う` を付ける。Task 7 で外す。

- [ ] **Step 6: テストが通ることを確かめる**

Run: `cargo test -p eml_types`
Expected: PASS

- [ ] **Step 7: 全体の確認とコミット**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`

```bash
git add Cargo.lock crates/eml_types
git commit -m "Represent types, rows, and kinds and unify them"
```

---

### Task 7: シグネチャに対する本体の検査と `main`

各関数の本体を、シグネチャの型に対して bidirectional に検査する。引数を1つ消費するごとにシグネチャの矢印を1つたどり、本体の row は最後にたどった矢印の row にする。呼び出しでは呼び出し先の閉じた row を新しい row 変数で開き、本体の row と単一化する (Koka と同じ)。関数値 (部分適用、関数を値として使うこと) は E0004 にする。`eml_cli::compile` は `main` がなければ E2003 を足す。

**Files:**
- Create: `crates/eml_types/src/builtins.rs`、`crates/eml_types/src/check.rs`
- Modify: `crates/eml_types/src/lib.rs`
- Create: `crates/eml_types/tests/common/mod.rs`、`crates/eml_types/tests/check.rs`
- Modify: `crates/eml_cli/src/lib.rs` (`compile` の E2003)
- Test: `crates/eml_cli/tests/api.rs`、`crates/eml_cli/tests/cli.rs`

**Interfaces:**
- Consumes: Task 4〜5 の HIR、Task 6 の `Table` など
- Produces:
  - `eml_types::check(module: &Module) -> (TypedModule, Vec<Diagnostic>)`
  - `TypedModule { signatures: ArenaMap<FunctionId, Type>, bodies: ArenaMap<FunctionId, BodyTypes>, main: Option<FunctionId> }`、`BodyTypes { exprs: ArenaMap<ExprId, Type>, locals: ArenaMap<LocalId, Type> }`。`signatures` はシグネチャのある関数だけ、`bodies` はシグネチャと等式の両方がある関数だけを含む。`bodies.exprs` には検査した式 (呼び出し先の `Path` を含む) の型が入る
  - `eml_types::missing_main(file: FileId) -> Diagnostic` (E2003)
  - `eml_types::dump(module: &Module, typed: &TypedModule) -> String` (テスト用)
  - `eml_types::codes::{TYPE_MISMATCH, EFFECT_NOT_IN_ROW, MISSING_MAIN, INVALID_MAIN_TYPE, INFINITE_TYPE}` (E2001〜E2005)

- [ ] **Step 1: テストの補助を書く**

`crates/eml_types/tests/common/mod.rs` を作る。型検査の診断は出した順のまま並べる (同じ式の中の診断の順を見るため)。

```rust
use std::fmt::Write;

use eml_diagnostics::{SourceFiles, TextRange};

/// 推論結果と、構文・HIR・型の診断。診断はラベル、note、help まで表示する。
pub fn check_text(text: &str) -> String {
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (parse, mut diagnostics) = eml_syntax::parse(file, text);
    let (module, stage) = eml_hir::lower(file, &parse.tree());
    diagnostics.extend(stage);
    let (typed, stage) = eml_types::check(&module);
    diagnostics.extend(stage);
    let mut out = eml_types::dump(&module, &typed);
    if !diagnostics.is_empty() {
        out.push_str("---\n");
        for d in &diagnostics {
            let at = position(text, d.primary.range);
            writeln!(out, "{} {at} {}", d.code, d.message).unwrap();
            writeln!(out, "  {at} {}", d.primary.message).unwrap();
            for label in &d.secondary {
                writeln!(out, "  {} {}", position(text, label.range), label.message).unwrap();
            }
            for note in &d.notes {
                writeln!(out, "  note: {note}").unwrap();
            }
            for help in &d.help {
                writeln!(out, "  help: {help}").unwrap();
            }
        }
    }
    out
}

fn position(text: &str, range: TextRange) -> String {
    let offset = u32::from(range.start()) as usize;
    let before = &text[..offset];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    format!("{line}:{column}")
}
```

- [ ] **Step 2: 失敗するテストを書く**

`crates/eml_types/tests/check.rs` を作る。

```rust
mod common;

use common::check_text;

#[test]
fn signatures_and_local_types() {
    let text = "add : Int -> Int -> Int\nadd a b =\n  let sum = a + b\n  sum\n\nmain : Unit -> <IO> Unit\nmain () =\n  let message = \"sum: \" ++ show_int (add 1 2)\n  println message";
    insta::assert_snapshot!(check_text(text), @r"
    add : Int -> Int -> Int
      a#0 : Int
      b#1 : Int
      sum#2 : Int
    main : Unit -> <IO> Unit
      message#0 : String
    ");
}

#[test]
fn argument_mismatch_points_at_the_callee() {
    let text = "f : Int -> Int\nf x = x\n\ng : Unit -> Int\ng () = f \"one\"";
    insta::assert_snapshot!(check_text(text), @r"
    f : Int -> Int
      x#0 : Int
    g : Unit -> Int
    ---
    E2001 5:10 mismatched types
      5:10 expected `Int`, found `String`
      5:8 argument 1 of `f`
    ");
}

#[test]
fn statements_annotations_conditions_and_signatures() {
    let text = "h : Bool -> Int\nh b =\n  1\n  let n : Int = \"x\"\n  if n then 1 else \"two\"";
    insta::assert_snapshot!(check_text(text), @r#"
    h : Bool -> Int
      b#0 : Bool
      n#1 : Int
    ---
    E2001 3:3 mismatched types
      3:3 expected `Unit`, found `Int`
      note: a statement that is not the last one in a block must have type `Unit`
    E2001 4:17 mismatched types
      4:17 expected `Int`, found `String`
      4:11 expected because of this annotation
    E2001 5:6 mismatched types
      5:6 expected `Bool`, found `Int`
      note: the condition of `if` must have type `Bool`
    E2001 5:20 mismatched types
      5:20 expected `Int`, found `String`
      1:5 expected because of the signature of `h`
    "#);
}

#[test]
fn if_without_else_must_be_unit() {
    insta::assert_snapshot!(check_text("k : Bool -> Int\nk b = if b then 1"), @r"
    k : Bool -> Int
      b#0 : Bool
    ---
    E2001 2:17 mismatched types
      2:17 expected `Unit`, found `Int`
      note: an `if` without `else` must have type `Unit`
    E2001 2:7 mismatched types
      2:7 expected `Int`, found `Unit`
      1:5 expected because of the signature of `k`
    ");
}

#[test]
fn effects_must_be_in_the_signature() {
    let text = "greet : String -> Unit\ngreet name = println name\n\nshout : String -> <IO> Unit\nshout name = println (name ++ \"!\")";
    insta::assert_snapshot!(check_text(text), @r"
    greet : String -> Unit
      name#0 : String
    shout : String -> <IO> Unit
      name#0 : String
    ---
    E2002 2:14 `println` performs `IO`, which the signature of `greet` does not allow
      2:14 this call performs `IO`
      1:9 the row of this signature does not include it
      help: add `IO` to the row of the signature of `greet`, as in `-> <IO> ...`
    ");
}

#[test]
fn main_type_and_parameter_count() {
    let text = "main : Int -> Int\nmain n = n\n\ntwo : Int -> Int\ntwo a b = a";
    insta::assert_snapshot!(check_text(text), @r"
    main : Int -> Int
      n#0 : Int
    two : Int -> Int
      a#0 : Int
      b#1 : {error}
    ---
    E2004 1:8 `main` must have type `Unit -> <IO> Unit`
      1:8 found `Int -> Int`
    E2001 5:7 `two` has 2 parameters but its signature has 1 arrow
      5:7 this parameter has no arrow in the signature
      4:7 the signature
    ");
}

#[test]
fn function_values_are_not_yet_supported() {
    let text = "add : Int -> Int -> Int\nadd a b = a + b\n\npartial : Unit -> Int\npartial () =\n  let f = add 1\n  let g = add\n  0\n\ncurried : Int -> Int -> Int\ncurried a = a";
    insta::assert_snapshot!(check_text(text), @r"
    add : Int -> Int -> Int
      a#0 : Int
      b#1 : Int
    partial : Unit -> Int
      f#0 : {error}
      g#1 : {error}
    curried : Int -> Int -> Int
      a#0 : Int
    ---
    E0004 6:11 partial application is not supported yet
      6:11 this is implemented in a later stage
    E0004 7:11 using a function as a value is not supported yet
      7:11 this is implemented in a later stage
    E0004 11:1 an equation with fewer parameters than arrows in its signature is not supported yet
      11:1 this is implemented in a later stage
    ");
}

#[test]
fn name_errors_do_not_cascade() {
    let text = "f : Int -> Int\nf x = g (x + undefined_thing) 1\nb : Bool\nb = 1 < 2 < 3";
    insta::assert_snapshot!(check_text(text), @r"
    f : Int -> Int
      x#0 : Int
    b : Bool
    ---
    E1001 2:7 cannot find value `g`
      2:7 not found in this scope
    E1001 2:14 cannot find value `undefined_thing`
      2:14 not found in this scope
    E1006 4:11 `<` and `<` cannot be combined without parentheses
      4:11 use parentheses to group the operators
    ");
}

#[test]
fn too_many_arguments_and_non_functions() {
    let text = "one : Int -> Int\none x = x\n\nbad : Unit -> Int\nbad () = one 1 2 + True 3";
    insta::assert_snapshot!(check_text(text), @r"
    one : Int -> Int
      x#0 : Int
    bad : Unit -> Int
    ---
    E2001 5:16 `one` takes 1 argument but 2 were given
      5:16 unexpected argument
    E2001 5:25 `True` is not a function
      5:25 unexpected argument
    ");
}
```

- [ ] **Step 3: 失敗を確かめる**

Run: `cargo test -p eml_types --test check`
Expected: コンパイルエラー (`dump` がない)

- [ ] **Step 4: 組み込みの型を書く**

`crates/eml_types/src/builtins.rs` を作る。

```rust
use eml_hir::builtin::Builtin;

use crate::table::{Row, Table, Ty};
use crate::ty::Effect;

/// 組み込みの型。型と意味は docs/spec/declarations.md の標準の演算子の表と、docs/spec/effects.md の組み込みの
/// `IO` にある。
pub(crate) fn builtin_type(table: &mut Table, builtin: Builtin) -> Ty {
    let (int, string, bool, unit) = (table.int, table.string, table.bool, table.unit);
    match builtin {
        Builtin::Println => table.function(string, Row::closed(vec![Effect::Io]), unit),
        Builtin::ShowInt => table.function(int, Row::pure(), string),
        Builtin::Not => table.function(bool, Row::pure(), bool),
        Builtin::True | Builtin::False => bool,
        Builtin::IntNeg => table.function(int, Row::pure(), int),
        Builtin::IntAdd | Builtin::IntSub | Builtin::IntMul | Builtin::IntDiv | Builtin::IntMod => {
            binary(table, int, int)
        }
        Builtin::IntEq
        | Builtin::IntNe
        | Builtin::IntLt
        | Builtin::IntLe
        | Builtin::IntGt
        | Builtin::IntGe => binary(table, int, bool),
        Builtin::StrConcat => binary(table, string, string),
    }
}

fn binary(table: &mut Table, operand: Ty, result: Ty) -> Ty {
    let inner = table.function(operand, Row::pure(), result);
    table.function(operand, Row::pure(), inner)
}
```

- [ ] **Step 5: 検査器を書く**

`crates/eml_types/src/check.rs` を作る。

```rust
use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_hir::builtin::BuiltinType;
use eml_hir::{
    Body, EffectRef, ExprId, ExprKind, Function, FunctionId, Literal, LocalId, Module, PatId,
    PatKind, Res, RowRef, Stmt, TypeRefId, TypeRefKind, not_yet_supported,
};
use la_arena::ArenaMap;

use crate::builtins::builtin_type;
use crate::table::{Row, Table, Ty, TyKind, UnifyError};
use crate::ty::{Effect, Linearity, Type};
use crate::{BodyTypes, TypedModule, codes};

pub(crate) fn check_module(module: &Module) -> (TypedModule, Vec<Diagnostic>) {
    let mut table = Table::new();
    let mut diagnostics = Vec::new();
    let mut signatures = ArenaMap::default();
    for (id, function) in module.functions.iter() {
        if let Some(signature) = &function.signature {
            signatures.insert(id, lower_type(&mut table, function, signature.ty));
        }
    }
    let main = module
        .functions
        .iter()
        .find(|(_, function)| function.name == "main")
        .map(|(id, _)| id);
    if let Some(id) = main {
        check_main(module, &table, &signatures, id, &mut diagnostics);
    }
    let mut bodies = Vec::new();
    for (id, function) in module.functions.iter() {
        let (Some(&signature), Some(body)) = (signatures.get(id), &function.body) else {
            continue;
        };
        let mut checker = BodyCheck {
            module,
            function,
            body,
            signatures: &signatures,
            table: &mut table,
            diagnostics: &mut diagnostics,
            ambient: Row::pure(),
            exprs: ArenaMap::default(),
            locals: ArenaMap::default(),
        };
        checker.check_function(signature);
        bodies.push((id, checker.exprs, checker.locals));
    }
    let mut typed = TypedModule {
        main,
        ..TypedModule::default()
    };
    for (id, &ty) in signatures.iter() {
        typed.signatures.insert(id, table.export(ty));
    }
    for (id, exprs, locals) in bodies {
        let mut types = BodyTypes::default();
        for (expr, &ty) in exprs.iter() {
            types.exprs.insert(expr, table.export(ty));
        }
        for (local, &ty) in locals.iter() {
            types.locals.insert(local, table.export(ty));
        }
        typed.bodies.insert(id, types);
    }
    (typed, diagnostics)
}

fn lower_type(table: &mut Table, function: &Function, id: TypeRefId) -> Ty {
    match &function.types[id].kind {
        TypeRefKind::Error => table.error,
        TypeRefKind::Builtin(BuiltinType::Int) => table.int,
        TypeRefKind::Builtin(BuiltinType::String) => table.string,
        TypeRefKind::Builtin(BuiltinType::Bool) => table.bool,
        TypeRefKind::Builtin(BuiltinType::Unit) => table.unit,
        TypeRefKind::Fn { param, row, ret } => {
            let param = lower_type(table, function, *param);
            let ret = lower_type(table, function, *ret);
            let row = match row {
                // 省略した row は空の row である (docs/spec/types.md の「関数型」)
                RowRef::Omitted => Row::pure(),
                RowRef::Closed { effects, .. } => {
                    Row::closed(effects.iter().map(|EffectRef::Io| Effect::Io).collect())
                }
                // 未対応の row の跡。どのエフェクトも受け入れて、診断を連鎖させない
                RowRef::Error => Row {
                    labels: Vec::new(),
                    tail: Some(table.fresh_row_var()),
                },
            };
            table.function(param, row, ret)
        }
    }
}

fn check_main(
    module: &Module,
    table: &Table,
    signatures: &ArenaMap<FunctionId, Ty>,
    id: FunctionId,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let function = &module.functions[id];
    let (Some(&ty), Some(signature)) = (signatures.get(id), &function.signature) else {
        return;
    };
    let found = table.export(ty);
    let expected = Type::Fn {
        param: Box::new(Type::unit()),
        linearity: Linearity::Unr,
        effects: vec![Effect::Io],
        ret: Box::new(Type::unit()),
    };
    if !found.contains_error() && found != expected {
        diagnostics.push(Diagnostic::error(
            codes::INVALID_MAIN_TYPE,
            "`main` must have type `Unit -> <IO> Unit`",
            Label::new(module.file, signature.range, format!("found `{found}`")),
        ));
    }
}

/// 型の不一致の由来。診断のラベルと note を決める (docs/spec/diagnostics.md の「型エラー」)。
#[derive(Debug, Clone)]
enum Origin {
    Argument {
        callee: TextRange,
        name: String,
        index: usize,
    },
    Return,
    Annotation(TextRange),
    IfCondition,
    IfBranches(TextRange),
    IfWithoutElse,
    Statement,
    UnitPattern,
}

struct BodyCheck<'a> {
    module: &'a Module,
    function: &'a Function,
    body: &'a Body,
    signatures: &'a ArenaMap<FunctionId, Ty>,
    table: &'a mut Table,
    diagnostics: &'a mut Vec<Diagnostic>,
    /// 本体が起こしてよいエフェクト。最後にたどったシグネチャの矢印の row である。
    ambient: Row,
    exprs: ArenaMap<ExprId, Ty>,
    locals: ArenaMap<LocalId, Ty>,
}

impl BodyCheck<'_> {
    fn file(&self) -> FileId {
        self.module.file
    }

    fn signature_range(&self) -> TextRange {
        self.function
            .signature
            .as_ref()
            .map_or(self.function.name_range, |signature| signature.range)
    }

    /// 等式 `f x y = e` は `fn x -> fn y -> e` と同じなので、引数を1つ消費するごとにシグネチャの矢印を1つたどる。
    /// 本体の row は、最後にたどった矢印の row になる。
    fn check_function(&mut self, signature: Ty) {
        let body = self.body;
        let mut expected = signature;
        for (index, &pat) in body.params.iter().enumerate() {
            match self.table.kind(expected).clone() {
                TyKind::Fn { param, row, ret, .. } => {
                    self.bind_pat(pat, param);
                    self.ambient = row;
                    expected = ret;
                }
                TyKind::Error => {
                    let error = self.table.error;
                    self.bind_pat(pat, error);
                }
                _ => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            codes::TYPE_MISMATCH,
                            format!(
                                "`{}` has {} but its signature has {}",
                                self.function.name,
                                count(body.params.len(), "parameter"),
                                count(index, "arrow"),
                            ),
                            Label::new(
                                self.file(),
                                body.pats[pat].range,
                                "this parameter has no arrow in the signature",
                            ),
                        )
                        .with_secondary(Label::new(
                            self.file(),
                            self.signature_range(),
                            "the signature",
                        )),
                    );
                    let error = self.table.error;
                    for &rest in &body.params[index..] {
                        self.bind_pat(rest, error);
                    }
                    expected = error;
                    break;
                }
            }
        }
        if matches!(self.table.kind(expected), TyKind::Fn { .. }) {
            // 関数を返す等式は、関数値と一緒に段階2で扱う
            self.diagnostics.push(not_yet_supported(
                self.file(),
                self.function.name_range,
                "an equation with fewer parameters than arrows in its signature is not supported yet",
            ));
            expected = self.table.error;
        }
        self.check_expr(body.root, expected, Origin::Return);
    }

    fn check_expr(&mut self, id: ExprId, expected: Ty, origin: Origin) {
        let body = self.body;
        let expr = &body.exprs[id];
        match &expr.kind {
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let bool = self.table.bool;
                self.check_expr(*condition, bool, Origin::IfCondition);
                match else_branch {
                    Some(else_branch) => {
                        self.check_expr(*then_branch, expected, origin.clone());
                        self.check_expr(*else_branch, expected, origin);
                    }
                    None => {
                        let unit = self.table.unit;
                        self.check_expr(*then_branch, unit, Origin::IfWithoutElse);
                        self.expect(expr.range, expected, unit, &origin);
                    }
                }
                self.exprs.insert(id, expected);
            }
            ExprKind::Block { stmts, tail } => {
                self.stmts(stmts);
                match tail {
                    Some(tail) => self.check_expr(*tail, expected, origin),
                    None => {
                        let unit = self.table.unit;
                        self.expect(expr.range, expected, unit, &origin);
                    }
                }
                self.exprs.insert(id, expected);
            }
            _ => {
                let found = self.infer_expr(id);
                self.expect(expr.range, expected, found, &origin);
            }
        }
    }

    fn infer_expr(&mut self, id: ExprId) -> Ty {
        let body = self.body;
        let expr = &body.exprs[id];
        let ty = match &expr.kind {
            ExprKind::Missing => self.table.error,
            ExprKind::Literal(Literal::Int(_)) => self.table.int,
            ExprKind::Literal(Literal::String(_)) => self.table.string,
            ExprKind::Literal(Literal::Unit) => self.table.unit,
            ExprKind::Path(res) => self.value(*res, expr.range),
            ExprKind::Call { callee, args } => self.call(id, *callee, args),
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let bool = self.table.bool;
                self.check_expr(*condition, bool, Origin::IfCondition);
                match else_branch {
                    Some(else_branch) => {
                        let ty = self.infer_expr(*then_branch);
                        let then_range = body.exprs[*then_branch].range;
                        self.check_expr(*else_branch, ty, Origin::IfBranches(then_range));
                        ty
                    }
                    None => {
                        let unit = self.table.unit;
                        self.check_expr(*then_branch, unit, Origin::IfWithoutElse);
                        unit
                    }
                }
            }
            ExprKind::Block { stmts, tail } => {
                self.stmts(stmts);
                match tail {
                    Some(tail) => self.infer_expr(*tail),
                    None => self.table.unit,
                }
            }
            ExprKind::Annot { expr: inner, ty } => {
                let annotated = lower_type(self.table, self.function, *ty);
                let range = self.function.types[*ty].range;
                self.check_expr(*inner, annotated, Origin::Annotation(range));
                annotated
            }
        };
        self.exprs.insert(id, ty);
        ty
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            match stmt {
                Stmt::Let { pat, ty, init } => {
                    let ty = match ty {
                        Some(ty) => {
                            let annotated = lower_type(self.table, self.function, *ty);
                            let range = self.function.types[*ty].range;
                            self.check_expr(*init, annotated, Origin::Annotation(range));
                            annotated
                        }
                        None => self.infer_expr(*init),
                    };
                    self.bind_pat(*pat, ty);
                }
                Stmt::Expr(expr) => {
                    let unit = self.table.unit;
                    self.check_expr(*expr, unit, Origin::Statement);
                }
            }
        }
    }

    fn value(&mut self, res: Res, range: TextRange) -> Ty {
        let ty = match res {
            Res::Local(local) => return self.locals.get(local).copied().unwrap_or(self.table.error),
            Res::Function(function) => self
                .signatures
                .get(function)
                .copied()
                .unwrap_or(self.table.error),
            Res::Builtin(builtin) => builtin_type(self.table, builtin),
        };
        if matches!(self.table.kind(ty), TyKind::Fn { .. }) {
            // 関数値はクロージャと一緒に段階2で入れる (docs/implementation/status.md)
            self.diagnostics.push(not_yet_supported(
                self.file(),
                range,
                "using a function as a value is not supported yet",
            ));
            return self.table.error;
        }
        ty
    }

    fn call(&mut self, id: ExprId, callee: ExprId, args: &[ExprId]) -> Ty {
        let body = self.body;
        let callee_expr = &body.exprs[callee];
        // 呼び出し先が名前なら、関数値の門 (`value`) を通さずに型を引く
        let (callee_ty, name) = match &callee_expr.kind {
            ExprKind::Path(Res::Function(function)) => (
                self.signatures
                    .get(*function)
                    .copied()
                    .unwrap_or(self.table.error),
                self.module.functions[*function].name.clone(),
            ),
            ExprKind::Path(Res::Builtin(builtin)) => (
                builtin_type(self.table, *builtin),
                builtin.name().to_string(),
            ),
            ExprKind::Path(Res::Local(local)) => (
                self.locals.get(*local).copied().unwrap_or(self.table.error),
                body.locals[*local].name.clone(),
            ),
            _ => (self.infer_expr(callee), "this expression".to_string()),
        };
        self.exprs.insert(callee, callee_ty);
        let mut ty = callee_ty;
        for (index, &arg) in args.iter().enumerate() {
            match self.table.kind(ty).clone() {
                TyKind::Fn { param, row, ret, .. } => {
                    let origin = Origin::Argument {
                        callee: callee_expr.range,
                        name: name.clone(),
                        index,
                    };
                    self.check_expr(arg, param, origin);
                    self.perform(row, body.exprs[id].range, &name);
                    ty = ret;
                }
                TyKind::Error => {
                    self.infer_expr(arg);
                }
                _ => {
                    let message = if index == 0 {
                        format!("`{name}` is not a function")
                    } else {
                        format!(
                            "`{name}` takes {} but {} were given",
                            count(index, "argument"),
                            args.len()
                        )
                    };
                    self.diagnostics.push(Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        message,
                        Label::new(self.file(), body.exprs[arg].range, "unexpected argument"),
                    ));
                    for &rest in &args[index..] {
                        self.infer_expr(rest);
                    }
                    return self.table.error;
                }
            }
        }
        if matches!(self.table.kind(ty), TyKind::Fn { .. }) {
            self.diagnostics.push(not_yet_supported(
                self.file(),
                body.exprs[id].range,
                "partial application is not supported yet",
            ));
            return self.table.error;
        }
        ty
    }

    /// 呼び出し先の閉じた row を新しい row 変数で開いてから、本体の row と単一化する (Koka と同じ)。
    /// 純粋な関数はどこからでも呼べ、`IO` を起こす関数は row に `IO` がある本体からだけ呼べる。
    fn perform(&mut self, row: Row, range: TextRange, name: &str) {
        let opened = if row.tail.is_none() {
            Row {
                labels: row.labels,
                tail: Some(self.table.fresh_row_var()),
            }
        } else {
            row
        };
        let ambient = self.ambient.clone();
        if let Err(UnifyError::MissingEffects(missing)) = self.table.unify_row(&opened, &ambient) {
            let quoted: Vec<String> = missing.iter().map(|e| format!("`{}`", e.name())).collect();
            let quoted = quoted.join(", ");
            let names: Vec<&str> = missing.iter().map(|e| e.name()).collect();
            let function = &self.function.name;
            self.diagnostics.push(
                Diagnostic::error(
                    codes::EFFECT_NOT_IN_ROW,
                    format!("`{name}` performs {quoted}, which the signature of `{function}` does not allow"),
                    Label::new(self.file(), range, format!("this call performs {quoted}")),
                )
                .with_secondary(Label::new(
                    self.file(),
                    self.signature_range(),
                    "the row of this signature does not include it",
                ))
                .with_help(format!(
                    "add {quoted} to the row of the signature of `{function}`, as in `-> <{}> ...`",
                    names.join(", ")
                )),
            );
        }
    }

    fn bind_pat(&mut self, pat: PatId, ty: Ty) {
        let body = self.body;
        match &body.pats[pat].kind {
            PatKind::Bind(local) => {
                self.locals.insert(*local, ty);
            }
            PatKind::Wildcard | PatKind::Missing => {}
            PatKind::Unit => {
                let unit = self.table.unit;
                self.expect(body.pats[pat].range, ty, unit, &Origin::UnitPattern);
            }
        }
    }

    fn expect(&mut self, range: TextRange, expected: Ty, found: Ty, origin: &Origin) {
        match self.table.unify(found, expected) {
            Ok(()) => {}
            Err(UnifyError::Occurs) => self.diagnostics.push(Diagnostic::error(
                codes::INFINITE_TYPE,
                "this expression would have an infinite type",
                Label::new(self.file(), range, "infinite type"),
            )),
            Err(_) => self.mismatch(range, expected, found, origin),
        }
    }

    fn mismatch(&mut self, range: TextRange, expected: Ty, found: Ty, origin: &Origin) {
        let file = self.file();
        let expected = self.table.export(expected);
        let found = self.table.export(found);
        let mut diagnostic = Diagnostic::error(
            codes::TYPE_MISMATCH,
            "mismatched types",
            Label::new(file, range, format!("expected `{expected}`, found `{found}`")),
        );
        diagnostic = match origin {
            Origin::Argument { callee, name, index } => diagnostic.with_secondary(Label::new(
                file,
                *callee,
                format!("argument {} of `{name}`", index + 1),
            )),
            Origin::Return => diagnostic.with_secondary(Label::new(
                file,
                self.signature_range(),
                format!("expected because of the signature of `{}`", self.function.name),
            )),
            Origin::Annotation(annotation) => diagnostic.with_secondary(Label::new(
                file,
                *annotation,
                "expected because of this annotation",
            )),
            Origin::IfCondition => {
                diagnostic.with_note("the condition of `if` must have type `Bool`")
            }
            Origin::IfBranches(then_branch) => diagnostic.with_secondary(Label::new(
                file,
                *then_branch,
                "the `then` branch has this type",
            )),
            Origin::IfWithoutElse => {
                diagnostic.with_note("an `if` without `else` must have type `Unit`")
            }
            Origin::Statement => diagnostic.with_note(
                "a statement that is not the last one in a block must have type `Unit`",
            ),
            Origin::UnitPattern => diagnostic.with_note("the pattern `()` matches only `Unit`"),
        };
        self.diagnostics.push(diagnostic);
    }
}

fn count(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

#[cfg(test)]
mod tests {
    use super::count;

    #[test]
    fn counts_are_pluralized() {
        assert_eq!(count(1, "arrow"), "1 arrow");
        assert_eq!(count(2, "arrow"), "2 arrows");
    }
}
```

- [ ] **Step 6: lib.rs を本物にする**

`crates/eml_types/src/lib.rs` を次にする (Task 6 で付けた `#[allow(dead_code)]` は外す)。

```rust
//! 型、row、Kind の検査 (docs/spec/types.md)。

mod builtins;
mod check;
mod kind;
mod table;
mod ty;

use std::fmt::Write;

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_hir::{ExprId, FunctionId, LocalId, Module};
use la_arena::ArenaMap;

pub use ty::{Effect, Linearity, Multiplicity, Type};

pub mod codes {
    use eml_diagnostics::ErrorCode;

    pub const TYPE_MISMATCH: ErrorCode = ErrorCode(2001);
    pub const EFFECT_NOT_IN_ROW: ErrorCode = ErrorCode(2002);
    pub const MISSING_MAIN: ErrorCode = ErrorCode(2003);
    pub const INVALID_MAIN_TYPE: ErrorCode = ErrorCode(2004);
    pub const INFINITE_TYPE: ErrorCode = ErrorCode(2005);
}

/// 型付き HIR。HIR は複製せず、型を別テーブルに持つ (docs/implementation/architecture.md)。
#[derive(Debug, Default)]
pub struct TypedModule {
    /// シグネチャのある関数だけを含む。
    pub signatures: ArenaMap<FunctionId, Type>,
    /// シグネチャと等式の両方がある関数だけを含む。
    pub bodies: ArenaMap<FunctionId, BodyTypes>,
    pub main: Option<FunctionId>,
}

#[derive(Debug, Default)]
pub struct BodyTypes {
    pub exprs: ArenaMap<ExprId, Type>,
    pub locals: ArenaMap<LocalId, Type>,
}

pub fn check(module: &Module) -> (TypedModule, Vec<Diagnostic>) {
    check::check_module(module)
}

/// `main` がないこと。`eml check` では検査せず、`eml run` だけが報告する (docs/spec/types.md の「推論」)。
pub fn missing_main(file: FileId) -> Diagnostic {
    Diagnostic::error(
        codes::MISSING_MAIN,
        "`main` is not defined",
        Label::new(file, TextRange::empty(0.into()), "`eml run` starts the program from `main`"),
    )
    .with_help("add `main : Unit -> <IO> Unit` and an equation `main () = ...`")
}

/// テストで推論結果を確かめるための表示。
pub fn dump(module: &Module, typed: &TypedModule) -> String {
    let mut out = String::new();
    for (id, function) in module.functions.iter() {
        if let Some(signature) = typed.signatures.get(id) {
            writeln!(out, "{} : {signature}", function.name).unwrap();
        }
        let (Some(body), Some(types)) = (&function.body, typed.bodies.get(id)) else {
            continue;
        };
        for (local, data) in body.locals.iter() {
            if let Some(ty) = types.locals.get(local) {
                writeln!(out, "  {}#{} : {ty}", data.name, u32::from(local.into_raw())).unwrap();
            }
        }
    }
    out
}
```

- [ ] **Step 7: 型検査のテストが通ることを確かめる**

Run: `cargo test -p eml_types`
Expected: PASS

- [ ] **Step 8: `compile` に E2003 を足す (テストを先に)**

`crates/eml_cli/tests/api.rs` の末尾に足す。

```rust
#[test]
fn compile_reports_a_missing_main() {
    let mut files = SourceFiles::new();
    let file = files.add("a.em", "f : Int -> Int\nf x = x");
    let compiled = compile(&files, file);
    let codes: Vec<String> = compiled.diagnostics.iter().map(|d| d.code.to_string()).collect();
    assert_eq!(codes, ["E2003"]);
    assert!(compiled.program.is_none());
}

#[test]
fn check_accepts_a_file_without_main() {
    let mut files = SourceFiles::new();
    let file = files.add("a.em", "f : Int -> Int\nf x = x");
    assert!(eml_cli::check(&files, file).is_empty());
}
```

`crates/eml_cli/tests/cli.rs` の末尾に足す (既存の `non_utf8_file_is_a_usage_error` と同じく一時ファイルを使う)。

```rust
#[test]
fn run_fails_without_main_but_check_succeeds() {
    let path = std::env::temp_dir().join(format!("eml-cli-no-main-{}.em", std::process::id()));
    std::fs::write(&path, "f : Int -> Int\nf x = x\n").unwrap();
    let path_text = path.to_str().unwrap();
    assert_eq!(eml(&["check", path_text]).status.code(), Some(0));
    let output = eml(&["run", path_text]);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("E2003"));
}
```

`eml` 補助関数が `tests/ui` を作業ディレクトリにしている場合でも、絶対パスなので読める。

Run: `cargo test -p eml_cli --test api --test cli`
Expected: FAIL (`compile_reports_a_missing_main` と `run_fails_without_main_but_check_succeeds`)

`crates/eml_cli/src/lib.rs` の `compile` と `analyze` を次にする (`check` は `analyze(files, file).1` のまま)。`analyze` が `main` の有無も返すように、構造体 `Analysis` を置く。Core IR の呼び方は Task 9 で変える。

```rust
pub fn compile(files: &SourceFiles, file: FileId) -> Compiled {
    let (analysis, mut diagnostics) = analyze(files, file);
    // `main` がないことは実行するときだけ誤りにする。モジュール (S2) は `main` を持たないため (docs/spec/types.md)
    if analysis.main.is_none() {
        diagnostics.push(eml_types::missing_main(file));
    }
    let program = (!has_errors(&diagnostics)).then(|| Arc::new(analysis.program));
    Compiled {
        diagnostics,
        program,
    }
}

struct Analysis {
    program: Program,
    main: Option<eml_hir::FunctionId>,
}

/// エラーがあっても止めずに全段階を実行する。1回の実行で、独立した複数のエラーを報告するため。
fn analyze(files: &SourceFiles, file: FileId) -> (Analysis, Vec<Diagnostic>) {
    let (parse, mut diagnostics) = eml_syntax::parse(file, files.text(file));
    let (module, stage) = eml_hir::lower(file, &parse.tree());
    diagnostics.extend(stage);
    let (typed, stage) = eml_types::check(&module);
    diagnostics.extend(stage);
    let main = typed.main;
    let (program, stage) = eml_core_ir::lower(&typed);
    diagnostics.extend(stage);
    (Analysis { program, main }, diagnostics)
}
```

- [ ] **Step 9: テストが通ることを確かめる**

Run: `cargo test`
Expected: PASS。`check-fail/` のスナップショットに E2xxx が加わった場合は、Task 4 の Step 13 と同じ基準で確かめて承認する

- [ ] **Step 10: 全体の確認とコミット**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`

```bash
git add crates/eml_types crates/eml_cli
git commit -m "Check bodies against their signatures and require main for run"
```

---
### Task 8: ランタイムのヒープと参照カウント

`eml_runtime` に、世代番号つきのインデックス方式のアリーナ、符号付きの RC、記述子、`debug_heap` のリーク検出を作る ([ランタイム](../../spec/runtime.md))。継続のフレームもランタイムのオブジェクトにする。

**Files:**
- Create: `crates/eml_runtime/src/heap.rs`
- Modify: `crates/eml_runtime/src/lib.rs`

**Interfaces:**
- Produces (すべて `eml_runtime` から公開):
  - `Value::{Int(i64), Unit, Tag(u32), Obj(ObjRef)}` (`Copy`)、`ObjRef` (`Copy`、`Eq`)
  - `DescId` と定数 `DescId::STRING`、`DescId::FRAME`、`Descriptor { name: String }`
  - `Payload::{Str(String), Frame(Frame)}`、`Frame { function: u32, resume: u32, bind: u32, slots: Option<Vec<Option<Value>>>, next: Option<ObjRef> }`
  - `HeapError::{UseAfterFree, Shared, NotImplemented(&'static str)}` (`Display` あり)
  - `Heap::new()`、`register(Descriptor) -> DescId`、`alloc(DescId, Payload) -> ObjRef`、`get(ObjRef) -> Result<&Payload, HeapError>`、`get_mut`、`dup(ObjRef) -> Result<(), HeapError>`、`decref(ObjRef) -> Result<(), HeapError>`、`is_unique(ObjRef) -> Result<bool, HeapError>`、`take(ObjRef) -> Result<Payload, HeapError>` (一意なオブジェクトを解放して中身を返す。子の所有権は呼び出し側に移る)、`mark_shared(ObjRef) -> Result<(), HeapError>` (常に `NotImplemented`)、`live_objects() -> Vec<(String, usize)>` (記述子の名前の順)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_runtime/src/heap.rs` を作り、まずテストのモジュールだけを書く。

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn string(heap: &mut Heap, text: &str) -> ObjRef {
        heap.alloc(DescId::STRING, Payload::Str(text.to_string()))
    }

    fn frame(heap: &mut Heap, slots: Vec<Option<Value>>, next: Option<ObjRef>) -> ObjRef {
        heap.alloc(
            DescId::FRAME,
            Payload::Frame(Frame {
                function: 0,
                resume: 0,
                bind: 0,
                slots: Some(slots),
                next,
            }),
        )
    }

    #[test]
    fn decref_to_zero_frees_the_object() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        heap.decref(s).unwrap();
        assert!(heap.live_objects().is_empty());
        assert_eq!(heap.get(s), Err(HeapError::UseAfterFree));
    }

    #[test]
    fn dup_keeps_the_object_alive() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        heap.dup(s).unwrap();
        heap.decref(s).unwrap();
        assert_eq!(heap.get(s).unwrap(), &Payload::Str("a".to_string()));
        heap.decref(s).unwrap();
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn a_reused_slot_does_not_revive_old_references() {
        let mut heap = Heap::new();
        let old = string(&mut heap, "a");
        heap.decref(old).unwrap();
        let new = string(&mut heap, "b");
        assert_ne!(old, new);
        assert_eq!(heap.get(old), Err(HeapError::UseAfterFree));
        assert_eq!(heap.decref(old), Err(HeapError::UseAfterFree));
        assert_eq!(heap.get(new).unwrap(), &Payload::Str("b".to_string()));
    }

    #[test]
    fn decref_releases_children() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        let inner = frame(&mut heap, vec![], None);
        let outer = frame(&mut heap, vec![Some(Value::Obj(s)), Some(Value::Int(1)), None], Some(inner));
        heap.decref(outer).unwrap();
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn releasing_a_long_chain_does_not_overflow_the_stack() {
        let mut heap = Heap::new();
        let mut next = None;
        for _ in 0..200_000 {
            next = Some(frame(&mut heap, vec![], next));
        }
        heap.decref(next.unwrap()).unwrap();
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn live_objects_are_counted_by_descriptor() {
        let mut heap = Heap::new();
        string(&mut heap, "a");
        string(&mut heap, "b");
        frame(&mut heap, vec![], None);
        assert_eq!(
            heap.live_objects(),
            [("Frame".to_string(), 1), ("String".to_string(), 2)]
        );
    }

    #[test]
    fn take_requires_a_unique_object() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        heap.dup(s).unwrap();
        assert_eq!(heap.take(s), Err(HeapError::Shared));
        heap.decref(s).unwrap();
        assert_eq!(heap.take(s), Ok(Payload::Str("a".to_string())));
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn mark_shared_is_reserved_for_multicore() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        assert_eq!(heap.mark_shared(s), Err(HeapError::NotImplemented("mark_shared")));
    }

    #[test]
    fn registered_descriptors_are_counted_by_name() {
        let mut heap = Heap::new();
        let desc = heap.register(Descriptor { name: "Cell".to_string() });
        heap.alloc(desc, Payload::Str(String::new()));
        assert_eq!(heap.live_objects(), [("Cell".to_string(), 1)]);
    }
}
```

`crates/eml_runtime/src/lib.rs` に `mod heap;` を足す。

Run: `cargo test -p eml_runtime`
Expected: コンパイルエラー (`Heap` などがない)

- [ ] **Step 2: ヒープを実装する**

`crates/eml_runtime/src/heap.rs` のテストのモジュールの前に書く。

```rust
//! ヒープと参照カウント (docs/spec/runtime.md)。インデックス方式のアリーナと世代番号で、解放済みのオブジェクトへの
//! アクセスを `unsafe` なしに検出する。

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicI32, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObjRef {
    index: u32,
    generation: u32,
}

/// インタプリタの値。`Copy` で、`Rc` も `RefCell` も使わない (docs/spec/core-ir.md の「実行時の規約」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    Int(i64),
    Unit,
    /// 引数のないコンストラクタ。`Bool` は `False` = 0、`True` = 1 である。
    Tag(u32),
    Obj(ObjRef),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DescId(u32);

impl DescId {
    pub const STRING: DescId = DescId(0);
    pub const FRAME: DescId = DescId(1);
}

/// オブジェクトの種類。ヘッダから引けるようにし、後の段階でフィールドのレイアウトと `Lin` の破棄処理を足す
/// (docs/spec/runtime.md の「オブジェクトのヘッダ」)。
#[derive(Debug, Clone)]
pub struct Descriptor {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Payload {
    Str(String),
    Frame(Frame),
}

/// CEK 機械の継続のフレーム。継続もランタイムのオブジェクトにする (docs/spec/runtime.md)。各フィールドの意味は
/// インタプリタが決める。
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub function: u32,
    pub resume: u32,
    pub bind: u32,
    /// `None` なら、戻った後も今の環境を使い続ける (入れ子の式のフレーム)。
    pub slots: Option<Vec<Option<Value>>>,
    pub next: Option<ObjRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeapError {
    UseAfterFree,
    Shared,
    NotImplemented(&'static str),
}

impl fmt::Display for HeapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HeapError::UseAfterFree => f.write_str("use of a freed object"),
            HeapError::Shared => f.write_str("an object is still shared"),
            HeapError::NotImplemented(name) => write!(f, "`{name}` is not implemented yet"),
        }
    }
}

struct Header {
    /// 正なら局所、負なら共有を表す。マイルストーン1 では常に正である (docs/spec/runtime.md)。
    rc: AtomicI32,
    desc: DescId,
}

struct Object {
    header: Header,
    payload: Payload,
}

struct Slot {
    generation: u32,
    object: Option<Object>,
}

pub struct Heap {
    slots: Vec<Slot>,
    free: Vec<u32>,
    descriptors: Vec<Descriptor>,
}

impl Default for Heap {
    fn default() -> Self {
        Self::new()
    }
}

impl Heap {
    pub fn new() -> Heap {
        Heap {
            slots: Vec::new(),
            free: Vec::new(),
            descriptors: vec![
                Descriptor {
                    name: "String".to_string(),
                },
                Descriptor {
                    name: "Frame".to_string(),
                },
            ],
        }
    }

    pub fn register(&mut self, descriptor: Descriptor) -> DescId {
        self.descriptors.push(descriptor);
        DescId(self.descriptors.len() as u32 - 1)
    }

    pub fn alloc(&mut self, desc: DescId, payload: Payload) -> ObjRef {
        let object = Object {
            header: Header {
                rc: AtomicI32::new(1),
                desc,
            },
            payload,
        };
        match self.free.pop() {
            Some(index) => {
                let slot = &mut self.slots[index as usize];
                slot.object = Some(object);
                ObjRef {
                    index,
                    generation: slot.generation,
                }
            }
            None => {
                self.slots.push(Slot {
                    generation: 0,
                    object: Some(object),
                });
                ObjRef {
                    index: self.slots.len() as u32 - 1,
                    generation: 0,
                }
            }
        }
    }

    pub fn get(&self, obj: ObjRef) -> Result<&Payload, HeapError> {
        Ok(&self.object(obj)?.payload)
    }

    pub fn get_mut(&mut self, obj: ObjRef) -> Result<&mut Payload, HeapError> {
        Ok(&mut self.object_mut(obj)?.payload)
    }

    pub fn dup(&mut self, obj: ObjRef) -> Result<(), HeapError> {
        *self.object_mut(obj)?.header.rc.get_mut() += 1;
        Ok(())
    }

    pub fn is_unique(&self, obj: ObjRef) -> Result<bool, HeapError> {
        Ok(self.object(obj)?.header.rc.load(Ordering::Relaxed) == 1)
    }

    /// 子は再帰ではなく作業リストでたどる。長い連鎖の解放で Rust のスタックを溢れさせないため (docs/spec/runtime.md)。
    pub fn decref(&mut self, obj: ObjRef) -> Result<(), HeapError> {
        let mut work = vec![obj];
        while let Some(obj) = work.pop() {
            let remaining = {
                let rc = self.object_mut(obj)?.header.rc.get_mut();
                *rc -= 1;
                *rc
            };
            if remaining == 0 {
                let payload = self.release(obj);
                children(&payload, &mut work);
            }
        }
        Ok(())
    }

    /// 一意なオブジェクトを解放して中身を返す。子の所有権は呼び出し側に移る。
    pub fn take(&mut self, obj: ObjRef) -> Result<Payload, HeapError> {
        if !self.is_unique(obj)? {
            return Err(HeapError::Shared);
        }
        Ok(self.release(obj))
    }

    /// 共有の印付けは、名前だけ予約する。実装はマルチコアの段階で行う (docs/spec/runtime.md)。
    pub fn mark_shared(&mut self, obj: ObjRef) -> Result<(), HeapError> {
        self.object(obj)?;
        Err(HeapError::NotImplemented("mark_shared"))
    }

    /// まだ解放されていないオブジェクトの数を、記述子の名前ごとに数える。`debug_heap` のリーク検出で使う。
    pub fn live_objects(&self) -> Vec<(String, usize)> {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for object in self.slots.iter().filter_map(|slot| slot.object.as_ref()) {
            let name = &self.descriptors[object.header.desc.0 as usize].name;
            *counts.entry(name.clone()).or_default() += 1;
        }
        counts.into_iter().collect()
    }

    fn object(&self, obj: ObjRef) -> Result<&Object, HeapError> {
        self.slots
            .get(obj.index as usize)
            .filter(|slot| slot.generation == obj.generation)
            .and_then(|slot| slot.object.as_ref())
            .ok_or(HeapError::UseAfterFree)
    }

    fn object_mut(&mut self, obj: ObjRef) -> Result<&mut Object, HeapError> {
        self.slots
            .get_mut(obj.index as usize)
            .filter(|slot| slot.generation == obj.generation)
            .and_then(|slot| slot.object.as_mut())
            .ok_or(HeapError::UseAfterFree)
    }

    /// スロットを空けて世代番号を進める。古い `ObjRef` は、以後の検査で解放済みとして見つかる。
    fn release(&mut self, obj: ObjRef) -> Payload {
        let slot = &mut self.slots[obj.index as usize];
        let object = slot.object.take().expect("the caller checked the reference");
        slot.generation = slot.generation.wrapping_add(1);
        self.free.push(obj.index);
        object.payload
    }
}

fn children(payload: &Payload, work: &mut Vec<ObjRef>) {
    if let Payload::Frame(frame) = payload {
        work.extend(
            frame
                .slots
                .iter()
                .flatten()
                .flatten()
                .filter_map(|value| match value {
                    Value::Obj(obj) => Some(*obj),
                    _ => None,
                }),
        );
        work.extend(frame.next);
    }
}
```

`crates/eml_runtime/src/lib.rs` を次にする。

```rust
mod heap;
mod output;

pub use heap::{DescId, Descriptor, Frame, Heap, HeapError, ObjRef, Payload, Value};
pub use output::OutputSink;
```

- [ ] **Step 3: テストが通ることを確かめる**

Run: `cargo test -p eml_runtime`
Expected: PASS

- [ ] **Step 4: 全体の確認とコミット**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`

```bash
git add crates/eml_runtime
git commit -m "Add the generational heap with reference counting and leak detection"
```

---

### Task 9: Core IR への変換と Perceus の挿入

型付き HIR を ANF の Core IR に変換し、Perceus の `dup` / `decref` を挿入する ([Core IR とインタプリタ](../../spec/core-ir.md))。`if` は値を返す入れ子の式 (`Rhs::Nested`) の中の `Switch` にする。Core IR はエラーのないプログラムだけを受け取るので、`eml_cli::compile` はエラーがあれば Core IR を作らない。

**Files:**
- Modify: `crates/eml_core_ir/Cargo.toml`
- Modify: `crates/eml_core_ir/src/lib.rs`
- Create: `crates/eml_core_ir/src/lower.rs`、`crates/eml_core_ir/src/perceus.rs`、`crates/eml_core_ir/src/pretty.rs`
- Create: `crates/eml_core_ir/tests/common/mod.rs`、`crates/eml_core_ir/tests/lower.rs`
- Modify: `crates/eml_cli/src/lib.rs` (`analyze` を `front` に置き換える)

**Interfaces:**
- Consumes: Task 4〜7 の `Module`、`TypedModule`、`Type`、`Linearity`
- Produces:
  - `eml_core_ir::lower(module: &Module, typed: &TypedModule) -> Program` (前提: 診断のエラーがない)
  - `eml_core_ir::pretty(program: &Program) -> String`
  - 型: `Program { functions: Vec<CoreFn>, main: FnIdx, strings: Vec<String> }`、`Program::function(FnIdx) -> &CoreFn`、`CoreFn { name, params: Vec<VarId>, vars: Vec<VarInfo>, body: CExprId, exprs: Vec<CExpr> }`、`CoreFn::expr(CExprId) -> &CExpr`、`VarInfo { name, linearity: Linearity, boxed: bool }`、`FnIdx(pub u32)`、`VarId(pub u32)`、`CExprId(pub u32)`、`CExpr::{Let { var, rhs, body }, Switch { scrutinee: Atom, arms: Vec<(u32, CExprId)> }, Return(Atom), Dup { var, body }, Decref { var, body }}`、`Rhs::{Atom, CallDirect(FnIdx, Vec<Atom>), Prim(PrimOp, Vec<Atom>), ConstString(u32), Perform(IoOp, Vec<Atom>), Nested(CExprId)}`、`Atom::{Var(VarId), Int(i64), Unit, Tag(u32)}`、`PrimOp::{IntAdd, IntSub, IntMul, IntDiv, IntMod, IntNeg, IntEq, IntNe, IntLt, IntLe, IntGt, IntGe, StrConcat, ShowInt, Not}` と `PrimOp::name()`、`IoOp::Println`、定数 `FALSE = 0`、`TRUE = 1`、再公開 `eml_core_ir::Linearity`
  - 所有権の規約: 関数の引数とプリミティブと `perform` の引数は、どれも所有権を受け取る

- [ ] **Step 1: 依存を変える**

`crates/eml_core_ir/Cargo.toml` を次にする。

```toml
[package]
name = "eml_core_ir"
version.workspace = true
edition.workspace = true

[dependencies]
eml_hir.workspace = true
eml_types.workspace = true
la-arena.workspace = true

[dev-dependencies]
eml_diagnostics.workspace = true
eml_syntax.workspace = true
insta.workspace = true
```

- [ ] **Step 2: テストの補助と失敗するテストを書く**

`crates/eml_core_ir/tests/common/mod.rs` を作る。

```rust
use eml_diagnostics::{SourceFiles, has_errors};

/// 誤りのないプログラムを Core IR にして表示する。
pub fn core_text(text: &str) -> String {
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (parse, mut diagnostics) = eml_syntax::parse(file, text);
    let (module, stage) = eml_hir::lower(file, &parse.tree());
    diagnostics.extend(stage);
    let (typed, stage) = eml_types::check(&module);
    diagnostics.extend(stage);
    assert!(!has_errors(&diagnostics), "{diagnostics:#?}");
    eml_core_ir::pretty(&eml_core_ir::lower(&module, &typed))
}
```

`crates/eml_core_ir/tests/lower.rs` を作る。表示の約束: 変数は `名前番号` (`s1`)、`#0` / `#1` はタグ (`False` / `True`)、`let t = {` から `}` までは値を返す入れ子の式。

```rust
mod common;

use common::core_text;

#[test]
fn hello_world() {
    insta::assert_snapshot!(core_text("main : Unit -> <IO> Unit\nmain () = println \"hi\""), @r#"
    fn main(p0) {
      let s1 = const "hi"
      let t2 = perform println(s1)
      return t2
    }
    "#);
}

#[test]
fn strings_are_dupped_and_decreffed() {
    let text = "twice : String -> String\ntwice s = s ++ s\n\nignore : String -> Int\nignore s = 1\n\nmain : Unit -> <IO> Unit\nmain () = println (twice \"a\")";
    insta::assert_snapshot!(core_text(text), @r#"
    fn twice(s0) {
      dup s0
      let t1 = prim ++(s0, s0)
      return t1
    }
    fn ignore(s0) {
      decref s0
      return 1
    }
    fn main(p0) {
      let s1 = const "a"
      let t2 = call twice(s1)
      let t3 = perform println(t2)
      return t3
    }
    "#);
}

#[test]
fn shadowed_and_discarded_strings() {
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let s = \"x\"\n  let s = s ++ \"y\"\n  let _ = \"z\"\n  println s";
    insta::assert_snapshot!(core_text(text), @r#"
    fn main(p0) {
      let s1 = const "x"
      let s2 = const "y"
      let t3 = prim ++(s1, s2)
      let s4 = const "z"
      decref s4
      let t5 = perform println(t3)
      return t5
    }
    "#);
}

#[test]
fn a_non_tail_if_keeps_strings_used_later() {
    let text = "pick : Bool -> String -> String\npick b s =\n  let t = if b then s else \"none\"\n  t ++ s\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text), @r#"
    fn pick(b0, s1) {
      let t3 = {
        switch b0 {
          #0 ->
            let s2 = const "none"
            return s2
          #1 ->
            dup s1
            return s1
        }
      }
      let t4 = prim ++(t3, s1)
      return t4
    }
    fn main(p0) {
      return ()
    }
    "#);
}

#[test]
fn recursion_and_top_level_values() {
    let text = "answer : Int\nanswer = 42\n\ncount : Int -> Int\ncount n = if n == 0 then answer else count (n - 1)\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (count 3))";
    insta::assert_snapshot!(core_text(text), @r"
    fn answer() {
      return 42
    }
    fn count(n0) {
      let t1 = prim ==(n0, 0)
      let t5 = {
        switch t1 {
          #0 ->
            let t3 = prim -(n0, 1)
            let t4 = call count(t3)
            return t4
          #1 ->
            let answer2 = call answer()
            return answer2
        }
      }
      return t5
    }
    fn main(p0) {
      let t1 = call count(3)
      let t2 = prim show_int(t1)
      let t3 = perform println(t2)
      return t3
    }
    ");
}
```

Run: `cargo test -p eml_core_ir`
Expected: コンパイルエラー (`pretty` などがない)

- [ ] **Step 3: Core IR の型を書く**

`crates/eml_core_ir/src/lib.rs` を次にする。

```rust
//! Core IR (docs/spec/core-ir.md)。型付き HIR から変換する ANF 形式の IR で、RC とエフェクトを明示する。

mod lower;
mod perceus;
mod pretty;

pub use eml_types::Linearity;
pub use lower::lower;
pub use pretty::pretty;

/// 複数のスレッドが同じプログラムを実行できるように、実行時は `Arc<Program>` で読み取り専用で共有する。
#[derive(Debug)]
pub struct Program {
    pub functions: Vec<CoreFn>,
    pub main: FnIdx,
    /// 文字列リテラルの定数表。`ConstString` が添字で引き、実行のたびに新しい文字列をヒープに作る。
    pub strings: Vec<String>,
}

impl Program {
    pub fn function(&self, idx: FnIdx) -> &CoreFn {
        &self.functions[idx.0 as usize]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FnIdx(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VarId(pub u32);

/// 継続のフレームが「どこから再開するか」を持てるように、式に ID を付ける。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CExprId(pub u32);

#[derive(Debug)]
pub struct CoreFn {
    pub name: String,
    pub params: Vec<VarId>,
    pub vars: Vec<VarInfo>,
    pub body: CExprId,
    pub exprs: Vec<CExpr>,
}

impl CoreFn {
    pub fn expr(&self, id: CExprId) -> &CExpr {
        &self.exprs[id.0 as usize]
    }
}

/// 型の情報は消し、Kind とボックス化の有無だけを残す (docs/spec/core-ir.md)。
#[derive(Debug, Clone)]
pub struct VarInfo {
    pub name: String,
    pub linearity: Linearity,
    /// ヒープに置く値。`Unr` でボックス化した変数が RC の対象になる。
    pub boxed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CExpr {
    Let {
        var: VarId,
        rhs: Rhs,
        body: CExprId,
    },
    /// タグで分岐する。`if` もここに変換し、段階4の `match` と同じ命令にする。
    Switch {
        scrutinee: Atom,
        arms: Vec<(u32, CExprId)>,
    },
    Return(Atom),
    Dup {
        var: VarId,
        body: CExprId,
    },
    Decref {
        var: VarId,
        body: CExprId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rhs {
    Atom(Atom),
    CallDirect(FnIdx, Vec<Atom>),
    Prim(PrimOp, Vec<Atom>),
    ConstString(u32),
    Perform(IoOp, Vec<Atom>),
    /// 値を返す入れ子の式。中の `Return` が、この `Let` の変数に値を渡す。
    Nested(CExprId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Atom {
    Var(VarId),
    Int(i64),
    Unit,
    Tag(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimOp {
    IntAdd,
    IntSub,
    IntMul,
    IntDiv,
    IntMod,
    IntNeg,
    IntEq,
    IntNe,
    IntLt,
    IntLe,
    IntGt,
    IntGe,
    StrConcat,
    ShowInt,
    Not,
}

impl PrimOp {
    pub fn name(self) -> &'static str {
        match self {
            PrimOp::IntAdd => "+",
            PrimOp::IntSub => "-",
            PrimOp::IntMul => "*",
            PrimOp::IntDiv => "/",
            PrimOp::IntMod => "%",
            PrimOp::IntNeg => "negate",
            PrimOp::IntEq => "==",
            PrimOp::IntNe => "!=",
            PrimOp::IntLt => "<",
            PrimOp::IntLe => "<=",
            PrimOp::IntGt => ">",
            PrimOp::IntGe => ">=",
            PrimOp::StrConcat => "++",
            PrimOp::ShowInt => "show_int",
            PrimOp::Not => "not",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoOp {
    Println,
}

/// `Bool` のタグ (docs/spec/core-ir.md)。
pub const FALSE: u32 = 0;
pub const TRUE: u32 = 1;
```

- [ ] **Step 4: ANF への変換を書く**

`crates/eml_core_ir/src/lower.rs` を作る。

```rust
use std::collections::HashMap;

use eml_hir::builtin::Builtin;
use eml_hir::{Body, ExprId, ExprKind, FunctionId, Literal, LocalId, Module, PatId, PatKind, Res, Stmt};
use eml_types::{BodyTypes, Linearity, Type, TypedModule};
use la_arena::ArenaMap;

use crate::{
    Atom, CExpr, CExprId, CoreFn, FALSE, FnIdx, IoOp, PrimOp, Program, Rhs, TRUE, VarId, VarInfo,
    perceus,
};

/// 診断のエラーがないプログラムだけを受け取る。エラーがあれば `eml_cli` は Core IR を作らない
/// (docs/implementation/architecture.md)。
pub fn lower(module: &Module, typed: &TypedModule) -> Program {
    let mut indices = ArenaMap::default();
    for (index, (id, _)) in module.functions.iter().enumerate() {
        indices.insert(id, FnIdx(index as u32));
    }
    let mut strings = Strings::default();
    let mut functions = Vec::new();
    for (id, function) in module.functions.iter() {
        let body = function
            .body
            .as_ref()
            .expect("a program without errors has an equation for every function");
        let lowering = FnLowering {
            module,
            body,
            types: typed.bodies.get(id).expect("every body is type-checked"),
            indices: &indices,
            strings: &mut strings,
            exprs: Vec::new(),
            vars: Vec::new(),
            locals: ArenaMap::default(),
        };
        let signature = typed.signatures.get(id).expect("every function has a signature");
        let mut core = lowering.lower(&function.name, signature);
        perceus::insert_rc(&mut core);
        functions.push(core);
    }
    let main = typed.main.expect("`eml_cli::compile` reports a missing `main`");
    Program {
        functions,
        main: indices[main],
        strings: strings.values,
    }
}

#[derive(Default)]
struct Strings {
    values: Vec<String>,
    ids: HashMap<String, u32>,
}

impl Strings {
    fn intern(&mut self, text: &str) -> u32 {
        if let Some(&id) = self.ids.get(text) {
            return id;
        }
        let id = self.values.len() as u32;
        self.values.push(text.to_string());
        self.ids.insert(text.to_string(), id);
        id
    }
}

type Bindings = Vec<(VarId, Rhs)>;

struct FnLowering<'a> {
    module: &'a Module,
    body: &'a Body,
    types: &'a BodyTypes,
    indices: &'a ArenaMap<FunctionId, FnIdx>,
    strings: &'a mut Strings,
    exprs: Vec<CExpr>,
    vars: Vec<VarInfo>,
    locals: ArenaMap<LocalId, Atom>,
}

impl FnLowering<'_> {
    fn lower(mut self, name: &str, signature: &Type) -> CoreFn {
        let body = self.body;
        let mut params = Vec::new();
        let mut ty = signature;
        for &pat in &body.params {
            let Type::Fn { param, ret, .. } = ty else {
                unreachable!("the type checker matched parameters with arrows");
            };
            let name = match body.pats[pat].kind {
                PatKind::Bind(local) => body.locals[local].name.clone(),
                _ => "p".to_string(),
            };
            let var = self.new_var(&name, param);
            if let PatKind::Bind(local) = body.pats[pat].kind {
                self.locals.insert(local, Atom::Var(var));
            }
            params.push(var);
            ty = ret.as_ref();
        }
        let root = self.tail(body.root);
        CoreFn {
            name: name.to_string(),
            params,
            vars: self.vars,
            body: root,
            exprs: self.exprs,
        }
    }

    fn new_var(&mut self, name: &str, ty: &Type) -> VarId {
        self.vars.push(VarInfo {
            name: name.to_string(),
            // 段階1の値はすべて `Unr` で、ヒープに置くのは文字列だけである
            linearity: Linearity::Unr,
            boxed: matches!(ty, Type::String),
        });
        VarId(self.vars.len() as u32 - 1)
    }

    fn push(&mut self, expr: CExpr) -> CExprId {
        self.exprs.push(expr);
        CExprId(self.exprs.len() as u32 - 1)
    }

    fn seq(&mut self, bindings: Bindings, last: CExpr) -> CExprId {
        let mut id = self.push(last);
        for (var, rhs) in bindings.into_iter().rev() {
            id = self.push(CExpr::Let { var, rhs, body: id });
        }
        id
    }

    /// 式の値を返すコード。
    fn tail(&mut self, expr: ExprId) -> CExprId {
        let mut bindings = Vec::new();
        let atom = self.atom(expr, &mut bindings);
        self.seq(bindings, CExpr::Return(atom))
    }

    fn ty(&self, expr: ExprId) -> Type {
        self.types.exprs.get(expr).cloned().unwrap_or(Type::Error)
    }

    fn bind(&mut self, out: &mut Bindings, name: &str, ty: &Type, rhs: Rhs) -> Atom {
        let var = self.new_var(name, ty);
        out.push((var, rhs));
        Atom::Var(var)
    }

    /// 式の値をアトムにする。値の計算に要る束縛は `out` に積む。
    fn atom(&mut self, id: ExprId, out: &mut Bindings) -> Atom {
        let body = self.body;
        match &body.exprs[id].kind {
            ExprKind::Missing => unreachable!("a program without errors has no missing expressions"),
            ExprKind::Literal(Literal::Int(n)) => Atom::Int(*n),
            ExprKind::Literal(Literal::Unit) => Atom::Unit,
            ExprKind::Literal(Literal::String(text)) => {
                let index = self.strings.intern(text);
                self.bind(out, "s", &Type::String, Rhs::ConstString(index))
            }
            ExprKind::Path(Res::Local(local)) => self.locals[*local],
            ExprKind::Path(Res::Builtin(Builtin::True)) => Atom::Tag(TRUE),
            ExprKind::Path(Res::Builtin(Builtin::False)) => Atom::Tag(FALSE),
            ExprKind::Path(Res::Builtin(_)) => {
                unreachable!("the type checker allows builtin functions only as callees")
            }
            // 引数のないトップレベルの値は、参照するたびに呼び出す
            ExprKind::Path(Res::Function(function)) => {
                let ty = self.ty(id);
                let name = self.module.functions[*function].name.clone();
                let callee = self.indices[*function];
                self.bind(out, &name, &ty, Rhs::CallDirect(callee, Vec::new()))
            }
            ExprKind::Call { callee, args } => {
                let args: Vec<Atom> = args.iter().map(|&arg| self.atom(arg, out)).collect();
                let rhs = match &body.exprs[*callee].kind {
                    ExprKind::Path(Res::Function(function)) => {
                        Rhs::CallDirect(self.indices[*function], args)
                    }
                    ExprKind::Path(Res::Builtin(Builtin::Println)) => {
                        Rhs::Perform(IoOp::Println, args)
                    }
                    ExprKind::Path(Res::Builtin(builtin)) => Rhs::Prim(prim(*builtin), args),
                    _ => unreachable!("only functions and builtins are called in stage 1"),
                };
                let ty = self.ty(id);
                self.bind(out, "t", &ty, rhs)
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let scrutinee = self.atom(*condition, out);
                let then_code = self.tail(*then_branch);
                let else_code = match else_branch {
                    Some(else_branch) => self.tail(*else_branch),
                    None => self.push(CExpr::Return(Atom::Unit)),
                };
                let switch = self.push(CExpr::Switch {
                    scrutinee,
                    arms: vec![(FALSE, else_code), (TRUE, then_code)],
                });
                let ty = self.ty(id);
                self.bind(out, "t", &ty, Rhs::Nested(switch))
            }
            ExprKind::Block { stmts, tail } => {
                for stmt in stmts {
                    match stmt {
                        Stmt::Let { pat, init, .. } => {
                            let value = self.atom(*init, out);
                            self.bind_pat(*pat, value);
                        }
                        // 式文の値は `Unit` なので捨ててよい
                        Stmt::Expr(expr) => {
                            self.atom(*expr, out);
                        }
                    }
                }
                match tail {
                    Some(tail) => self.atom(*tail, out),
                    None => Atom::Unit,
                }
            }
            ExprKind::Annot { expr, .. } => self.atom(*expr, out),
        }
    }

    /// `_` と `()` で受けた値は以後使われないので、Perceus の挿入が decref する。
    fn bind_pat(&mut self, pat: PatId, value: Atom) {
        if let PatKind::Bind(local) = self.body.pats[pat].kind {
            self.locals.insert(local, value);
        }
    }
}

fn prim(builtin: Builtin) -> PrimOp {
    match builtin {
        Builtin::IntAdd => PrimOp::IntAdd,
        Builtin::IntSub => PrimOp::IntSub,
        Builtin::IntMul => PrimOp::IntMul,
        Builtin::IntDiv => PrimOp::IntDiv,
        Builtin::IntMod => PrimOp::IntMod,
        Builtin::IntNeg => PrimOp::IntNeg,
        Builtin::IntEq => PrimOp::IntEq,
        Builtin::IntNe => PrimOp::IntNe,
        Builtin::IntLt => PrimOp::IntLt,
        Builtin::IntLe => PrimOp::IntLe,
        Builtin::IntGt => PrimOp::IntGt,
        Builtin::IntGe => PrimOp::IntGe,
        Builtin::StrConcat => PrimOp::StrConcat,
        Builtin::ShowInt => PrimOp::ShowInt,
        Builtin::Not => PrimOp::Not,
        Builtin::Println | Builtin::True | Builtin::False => {
            unreachable!("`println` is performed and constructors are values")
        }
    }
}
```

- [ ] **Step 5: Perceus の挿入を書く**

`crates/eml_core_ir/src/perceus.rs` を作る。

```rust
//! Perceus の `dup` / `decref` の挿入 (docs/spec/core-ir.md)。変数を使うことを所有権の移動として扱い、後でも使う
//! 変数を複製し、使わなくなった変数をできるだけ早く捨てる。対象は `Unr` でボックス化した変数だけである。

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::{Atom, CExpr, CExprId, CoreFn, Linearity, Rhs, VarId};

type Vars = BTreeSet<VarId>;

pub(crate) fn insert_rc(function: &mut CoreFn) {
    let tracked: Vec<bool> = function
        .vars
        .iter()
        .map(|var| var.boxed && var.linearity == Linearity::Unr)
        .collect();
    let (body, exprs) = {
        let mut pass = Pass {
            old: &function.exprs,
            tracked: &tracked,
            new: Vec::new(),
            free: HashMap::new(),
        };
        let owned: Vars = function
            .params
            .iter()
            .copied()
            .filter(|var| tracked[var.0 as usize])
            .collect();
        let body = pass.transform(function.body, &owned, &Vars::new());
        (body, pass.new)
    };
    function.body = body;
    function.exprs = exprs;
}

struct Pass<'a> {
    old: &'a [CExpr],
    tracked: &'a [bool],
    new: Vec<CExpr>,
    free: HashMap<CExprId, Vars>,
}

impl Pass<'_> {
    fn atom_var(&self, atom: &Atom) -> Option<VarId> {
        match atom {
            Atom::Var(var) if self.tracked[var.0 as usize] => Some(*var),
            _ => None,
        }
    }

    /// 右辺が使う変数を、使う回数の分だけ並べる。
    fn uses(&self, rhs: &Rhs) -> Vec<VarId> {
        let atoms: &[Atom] = match rhs {
            Rhs::Atom(atom) => std::slice::from_ref(atom),
            Rhs::CallDirect(_, args) | Rhs::Prim(_, args) | Rhs::Perform(_, args) => args,
            Rhs::ConstString(_) | Rhs::Nested(_) => &[],
        };
        atoms.iter().filter_map(|atom| self.atom_var(atom)).collect()
    }

    /// 式の中で使う対象の変数。
    fn free(&mut self, id: CExprId) -> Vars {
        if let Some(vars) = self.free.get(&id) {
            return vars.clone();
        }
        let old = self.old;
        let vars = match &old[id.0 as usize] {
            CExpr::Let { var, rhs, body } => {
                let mut vars = self.free(*body);
                vars.remove(var);
                match rhs {
                    Rhs::Nested(inner) => vars.extend(self.free(*inner)),
                    other => vars.extend(self.uses(other)),
                }
                vars
            }
            CExpr::Switch { scrutinee, arms } => {
                let mut vars: Vars = self.atom_var(scrutinee).into_iter().collect();
                for &(_, arm) in arms {
                    vars.extend(self.free(arm));
                }
                vars
            }
            CExpr::Return(atom) => self.atom_var(atom).into_iter().collect(),
            CExpr::Dup { .. } | CExpr::Decref { .. } => {
                unreachable!("the pass runs once on code without RC instructions")
            }
        };
        self.free.insert(id, vars.clone());
        vars
    }

    fn push(&mut self, expr: CExpr) -> CExprId {
        self.new.push(expr);
        CExprId(self.new.len() as u32 - 1)
    }

    /// `owned` は入口で所有している変数。`keep` は、この式が値を返した後でも使うので、所有したまま残す変数。
    fn transform(&mut self, id: CExprId, owned: &Vars, keep: &Vars) -> CExprId {
        let old = self.old;
        match &old[id.0 as usize] {
            CExpr::Return(atom) => {
                let returned = self.atom_var(atom);
                let mut code = self.push(CExpr::Return(*atom));
                for &var in owned.iter().rev() {
                    if !keep.contains(&var) && Some(var) != returned {
                        code = self.push(CExpr::Decref { var, body: code });
                    }
                }
                // 返す値の所有権は呼び出し側に移るので、後でも使うなら複製する
                if let Some(var) = returned.filter(|var| keep.contains(var)) {
                    code = self.push(CExpr::Dup { var, body: code });
                }
                code
            }
            CExpr::Let {
                var,
                rhs: Rhs::Nested(inner),
                body,
            } => {
                let mut after = self.free(*body);
                after.remove(var);
                after.extend(keep.iter().copied());
                let keep_inner: Vars = after.intersection(owned).copied().collect();
                // 入れ子の式の各枝は、使わない変数を枝の先頭で捨てる
                let inner = self.transform(*inner, owned, &keep_inner);
                let mut owned_body = keep_inner;
                if self.tracked[var.0 as usize] {
                    owned_body.insert(*var);
                }
                let body = self.transform(*body, &owned_body, keep);
                self.push(CExpr::Let {
                    var: *var,
                    rhs: Rhs::Nested(inner),
                    body,
                })
            }
            CExpr::Let { var, rhs, body } => {
                let uses = self.uses(rhs);
                let mut after = self.free(*body);
                after.remove(var);
                after.extend(keep.iter().copied());
                let mut owned_body: Vars = owned.intersection(&after).copied().collect();
                if self.tracked[var.0 as usize] {
                    owned_body.insert(*var);
                }
                let body = self.transform(*body, &owned_body, keep);
                let mut code = self.push(CExpr::Let {
                    var: *var,
                    rhs: rhs.clone(),
                    body,
                });
                // 後で使わない変数は、この束縛の前で捨てる
                for &dead in owned.iter().rev() {
                    if !uses.contains(&dead) && !after.contains(&dead) {
                        code = self.push(CExpr::Decref { var: dead, body: code });
                    }
                }
                // 右辺は使うたびに所有権を1つ受け取るので、2回目以降の使用と、後でも使う変数の分を複製する
                let mut counts: BTreeMap<VarId, usize> = BTreeMap::new();
                for &used in &uses {
                    *counts.entry(used).or_default() += 1;
                }
                for (&used, &count) in counts.iter().rev() {
                    let dups = count - usize::from(!after.contains(&used));
                    for _ in 0..dups {
                        code = self.push(CExpr::Dup { var: used, body: code });
                    }
                }
                code
            }
            CExpr::Switch { scrutinee, arms } => {
                let arms = arms
                    .iter()
                    .map(|&(tag, arm)| (tag, self.transform(arm, owned, keep)))
                    .collect();
                self.push(CExpr::Switch {
                    scrutinee: *scrutinee,
                    arms,
                })
            }
            CExpr::Dup { .. } | CExpr::Decref { .. } => {
                unreachable!("the pass runs once on code without RC instructions")
            }
        }
    }
}
```

- [ ] **Step 6: 表示を書く**

`crates/eml_core_ir/src/pretty.rs` を作る。

```rust
//! テストで Core IR を確かめるための表示。

use std::fmt::Write;

use crate::{Atom, CExpr, CExprId, CoreFn, IoOp, Program, Rhs, VarId};

pub fn pretty(program: &Program) -> String {
    let mut out = String::new();
    for function in &program.functions {
        let params: Vec<String> = function.params.iter().map(|&p| var(function, p)).collect();
        writeln!(out, "fn {}({}) {{", function.name, params.join(", ")).unwrap();
        expr(program, function, function.body, 1, &mut out);
        out.push_str("}\n");
    }
    out
}

fn var(function: &CoreFn, var: VarId) -> String {
    format!("{}{}", function.vars[var.0 as usize].name, var.0)
}

fn atom(function: &CoreFn, atom: &Atom) -> String {
    match atom {
        Atom::Var(v) => var(function, *v),
        Atom::Int(n) => n.to_string(),
        Atom::Unit => "()".to_string(),
        Atom::Tag(tag) => format!("#{tag}"),
    }
}

fn expr(program: &Program, function: &CoreFn, id: CExprId, indent: usize, out: &mut String) {
    let pad = "  ".repeat(indent);
    match function.expr(id) {
        CExpr::Let { var: v, rhs, body } => {
            if let Rhs::Nested(inner) = rhs {
                writeln!(out, "{pad}let {} = {{", var(function, *v)).unwrap();
                expr(program, function, *inner, indent + 1, out);
                writeln!(out, "{pad}}}").unwrap();
            } else {
                writeln!(out, "{pad}let {} = {}", var(function, *v), rhs_text(program, function, rhs)).unwrap();
            }
            expr(program, function, *body, indent, out);
        }
        CExpr::Switch { scrutinee, arms } => {
            writeln!(out, "{pad}switch {} {{", atom(function, scrutinee)).unwrap();
            for (tag, arm) in arms {
                writeln!(out, "{pad}  #{tag} ->").unwrap();
                expr(program, function, *arm, indent + 2, out);
            }
            writeln!(out, "{pad}}}").unwrap();
        }
        CExpr::Return(a) => writeln!(out, "{pad}return {}", atom(function, a)).unwrap(),
        CExpr::Dup { var: v, body } => {
            writeln!(out, "{pad}dup {}", var(function, *v)).unwrap();
            expr(program, function, *body, indent, out);
        }
        CExpr::Decref { var: v, body } => {
            writeln!(out, "{pad}decref {}", var(function, *v)).unwrap();
            expr(program, function, *body, indent, out);
        }
    }
}

fn rhs_text(program: &Program, function: &CoreFn, rhs: &Rhs) -> String {
    let args = |args: &[Atom]| {
        args.iter()
            .map(|a| atom(function, a))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match rhs {
        Rhs::Atom(a) => atom(function, a),
        Rhs::CallDirect(callee, a) => format!("call {}({})", program.function(*callee).name, args(a)),
        Rhs::Prim(op, a) => format!("prim {}({})", op.name(), args(a)),
        Rhs::ConstString(index) => format!("const {:?}", program.strings[*index as usize]),
        Rhs::Perform(IoOp::Println, a) => format!("perform println({})", args(a)),
        Rhs::Nested(_) => unreachable!("nested expressions are printed as blocks"),
    }
}
```

- [ ] **Step 7: Core IR のテストが通ることを確かめる**

Run: `cargo test -p eml_core_ir`
Expected: PASS

- [ ] **Step 8: `eml_cli` を新しい Core IR につなぐ**

`crates/eml_cli/src/lib.rs` の `check`、`compile`、`Analysis`、`analyze` を次に置き換える。

```rust
pub fn check(files: &SourceFiles, file: FileId) -> Vec<Diagnostic> {
    front(files, file).2
}

pub fn compile(files: &SourceFiles, file: FileId) -> Compiled {
    let (module, typed, mut diagnostics) = front(files, file);
    // `main` がないことは実行するときだけ誤りにする。モジュール (S2) は `main` を持たないため (docs/spec/types.md)
    if typed.main.is_none() {
        diagnostics.push(eml_types::missing_main(file));
    }
    // Core IR は誤りのないプログラムだけを受け取る (docs/implementation/architecture.md)
    let program =
        (!has_errors(&diagnostics)).then(|| Arc::new(eml_core_ir::lower(&module, &typed)));
    Compiled {
        diagnostics,
        program,
    }
}

/// エラーがあっても止めずに、検査の段階をすべて実行する。1回の実行で、独立した複数のエラーを報告するため。
fn front(files: &SourceFiles, file: FileId) -> (eml_hir::Module, eml_types::TypedModule, Vec<Diagnostic>) {
    let (parse, mut diagnostics) = eml_syntax::parse(file, files.text(file));
    let (module, stage) = eml_hir::lower(file, &parse.tree());
    diagnostics.extend(stage);
    let (typed, stage) = eml_types::check(&module);
    diagnostics.extend(stage);
    (module, typed, diagnostics)
}
```

- [ ] **Step 9: 全体の確認とコミット**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`
Expected: PASS (インタプリタはまだ仮実装なので、`run/` は何も出力せずに終わる)

```bash
git add crates/eml_core_ir crates/eml_cli
git commit -m "Lower typed HIR to ANF Core IR and insert Perceus reference counting"
```

---

### Task 10: CEK インタプリタ

Core IR を CEK 機械で実行する ([Core IR とインタプリタ](../../spec/core-ir.md))。継続はヒープ上のフレームの連結リストで、最下部に `IO` の組み込み handler のフレームを置く。ヒープの値の読み出しは move、複製は `Dup` 命令だけで行う。

**Files:**
- Modify: `crates/eml_interp/Cargo.toml`
- Modify: `crates/eml_interp/src/lib.rs`
- Create: `crates/eml_interp/tests/run.rs`

**Interfaces:**
- Consumes: Task 8 の `Heap` など、Task 9 の `Program` など
- Produces: `eml_interp::run(program: Arc<Program>, config: &RunConfig, out: &OutputSink) -> Result<(), RuntimeError>` (シグネチャは既存と同じ)。実行時エラーのメッセージは `"{理由} in `{関数名}`"` (`integer overflow in `main``、`division by zero in `divide``)。リークは `memory leak: objects were not freed: 1 String` の形

- [ ] **Step 1: 依存を足す**

`crates/eml_interp/Cargo.toml` を次にする。

```toml
[package]
name = "eml_interp"
version.workspace = true
edition.workspace = true

[dependencies]
eml_core_ir.workspace = true
eml_runtime.workspace = true

[dev-dependencies]
eml_diagnostics.workspace = true
eml_hir.workspace = true
eml_syntax.workspace = true
eml_types.workspace = true
```

- [ ] **Step 2: 失敗するテストを書く**

`crates/eml_interp/tests/run.rs` を作る。

```rust
use std::sync::Arc;

use eml_core_ir::{Atom, CExpr, CExprId, CoreFn, FnIdx, Linearity, Program, Rhs, VarId, VarInfo};
use eml_diagnostics::{SourceFiles, has_errors};
use eml_interp::{RunConfig, RuntimeError};
use eml_runtime::OutputSink;

/// `debug_heap` を有効にして実行し、出力と結果を返す。
fn run(text: &str) -> (String, Result<(), RuntimeError>) {
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (parse, mut diagnostics) = eml_syntax::parse(file, text);
    let (module, stage) = eml_hir::lower(file, &parse.tree());
    diagnostics.extend(stage);
    let (typed, stage) = eml_types::check(&module);
    diagnostics.extend(stage);
    assert!(!has_errors(&diagnostics), "{diagnostics:#?}");
    execute(eml_core_ir::lower(&module, &typed), true)
}

fn execute(program: Program, debug_heap: bool) -> (String, Result<(), RuntimeError>) {
    let (sink, buffer) = OutputSink::capture();
    let mut config = RunConfig::default();
    config.debug_heap = debug_heap;
    let result = eml_interp::run(Arc::new(program), &config, &sink);
    let stdout = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
    (stdout, result)
}

fn main_with(body: &str) -> String {
    format!("main : Unit -> <IO> Unit\nmain () =\n{body}")
}

#[test]
fn hello_world() {
    assert_eq!(run(&main_with("  println \"hi\"")), ("hi\n".to_string(), Ok(())));
}

#[test]
fn arithmetic_truncates_toward_zero() {
    let body = "  println (show_int (1 + 2 * 3))\n  println (show_int ((-7) / 2))\n  println (show_int ((-7) % 2))\n  println (show_int (-2 * 3))";
    assert_eq!(run(&main_with(body)), ("7\n-3\n-1\n-6\n".to_string(), Ok(())));
}

#[test]
fn recursion() {
    let text = "factorial : Int -> Int\nfactorial n = if n <= 1 then 1 else n * factorial (n - 1)\n\n".to_string()
        + &main_with("  println (show_int (factorial 20))");
    assert_eq!(run(&text), ("2432902008176640000\n".to_string(), Ok(())));
}

#[test]
fn deep_recursion_does_not_overflow_the_stack() {
    let text = "count : Int -> Int\ncount n = if n == 0 then 0 else 1 + count (n - 1)\n\n".to_string()
        + &main_with("  println (show_int (count 100000))");
    assert_eq!(run(&text), ("100000\n".to_string(), Ok(())));
}

#[test]
fn strings_are_freed() {
    let text = "twice : String -> String\ntwice s = s ++ s\n\nignore : String -> Int\nignore s = 1\n\npick : Bool -> String -> String\npick b s =\n  let t = if b then s else \"none\"\n  t ++ s\n\n".to_string()
        + &main_with("  let s = \"x\"\n  let s = s ++ \"y\"\n  let _ = \"z\"\n  println (twice s)\n  println (show_int (ignore \"w\"))\n  println (pick True \"a\")\n  println (pick False \"b\")");
    assert_eq!(
        run(&text),
        ("xyxy\n1\naa\nnoneb\n".to_string(), Ok(()))
    );
}

#[test]
fn and_and_or_short_circuit() {
    let text = "noisy : Bool -> <IO> Bool\nnoisy b =\n  println \"evaluated\"\n  b\n\nshow_bool : Bool -> String\nshow_bool b = if b then \"True\" else \"False\"\n\n".to_string()
        + &main_with("  println (show_bool (False && noisy True))\n  println (show_bool (True || noisy False))\n  println (show_bool (True && noisy False))");
    assert_eq!(
        run(&text),
        ("False\nTrue\nevaluated\nFalse\n".to_string(), Ok(()))
    );
}

#[test]
fn integer_overflow_is_a_runtime_error() {
    let (stdout, result) = run(&main_with("  println \"before\"\n  println (show_int (9223372036854775807 + 1))"));
    assert_eq!(stdout, "before\n");
    assert_eq!(result, Err(RuntimeError("integer overflow in `main`".to_string())));
}

#[test]
fn division_by_zero_is_a_runtime_error() {
    let text = "divide : Int -> Int -> Int\ndivide a b = a / b\n\n".to_string()
        + &main_with("  println (show_int (divide 1 0))");
    assert_eq!(
        run(&text).1,
        Err(RuntimeError("division by zero in `divide`".to_string()))
    );
}

/// Perceus の挿入を経ない手書きの Core IR で、`debug_heap` がリークを見つけることを確かめる。
fn leaking_program() -> Program {
    let var = |name: &str, boxed| VarInfo {
        name: name.to_string(),
        linearity: Linearity::Unr,
        boxed,
    };
    Program {
        functions: vec![CoreFn {
            name: "main".to_string(),
            params: vec![VarId(0)],
            vars: vec![var("p", false), var("s", true)],
            body: CExprId(1),
            exprs: vec![
                CExpr::Return(Atom::Unit),
                CExpr::Let {
                    var: VarId(1),
                    rhs: Rhs::ConstString(0),
                    body: CExprId(0),
                },
            ],
        }],
        main: FnIdx(0),
        strings: vec!["leaked".to_string()],
    }
}

#[test]
fn debug_heap_reports_leaks() {
    assert_eq!(
        execute(leaking_program(), true).1,
        Err(RuntimeError(
            "memory leak: objects were not freed: 1 String".to_string()
        ))
    );
    assert_eq!(execute(leaking_program(), false).1, Ok(()));
}
```

`strings_are_freed` の期待値: `twice "xy"` は `xyxy`、`ignore "w"` は 1、`pick True "a"` は `t = "a"` で `"a" ++ "a"`、`pick False "b"` は `t = "none"` で `"none" ++ "b"`。

Run: `cargo test -p eml_interp`
Expected: FAIL (仮実装は何も出力しない)

- [ ] **Step 3: インタプリタを実装する**

`crates/eml_interp/src/lib.rs` を次にする (`RunConfig` と `RuntimeError` は既存のまま)。

```rust
use std::fmt;
use std::sync::Arc;

use eml_core_ir::{Atom, CExpr, CExprId, FALSE, FnIdx, IoOp, PrimOp, Program, Rhs, TRUE, VarId};
use eml_runtime::{DescId, Frame, Heap, HeapError, ObjRef, OutputSink, Payload, Value};

/// 将来 `threads` などを足しても呼び出し側を壊さないように、`non_exhaustive` にして `RunConfig::default()` から作らせる。
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct RunConfig {
    pub debug_heap: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError(pub String);

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RuntimeError {}

pub fn run(program: Arc<Program>, config: &RunConfig, out: &OutputSink) -> Result<(), RuntimeError> {
    let mut machine = Machine::new(&program, out);
    machine.run()?;
    // 実行時エラーで止まった場合はリークを数えない。途中のフレームが残っているのは当然だから
    if config.debug_heap {
        machine.check_leaks()?;
    }
    Ok(())
}

/// 継続の最下部にある、`IO` の組み込み handler のフレームの印 (docs/spec/core-ir.md)。
const IO_HANDLER: u32 = u32::MAX;

/// CEK 機械。制御 (`function` と `control`)、環境 (`slots`)、継続 (`cont`) からなる。
struct Machine<'p> {
    program: &'p Program,
    out: &'p OutputSink,
    heap: Heap,
    function: FnIdx,
    control: CExprId,
    slots: Vec<Option<Value>>,
    /// 継続の先頭のフレーム。最下部には常に `IO` の handler のフレームがある。
    cont: ObjRef,
}

impl<'p> Machine<'p> {
    fn new(program: &'p Program, out: &'p OutputSink) -> Self {
        let mut heap = Heap::new();
        let cont = heap.alloc(
            DescId::FRAME,
            Payload::Frame(Frame {
                function: IO_HANDLER,
                resume: 0,
                bind: 0,
                slots: None,
                next: None,
            }),
        );
        let main = program.function(program.main);
        let mut slots = vec![None; main.vars.len()];
        // `main : Unit -> <IO> Unit` の引数
        for param in &main.params {
            slots[param.0 as usize] = Some(Value::Unit);
        }
        Machine {
            program,
            out,
            heap,
            function: program.main,
            control: main.body,
            slots,
            cont,
        }
    }

    fn run(&mut self) -> Result<(), RuntimeError> {
        loop {
            match self.step() {
                Ok(true) => return Ok(()),
                Ok(false) => {}
                Err(message) => {
                    let function = &self.program.function(self.function).name;
                    return Err(RuntimeError(format!("{message} in `{function}`")));
                }
            }
        }
    }

    /// 1つの命令を実行する。プログラムが終わったら真を返す。
    fn step(&mut self) -> Result<bool, String> {
        let program = self.program;
        match program.function(self.function).expr(self.control) {
            CExpr::Let { var, rhs, body } => self.bind(*var, rhs, *body)?,
            CExpr::Switch { scrutinee, arms } => {
                let Value::Tag(tag) = self.atom(scrutinee)? else {
                    return Err(internal("a switch on a value that is not a tag"));
                };
                self.control = arms
                    .iter()
                    .find(|(arm_tag, _)| *arm_tag == tag)
                    .map(|&(_, arm)| arm)
                    .ok_or_else(|| internal("a switch without a matching arm"))?;
            }
            CExpr::Return(atom) => {
                let value = self.atom(atom)?;
                return self.ret(value);
            }
            CExpr::Dup { var, body } => {
                if let Value::Obj(obj) = self.peek(*var)? {
                    self.heap.dup(obj).map_err(heap_error)?;
                }
                self.control = *body;
            }
            CExpr::Decref { var, body } => {
                if let Value::Obj(obj) = self.atom(&Atom::Var(*var))? {
                    self.heap.decref(obj).map_err(heap_error)?;
                }
                self.control = *body;
            }
        }
        Ok(false)
    }

    fn bind(&mut self, var: VarId, rhs: &Rhs, body: CExprId) -> Result<(), String> {
        let value = match rhs {
            Rhs::Atom(atom) => self.atom(atom)?,
            Rhs::ConstString(index) => {
                let text = self.program.strings[*index as usize].clone();
                Value::Obj(self.heap.alloc(DescId::STRING, Payload::Str(text)))
            }
            Rhs::Prim(op, args) => {
                let args = self.atoms(args)?;
                self.prim(*op, &args)?
            }
            Rhs::Perform(IoOp::Println, args) => {
                // `IO` はユーザーが handle できず、最下部の handler が必ずすぐに再開するので、継続を遡らずにその場で
                // 実行する (docs/spec/core-ir.md)
                let args = self.atoms(args)?;
                let text = self.take_string(args[0])?;
                self.out
                    .write_str(&format!("{text}\n"))
                    .map_err(|error| format!("cannot write the output: {error}"))?;
                Value::Unit
            }
            Rhs::CallDirect(callee, args) => {
                let args = self.atoms(args)?;
                self.push_frame(var, body, true);
                let target = self.program.function(*callee);
                let mut slots = vec![None; target.vars.len()];
                for (param, value) in target.params.iter().zip(args) {
                    slots[param.0 as usize] = Some(value);
                }
                self.slots = slots;
                self.function = *callee;
                self.control = target.body;
                return Ok(());
            }
            Rhs::Nested(inner) => {
                self.push_frame(var, body, false);
                self.control = *inner;
                return Ok(());
            }
        };
        self.slots[var.0 as usize] = Some(value);
        self.control = body;
        Ok(())
    }

    /// 呼び出しでは環境ごと退避する。入れ子の式は同じ関数の中なので、環境をそのまま使い続ける。
    fn push_frame(&mut self, bind: VarId, resume: CExprId, save_env: bool) {
        let slots = save_env.then(|| std::mem::take(&mut self.slots));
        let frame = Frame {
            function: self.function.0,
            resume: resume.0,
            bind: bind.0,
            slots,
            next: Some(self.cont),
        };
        self.cont = self.heap.alloc(DescId::FRAME, Payload::Frame(frame));
    }

    /// 継続の先頭のフレームに値を返す。最下部の `IO` の handler に届いたら、プログラムが終わる。
    fn ret(&mut self, value: Value) -> Result<bool, String> {
        // 段階1では継続を複製しないので、フレームは常に一意である。共有されたフレームは段階3の `multi` で扱う
        let Payload::Frame(frame) = self.heap.take(self.cont).map_err(heap_error)? else {
            return Err(internal("the continuation is not a frame"));
        };
        if frame.function == IO_HANDLER {
            if let Value::Obj(obj) = value {
                self.heap.decref(obj).map_err(heap_error)?;
            }
            return Ok(true);
        }
        if let Some(slots) = frame.slots {
            self.slots = slots;
        }
        self.function = FnIdx(frame.function);
        self.control = CExprId(frame.resume);
        self.slots[frame.bind as usize] = Some(value);
        self.cont = frame
            .next
            .ok_or_else(|| internal("a frame without a next frame"))?;
        Ok(false)
    }

    /// ヒープの値の読み出しは所有権の移動なので、スロットを空にする。複製は `dup` 命令だけが行う
    /// (docs/spec/core-ir.md)。ヒープにない値は何度でも読める。
    fn atom(&mut self, atom: &Atom) -> Result<Value, String> {
        Ok(match *atom {
            Atom::Var(var) => {
                let slot = &mut self.slots[var.0 as usize];
                match *slot {
                    Some(Value::Obj(obj)) => {
                        *slot = None;
                        Value::Obj(obj)
                    }
                    Some(value) => value,
                    None => return Err(internal("a variable read after it was moved")),
                }
            }
            Atom::Int(n) => Value::Int(n),
            Atom::Unit => Value::Unit,
            Atom::Tag(tag) => Value::Tag(tag),
        })
    }

    fn atoms(&mut self, atoms: &[Atom]) -> Result<Vec<Value>, String> {
        atoms.iter().map(|atom| self.atom(atom)).collect()
    }

    fn peek(&self, var: VarId) -> Result<Value, String> {
        self.slots[var.0 as usize].ok_or_else(|| internal("a variable read after it was moved"))
    }

    fn prim(&mut self, op: PrimOp, args: &[Value]) -> Result<Value, String> {
        let int = |index: usize| match args[index] {
            Value::Int(n) => Ok(n),
            _ => Err(internal("an integer operation on a value that is not an integer")),
        };
        let overflow = || "integer overflow".to_string();
        let tag = |b: bool| Value::Tag(if b { TRUE } else { FALSE });
        Ok(match op {
            PrimOp::IntAdd => Value::Int(int(0)?.checked_add(int(1)?).ok_or_else(overflow)?),
            PrimOp::IntSub => Value::Int(int(0)?.checked_sub(int(1)?).ok_or_else(overflow)?),
            PrimOp::IntMul => Value::Int(int(0)?.checked_mul(int(1)?).ok_or_else(overflow)?),
            PrimOp::IntDiv | PrimOp::IntMod => {
                let (a, b) = (int(0)?, int(1)?);
                if b == 0 {
                    return Err("division by zero".to_string());
                }
                // Rust と同じく 0 の方向に切り捨てる (docs/spec/declarations.md)
                let result = if op == PrimOp::IntDiv {
                    a.checked_div(b)
                } else {
                    a.checked_rem(b)
                };
                Value::Int(result.ok_or_else(overflow)?)
            }
            PrimOp::IntNeg => Value::Int(int(0)?.checked_neg().ok_or_else(overflow)?),
            PrimOp::IntEq => tag(int(0)? == int(1)?),
            PrimOp::IntNe => tag(int(0)? != int(1)?),
            PrimOp::IntLt => tag(int(0)? < int(1)?),
            PrimOp::IntLe => tag(int(0)? <= int(1)?),
            PrimOp::IntGt => tag(int(0)? > int(1)?),
            PrimOp::IntGe => tag(int(0)? >= int(1)?),
            PrimOp::Not => match args[0] {
                Value::Tag(t) => tag(t == FALSE),
                _ => return Err(internal("`not` on a value that is not a tag")),
            },
            PrimOp::ShowInt => {
                let text = int(0)?.to_string();
                Value::Obj(self.heap.alloc(DescId::STRING, Payload::Str(text)))
            }
            PrimOp::StrConcat => {
                let left = self.take_string(args[0])?;
                let right = self.take_string(args[1])?;
                Value::Obj(self.heap.alloc(DescId::STRING, Payload::Str(left + &right)))
            }
        })
    }

    /// プリミティブは引数の所有権を受け取るので、読んだ文字列は decref する。
    fn take_string(&mut self, value: Value) -> Result<String, String> {
        let Value::Obj(obj) = value else {
            return Err(internal("a string operation on a value that is not a string"));
        };
        let text = match self.heap.get(obj).map_err(heap_error)? {
            Payload::Str(text) => text.clone(),
            Payload::Frame(_) => {
                return Err(internal("a string operation on a frame"));
            }
        };
        self.heap.decref(obj).map_err(heap_error)?;
        Ok(text)
    }

    fn check_leaks(&self) -> Result<(), RuntimeError> {
        let live = self.heap.live_objects();
        if live.is_empty() {
            return Ok(());
        }
        let parts: Vec<String> = live.iter().map(|(name, n)| format!("{n} {name}")).collect();
        Err(RuntimeError(format!(
            "memory leak: objects were not freed: {}",
            parts.join(", ")
        )))
    }
}

fn internal(what: &str) -> String {
    format!("internal error: {what}")
}

fn heap_error(error: HeapError) -> String {
    error.to_string()
}
```

- [ ] **Step 4: テストが通ることを確かめる**

Run: `cargo test -p eml_interp`
Expected: PASS

- [ ] **Step 5: 全体の確認とコミット**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`
Expected: PASS (`run/empty.em` と `comments_only.em` は `main () = ()` を実行し、出力のスナップショットは変わらない)

```bash
git add crates/eml_interp
git commit -m "Run Core IR on a CEK machine with heap-allocated frames"
```

---

### Task 11: UI テストのコーパスと `run-fail/`

段階1の完了条件にあるプログラムを UI テストに置く。コンパイルは通って実行時エラーになるプログラムのために `tests/ui/run-fail/` を足す ([テスト](../../implementation/testing.md))。

**Files:**
- Modify: `crates/eml_cli/tests/ui.rs` (`run_fail`)
- Create: `tests/ui/run/hello.em`、`factorial.em`、`fibonacci.em`、`deep_recursion.em`、`strings_and_let.em`、`operators.em`
- Create: `tests/ui/run-fail/integer_overflow.em`、`division_by_zero.em`
- Create: `tests/ui/check-fail/undefined_name.em`、`missing_signature.em`、`missing_equation.em`、`duplicate_definition.em`、`non_associative.em`、`type_mismatch.em`、`invalid_main_type.em`、`missing_io_row.em`、`not_yet_supported.em`
- Test: `crates/eml_cli/tests/cli.rs`
- Snapshots: `crates/eml_cli/tests/snapshots/` (新しいファイル)

**Interfaces:**
- Consumes: Task 0〜10 のすべて

- [ ] **Step 1: `run-fail` の仕組みを書く**

`crates/eml_cli/tests/ui.rs` の先頭のコメントの `(`run/`、`check-fail/`)` を `(`run/`、`run-fail/`、`check-fail/`)` に変え、`check_fail` の前に足す。

```rust
#[test]
fn run_fail() {
    insta::glob!("../../../tests/ui", "run-fail/*.em", |path| {
        let (files, id) = load(path);
        let mut config = RunConfig::default();
        config.debug_heap = true;
        let compiled = eml_cli::compile(&files, id);
        let stderr = render(&compiled.diagnostics, &files);
        let program = compiled
            .program
            .unwrap_or_else(|| panic!("unexpected errors:\n{stderr}"));
        let (sink, buffer) = OutputSink::capture();
        let RunResult::RuntimeError(message) = eml_cli::execute(program, &config, sink) else {
            panic!("expected a runtime error");
        };
        let stdout = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        insta::assert_snapshot!(format!("--- stdout ---\n{stdout}--- runtime error ---\n{message}\n"));
    });
}
```

- [ ] **Step 2: 実行するプログラムを置く**

`tests/ui/run/hello.em`:

```haskell
-- The smallest program.
main : Unit -> <IO> Unit
main () = println "Hello, world!"
```

`tests/ui/run/factorial.em`:

```haskell
-- Recursion and arithmetic up to the edge of Int.
factorial : Int -> Int
factorial n = if n <= 1 then 1 else n * factorial (n - 1)

main : Unit -> <IO> Unit
main () =
  println (show_int (factorial 10))
  println (show_int (factorial 20))
```

`tests/ui/run/fibonacci.em`:

```haskell
-- An `if` without `else` whose `then` branch is a block.
fib : Int -> Int
fib n = if n < 2 then n else fib (n - 1) + fib (n - 2)

print_fibs : Int -> Int -> <IO> Unit
print_fibs i n =
  if i < n then
    println (show_int (fib i))
    print_fibs (i + 1) n

main : Unit -> <IO> Unit
main () = print_fibs 0 10
```

`tests/ui/run/deep_recursion.em`:

```haskell
-- 100000 nested calls must not overflow the Rust stack: frames live on the heap.
count : Int -> Int
count n = if n == 0 then 0 else 1 + count (n - 1)

main : Unit -> <IO> Unit
main () = println (show_int (count 100000))
```

`tests/ui/run/strings_and_let.em`:

```haskell
-- Strings that are shadowed, used twice, and discarded must all be freed.
greet : String -> String
greet name = "Hello, " ++ name ++ "!"

main : Unit -> <IO> Unit
main () =
  let name = "eml"
  let message = greet name
  println message
  println name
  let name = name ++ name
  println name
  let _ = greet "unused"
  println (show_int (-42))
```

`tests/ui/run/operators.em`:

```haskell
-- Precedence, prefix minus, truncating division, short-circuit logic, and pipes.
noisy : Bool -> <IO> Bool
noisy b =
  println "evaluated"
  b

show_bool : Bool -> String
show_bool b = if b then "True" else "False"

main : Unit -> <IO> Unit
main () =
  println (show_int (1 + 2 * 3 - 4))
  println (show_int (-2 * 3))
  println (show_int ((-7) / 2))
  println (show_int ((-7) % 2))
  println (show_bool (1 < 2 && 2 < 3))
  println (show_bool (False && noisy True))
  println (show_bool (True || noisy False))
  println (show_bool (not (1 == 2)))
  10 |> show_int |> println
```

- [ ] **Step 3: 実行時エラーになるプログラムを置く**

`tests/ui/run-fail/integer_overflow.em`:

```haskell
-- Int arithmetic is checked: overflow stops the program.
main : Unit -> <IO> Unit
main () =
  println "before"
  println (show_int (9223372036854775807 + 1))
```

`tests/ui/run-fail/division_by_zero.em`:

```haskell
-- Division by zero stops the program and names the function.
divide : Int -> Int -> Int
divide a b = a / b

main : Unit -> <IO> Unit
main () = println (show_int (divide 1 0))
```

- [ ] **Step 4: 診断を確かめるプログラムを置く**

各ファイルの1行目のコメントに、期待する診断の番号を書く。

`tests/ui/check-fail/undefined_name.em`:

```haskell
-- E1001: `shout` is not defined, and nothing else is reported.
main : Unit -> <IO> Unit
main () = println (shout "hi")
```

`tests/ui/check-fail/missing_signature.em`:

```haskell
-- E1004: every top-level definition needs a signature.
helper x = x

main : Unit -> <IO> Unit
main () = ()
```

`tests/ui/check-fail/missing_equation.em`:

```haskell
-- E1005: a signature without an equation.
helper : Int -> Int

main : Unit -> <IO> Unit
main () = ()
```

`tests/ui/check-fail/duplicate_definition.em`:

```haskell
-- E1003: the second signature of `twice`.
twice : Int -> Int
twice x = x + x

main : Unit -> <IO> Unit
main () = ()

twice : Int -> Int
```

`tests/ui/check-fail/non_associative.em`:

```haskell
-- E1006: comparisons do not associate.
main : Unit -> <IO> Unit
main () = if 1 < 2 < 3 then println "yes"
```

`tests/ui/check-fail/type_mismatch.em`:

```haskell
-- E2001 four times: an argument, an annotation, a condition, and a statement.
add : Int -> Int -> Int
add a b = a + b

main : Unit -> <IO> Unit
main () =
  println (add 1 2)
  let x : Int = "one"
  if x then println "x"
  x
  ()
```

`tests/ui/check-fail/invalid_main_type.em`:

```haskell
-- E2004: `main` must have type `Unit -> <IO> Unit`.
main : Int -> Int
main n = n
```

`tests/ui/check-fail/missing_io_row.em`:

```haskell
-- E2002: `greet` calls `println` but its signature has no `IO`.
greet : String -> Unit
greet name = println name

main : Unit -> <IO> Unit
main () = greet "eml"
```

`tests/ui/check-fail/not_yet_supported.em`:

```haskell
-- E0004: constructs of later stages in HIR and in the type checker.
data Color = | Red | Green

add : Int -> Int -> Int
add a b = a + b

main : Unit -> <IO> Unit
main () =
  let f = fn x -> x
  let g = add
  let h = add 1
  println "done"
```

- [ ] **Step 5: CLI の終了コードのテストを足す**

`crates/eml_cli/tests/cli.rs` の末尾に足す。

```rust
#[test]
fn runtime_errors_exit_with_one() {
    let output = eml(&["run", "--debug-heap", "run-fail/division_by_zero.em"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("runtime error: division by zero in `divide`")
    );
}

#[test]
fn run_prints_the_program_output() {
    let output = eml(&["run", "--debug-heap", "run/hello.em"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Hello, world!\n");
}
```

- [ ] **Step 6: スナップショットを作って確かめる**

Run: `cargo test -p eml_cli`

新しいスナップショットができる。`cargo insta review` で、次の内容と一致することを確かめてから承認する。

| ファイル | stdout | 結果 |
|---|---|---|
| `run/hello.em` | `Hello, world!` | 正常終了 |
| `run/factorial.em` | `3628800`、`2432902008176640000` | 正常終了 |
| `run/fibonacci.em` | `0` `1` `1` `2` `3` `5` `8` `13` `21` `34` (1行に1つ) | 正常終了 |
| `run/deep_recursion.em` | `100000` | 正常終了 |
| `run/strings_and_let.em` | `Hello, eml!`、`eml`、`emleml`、`-42` | 正常終了 |
| `run/operators.em` | `3`、`-6`、`-3`、`-1`、`True`、`False`、`True`、`True`、`10` (`evaluated` は出ない) | 正常終了 |
| `run-fail/integer_overflow.em` | `before` | `integer overflow in `main`` |
| `run-fail/division_by_zero.em` | (なし) | `division by zero in `divide`` |

`check-fail/` の新しいスナップショットは、各ファイルの1行目のコメントの番号の診断だけを含むこと (`not_yet_supported.em` は E0004 が4件: `data`、ラムダ、`add` を値として使う、`add 1` の部分適用。`type_mismatch.em` は E2001 が4件で、`x` の行は「最後でない文」の note を持つ)。それ以外の診断が出ていたら、連鎖を抑えられていないので実装を直す。

- [ ] **Step 7: 全体の確認とコミット**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`

```bash
git add tests/ui crates/eml_cli/tests
git commit -m "Add UI tests for the first vertical slice and run-fail tests"
```

---

### Task 12: 文書を実装に合わせる

段階1を終えたことを記録し、設計文書の内容のうち残す価値のあるものを `architecture.md` に移して、設計文書を削除する。

**Files:**
- Modify: `docs/implementation/status.md`、`docs/implementation/architecture.md`、`docs/implementation/testing.md`、`docs/spec/expressions.md`、`CLAUDE.md`
- Delete: `docs/superpowers/specs/2026-10-04-vertical-slice-1-design.md`

- [ ] **Step 1: status.md を更新する**

- 「名前解決以降の実装段階」の表で、段階1の状態を「完了」にする。表の後の「段階1の内部設計は ... にある」の文を削除する
- 「各 crate の実装状況」の表を次の内容にする

| crate | 状態 |
|---|---|
| `eml_diagnostics` | 実装済み。`Diagnostic`、`FileId` と `SourceFiles`、ariadne による表示 |
| `eml_syntax` | S1 まで実装済み。lexer、レイアウト段、パーサ、型付き AST ラッパ。入れ子の深さの上限 (E0013、フィールドアクセスの連鎖を含む) を持つ |
| `eml_hir` | 段階1まで実装済み。宣言の対応づけ、名前解決、演算子の列の組み直し、`&&` / `\|\|` / `\|>` / `<\|` の脱糖、E1001〜E1006 |
| `eml_types` | 段階1まで実装済み。型・row・Kind の表現と単一化、シグネチャに対する本体の検査、E2001〜E2005。線形性と網羅性の検査は未実装 |
| `eml_core_ir` | 段階1まで実装済み。ANF への変換、Perceus の `dup` / `decref` の挿入 |
| `eml_runtime` | 世代番号つきのヒープ、RC、記述子、`debug_heap` のリーク検出、`OutputSink` |
| `eml_interp` | 段階1まで実装済み。CEK 機械 (ヒープ上の継続のフレーム、最下部の `IO` の handler)、プリミティブ、`println` |
| `eml_cli` | 実装済み。`check` / `run` コマンド、lib API (`check` / `compile` / `execute`)、終了コード、UI テスト (`run/`、`run-fail/`、`check-fail/`) と CLI テスト |

- 「次の作業の注意点」から、`la-arena` の項目と、フィールドアクセスの連鎖の項目を削除する (どちらも済んだ)
- 「完了した作業」の表に行を足す: `| 縦の貫通 段階1 | 名前解決、型検査 (row と Kind の表現、省略した row は \`<>\`)、Core IR と Perceus、ヒープと RC、CEK インタプリタを、関数、\`Int\` / \`String\` / \`Bool\`、\`if\`、\`let\`、標準の演算子、\`println\` の範囲で通した。\`run-fail/\` の UI テストを足した |`

- [ ] **Step 2: architecture.md を更新する**

- 「各段階の規律」の「HIR の各ノードは、元の構文ノードへのポインタ (`SyntaxNodePtr`) を持つ」を次にする: 「HIR の各ノードは、元の構文の範囲 (`TextRange`) を持つ。演算子の列を組み直した部分式のように、対応する構文ノードのない式があるため」
- 各段階の入口の表の `eml_core_ir` の行を `lower(&Module, &TypedModule) -> Program` にする。表の直前の「`eml_syntax` 以外はまだ仮実装で、空の結果を返す」の文を削除する
- 「エラーが出ても止まらない」の最後に足す: 「Core IR は、診断のエラーがないプログラムだけを受け取る。`compile` は、エラーがあれば Core IR を作らない。型付きで正しい入力を前提にできるので、Core IR への変換は診断を返さない」
- 「`eml_hir` で行う脱糖と検査」の後に、次の3つの節を足す (設計文書から移す)

```markdown
## `eml_hir` の内部

- `Module` はトップレベルの関数の `Arena<Function>` を持つ。本体は関数ごとの `Body` (`exprs`、`pats`、`locals` のアリーナ) に置く。後でクエリ化したときに関数単位で再計算できるようにするため (rust-analyzer と同じ分け方)。型の注釈は関数ごとの `types` に置く
- シグネチャと等式は名前で対応づけてから、並び方を検査する。シグネチャか等式のない関数も `Function` として残し (`signature` か `body` が `None`)、呼び出し側で名前の誤りを連鎖させない
- 名前は `Res::{Local, Function, Builtin}` に解決する。組み込み (`eml_hir::builtin::Builtin`) は名前解決の最も外側のスコープで、ユーザーの定義で隠せる。S2 で `Prelude` モジュールに移す
- 演算子の列は、標準の演算子の表で precedence climbing により組み直す。`&&` / `||` は `if` に、`|>` / `<|` は関数適用に脱糖する。`else` のない `if` は `else_branch: None` のまま残し、型検査が `Unit` を求める

## `eml_types` の内部

- 型は検査器の中の表に置いて ID で引き、型変数の束縛を辿って単一化する。row は「ラベルの並び + 末尾の row 変数」で、scoped labels の書き換えで単一化する。Kind の変数と `≤` の制約は束の上で最小解を求める
- 各関数の本体は、シグネチャだけを見て検査する。引数を1つ消費するごとにシグネチャの矢印を1つたどり、本体の row は最後にたどった矢印の row になる。呼び出しでは、呼び出し先の閉じた row を新しい row 変数で開いてから本体の row と単一化する
- 結果の `TypedModule` は、HIR を複製せずに、関数ごとの `Type` (変数を解決した型) の別テーブルを持つ

## `eml_core_ir`、`eml_runtime`、`eml_interp` の内部

- Core IR の関数は、ANF の木をアリーナに置き、`CExprId` で参照する。継続のフレームが再開する位置を ID で持てるようにするため。値を返す入れ子の式は `Rhs::Nested` で、`if` はその中の `Switch` にする
- Perceus の挿入は、ANF の上の後ろ向きの生存解析で行う。関数、プリミティブ、`perform` の引数は、どれも所有権を受け取る
- ヒープはインデックス方式のアリーナで、スロットごとに世代番号を持つ。値は `Copy` な `Value` で、ヒープの値の読み出しはスロットを空にする move である
- CEK 機械の継続は、ヒープ上のフレームの連結リストである。呼び出しのフレームは環境 (スロットの配列) を退避し、入れ子の式のフレームは環境をそのまま使い続ける。最下部に `IO` の handler のフレームを置く
```

- 「外部 crate」の表の `la-arena` の行を `| \`la-arena\` | 0.3 | HIR の ID とアリーナ、型検査の別テーブル (\`ArenaMap\`) |` にする

- [ ] **Step 3: testing.md と expressions.md を更新する**

- testing.md の「全体 (UI テスト)」に関わる「UI テスト」の節に、`tests/ui/run-fail/*.em` が既に書かれていることを確かめる (brainstorming で追記済み)。「現在あるテストの置き場所」に次の行を足す: 「`crates/eml_hir/tests/`、`crates/eml_types/tests/`、`crates/eml_core_ir/tests/`、`crates/eml_interp/tests/`: 各段階の変換結果と診断のスナップショット、実行の結果。ランタイムのヒープの単体テストは `crates/eml_runtime/src/heap.rs` にある」
- expressions.md の「`if`」の節の「HIR で `else ()` を補う」を「`else ()` を補ったものとして扱う (HIR は `else` のない `if` として残し、型検査は then 節が `Unit` でなければ診断する)」にする

- [ ] **Step 4: CLAUDE.md を更新する**

`CLAUDE.md` の Architecture の最後の箇条書き「`eml_syntax` implements stage S1 ... The later stages (hir / types / core_ir / interp) are still stubs.」の最後の文を次に置き換える。

```markdown
The later stages implement step 1 of the vertical slices in `docs/implementation/status.md` (functions, `Int` / `String` / `Bool`, `if`, `let`, standard operators, `println`); constructs of later steps are reported as E0004 by HIR or the type checker.
```

- [ ] **Step 5: 設計文書を削除する**

```bash
git rm docs/superpowers/specs/2026-10-04-vertical-slice-1-design.md
```

- [ ] **Step 6: 全体の確認とコミット**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`

```bash
git add docs CLAUDE.md
git commit -m "Record the first vertical slice as done and move its design into architecture.md"
```
