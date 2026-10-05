# 縦の貫通 段階4b Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** タプル (式、型、パターン)、`Int` と `String` のリテラルのパターン、`Int`・`String`・`Bool` の `==` と `!=` を、HIR、型検査、網羅性の検査、Core IR、インタプリタの全体に通す。

**Architecture:** 上流から積む。HIR にタプルとリテラルのパターンを入れる (Task 1)。型検査でタプルを数字ラベルの閉じたレコードにし、リテラルのパターンを検査し、`==` の比べ方を決めて Core IR とインタプリタまで通す (Task 2)。網羅性の検査にタプルとリテラルを足す (Task 3)。Core IR でタプルの値を作り、決定木にタプルとリテラルの列を足す (Task 4)。文書は Task 5 で直す。

**Tech Stack:** Rust (edition 2024)、rowan 0.16、la-arena 0.3、insta 1.49。新しい外部 crate は足さない。

**Spec:** `docs/superpowers/specs/2026-10-05-stage-4b-tuples-literals-equality-design.md` (段階4b の設計)。規範は `docs/spec/` の `records.md`、`declarations.md`、`types.md`、`exhaustiveness.md`、`core-ir.md`、`diagnostics.md`。

## Global Constraints

- 作業は `main` から切ったブランチ `stage-4b` で行う。タスクごとにコミットする。コミットメッセージは英語で、末尾に次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_014YGQm6f7Mtged1yVY4dmJG
  ```

- 外部 crate は増やさない
- コードのコメントは日本語で、何をするかではなく理由を書く。規則を指すときは `docs/` のパスを書く。日本語を書く前に `yomiyasu:yomiyasu` スキルを読み、その規則に従う (CLAUDE.md)。`.em` のテストの先頭のコメントは、既存のテストと同じく英語で書く
- 各段階は `fn stage(&In) -> (Out, Vec<Diagnostic>)` の純粋関数のままにする
- 診断の help / note は eml 自身の規則の説明に限る。他の言語の書き方を前提にしたヒントは入れない
- インタプリタとランタイムの値とフレームに `Rc` と `RefCell` を使わない。`unsafe` を書かない
- 既存のテストで期待値を変えてよいのは、spec の6節の種類1の表にある3件だけである (`not-yet-supported/tuples.em` を `projections.em` に置き換える、`eml_hir/tests/lower.rs` の `constructs_of_later_stages_are_not_yet_supported`、`eml_types/tests/check.rs` の `later_stage_constructs_add_no_type_errors`)。3件とも Task 1 で変える。種類3 は期待値を変えずに追随する。表にないテストの期待値が変わったら、変えずに止まり、差分と理由をユーザーに示して承認を得る。設計を曲げてテストを守ることはしない
- 各タスクの最後に `cargo test`、`cargo clippy --all-targets`、`cargo fmt` を通す。clippy の警告を残さない
- インラインスナップショットと UI テストの期待値は、このプランのコードが出す形を書いてある。食い違ったら、まず実装がプランのコードと一致しているかを確かめる。プランの期待値の誤り (位置の数え違い、局所変数や Core IR の変数の番号など) だと判断した場合は、理由をコミットメッセージに書いてから直す。UI テストの stdout が食い違ったら、それは本物の食い違いとして原因を調べる。新しい UI テストのスナップショットは、ステップに書いた stdout と診断の番号と文言に一致することを確かめてから承認する
- 診断の位置の表記は、1 始まりの行と、文字数で数えた列 (`2:7`) である
- 長い連鎖をたどる処理はループで書く。Rust の再帰は、E0013 の入れ子の上限で深さが抑えられる木 (式、パターン、型、`Switch` の枝) に限る
- 途中のタスクでは、ワークスペース全体をビルドできる状態に保つ。Task 1 は、型検査でタプルの式・型・パターンとリテラルのパターンを `Error` の型にして E0004 を出す。Task 2 がこれを外す。Task 2 から Task 4 までは、タプルの値かリテラルのパターンを含むプログラムを `eml run` すると、Core IR の変換で止まる。この間に、そうしたプログラムを実行するテストはない。仮の扱いには「後で実装する」という趣旨のコメントを書かない

## Review Focus

- `String` のリテラルの `match` を何度も実行する。比べるたびに作る文字列と、比べる出現の複製が解放され、リークしない → Task 4 の `run/data/literal_patterns.em` と `perceus.rs` の `a_string_compared_twice_is_dupped_before_each_comparison`
- `==` の引数の型が、演算子より後で決まる (`let eq = fn x -> fn y -> x == y` を後で `String` に適用する)。本体の検査の後に決めるので `String` の比べ方になる → Task 2 の `tuples.rs` の `an_operand_type_decided_after_the_operator_is_used`
- タプルが multi-shot の handler を通る。節の引数のタプルのパターンで分解し、再開のたびに文字列を持つタプルを作って分解する → Task 4 の `run/data/tuples_with_effects.em`
- タプルの中のリテラルで等式の引数が反駁可能になる (`f (0, b) = 1`)。漏れの例は `f (_, _)` で、括弧を重ねない → Task 3 の `a_literal_inside_a_tuple_parameter_makes_the_equation_refutable`
- 範囲外の負の数のパターン (`-9223372036854775808`)。字句の E0007 を1件だけ出し、パターンは `Missing` にして診断を重ねない → Task 1 の `tuples.rs` の `a_literal_too_large_for_int_becomes_a_missing_pattern`

1つの列に異なるリテラルが非常に多い `match` で `Switch` の入れ子が深くなる問題は、どのタスクのテストでも確かめない。Task 5 で status.md に既知の制限として記録する。

---

### Task 1: HIR にタプルとリテラルのパターンを入れる

タプルの式・型・パターンと、`Int` (負の数を含む) と `String` のリテラルのパターンを HIR に通す。型検査はこれらをまだ扱わず、タプルの式とパターン、リテラルのパターンに E0004 を出して `Error` の型にする (Task 2 で外す)。Core IR の変換には、誤りのないプログラムでは届かない `unreachable!` を置く。

決めたことは次のとおり。

- **HIR の形**: タプルは `ExprKind::Tuple`、`PatKind::Tuple`、`TypeRefKind::Tuple` の専用の種類にし、数字ラベルのレコードへの変換は型検査で行う (spec の1節)。要素の数は、文法が2つ以上にする。
- **リテラルのパターン**: `PatKind::Literal(Literal)` で、`Literal::Int` (負の数を含む) か `Literal::String` を持つ。値が壊れたリテラル (範囲外の整数、閉じていない文字列) と文字のリテラルは、字句解析とパーサが報告済みなので `Missing` にする。`-9223372036854775808` は、式と同じく字句の段階で E0007 になり、パターンは `Missing` になる。
- **E1017 の組**: タプルのパターンの要素は、外側のパターンと同じ組で変換する。そのため `(x, x)` も、等式の引数の `f (x, x)` も E1017 になる (spec の「1つのパターン」)。
- **表示**: 式とパターンは `(a#0, b#1)`、型は `(Int, String)`、リテラルのパターンは `0`、`-1`、`"x"`。コンストラクタの引数の位置のリテラルは括弧で囲まない (`Some -1`)。文法の `apat` が `-` INT を受けるので、ソースと同じ形になる。

**Files:**
- Modify: `crates/eml_syntax/src/ast.rs` (`TupleExpr`、`TuplePat`、`TupleType`、`LiteralPat` の取り出し口)
- Modify: `crates/eml_hir/src/hir.rs`、`lower/expr.rs`、`lower/types.rs`、`lower/effect.rs`、`pretty.rs`
- Modify (仮の扱い): `crates/eml_types/src/check/body.rs`、`scheme.rs`、`exhaustive.rs`、`crates/eml_core_ir/src/translate/expr.rs`、`translate/pattern.rs`
- Modify (最終の形): `crates/eml_types/src/data.rs`、`check/mod.rs`、`usage.rs`
- Modify (種類1): `crates/eml_hir/tests/lower.rs` の `constructs_of_later_stages_are_not_yet_supported`、`crates/eml_types/tests/check.rs` の `later_stage_constructs_add_no_type_errors`
- Delete (種類1): `tests/ui/check-fail/not-yet-supported/tuples.em`、`crates/eml_cli/tests/snapshots/ui__check_fail@not-yet-supported__tuples.em.snap`
- Create: `tests/ui/check-fail/not-yet-supported/projections.em` とそのスナップショット
- Test: `crates/eml_syntax/tests/ast.rs`、`crates/eml_hir/tests/tuples.rs` (新規)

**Interfaces:**
- Produces (`eml_syntax::ast`):
  - `TupleExpr::elements(&self) -> AstChildren<Expr>`、`TuplePat::elements(&self) -> AstChildren<Pat>`、`TupleType::elements(&self) -> AstChildren<Type>`
  - `LiteralPat::value(&self) -> Option<LiteralValue>` (`-` があれば整数の符号を反転する)
- Produces (`eml_hir`):

  ```rust
  // ExprKind に追加
  Tuple(Vec<ExprId>),
  // PatKind に追加
  Tuple(Vec<PatId>),
  Literal(Literal),        // Literal::Int (負の数を含む) か Literal::String
  // TypeRefKind に追加
  Tuple(Vec<TypeRefId>),
  ```

  - `Body::walk_child_exprs`、`pat_bindings`、`captures` が `Tuple` をたどる
  - HIR の表示: 式とパターンは `(a#0, b#1)`、型は `(Int, String)`、リテラルのパターンは `0`、`-1`、`"x"`
- Produces (仮の扱い。Task 2 と Task 4 が置き換える):
  - `eml_types` の `check/body.rs`: タプルの式は要素を推論してから E0004 「tuples are not supported yet」(式の範囲) を出し、`Error` の型にする。タプルのパターンは E0004 「tuple patterns are not supported yet」、リテラルのパターンは E0004 「literal patterns are not supported yet」(どちらもパターンの範囲) を出し、パターンの型と中の局所変数を `Error` にする
  - `eml_types` の `scheme.rs`: `TypeRefKind::Tuple` は診断を出さずに `Error` の型
  - `eml_types` の `exhaustive.rs`: `PatKind::Tuple` と `PatKind::Literal` を含む行列は調べない
  - `eml_core_ir` の変換: `ExprKind::Tuple`、`PatKind::Tuple`、`PatKind::Literal` に `unreachable!`

**Interface notes (骨組みの Interfaces から変えた点と、前後のタスクとの取り決め):**

骨組みの Interfaces から、次の点を変えた、または足した。

1. spec の種類1の表の `crates/eml_types/tests/check.rs` の `later_stage_constructs_add_no_type_errors` は、骨組みでは Task 2 にあるが、このタスクで変える。HIR がタプルを `Missing` にしなくなると、タプルの E0004 を出すのが型検査に移る。そのため、このテストの出力 (診断の順と局所変数の番号) がこのタスクで変わるためである。変更の中身は spec の表のとおり (タプルを射影に置き換える) で、Task 2 では変えない。
2. 構文木の取り出し口として、`TupleExpr::elements`、`TuplePat::elements`、`TupleType::elements`、`LiteralPat::value` を足す。`-1` の符号は `LiteralPat::value` が付ける。
3. 型検査の仮の扱いのうち、型の注釈の `TypeRefKind::Tuple` は、診断を出さずに `Error` の型にする。`Error` の型の値を検査しても診断は出ないので、タプルの式とパターン (E0004 を出す) より先に誤りが連鎖することはない。`data.rs` の Kind に効く型引数の位置、`check/mod.rs` の `has_error`、`usage.rs` は、仮ではなく最終の形で入れる。Task 2 で変える必要はない。
4. HIR のテストは新しいファイル `crates/eml_hir/tests/tuples.rs` に置く。
5. spec の種類1の表のほかに、タプルとリテラルのパターンの E0004 を期待する既存のテストはない (`tuples are not supported yet`、`tuple patterns are not supported yet`、`literal patterns are not supported yet`、`tuple types are not supported yet` で `crates/` と `tests/` を探した結果、表の3件だけだった)。

- [ ] **Step 1: ブランチを切る**

```bash
git switch -c stage-4b
```

- [ ] **Step 2: 構文木の取り出し口の失敗するテストを書く**

`crates/eml_syntax/tests/ast.rs` の先頭の `use` を次のようにする (`LiteralValue` を足す)。

```rust
use eml_syntax::ast::{
    AppExpr, Clause, Expr, Item, LiteralValue, OpSeqElement, Pat, SourceFile, Stmt, Type,
};
```

ファイルの末尾に次のテストを足す。

```rust
#[test]
fn tuple_and_literal_pattern_parts() {
    let file = source(
        "f : (Int, String) -> Int\nf (n, -1) = match (n, \"s\") with | (0, \"t\") -> 1 | _ -> 2",
    );
    let Some(Item::Signature(signature)) = file.items().next() else {
        panic!("expected a signature");
    };
    let Some(Type::FnType(function)) = signature.ty() else {
        panic!("expected a function type");
    };
    let Some(Type::TupleType(tuple)) = function.param() else {
        panic!("expected a tuple type");
    };
    let elements: Vec<SyntaxKind> = tuple.elements().map(|ty| ty.syntax().kind()).collect();
    assert_eq!(elements, [PATH_TYPE, PATH_TYPE]);
    let literal = |pat: Pat| match pat {
        Pat::LiteralPat(literal) => literal.value(),
        _ => None,
    };
    let equation = first_equation(&file);
    let Some(Pat::TuplePat(params)) = equation.params().next() else {
        panic!("expected a tuple pattern");
    };
    let values: Vec<Option<LiteralValue>> = params.elements().map(literal).collect();
    assert_eq!(values, [None, Some(LiteralValue::Int(-1))]);
    let Some(Expr::MatchExpr(expr)) = equation.body() else {
        panic!("expected a match");
    };
    let Some(Expr::TupleExpr(scrutinee)) = expr.scrutinee() else {
        panic!("expected a tuple");
    };
    let elements: Vec<SyntaxKind> = scrutinee.elements().map(|e| e.syntax().kind()).collect();
    assert_eq!(elements, [PATH_EXPR, LITERAL]);
    let Some(Pat::TuplePat(arm)) = expr.arms().next().and_then(|arm| arm.pat()) else {
        panic!("expected a tuple pattern in the first arm");
    };
    let values: Vec<Option<LiteralValue>> = arm.elements().map(literal).collect();
    assert_eq!(
        values,
        [
            Some(LiteralValue::Int(0)),
            Some(LiteralValue::String("t".to_string()))
        ]
    );
}
```

- [ ] **Step 3: テストが失敗することを確かめる**

Run: `cargo test -p eml_syntax --test ast tuple_and_literal_pattern_parts`
Expected: コンパイルエラー (`no method named `elements` found for struct `TupleType``、`no method named `value` found for struct `LiteralPat`` など)

- [ ] **Step 4: 取り出し口を足す**

`crates/eml_syntax/src/ast.rs` の `impl AppType` の後に次を足す。

```rust
impl TupleExpr {
    /// 要素の式。文法が2つ以上にする (docs/spec/grammar.md の `atom`)。
    pub fn elements(&self) -> AstChildren<Expr> {
        support::children(&self.syntax)
    }
}

impl TuplePat {
    pub fn elements(&self) -> AstChildren<Pat> {
        support::children(&self.syntax)
    }
}

impl TupleType {
    pub fn elements(&self) -> AstChildren<Type> {
        support::children(&self.syntax)
    }
}

impl LiteralPat {
    /// `-1` の `-` は字句の一部ではなくパターンの一部なので (docs/spec/grammar.md の `apat`)、ここで符号を付ける。
    /// 値の壊れたリテラルと未対応のリテラル (文字) は `None` を返す。どれも字句解析かパーサが報告済みである。
    pub fn value(&self) -> Option<LiteralValue> {
        let negative = support::token(&self.syntax, SyntaxKind::MINUS).is_some();
        let token = self
            .syntax
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .find(|token| matches!(token.kind(), SyntaxKind::INT | SyntaxKind::STRING))?;
        match token.kind() {
            SyntaxKind::INT => {
                let n = crate::literal::int_value(token.text())?;
                Some(LiteralValue::Int(if negative { -n } else { n }))
            }
            _ => crate::literal::decode_string(token.text()).map(LiteralValue::String),
        }
    }
}
```

`int_value` は `i64::MAX` 以下の値だけを返すので、符号の反転はあふれない。

- [ ] **Step 5: 構文木のテストが通ることを確かめる**

Run: `cargo test -p eml_syntax --test ast`
Expected: PASS

- [ ] **Step 6: HIR の失敗するテストを書く**

`crates/eml_hir/tests/tuples.rs` を作る。

```rust
//! タプルの式・型・パターンと、リテラルのパターンの変換。

mod common;

use common::{diagnostics, lower_text};
use eml_hir::{Body, ExprKind, LocalId, Module, Stmt};

#[test]
fn tuples_in_expressions_types_and_patterns() {
    let text = "swap : (Int, String) -> (String, Int)\nswap (n, s) = (s, n)\n\nnested : Int -> ((Int, Int), String)\nnested x =\n  let (a, b) = (x, x)\n  let f = fn (p, q) -> (q, p)\n  match f (a, b) with\n    | (c, d) -> ((c, d), \"x\")";
    insta::assert_snapshot!(lower_text(text), @r#"
    swap : (Int, String) -> (String, Int)
    swap (n#0, s#1) = (s#1, n#0)
    nested : Int -> ((Int, Int), String)
    nested x#0 = {
      let (a#1, b#2) = (x#0, x#0)
      let f#5 = (fn (p#3, q#4) -> (q#4, p#3))
      (match (f#5 (a#1, b#2)) with | (c#6, d#7) -> ((c#6, d#7), "x"))
    }
    "#);
}

#[test]
fn literal_patterns_include_negative_numbers_and_strings() {
    let text = "data Option a = | None | Some a\n\ndescribe : Int -> String\ndescribe n = match n with\n  | 0 -> \"zero\"\n  | -1 -> \"minus one\"\n  | _ -> \"other\"\n\ngreet : String -> Int\ngreet s = match s with\n  | \"hi\" -> 1\n  | \"\" -> 0\n  | other -> 2\n\nnested : Option Int -> Int\nnested o = match o with | Some -1 -> 1 | _ -> 0";
    insta::assert_snapshot!(lower_text(text), @r#"
    data Option a
      | None
      | Some a
    describe : Int -> String
    describe n#0 = (match n#0 with | 0 -> "zero" | -1 -> "minus one" | _ -> "other")
    greet : String -> Int
    greet s#0 = (match s#0 with | "hi" -> 1 | "" -> 0 | other#1 -> 2)
    nested : Option Int -> Int
    nested o#0 = (match o#0 with | Some -1 -> 1 | _ -> 0)
    "#);
}

#[test]
fn a_literal_too_large_for_int_becomes_a_missing_pattern() {
    // `-` の後の数が `Int` に収まらなければ、式と同じく字句の段階の E0007 だけを出す
    let text = "f : Int -> Int\nf n = match n with | -9223372036854775808 -> 0 | _ -> 1";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> Int
    f n#0 = (match n#0 with | <missing> -> 0 | _ -> 1)
    ---
    E0007 2:23 integer literal `9223372036854775808` is too large
    ");
}

#[test]
fn a_name_is_bound_once_per_tuple_pattern() {
    // タプルの要素は外側のパターンと同じ組なので、要素どうしの重複も E1017 になる
    let text = "f : (Int, Int) -> Int\nf (x, x) = x\n\ng : (Int, Int) -> Int\ng p =\n  let (y, (z, y)) = (p, p)\n  z";
    assert_eq!(
        diagnostics(text),
        [
            "E1017 2:7 `x` is bound more than once",
            "E1017 6:15 `y` is bound more than once",
        ]
    );
}

fn body<'m>(module: &'m Module, name: &str) -> &'m Body {
    module
        .functions
        .iter()
        .map(|(_, function)| function)
        .find(|function| function.name == name)
        .and_then(|function| function.body.as_ref())
        .expect("the body")
}

#[test]
fn walkers_see_through_tuples() {
    let text = "f : Int -> Int -> (Int, Int)\nf a b =\n  let ((x, _), y) = ((a, a), b)\n  (fn u -> (x, u)) y";
    let module = eml_test_support::lower_clean(text).module;
    let body = body(&module, "f");
    let names = |locals: Vec<LocalId>| -> Vec<String> {
        locals
            .iter()
            .map(|&local| body.locals[local].name.clone())
            .collect()
    };
    let (lambda, _) = body
        .exprs
        .iter()
        .find(|(_, expr)| matches!(expr.kind, ExprKind::Lambda { .. }))
        .expect("the lambda");
    // タプルの中の `x` を捕まえ、引数の `u` は捕まえない
    assert_eq!(names(body.lambda_captures(lambda)), ["x"]);
    let ExprKind::Block { stmts, .. } = &body.exprs[body.root].kind else {
        panic!("expected a block body");
    };
    let Stmt::Let { pat, .. } = &stmts[0] else {
        panic!("expected a `let`");
    };
    assert_eq!(names(body.pat_bindings(*pat)), ["x", "y"]);
    for (id, expr) in body.exprs.iter() {
        if let ExprKind::Tuple(elements) = &expr.kind {
            let mut children = Vec::new();
            body.walk_child_exprs(id, |child| children.push(child));
            assert_eq!(&children, elements);
        }
    }
}
```

`crates/eml_hir/tests/lower.rs` の `constructs_of_later_stages_are_not_yet_supported` を次に置き換える (種類1)。タプルとリテラルのパターンが E0004 でなくなるので、後の段階の構文の E0004 を、射影と `let ... in` で確かめ続ける。

```rust
#[test]
fn constructs_of_later_stages_are_not_yet_supported() {
    let text = "f : Int -> Int\nf x =\n  let first = fn t -> t.0\n  let plus = (+)\n  let y = x in y";
    insta::assert_snapshot!(lower_text(text), @"
    f : Int -> Int
    f x#0 = {
      let first#2 = (fn t#1 -> <missing>)
      let plus#3 = <missing>
      <missing>
    }
    ---
    E0004 3:23 field access is not supported yet
    E0004 4:14 operator references are not supported yet
    E0004 5:3 `let ... in` is not supported yet
    ");
}
```

- [ ] **Step 7: テストが失敗することを確かめる**

Run: `cargo test -p eml_hir --test tuples --test lower`
Expected: コンパイルエラー (`no variant or associated item named `Tuple` found for enum `ExprKind``)。`lower.rs` の置き換えたテストは、入力がどちらも今の E0004 の構文なので、実装の前でも通る。

- [ ] **Step 8: HIR の型と走査関数を変える**

`crates/eml_hir/src/hir.rs` の `ExprKind` の `Match` の後に次を足す。

```rust
    /// 要素は2つ以上である。数字ラベルのレコードへの変換は型検査で行う (docs/spec/records.md)。
    Tuple(Vec<ExprId>),
```

`PatKind` の `Con` の後に次を足す。

```rust
    /// 要素は2つ以上である。
    Tuple(Vec<PatId>),
    /// `Int` (負の数を含む) と `String` のリテラル。`()` は `Unit` である。
    Literal(Literal),
```

`TypeRefKind` の `Fn` の後に次を足す。

```rust
    /// 要素は2つ以上である。数字ラベルの閉じたレコードへの変換は型検査で行う (docs/spec/records.md)。
    Tuple(Vec<TypeRefId>),
```

`Body::walk_child_exprs` の `ExprKind::Drop(value) => f(*value),` の前に次を足す。

```rust
            ExprKind::Tuple(elements) => {
                for &element in elements {
                    f(element);
                }
            }
```

`Body::collect_bindings` を次にする。

```rust
    fn collect_bindings(&self, pat: PatId, out: &mut Vec<LocalId>) {
        match &self.pats[pat].kind {
            PatKind::Bind(local) => out.push(*local),
            PatKind::Annot { pat, .. } => self.collect_bindings(*pat, out),
            PatKind::Con { args, .. } | PatKind::Tuple(args) => {
                for &arg in args {
                    self.collect_bindings(arg, out);
                }
            }
            PatKind::Missing | PatKind::Wildcard | PatKind::Unit | PatKind::Literal(_) => {}
        }
    }
```

`Body::captures` は `walk_child_exprs` と `pat_bindings` を使うので、変えなくてよい。

- [ ] **Step 9: タプルとリテラルのパターンを変換する**

`crates/eml_hir/src/lower/expr.rs` の `lower_expr` の `ast::Expr::TupleExpr(_) => self.unsupported(range, "tuples are not supported yet"),` を次にする。

```rust
            ast::Expr::TupleExpr(tuple) => {
                let elements = tuple
                    .elements()
                    .map(|element| {
                        let element_range = element.range();
                        self.lower_expr(Some(element), element_range)
                    })
                    .collect();
                self.alloc(ExprKind::Tuple(elements), range)
            }
```

`lower_pat_in_group` の `ast::Pat::LiteralPat(_)` と `ast::Pat::TuplePat(_)` の腕を次にする。

```rust
            ast::Pat::LiteralPat(literal) => match literal.value() {
                Some(ast::LiteralValue::Int(n)) => PatKind::Literal(Literal::Int(n)),
                Some(ast::LiteralValue::String(s)) => PatKind::Literal(Literal::String(s)),
                // 範囲外の整数、壊れた文字列、文字のリテラルは、字句解析とパーサが報告済み
                None => PatKind::Missing,
            },
            // 要素は外側のパターンと同じ組で変換する。`(x, x)` も1つのパターンの中の重複である (E1017)
            ast::Pat::TuplePat(tuple) => {
                let elements = tuple
                    .elements()
                    .map(|element| {
                        let element_range = element.range();
                        self.lower_pat_in_group(Some(element), element_range)
                    })
                    .collect();
                PatKind::Tuple(elements)
            }
```

`crates/eml_hir/src/lower/types.rs` の `ast::Type::TupleType(_) => self.unsupported(range, "tuple types are not supported yet"),` を次にする。

```rust
            ast::Type::TupleType(tuple) => TypeRefKind::Tuple(
                tuple
                    .elements()
                    .map(|element| {
                        let element_range = element.range();
                        self.lower(Some(element), element_range)
                    })
                    .collect(),
            ),
```

`crates/eml_hir/src/lower/effect.rs` の `never` の操作の結果の型の検査で、`TypeRefKind::Con(..) | TypeRefKind::Fn { .. } => false,` を次にする。

```rust
            TypeRefKind::Con(..) | TypeRefKind::Fn { .. } | TypeRefKind::Tuple(_) => false,
```

同じファイルの `mentions` の `TypeRefKind::Con(_, args) => ...` の後に次を足す。

```rust
        TypeRefKind::Tuple(elements) => elements
            .iter()
            .any(|&element| mentions(types, element, var)),
```

- [ ] **Step 10: HIR の表示を足す**

`crates/eml_hir/src/pretty.rs` の `expr` の `ExprKind::Drop(value) => ...` の前に次を足す。

```rust
            ExprKind::Tuple(elements) => {
                let elements: Vec<String> = elements
                    .iter()
                    .map(|&element| self.expr(body, element, indent))
                    .collect();
                format!("({})", elements.join(", "))
            }
```

`pat` の `PatKind::Annot { pat, ty } => ...` の前に次を足す。

```rust
            PatKind::Tuple(elements) => {
                let elements: Vec<String> = elements
                    .iter()
                    .map(|&element| self.pat(body, element))
                    .collect();
                format!("({})", elements.join(", "))
            }
            PatKind::Literal(Literal::Int(n)) => n.to_string(),
            PatKind::Literal(Literal::String(s)) => format!("{s:?}"),
            PatKind::Literal(Literal::Unit) => "()".to_string(),
```

`ty` の `TypeRefKind::Var(id) => ...` の前に次を足す。

```rust
            TypeRefKind::Tuple(elements) => {
                let elements: Vec<String> = elements
                    .iter()
                    .map(|&element| self.ty(types, element))
                    .collect();
                format!("({})", elements.join(", "))
            }
```

`ty_atom` と `ty_operand` と `pat_atom` は変えない。タプルは自分で括弧を持つので、括弧を重ねない。`pretty.rs` の `use` に `Literal` がなければ足す。

- [ ] **Step 11: 型検査と Core IR を新しい形に追随させる**

型検査の仮の扱い (Task 2 で外す) と、Task 2 以降も変えない最終の形を入れる。仮の扱いには「後で実装する」という趣旨のコメントを書かない。

`crates/eml_types/src/check/body.rs` の `infer_expr` の `ExprKind::Drop(value) => ...` の前に次を足す。`use` に `Diagnostic` はすでにある。

```rust
            ExprKind::Tuple(elements) => {
                for &element in elements {
                    self.infer_expr(element);
                }
                self.diagnostics.push(Diagnostic::not_yet_supported(
                    self.file(),
                    expr.range,
                    "tuples are not supported yet",
                ));
                self.table.error
            }
```

同じファイルの `bind_pat` の `PatKind::Wildcard | PatKind::Missing => {}` の前に次を足す。

```rust
            PatKind::Tuple(_) | PatKind::Literal(_) => {
                let message = match &body.pats[pat].kind {
                    PatKind::Tuple(_) => "tuple patterns are not supported yet",
                    _ => "literal patterns are not supported yet",
                };
                self.diagnostics.push(Diagnostic::not_yet_supported(
                    self.file(),
                    body.pats[pat].range,
                    message,
                ));
                // 網羅性の検査と使用回数のパスに誤りを連鎖させないよう、パターンと中の変数を `Error` にする
                let error = self.table.error;
                self.typing.pats.insert(pat, error);
                for local in body.pat_bindings(pat) {
                    self.typing.locals.insert(local, error);
                }
            }
```

`crates/eml_types/src/scheme.rs` の `lower` の `TypeRefKind::Var(var) => rigids.tys[*var],` の前に次を足す。

```rust
        // 型検査はタプルの式とパターンを未対応として報告する。型の注釈のタプルは `Error` にして、診断を重ねない
        TypeRefKind::Tuple(_) => table.error,
```

`crates/eml_types/src/data.rs` の `collect` の `TypeRefKind::Fn { .. } | TypeRefKind::Error => {}` の前に次を足す (最終の形)。

```rust
        // タプルの Kind は要素の Kind の join なので、要素に書いた型引数はすべて効く (docs/spec/records.md の「Kind」)
        TypeRefKind::Tuple(elements) => {
            for &element in elements {
                collect(types, element, effective, out);
            }
        }
```

`crates/eml_types/src/check/mod.rs` の `has_error` の `TypeRefKind::Var(_) => false,` の前に次を足す (最終の形)。

```rust
        TypeRefKind::Tuple(elements) => elements.iter().any(|&element| has_error(types, element)),
```

`crates/eml_types/src/usage.rs` の `expr` の `ExprKind::Drop(value) => self.expr(*value),` の前に次を足す (最終の形)。

```rust
            ExprKind::Tuple(elements) => {
                let mut uses = Uses::new();
                for &element in elements {
                    let next = self.expr(element);
                    sequence(&mut uses, next);
                }
                uses
            }
```

同じファイルの `check_pat` の `PatKind::Con { args, .. } => { ... }` を `PatKind::Con { args, .. } | PatKind::Tuple(args) => { ... }` にし、最後の腕を `PatKind::Unit | PatKind::Missing | PatKind::Literal(_) => {}` にする。リテラルのパターンは値を束縛せず、比べる値 (`Int` と `String`) はどれも `Unr` なので、制約は要らない。

`crates/eml_types/src/exhaustive.rs` の `pat` の `PatKind::Con { ctor, args } => ...` の後に次を足す。

```rust
            // 型検査がまだ未対応として報告し、型を `Error` にしている
            PatKind::Tuple(_) | PatKind::Literal(_) => None,
```

`crates/eml_core_ir/src/translate/expr.rs` の `atom` の `ExprKind::Drop(value) => { ... }` の前に次を足す。

```rust
            ExprKind::Tuple(_) => {
                unreachable!("the type checker reports tuples as not yet supported")
            }
```

`crates/eml_core_ir/src/translate/pattern.rs` の `head` のループの `PatKind::Wildcard | PatKind::Unit | PatKind::Missing => return Head::Any(None),` の前に次を足す。

```rust
            PatKind::Tuple(_) | PatKind::Literal(_) => {
                unreachable!("the type checker reports tuple and literal patterns as not yet supported")
            }
```

同じファイルの `has_constructor` の最後の腕の前に次を足す。

```rust
        PatKind::Tuple(_) | PatKind::Literal(_) => {
            unreachable!("the type checker reports tuple and literal patterns as not yet supported")
        }
```

`crates/eml_types/tests/check.rs` の `later_stage_constructs_add_no_type_errors` を次に置き換える (種類1)。HIR がタプルを `Missing` にしなくなり、このテストの出力が変わるためである。E0004 の跡を型検査に通しても誤りを重ねないことを、射影で確かめ続ける。

```rust
#[test]
fn later_stage_constructs_add_no_type_errors() {
    // E0004 の跡 (`<missing>` の値、`from` の handle) を型検査に通しても、誤りを重ねて出さない。
    let text = "counter : Unit -> Int\ncounter () = 0\n\nf : Int -> Int\nf n =\n  let first = fn t -> t.0\n  let plus = (+)\n  handle counter () from n with\n    | return x st -> x";
    insta::assert_snapshot!(check_text(text), @"
    counter : Unit -> Int
    f : Int -> Int
      n#0 : Int
      t#1 : _
      first#2 : _ -> <_> {error}
      plus#3 : {error}
    ---
    E0004 6:23 field access is not supported yet
      6:23 this is implemented in a later stage
    E0004 7:14 operator references are not supported yet
      7:14 this is implemented in a later stage
    E0004 8:21 handlers with `from` are not supported yet
      8:21 this is implemented in a later stage
    ");
}
```

- [ ] **Step 12: HIR と型検査のテストが通ることを確かめる**

Run: `cargo test -p eml_syntax --test ast && cargo test -p eml_hir && cargo test -p eml_types`
Expected: PASS。`eml_hir` の `tuples.rs` は5件。

- [ ] **Step 13: UI テストを置き換える**

`tests/ui/check-fail/not-yet-supported/tuples.em` とそのスナップショットを削除する (種類1)。

```bash
git rm tests/ui/check-fail/not-yet-supported/tuples.em crates/eml_cli/tests/snapshots/ui__check_fail@not-yet-supported__tuples.em.snap
```

`not-yet-supported/` に E0004 の UI テストを残すため、`tests/ui/check-fail/not-yet-supported/projections.em` を作る。

```haskell
-- E0004: projections, which come with named records in stage S2.
first : Int -> Int
first t = t.0

main : Unit -> <IO> Unit
main () = println "done"
```

Run: `cargo test -p eml_cli --test ui`
Expected: 新しいスナップショットが1件未承認で FAIL する。`.snap.new` が次の形 (番号、文言、位置) であることを確かめてから、`cargo insta accept` で承認する。罫線の細部は ariadne の出力に従う。

```
[E0004] Error: field access is not supported yet
   ╭─[ check-fail/not-yet-supported/projections.em:3:11 ]
   │
 3 │ first t = t.0
   │           ─┬─  
   │            ╰─── this is implemented in a later stage
───╯
```

承認した後にもう一度 `cargo test -p eml_cli --test ui` を実行し、PASS を確かめる。

- [ ] **Step 14: 全体を確かめる**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`
Expected: すべて PASS し、clippy の警告がない。期待値の変わった既存のテストが、種類1の3件 (`lower.rs`、`check.rs`、`tuples.em` の置き換え) のほかにないこと。

- [ ] **Step 15: コミットする**

```bash
git add crates/eml_syntax crates/eml_hir crates/eml_types crates/eml_core_ir tests/ui/check-fail/not-yet-supported crates/eml_cli/tests/snapshots
git commit -m "$(cat <<'EOF'
Lower tuples and literal patterns into HIR

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014YGQm6f7Mtged1yVY4dmJG
EOF
)"
```

### Task 2: タプルとリテラルのパターンを型検査し、`==` の比べ方を決める

タプルの式・パターン・型の注釈を、数字ラベルの閉じたレコード `Record([("0", A), ("1", B)])` で型検査し、`(A, B)` と表示する。`Int` と `String` のリテラルのパターンを検査する。`==` と `!=` のシグネチャを `a -> a -> Bool` にし、本体の検査が終わった後に引数の型から比べ方 (`Int`、`String`、`Bool`) を決める。決まらなければ E2006 にする。Core IR は決まった比べ方から `prim` を選び、インタプリタは文字列とタグの比較を実行する。Task 1 が型検査に入れた仮の扱い (タプルの式とパターン、リテラルのパターンの E0004 と、型の注釈のタプルの `Error`) は外す。タプルの値とリテラルのパターンの Core IR への変換は Task 4 で行う。網羅性の検査は Task 3 で行う。

**Files:**
- Create: `crates/eml_types/src/check/equality.rs`
- Modify: `crates/eml_types/src/lib.rs` (codes、`Equality`、`BodyTypes::equalities`)、`ty.rs` (タプルの表示)、`table/mod.rs` (`Table::tuple`)、`scheme.rs` (`TypeRefKind::Tuple`)、`check/mod.rs` (`mod equality`、呼び出し、書き出し)、`check/body.rs`、`check/report.rs`
- Modify (Task 1 の仮の扱いの文言だけ): `crates/eml_types/src/exhaustive.rs`、`crates/eml_core_ir/src/translate/pattern.rs`
- Modify: `crates/eml_hir/src/prelude.em`
- Modify: `crates/eml_core_ir/src/lib.rs` (`PrimOp`)、`translate/types.rs`、`translate/expr.rs`、`translate/program.rs`
- Modify: `crates/eml_interp/src/lib.rs`
- Test: `crates/eml_types/tests/tuples.rs` (新規)、`crates/eml_core_ir/tests/translate.rs`、`tests/ui/run/data/equality.em`、`tests/ui/check-fail/types/not_comparable.em`

**Interfaces:**
- Consumes: Task 1 の HIR (`ExprKind::Tuple(Vec<ExprId>)`、`PatKind::Tuple(Vec<PatId>)`、`PatKind::Literal(Literal)`、`TypeRefKind::Tuple(Vec<TypeRefId>)`)
- Produces:

  ```rust
  // eml_types
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum Equality { Int, String, Bool }
  // BodyTypes に追加。キーは `==` / `!=` を指す呼ばれる側の Path の式
  pub equalities: ArenaMap<ExprId, Equality>,
  pub mod codes { pub const NOT_COMPARABLE: ErrorCode = ErrorCode(2006); }

  // eml_core_ir
  PrimOp::{StrEq, StrNe, BoolEq, BoolNe}   // 表示は "string==" "string!=" "bool==" "bool!="
  ```

  - タプルの型は `Type::Record(vec![("0", A), ("1", B)])` で、表示は `(A, B)` である。要素が2つ以上で、ラベルが 0 から連番のレコードだけをタプルとして表示する
  - Core IR の `==` / `!=` は、`Equality::Int` なら `IntEq` / `IntNe` (表示は今の `==` / `!=`)、`String` なら `StrEq` / `StrNe`、`Bool` なら `BoolEq` / `BoolNe` になる
  - `Table::tuple(&mut self, elements: Vec<Ty>) -> Ty` (crate の中)

**Interface notes (骨組みの Interfaces から変えた点と、前後のタスクとの取り決め):**

- `==` と `!=` の組み込みの名前は `Builtin::IntEq` と `Builtin::IntNe` のままにする (spec の1節の「等値」)。型は `a -> a -> Bool` になるので名前と中身がずれるが、名前を変えると HIR のテストと表の行に手が入る。改名は型クラスを入れるときにまとめて行う
- 型検査は `==` / `!=` の参照を `BodyCheck::comparisons` (新しい `check/equality.rs` の `Comparison`) に記録し、本体の検査の直後に `BodyCheck::resolve_equalities` で比べ方を決める。結果は `BodyTyping::equalities` を経て `BodyTypes::equalities` に入る
- 型の表に `Table::tuple(Vec<Ty>) -> Ty` を足す。タプルの式、パターン、型の注釈の3か所が同じ形のレコードを作るため
- `Origin` に `TuplePattern(usize)` と `LiteralPattern` を足す
- Core IR の `Lowering` に `Equality { negated: bool }` を足し、`translate/types.rs` に `equality_op(Equality, negated) -> PrimOp` を置く。`call_builtin` は呼ばれる側の式の ID (`callee: ExprId`) を受けるようになる
- Task 1 (`plan-task-1.md`) との取り決め: spec の種類1の `later_stage_constructs_add_no_type_errors` は Task 1 が書き換える (HIR がタプルを `Missing` にしなくなった時点で出力が変わるため)。Task 2 はこのテストを変えない。Task 1 が最終の形で入れた `data.rs` の `TypeRefKind::Tuple`、`check/mod.rs` の `has_error`、`usage.rs` のタプルとリテラルのパターンも変えない。Task 2 が外すのは、Task 1 の仮の扱いのうち型検査の3か所 (`check/body.rs` の `ExprKind::Tuple` の腕、`bind_pat` の `PatKind::Tuple(_) | PatKind::Literal(_)` の腕、`scheme.rs` の `TypeRefKind::Tuple(_) => table.error`) だけである。Task 1 が Core IR (`translate/expr.rs`、`translate/pattern.rs`) と網羅性の検査 (`exhaustive.rs`) に置いた仮の扱いは、Task 4 と Task 3 が置き換えるまで残す。ただし、その `unreachable!` の文言とコメントは「型検査が未対応として報告する」と書いてあり、Task 2 の後は事実と合わなくなるので、文言だけを今の状態に合わせる (Step 6)
- Task 3 への申し送り: このタスクのテストは、リテラルのパターンには必ずワイルドカードの枝を置き、タプルのパターンは反駁不可能な形だけを使う。網羅性の検査がタプルとリテラルを調べるようになっても (Task 3)、このタスクの期待値は変わらない

- [ ] **Step 1: 失敗する型検査のテストを書く**

`crates/eml_types/tests/tuples.rs` を作る。リテラルのパターンには必ずワイルドカードの枝を置き、タプルのパターンは反駁不可能な形だけを使う。網羅性の検査 (Task 3) が後でタプルとリテラルを調べるようになっても、期待値が変わらないようにするためである。

```rust
//! タプル、リテラルのパターン、`==` の比べ方の型検査 (docs/spec/records.md、docs/spec/declarations.md の標準の
//! 演算子の表)。

mod common;

use common::check_text;
use eml_hir::{ExprKind, Res};
use eml_types::Equality;

/// 関数 `name` の本体で決まった `==` / `!=` の比べ方を、ソースの順に並べる。
fn decided(text: &str, name: &str) -> Vec<(&'static str, Equality)> {
    let checked = eml_test_support::check(text);
    assert!(
        checked.diagnostics.is_empty(),
        "{}",
        eml_test_support::short_text(&checked.files, &checked.diagnostics)
    );
    let (id, function) = checked
        .module
        .functions
        .iter()
        .find(|(_, function)| function.name == name)
        .unwrap();
    let body = function.body.as_ref().unwrap();
    let mut found: Vec<(u32, &'static str, Equality)> = checked.typed.bodies[id]
        .equalities
        .iter()
        .map(|(expr, &equality)| {
            let ExprKind::Path(Res::Builtin(builtin)) = &body.exprs[expr].kind else {
                panic!("equalities are keyed by the operator");
            };
            (
                u32::from(body.exprs[expr].range.start()),
                builtin.name(),
                equality,
            )
        })
        .collect();
    found.sort_by_key(|&(start, _, _)| start);
    found
        .into_iter()
        .map(|(_, name, equality)| (name, equality))
        .collect()
}

#[test]
fn tuples_are_closed_records_shown_with_parentheses() {
    // `both` は `x` を2回使うので、タプルの Kind (要素の Kind の join) を通して `a` に `Unr` の制約が付く
    let text = "swap : (Int, String) -> (String, Int)\nswap p = match p with\n  | (n, s) -> (s, n)\n\nnested : ((Int, Bool), String) -> Int\nnested t =\n  let ((n, _), _) = t\n  n\n\nboth : a -> (a, a)\nboth x = (x, x)";
    insta::assert_snapshot!(check_text(text), @r"
    swap : (Int, String) -> (String, Int)
      p#0 : (Int, String)
      n#1 : Int
      s#2 : String
    nested : ((Int, Bool), String) -> Int
      t#0 : ((Int, Bool), String)
      n#1 : Int
    both : a -> (a, a)
      kinds: a <= Unr
      x#0 : a
    ");
}

#[test]
fn a_tuple_pattern_with_another_number_of_elements_is_a_mismatch() {
    let text = "first : (Int, Int) -> Int\nfirst p = match p with\n  | (a, b, c) -> a";
    insta::assert_snapshot!(check_text(text), @r"
    first : (Int, Int) -> Int
      p#0 : (Int, Int)
      a#1 : {error}
      b#2 : {error}
      c#3 : {error}
    ---
    E2001 3:5 mismatched types
      3:5 expected `(Int, Int)`, found `(_, _, _)`
      note: this pattern matches a tuple of 3 elements
    ");
}

#[test]
fn a_literal_pattern_of_another_type_is_a_mismatch() {
    let text = "name : Int -> String\nname n = match n with\n  | \"zero\" -> \"zero\"\n  | -1 -> \"minus one\"\n  | _ -> \"other\"";
    insta::assert_snapshot!(check_text(text), @r#"
    name : Int -> String
      n#0 : Int
    ---
    E2001 3:5 mismatched types
      3:5 expected `Int`, found `String`
      note: a literal pattern matches only values of the type of the literal
    "#);
}

#[test]
fn the_operand_type_decides_how_equality_compares() {
    let text = "same : Int -> String -> Bool -> Bool\nsame n s b = n == 1 && s != \"x\" && b == True";
    assert_eq!(
        decided(text, "same"),
        [
            ("==", Equality::Int),
            ("!=", Equality::String),
            ("==", Equality::Bool),
        ]
    );
}

#[test]
fn an_operand_type_decided_after_the_operator_is_used() {
    // ラムダの引数の型は、`eq` を呼んだ後で `String` に決まる。比べ方は本体の検査が終わってから決める
    let text = "later : Unit -> Bool\nlater () =\n  let eq = fn x -> fn y -> x == y\n  eq \"a\" \"b\"";
    assert_eq!(decided(text, "later"), [("==", Equality::String)]);
}

#[test]
fn only_int_string_and_bool_can_be_compared() {
    let text = "data Color =\n  | Red\n  | Green\n\ncolors : Color -> Color -> Bool\ncolors a b = a == b\n\npairs : (Int, Int) -> Bool\npairs p = p != (1, 2)\n\nfuns : (Int -> Int) -> Bool\nfuns f = f == (fn n -> n)\n\npoly : a -> a -> Bool\npoly x y = x == y";
    insta::assert_snapshot!(check_text(text), @r"
    colors : Color -> Color -> Bool
      a#0 : Color
      b#1 : Color
    pairs : (Int, Int) -> Bool
      p#0 : (Int, Int)
    funs : (Int -> Int) -> Bool
      f#0 : Int -> Int
      n#1 : Int
    poly : a -> a -> Bool
      x#0 : a
      y#1 : a
    ---
    E2006 6:16 values of type `Color` cannot be compared with `==`
      6:16 `==` cannot compare `Color`
      note: `==` and `!=` compare only values of type `Int`, `String` and `Bool`
    E2006 9:13 values of type `(Int, Int)` cannot be compared with `!=`
      9:13 `!=` cannot compare `(Int, Int)`
      note: `==` and `!=` compare only values of type `Int`, `String` and `Bool`
    E2006 12:12 values of type `Int -> Int` cannot be compared with `==`
      12:12 `==` cannot compare `Int -> Int`
      note: `==` and `!=` compare only values of type `Int`, `String` and `Bool`
    E2006 15:14 values of type `a` cannot be compared with `==`
      15:14 `==` cannot compare `a`
      note: `==` and `!=` compare only values of type `Int`, `String` and `Bool`
    ");
}

#[test]
fn undecided_operands_are_reported_and_errors_are_not() {
    // `same` の引数の型はどこでも決まらない。`missing` の型は報告済みの誤りの跡 (`Error`) なので、E2006 を重ねない
    let text = "undecided : Unit -> Bool\nundecided () =\n  let same = fn x -> fn y -> x == y\n  True\n\nbroken : Int -> Bool\nbroken n = missing == n";
    insta::assert_snapshot!(check_text(text), @r"
    undecided : Unit -> Bool
      x#0 : _
      y#1 : _
      same#2 : _ -> <_> _ -> <_> Bool
    broken : Int -> Bool
      n#0 : Int
    ---
    E1001 7:12 cannot find value `missing`
      7:12 not found in this scope
    E2006 3:32 values of type `_` cannot be compared with `==`
      3:32 `==` cannot compare `_`
      note: `==` and `!=` compare only values of type `Int`, `String` and `Bool`
    ");
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --test tuples`
Expected: コンパイルエラー (`eml_types::Equality` がない)

- [ ] **Step 3: タプルとリテラルのパターンを型の表に写す**

`crates/eml_types/src/table/mod.rs` の `impl Table` (`fresh_var` の近く) に足す。

```rust
    /// タプルの型。0 から始まる数字ラベルの閉じたレコードである (docs/spec/records.md)。式、パターン、型の注釈の
    /// どれも同じ形を作る。
    pub fn tuple(&mut self, elements: Vec<Ty>) -> Ty {
        let fields = elements
            .into_iter()
            .enumerate()
            .map(|(index, ty)| (index.to_string(), ty))
            .collect();
        self.alloc(TyShape::Record(fields))
    }
```

`crates/eml_types/src/scheme.rs` の `lower` で、Task 1 が入れた仮の腕とそのコメントを外す。

```rust
        // 型検査はタプルの式とパターンを未対応として報告する。型の注釈のタプルは `Error` にして、診断を重ねない
        TypeRefKind::Tuple(_) => table.error,
```

同じ位置 (`TypeRefKind::Var(var) => rigids.tys[*var],` の前) に、次の腕を置く。

```rust
        TypeRefKind::Tuple(elements) => {
            let mut lowered = Vec::new();
            for &element in elements {
                lowered.push(lower(table, types, rigids, element, arrows.inner()));
            }
            table.tuple(lowered)
        }
```

`data.rs` の `TypeRefKind::Tuple`、`check/mod.rs` の `has_error`、`usage.rs` は Task 1 が最終の形で入れてあるので、変えない。

- [ ] **Step 4: タプルを `(A, B)` と表示する**

`crates/eml_types/src/ty.rs` の `impl fmt::Display for Type` で、`Type::Record(fields) if fields.is_empty()` の腕の直後に次の腕を足す。

```rust
            // ラベルが 0 から連番の閉じたレコードは、タプルの書き方で表示する (docs/spec/records.md の「構成」)
            Type::Record(fields) if is_tuple(fields) => {
                f.write_str("(")?;
                for (index, (_, ty)) in fields.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{ty}")?;
                }
                f.write_str(")")
            }
```

`row_text` の前に関数を足す。

```rust
/// タプルとして書くレコード。タプルの構文は要素を2つ以上持つので、要素が1つのレコードは `{ 0 : A }` のまま書く。
fn is_tuple(fields: &[(String, Type)]) -> bool {
    fields.len() >= 2
        && fields
            .iter()
            .enumerate()
            .all(|(index, (label, _))| *label == index.to_string())
}
```

`atomic` は変えない。タプルは自分で括弧を書くので、型の適用の引数 (`Option (Int, Int)`) でも関数型の引数 (`(Int, Int) -> Int`) でも括弧を重ねない。

- [ ] **Step 5: 診断の由来を足す**

`crates/eml_types/src/check/report.rs` の `Origin` の `ConstructorPattern` の後に足す。

```rust
    /// タプルのパターン。パターンの要素の数を持つ。
    TuplePattern(usize),
    /// `Int` か `String` のリテラルのパターン。
    LiteralPattern,
```

`mismatch` の `match origin` の `Origin::ConstructorPattern` の腕の後に足す。

```rust
            Origin::TuplePattern(elements) => diagnostic.with_note(format!(
                "this pattern matches a tuple of {elements} elements"
            )),
            Origin::LiteralPattern => diagnostic
                .with_note("a literal pattern matches only values of the type of the literal"),
```

- [ ] **Step 6: タプルの式とパターン、リテラルのパターンを検査する**

`crates/eml_types/src/check/body.rs` で、Task 1 が入れた2つの仮の腕を外す。1つは `infer_expr` の `ExprKind::Tuple(elements) => { ... }` で、要素を推論してから E0004「tuples are not supported yet」を出し、`self.table.error` を返す腕である。もう1つは `bind_pat` の `PatKind::Tuple(_) | PatKind::Literal(_) => { ... }` で、E0004「tuple patterns are not supported yet」か「literal patterns are not supported yet」を出し、パターンと中の局所変数を `Error` にする腕である。`Diagnostic` の `use` はほかの関数でも使うので残す。

`infer_expr` の `match &expr.kind` で、外した腕の位置に次の腕を置く。

```rust
            ExprKind::Tuple(elements) => {
                let fields = elements
                    .iter()
                    .map(|&element| self.infer_expr(element))
                    .collect();
                self.table.tuple(fields)
            }
```

`bind_pat` の `match` で、外した腕の位置に次の2つの腕を置く。

```rust
            PatKind::Tuple(elements) => self.tuple_pattern(pat, elements, ty),
            PatKind::Literal(literal) => self.literal_pattern(pat, literal, ty),
```

`constructor_pattern` の後に2つの関数を足す。

```rust
    /// タプルのパターン。期待する型を、要素の数だけの新しい変数でできたタプルと単一化してから、要素のパターンを検査
    /// する。要素の数が違えば、型の合わないコンストラクタのパターンと同じく、中を検査しない。
    fn tuple_pattern(&mut self, pat: PatId, elements: &[PatId], expected: Ty) {
        let range = self.body.pats[pat].range;
        let fields: Vec<Ty> = elements.iter().map(|_| self.table.fresh_var()).collect();
        let tuple = self.table.tuple(fields.clone());
        let unified = self.with_kind_origin(range, KindReason::Unified, |this| {
            this.table.unify(tuple, expected)
        });
        let fields = if unified.is_ok() {
            fields
        } else {
            self.mismatch(
                range,
                expected,
                tuple,
                &Origin::TuplePattern(elements.len()),
            );
            // 網羅性の検査は、`Error` の型のパターンを含む `match` を飛ばす (docs/spec/exhaustiveness.md)
            let error = self.table.error;
            self.typing.pats.insert(pat, error);
            vec![error; elements.len()]
        };
        for (&element, field) in elements.iter().zip(fields) {
            self.bind_pat(element, field);
        }
    }

    /// `Int` と `String` のリテラルのパターン。リテラルの型を期待する型と単一化する。
    fn literal_pattern(&mut self, pat: PatId, literal: &Literal, expected: Ty) {
        let range = self.body.pats[pat].range;
        let found = match literal {
            Literal::Int(_) => self.table.int,
            Literal::String(_) => self.table.string,
            Literal::Unit => self.table.unit,
        };
        let unified = self.with_kind_origin(range, KindReason::Unified, |this| {
            this.table.unify(found, expected)
        });
        if unified.is_err() {
            self.mismatch(range, expected, found, &Origin::LiteralPattern);
            // 網羅性の検査は、`Error` の型のパターンを含む `match` を飛ばす (docs/spec/exhaustiveness.md)
            let error = self.table.error;
            self.typing.pats.insert(pat, error);
        }
    }
```

Task 1 が網羅性の検査と Core IR に置いた仮の扱いは、Task 3 と Task 4 が置き換えるまで残す。ただし、その文言は「型検査が未対応として報告する」と書いてあり、このタスクの後は事実と合わなくなるので、今の状態に合わせる。

`crates/eml_types/src/exhaustive.rs` の `pat` の仮の腕のコメントを次にする。

```rust
            // タプルとリテラルの列は、まだ調べない。行列ごと飛ばす
            PatKind::Tuple(_) | PatKind::Literal(_) => None,
```

`crates/eml_core_ir/src/translate/expr.rs` の `atom` の `ExprKind::Tuple(_)` の腕の文言を次にする。

```rust
            ExprKind::Tuple(_) => {
                unreachable!("the translation does not handle tuple values")
            }
```

`crates/eml_core_ir/src/translate/pattern.rs` の `head` と `has_constructor` の `PatKind::Tuple(_) | PatKind::Literal(_)` の腕の文言を、どちらも `unreachable!("the decision tree does not handle tuple and literal patterns")` にする。この間 (Task 2 から Task 4 まで) に、タプルの値かリテラルのパターンを含むプログラムを実行するテストはない。

- [ ] **Step 7: `==` の比べ方を決める**

`crates/eml_types/src/lib.rs` の `codes` の `INFINITE_TYPE` の後に足す。

```rust
    pub const NOT_COMPARABLE: ErrorCode = ErrorCode(2006);
```

`BodyTypes` の後に型を足し、`BodyTypes` にフィールドを足す。

```rust
/// `==` と `!=` の比べ方。型クラスがないので、型検査が引数の型から決め、比べられる型を限る
/// (docs/spec/declarations.md の標準の演算子の表)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Equality {
    Int,
    String,
    Bool,
}
```

```rust
    /// `==` と `!=` の比べ方。キーは演算子を指す呼ばれる側の式である。Core IR が、どの比べる命令にするかを決めるのに
    /// 使う。
    pub equalities: ArenaMap<ExprId, Equality>,
```

`crates/eml_types/src/check/equality.rs` を作る。

```rust
//! `==` と `!=` の比べ方を、引数の型から決める (docs/spec/declarations.md の標準の演算子の表)。型クラスがないので、
//! 比べられる型を `Int`、`String`、`Bool` に限る。

use eml_diagnostics::{Diagnostic, Label};
use eml_hir::ExprId;
use eml_hir::builtin::Builtin;

use crate::table::{Ty, TyShape};
use crate::{Equality, Type, codes};

use super::body::BodyCheck;

/// `==` か `!=` の参照。引数の型は、後の文の単一化で決まることがある (`let` で束縛したラムダの引数など)。そのため、
/// 参照の位置では記録だけをし、本体の検査が終わってから比べ方を決める。
pub(super) struct Comparison {
    /// 演算子を指す呼ばれる側の式。
    pub callee: ExprId,
    pub operator: Builtin,
    /// 参照を具体化した型。最初の矢印の引数が比べる値の型である。
    pub ty: Ty,
}

impl BodyCheck<'_> {
    pub(super) fn resolve_equalities(&mut self) {
        let lang = self.module.lang;
        for comparison in std::mem::take(&mut self.comparisons) {
            // Prelude のシグネチャがなければ参照の型は `Error` で、矢印を持たない
            let TyShape::Fn { param, .. } = self.table.shape(comparison.ty).clone() else {
                continue;
            };
            let equality = match self.table.shape(param) {
                TyShape::Con(id, _) if *id == lang.int => Some(Equality::Int),
                TyShape::Con(id, _) if *id == lang.string => Some(Equality::String),
                TyShape::Con(id, _) if *id == lang.bool => Some(Equality::Bool),
                _ => None,
            };
            if let Some(equality) = equality {
                self.typing.equalities.insert(comparison.callee, equality);
                continue;
            }
            let operand = self.table.display(param);
            // 報告済みの誤りの跡には診断を重ねない (docs/spec/diagnostics.md の「連鎖する診断の抑止」)
            if operand.contains_error() {
                continue;
            }
            let diagnostic = self.not_comparable(&comparison, &operand);
            self.diagnostics.push(diagnostic);
        }
    }

    /// 比べられない型の値を比べた (E2006)。演算子を指す。
    fn not_comparable(&self, comparison: &Comparison, operand: &Type) -> Diagnostic {
        let op = comparison.operator.name();
        Diagnostic::error(
            codes::NOT_COMPARABLE,
            format!("values of type `{operand}` cannot be compared with `{op}`"),
            Label::new(
                self.file(),
                self.body.exprs[comparison.callee].range,
                format!("`{op}` cannot compare `{operand}`"),
            ),
        )
        .with_note("`==` and `!=` compare only values of type `Int`, `String` and `Bool`")
    }
}
```

`crates/eml_types/src/check/body.rs` を直す。

1. `use` に `use super::equality::Comparison;` を足す。`Equality` は `crate::Equality` として `BodyTyping` で使う。
2. `BodyTyping` にフィールドを足す。

   ```rust
       /// `==` と `!=` の比べ方。`resolve_equalities` が埋める。
       pub equalities: ArenaMap<ExprId, crate::Equality>,
   ```

3. `BodyCheck` に、`typing` の前でフィールドを足す。

   ```rust
       /// 比べ方をまだ決めていない `==` と `!=` の参照。本体の検査が終わってから `resolve_equalities` が決める。
       pub(super) comparisons: Vec<Comparison>,
   ```

4. `infer_expr` の `ExprKind::Path(res) => self.value(*res, expr.range),` の腕を次の形にする。

   ```rust
            ExprKind::Path(res) => {
                let ty = self.value(*res, expr.range);
                if let Res::Builtin(operator @ (Builtin::IntEq | Builtin::IntNe)) = *res {
                    self.comparisons.push(Comparison {
                        callee: id,
                        operator,
                        ty,
                    });
                }
                ty
            }
   ```

`crates/eml_types/src/check/mod.rs` を直す。

1. `mod body;` の後に `mod equality;` を足す。
2. `BodyCheck { .. }` の組み立てに、`typing: BodyTyping::default(),` の前で `comparisons: Vec::new(),` を足す。
3. `checker.check_function(signature);` の直後に `checker.resolve_equalities();` を足す。
4. 本体の型を書き出すループ (`for (id, typing) in bodies`) で、`for (pat, &ty) in typing.pats.iter()` のループの後に `types.equalities = typing.equalities;` を足す。

`crates/eml_hir/src/prelude.em` の `(==)` と `(!=)` の行を次のようにする。

```haskell
-- `==` と `!=` で比べられるのは `Int`、`String`、`Bool` で、どれで比べるかは型検査が引数の型から決める
(==) : a -> a -> Bool
(!=) : a -> a -> Bool
```

- [ ] **Step 8: 型検査のテストが通ることを確かめる**

Run: `cargo test -p eml_types --test tuples`
Expected: 7件 PASS

Run: `cargo test -p eml_types`
Expected: PASS。`check.rs` の `later_stage_constructs_add_no_type_errors` (Task 1 が書き換えた形) と、`ping` / `pong` のテスト (`ping x n = if n == 0 ...`) の期待値が変わらないことを確かめる。`==` を `a -> a -> Bool` にしても、`n` の型が `Int` なので Kind の制約は増えない。

- [ ] **Step 9: 失敗する Core IR のテストを書く**

`crates/eml_core_ir/tests/translate.rs` の末尾に足す。

```rust
#[test]
fn equality_picks_the_comparison_of_the_operand_type() {
    // `Int` の `==` は今までどおり `prim ==` のまま表示し、`String` と `Bool` は型を前に付けた名前で表示する
    let text = "same : String -> Bool\nsame s = \"a\" == s\n\nflip : Bool -> Bool\nflip b = b != True\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
    fn same(s0) {
      let s1 = const "a"
      let t2 = prim string==(s1, s0)
      return t2
    }
    fn flip(b0) {
      let t1 = prim bool!=(b0, #1)
      return t1
    }
    fn main(p0) {
      return ()
    }
    fn entry$main() {
      tailcall main(())
    }
    "#);
}
```

Run: `cargo test -p eml_core_ir --test translate equality_picks_the_comparison_of_the_operand_type`
Expected: FAIL (`prim ==(s1, s0)` と `prim !=(b0, #1)` が出る。今の変換は `==` をつねに `IntEq` にするため)

- [ ] **Step 10: Core IR とインタプリタで比べ方を選ぶ**

`crates/eml_core_ir/src/lib.rs` の `PrimOp` の `StrConcat` の後に足す。

```rust
    StrEq,
    StrNe,
    BoolEq,
    BoolNe,
```

`PrimOp::name` の `PrimOp::StrConcat => "++",` の後に足す。`IntEq` と `IntNe` は今の `==` と `!=` のままにし、既存の Core IR のスナップショットを変えない。

```rust
            PrimOp::StrEq => "string==",
            PrimOp::StrNe => "string!=",
            PrimOp::BoolEq => "bool==",
            PrimOp::BoolNe => "bool!=",
```

`crates/eml_core_ir/src/translate/types.rs` を直す。

1. `use eml_types::{Linearity, Type};` を `use eml_types::{Equality, Linearity, Type};` にする。
2. `Lowering` に腕を足す。

   ```rust
       /// `==` と `!=`。比べ方は型検査が引数の型から決め、`BodyTypes::equalities` に入れてある
       /// (docs/spec/declarations.md の標準の演算子の表)。
       Equality {
           negated: bool,
       },
   ```

3. `lowering` の `Builtin::IntEq` と `Builtin::IntNe` の腕を次にする。

   ```rust
           Builtin::IntEq => Lowering::Equality { negated: false },
           Builtin::IntNe => Lowering::Equality { negated: true },
   ```

4. ファイルの末尾に関数を足す。

   ```rust
   /// 型検査が決めた比べ方の命令。
   pub(super) fn equality_op(equality: Equality, negated: bool) -> PrimOp {
       match (equality, negated) {
           (Equality::Int, false) => PrimOp::IntEq,
           (Equality::Int, true) => PrimOp::IntNe,
           (Equality::String, false) => PrimOp::StrEq,
           (Equality::String, true) => PrimOp::StrNe,
           (Equality::Bool, false) => PrimOp::BoolEq,
           (Equality::Bool, true) => PrimOp::BoolNe,
       }
   }
   ```

`crates/eml_core_ir/src/translate/expr.rs` を直す。

1. `use super::types::{Lowering, lowering, split_arrows};` を `use super::types::{Lowering, equality_op, lowering, split_arrows};` にする。
2. `call_builtin` の引数に、`builtin` の後で `callee: ExprId` を足す。`Lowering` の `match` に腕を足す。

   ```rust
            Lowering::Equality { negated } => {
                let equality = self
                    .types
                    .equalities
                    .get(callee)
                    .copied()
                    .expect("the type checker decides how every `==` and `!=` compares");
                Rhs::Prim(equality_op(equality, negated), args)
            }
   ```

3. `atom` の `ExprKind::Call` の中の `ExprKind::Path(Res::Builtin(builtin))` の腕の呼び出しを `self.call_builtin(*builtin, *callee, &callee_ty, args, &ty, out)` にする。

`crates/eml_core_ir/src/translate/program.rs` の `wrapper` の `match lowering(builtin)` に腕を足す。

```rust
            // `==` と `!=` は演算子の構文からしか書けず、2つの引数がそろって呼ばれる。演算子の参照 `(==)` とセクションは
            // まだ E0004 なので (docs/spec/expressions.md)、値として包む関数は作らない
            Lowering::Equality { .. } => {
                unreachable!("`==` and `!=` are always called with both operands")
            }
```

`crates/eml_interp/src/lib.rs` の `prim` の `match op` の `PrimOp::StrConcat` の腕の後に足す。

```rust
            // プリミティブは引数の所有権を受け取るので、比べた後に両方の文字列を手放す (`take_string`)
            PrimOp::StrEq | PrimOp::StrNe => {
                let left = self.take_string(args[0])?;
                let right = self.take_string(args[1])?;
                tag((left == right) == (op == PrimOp::StrEq))
            }
            PrimOp::BoolEq | PrimOp::BoolNe => {
                let (Value::Tag(left), Value::Tag(right)) = (args[0], args[1]) else {
                    return Err(Fault::Internal("a `Bool` comparison on a value that is not a tag"));
                };
                tag((left == right) == (op == PrimOp::BoolEq))
            }
```

- [ ] **Step 11: Core IR のテストが通ることを確かめる**

Run: `cargo test -p eml_core_ir --test translate`
Expected: PASS。既存の `prim ==(n0, 0)` のスナップショットは変わらない

- [ ] **Step 12: UI テストを足す**

`tests/ui/run/data/equality.em` を作る。

```haskell
-- `==` and `!=` on `Int`, `String` and `Bool`: the type checker picks how to compare from the operand type.
yes_no : Bool -> String
yes_no b = if b then "yes" else "no"

main : Unit -> <IO> Unit
main () =
  let name = "eml"
  println (yes_no (1 + 1 == 2))
  println (yes_no (name == "eml"))
  println (yes_no (name != "eml"))
  println (yes_no ("a" ++ "b" == "ab"))
  println (yes_no (True == False))
  println (yes_no ((1 == 2) == False))
```

stdout:

```
yes
yes
no
yes
no
yes
```

`name` は3回使うので、Perceus が `dup` し、`string==` と `string!=` が比べた後に手放す。`debug_heap` が有効なので、文字列が残ればリークで失敗する。

`tests/ui/check-fail/types/not_comparable.em` を作る。

```haskell
-- E2006: `==` and `!=` compare only `Int`, `String` and `Bool`.
data Color =
  | Red
  | Green

same : Color -> Color -> Bool
same a b = a == b

pair : (Int, Int) -> Bool
pair p = p != (1, 2)

main : Unit -> <IO> Unit
main () = println "done"
```

Run: `cargo test -p eml_cli --test ui`
Expected: 新しい2件のスナップショットが未承認で FAIL する。`.snap.new` を開いて次を確かめてから `cargo insta accept` で承認する。罫線の細部は ariadne の出力に従う。

- `run/data/equality.em`: stdout が上のとおりで、stderr が空である
- `check-fail/types/not_comparable.em`: E2006 が2件ある。1件目は `7:14` の `==` を指し、メッセージは「values of type `Color` cannot be compared with `==`」、ラベルは「`==` cannot compare `Color`」である。2件目は `10:12` の `!=` を指し、メッセージは「values of type `(Int, Int)` cannot be compared with `!=`」である。どちらにも note「`==` and `!=` compare only values of type `Int`, `String` and `Bool`」が付く

承認した後にもう一度 `cargo test -p eml_cli --test ui` を実行し、PASS を確かめる。

- [ ] **Step 13: 全体を確かめる**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`
Expected: すべて PASS し、clippy の警告がない。既存のテストの期待値は変わらない。`cargo fmt` の後に差分が出たら、その差分も含めてコミットする

- [ ] **Step 14: コミットする**

```bash
git add crates/eml_types crates/eml_hir/src/prelude.em crates/eml_core_ir crates/eml_interp \
  tests/ui/run/data/equality.em tests/ui/check-fail/types/not_comparable.em crates/eml_cli/tests/snapshots
git commit -m "$(cat <<'EOF'
Type-check tuples and literal patterns, and decide how == compares

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014YGQm6f7Mtged1yVY4dmJG
EOF
)"
```

### Task 3: 網羅性の検査にタプルとリテラルを足す

タプルを、要素の数だけのフィールドを持つコンストラクタが1つの型として扱い、`Int` と `String` のリテラルを、引数のないコンストラクタが無限にある型として扱う (spec の3節、docs/spec/exhaustiveness.md)。Task 1 の仮の扱い (タプルとリテラルを含む行列を調べない) をここで置き換える。

**Files:**
- Modify: `crates/eml_types/src/exhaustive.rs`
- Test: `crates/eml_types/tests/exhaustive.rs`、`tests/ui/check-fail/exhaustiveness/literal_patterns.em` (新規) とそのスナップショット

**Interfaces:**
- Consumes: Task 1 の `PatKind::Tuple(Vec<PatId>)`、`PatKind::Literal(Literal)` (`Literal::Int` は負の数を含む)。Task 2 のタプルの型 (`Type::Record`) と、タプルとリテラルのパターンの型検査 (型の誤りのあるパターンは `Error` の型になる)
- Produces: 漏れの例の書き方。タプルは `(Some _, False)`、リテラルの列の漏れは `_`、等式の引数のタプルは `f (_, _)`。外から見える名前は変わらない

**Interface notes (骨組みの Interfaces から変えた点と、前後のタスクとの取り決め):**

- Task 1 の仮の扱いは、`exhaustive.rs` の `Exhaustive::pat` で `PatKind::Tuple(_) | PatKind::Literal(_)` を `None` にして、その行列の検査をやめる形だと前提にする。違う形で入っていたら、このタスクの `pat` に置き換える。
- 外から見える名前は増やさない。`exhaustive.rs` の中で、パターンの頭を `Ctor` (`Data`、`Tuple`、`Literal`) の1つの enum にまとめ、`Pat::Con(Ctor, Vec<Pat>)` で表す。タプルとリテラルを、それぞれコンストラクタの一種として既存の usefulness と漏れの例の作り方に乗せるためである。
- 漏れの例にリテラルは現れない。リテラルの列はつねにそろわないので、その位置には `_` を置く。そのため `show` のリテラルの腕は `unreachable!` にする。

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/exhaustive.rs` の末尾に、次の10件を足す。`use` に変更は要らない。

```rust
#[test]
fn a_tuple_match_reports_a_missing_tuple() {
    let text = "data Option a = | None | Some a\n\nf : (Option Int, Bool) -> Int\nf p = match p with\n  | (Some n, True) -> n\n  | (None, _) -> 0";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4001 4:7 `match` does not cover every value
      4:7 no arm matches some values
      note: not covered: `(Some _, False)`
    ");
}

#[test]
fn a_tuple_inside_a_constructor_reports_the_whole_example() {
    let text = "data Option a = | None | Some a\n\nf : Option (Int, Int) -> Int\nf o = match o with\n  | Some (0, y) -> y\n  | None -> 0";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4001 4:7 `match` does not cover every value
      4:7 no arm matches some values
      note: not covered: `Some (_, _)`
    ");
}

#[test]
fn int_literals_alone_never_cover_every_value() {
    let text = "f : Int -> String\nf n = match n with\n  | 0 -> \"zero\"\n  | 1 -> \"one\"";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4001 2:7 `match` does not cover every value
      2:7 no arm matches some values
      note: not covered: `_`
    ");
}

#[test]
fn string_literals_alone_never_cover_every_value() {
    let text = "f : String -> Int\nf s = match s with\n  | \"a\" -> 1\n  | \"b\" -> 2";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4001 2:7 `match` does not cover every value
      2:7 no arm matches some values
      note: not covered: `_`
    ");
}

#[test]
fn a_repeated_literal_is_unreachable() {
    let text = "f : Int -> Int\nf n = match n with\n  | 0 -> 1\n  | 0 -> 2\n  | _ -> 3";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4004 4:5 unreachable `match` arm
      4:5 the arms above already match every value of this pattern
    ");
}

#[test]
fn negative_and_positive_literals_are_different_values() {
    // `-1` と `1` は別の値なので、2つ目の `-1` だけが到達しない
    let text = "f : Int -> Int\nf n = match n with\n  | -1 -> 0\n  | 1 -> 1\n  | -1 -> 2\n  | _ -> 3";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4004 5:5 unreachable `match` arm
      5:5 the arms above already match every value of this pattern
    ");
}

#[test]
fn literals_with_a_wildcard_cover_every_value() {
    let text = "f : String -> Int\nf s = match s with\n  | \"yes\" -> 1\n  | _ -> 0";
    assert_eq!(diagnostics(text), "");
}

#[test]
fn tuple_patterns_are_irrefutable() {
    let text = "swap : (Int, String) -> (String, Int)\nswap p =\n  let (a, b) = p\n  (b, a)\n\nfirst : (Int, Int) -> Int\nfirst (x, _) = x\n\npick : (Int, Int) -> Int\npick p = (fn (a, b) -> a + b) p";
    assert_eq!(diagnostics(text), "");
}

#[test]
fn a_literal_inside_a_tuple_makes_the_let_refutable() {
    let text = "f : (Int, Int) -> Int\nf p =\n  let (0, y) = p\n  y";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4003 3:7 this pattern does not match every value
      3:7 `let` needs a pattern that matches every value
      note: not covered: `(_, _)`
    ");
}

#[test]
fn a_literal_inside_a_tuple_parameter_makes_the_equation_refutable() {
    let text = "f : (Int, Bool) -> Int\nf (0, b) = 1";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4002 1:1 the equation of `f` does not cover every argument
      1:1 `f` is not defined for some arguments
      2:1 this equation does not match every argument
      note: not covered: `f (_, _)`
    ");
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --test exhaustive`
Expected: コンパイルは通る。`literals_with_a_wildcard_cover_every_value` と `tuple_patterns_are_irrefutable` 以外の新しい8件が、Task 1 の仮の扱いで E4xxx の診断が出ないので FAIL する。既存の14件は PASS する。

- [ ] **Step 3: パターンの頭に `Ctor` を入れる**

`crates/eml_types/src/exhaustive.rs` の `use` の `eml_hir` の並びに `Literal` を足す。

```rust
use eml_hir::{
    Body, ConstructorId, ExprId, ExprKind, Function, Literal, MatchArm, Module, PatId, PatKind,
    Stmt, TypeDefId, TypeDefKind,
};
```

`Pat` と `Column` を次のものに置き換え、`Ctor` を足す。

```rust
/// 検査に使うパターン。変数、`_`、`()` はどれも何にでも合うので区別しない。漏れの例もこの形で作る。
#[derive(Debug, Clone)]
enum Pat {
    Wild,
    Con(Ctor, Vec<Pat>),
}

/// パターンの頭。タプルは要素の数だけのフィールドを持つコンストラクタが1つの型として、`Int` と `String` の
/// リテラルは引数のないコンストラクタが無限にある型として扱う (docs/spec/exhaustiveness.md)。どれも同じ
/// usefulness の手続きに乗せるため、コンストラクタの一種にする。
#[derive(Debug, Clone, PartialEq)]
enum Ctor {
    Data(ConstructorId),
    Tuple(usize),
    Literal(Literal),
}

type Row = Vec<Pat>;

/// 1つの列に別の型のコンストラクタが混ざっている。型の誤りを E2001 で報告済みなので、その検査をやめる。
struct Mixed;

/// 行列の先頭の列に現れるコンストラクタ。
enum Column<'a> {
    /// どの行も先頭が `_` である。
    Wild,
    Data {
        seen: Vec<ConstructorId>,
        all: &'a [ConstructorId],
    },
    /// タプルのコンストラクタは1つなので、現れればつねにそろっている。
    Tuple(usize),
    /// リテラルの値は無限にあるので、どれだけ現れてもそろわない。
    Literal,
}
```

`pat` の `match` を次のようにする (Task 1 が入れた `PatKind::Tuple` と `PatKind::Literal` の腕を置き換える)。

```rust
        match &self.body.pats[id].kind {
            PatKind::Missing => None,
            PatKind::Bind(_) | PatKind::Wildcard | PatKind::Unit => Some(Pat::Wild),
            PatKind::Annot { pat, .. } => self.pat(*pat),
            PatKind::Con { ctor, args } => Some(Pat::Con(Ctor::Data(*ctor), self.row(args)?)),
            PatKind::Tuple(elements) => {
                Some(Pat::Con(Ctor::Tuple(elements.len()), self.row(elements)?))
            }
            PatKind::Literal(literal) => Some(Pat::Con(Ctor::Literal(literal.clone()), Vec::new())),
        }
```

`arity`、`column`、`specialize` を次のものに置き換え、`complete` と `fits` を足す。`constructors_of` はそのまま残す。

```rust
    fn arity(&self, ctor: &Ctor) -> usize {
        match ctor {
            Ctor::Data(id) => self.module.constructors[*id].fields.len(),
            Ctor::Tuple(width) => *width,
            Ctor::Literal(_) => 0,
        }
    }

    fn column(&self, rows: &[Row]) -> Result<Column<'a>, Mixed> {
        let mut column = Column::Wild;
        for row in rows {
            let Pat::Con(ctor, _) = &row[0] else {
                continue;
            };
            column = match (column, ctor) {
                (Column::Wild, Ctor::Data(id)) => {
                    let (_, all) = self.constructors_of(*id)?;
                    Column::Data {
                        seen: vec![*id],
                        all,
                    }
                }
                (Column::Data { mut seen, all }, Ctor::Data(id)) if all.contains(id) => {
                    if !seen.contains(id) {
                        seen.push(*id);
                    }
                    Column::Data { seen, all }
                }
                (Column::Wild, Ctor::Tuple(width)) => Column::Tuple(*width),
                (Column::Tuple(known), Ctor::Tuple(width)) if known == *width => {
                    Column::Tuple(known)
                }
                (Column::Wild | Column::Literal, Ctor::Literal(_)) => Column::Literal,
                _ => return Err(Mixed),
            };
        }
        Ok(column)
    }

    /// 列に現れたコンストラクタがその型のすべてなら、その並び。そろっていなければ `None` である。
    fn complete(&self, column: &Column) -> Option<Vec<Ctor>> {
        match column {
            Column::Data { seen, all } if seen.len() == all.len() => {
                Some(all.iter().map(|&id| Ctor::Data(id)).collect())
            }
            Column::Tuple(width) => Some(vec![Ctor::Tuple(*width)]),
            _ => None,
        }
    }

    /// 先頭の列がコンストラクタ `ctor` に合う行を、その引数を先頭に並べた行にする。
    fn specialize(&self, rows: &[Row], ctor: &Ctor) -> Vec<Row> {
        let arity = self.arity(ctor);
        rows.iter()
            .filter_map(|row| {
                let (head, rest) = row.split_first()?;
                let mut out = match head {
                    Pat::Con(other, args) if other == ctor => args.clone(),
                    Pat::Con(..) => return None,
                    Pat::Wild => vec![Pat::Wild; arity],
                };
                out.extend_from_slice(rest);
                Some(out)
            })
            .collect()
    }
```

`fits` は `impl` の外の関数にする (`default_rows` の前)。

```rust
/// `ctor` が、ほかの行から求めた列と同じ型のコンストラクタか。違えば、型の誤りを報告済みの混ざった列である。
fn fits(column: &Column, ctor: &Ctor) -> bool {
    match (column, ctor) {
        (Column::Wild, _) => true,
        (Column::Data { all, .. }, Ctor::Data(id)) => all.contains(id),
        (Column::Tuple(known), Ctor::Tuple(width)) => known == width,
        (Column::Literal, Ctor::Literal(_)) => true,
        _ => false,
    }
}
```

- [ ] **Step 4: `useful` と `missing` を `Ctor` で書き直す**

`useful` を次のものに置き換える。

```rust
    /// `vector` に合う値のうち、`rows` のどの行にも合わないものがあるか。到達しない枝を見つけるのに使う。
    fn useful(&self, rows: &[Row], vector: &[Pat]) -> Result<bool, Mixed> {
        let Some((head, rest)) = vector.split_first() else {
            return Ok(rows.is_empty());
        };
        let column = self.column(rows)?;
        match head {
            Pat::Con(ctor, args) => {
                if !fits(&column, ctor) {
                    return Err(Mixed);
                }
                let mut next = args.clone();
                next.extend_from_slice(rest);
                self.useful(&self.specialize(rows, ctor), &next)
            }
            Pat::Wild => match self.complete(&column) {
                Some(ctors) => {
                    for ctor in &ctors {
                        let mut next = vec![Pat::Wild; self.arity(ctor)];
                        next.extend_from_slice(rest);
                        if self.useful(&self.specialize(rows, ctor), &next)? {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                }
                None => self.useful(&default_rows(rows), rest),
            },
        }
    }
```

`missing` の doc コメントと本体を次のものに置き換える。

```rust
    /// 長さ `width` の値の並びのうち、`rows` のどの行にも合わないものを `limit` 個まで作る。`_` の並びの usefulness を、
    /// 例を組み立てながら解く。先頭の列にその型のすべてのコンストラクタが現れていれば (タプルはつねに)、
    /// コンストラクタごとに調べる。そうでなければ、先頭が `_` の行だけで残りの列を調べ、現れていない
    /// コンストラクタを先頭に付ける。どれも現れていない列とリテラルの列では、先頭は `_` になる。
    fn missing(&self, rows: &[Row], width: usize, limit: usize) -> Result<Vec<Row>, Mixed> {
        if width == 0 {
            return Ok(if rows.is_empty() {
                vec![Vec::new()]
            } else {
                Vec::new()
            });
        }
        let mut found = Vec::new();
        let column = self.column(rows)?;
        match self.complete(&column) {
            Some(ctors) => {
                for ctor in ctors {
                    if found.len() >= limit {
                        break;
                    }
                    let arity = self.arity(&ctor);
                    let inner = self.missing(
                        &self.specialize(rows, &ctor),
                        arity + width - 1,
                        limit - found.len(),
                    )?;
                    for mut values in inner {
                        let rest = values.split_off(arity);
                        let mut row = vec![Pat::Con(ctor.clone(), values)];
                        row.extend(rest);
                        found.push(row);
                    }
                }
            }
            None => {
                let rest = self.missing(&default_rows(rows), width - 1, limit)?;
                let heads: Vec<Pat> = match column {
                    Column::Data { seen, all } => all
                        .iter()
                        .filter(|ctor| !seen.contains(ctor))
                        .map(|&id| {
                            let ctor = Ctor::Data(id);
                            let arity = self.arity(&ctor);
                            Pat::Con(ctor, vec![Pat::Wild; arity])
                        })
                        .collect(),
                    Column::Wild | Column::Tuple(_) | Column::Literal => vec![Pat::Wild],
                };
                for head in &heads {
                    for values in &rest {
                        let mut row = vec![head.clone()];
                        row.extend(values.iter().cloned());
                        found.push(row);
                    }
                }
            }
        }
        found.truncate(limit);
        Ok(found)
    }
```

`Column::Tuple` は `complete` でつねにそろうので `None` の側には来ないが、`match` を網羅させるために `_` の頭の腕に並べる。

- [ ] **Step 5: 漏れの例の書き方にタプルを足す**

`show` と `atomic` を次のものに置き換える。

```rust
    fn show(&self, pat: &Pat) -> String {
        match pat {
            Pat::Wild => "_".to_string(),
            Pat::Con(Ctor::Data(ctor), args) => {
                let name = &self.module.constructors[*ctor].name;
                match args.as_slice() {
                    [] => name.clone(),
                    // 中置のコンストラクタは `:` で始まる演算子である (docs/spec/declarations.md の「`data` と `type`」)
                    [left, right] if name.starts_with(':') => {
                        format!("{} {name} {}", self.atomic(left), self.atomic(right))
                    }
                    args => iter::once(name.clone())
                        .chain(args.iter().map(|arg| self.atomic(arg)))
                        .collect::<Vec<_>>()
                        .join(" "),
                }
            }
            Pat::Con(Ctor::Tuple(_), elements) => {
                let elements: Vec<String> = elements.iter().map(|element| self.show(element)).collect();
                format!("({})", elements.join(", "))
            }
            Pat::Con(Ctor::Literal(_), _) => {
                unreachable!("a literal column is never complete, so examples hold `_` there")
            }
        }
    }

    /// 引数の位置に置く書き方。引数を持つコンストラクタは括弧で囲む。タプルは自分の括弧を持つ。
    fn atomic(&self, pat: &Pat) -> String {
        match pat {
            Pat::Con(Ctor::Data(_), args) if !args.is_empty() => format!("({})", self.show(pat)),
            _ => self.show(pat),
        }
    }
```

`cargo fmt` が `show` のタプルの腕の行を折り返したら、その形に従う。

- [ ] **Step 6: テストが通ることを確かめる**

Run: `cargo test -p eml_types --test exhaustive`
Expected: 24件すべて PASS (既存の14件と新しい10件)

- [ ] **Step 7: UI テストを足す**

`tests/ui/check-fail/exhaustiveness/literal_patterns.em` を作る。

```haskell
-- E4001: a `match` on string literals has no wildcard arm.
greeting : String -> String
greeting lang = match lang with
  | "en" -> "hello"
  | "ja" -> "konnichiwa"

main : Unit -> <IO> Unit
main () = println (greeting "en")
```

Run: `cargo test -p eml_cli --test ui`
Expected: `check_fail` が新しいスナップショットがないので FAIL する。`crates/eml_cli/tests/snapshots/ui__check_fail@exhaustiveness__literal_patterns.em.snap.new` を開き、次の内容であることを確かめる (罫線の細部は ariadne の出力に従う)。

```
[E4001] Error: `match` does not cover every value
   ╭─[ check-fail/exhaustiveness/literal_patterns.em:3:17 ]
   │
 3 │ greeting lang = match lang with
   │                 ──┬──  
   │                   ╰──── no arm matches some values
   │ 
   │ Note: not covered: `_`
───╯
```

診断がこの1件だけで、番号、文言、位置 (`3:17`)、note が一致することを確かめてから、`cargo insta accept` で承認する。既存のスナップショットとそろえるため、承認したファイルに `assertion_line:` の行があれば消す。もう一度 `cargo test -p eml_cli --test ui` を実行し、PASS を確かめる。

- [ ] **Step 8: 全体を確かめる**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`
Expected: すべて PASS し、clippy の警告がない。既存のテストの期待値は変わらない。

- [ ] **Step 9: コミットする**

```bash
git add crates/eml_types/src/exhaustive.rs crates/eml_types/tests/exhaustive.rs \
  tests/ui/check-fail/exhaustiveness/literal_patterns.em crates/eml_cli/tests/snapshots
git commit -m "Check tuple and literal patterns for exhaustiveness

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014YGQm6f7Mtged1yVY4dmJG"
```

### Task 4: Core IR でタプルの値を作り、決定木にタプルとリテラルの列を足す

タプルの値を、コンストラクタが1つの `data` と同じ `Rhs::Con { tag: 0, args }` にする。決定木の欄の頭にタプルとリテラルを足し、タプルの欄は枝が1つの `Switch` で分解し、リテラルの欄は「比べる `prim` と、その結果の `Bool` の `Switch`」の連なりにする (spec の4節)。Task 1 と Task 2 が `translate/` に置いた `unreachable!` (タプルの式、`PatKind::Tuple`、`PatKind::Literal`) を置き換える。

**Files:**
- Modify: `crates/eml_core_ir/src/lib.rs` (`TUPLE`)
- Modify: `crates/eml_core_ir/src/translate/types.rs` (boxed)
- Modify: `crates/eml_core_ir/src/translate/expr.rs` (タプルの値、`lang_type`)
- Modify: `crates/eml_core_ir/src/translate/pattern.rs` (欄の頭、タプルとリテラルの欄)
- Modify: `crates/eml_core_ir/src/translate/mod.rs` (`needs_decision_tree` への改名)
- Test: `crates/eml_core_ir/tests/translate.rs`、`crates/eml_core_ir/tests/perceus.rs`
- Create: `tests/ui/run/data/tuples.em`、`tests/ui/run/data/literal_patterns.em`、`tests/ui/run/data/tuples_with_effects.em` とそのスナップショット

**Interfaces:**
- Consumes:
  - Task 1 の HIR の `ExprKind::Tuple(Vec<ExprId>)`、`PatKind::Tuple(Vec<PatId>)`、`PatKind::Literal(Literal)`
  - Task 2 のタプルの型 `Type::Record(vec![("0", A), ("1", B), ..])` (ラベルの順に並ぶ)、`PrimOp::StrEq` (表示は `string==`)、`String` の `==`
  - Task 3 の網羅性の検査 (リテラルの欄の残りの行列は空にならない)
- Produces:
  - `eml_core_ir::TUPLE: u32` (= 0)
  - タプルの値は `Rhs::Con { tag: TUPLE, args }` で、結果の変数の名前は `d`
  - 空でない閉じたレコードの型の変数は boxed。`Unit` (`Record([])`) は boxed でない
  - 決定木の欄の頭 `Head::Tuple` と `Head::Literal`

**Interface notes (骨組みの Interfaces から変えた点と、前後のタスクとの取り決め):**

- `translate/pattern.rs` の `has_constructor` を `needs_decision_tree` に改名する。タプルとリテラルのパターンも決定木で調べるので、「コンストラクタを含むか」では意味が合わなくなる。呼び出し側は `translate/mod.rs` の3か所 (`use`、`lower` の引数、`stmts` の `let`) である。外から見える名前ではない。
- タプルのタグを、`FALSE` / `TRUE` と同じく `eml_core_ir` の定数 `pub const TUPLE: u32 = 0;` (`lib.rs`) にする。変換の値の側 (`expr.rs`) と決定木の側 (`pattern.rs`) で同じタグを使うためである。
- `FnLowering::lang_type(id: TypeDefId) -> Type` (`translate/expr.rs`) を足す。文字列のリテラルの型と、リテラルの比較の結果の `Bool` の型を作るのに使う。今の `ExprKind::Literal(Literal::String(..))` の腕の中の型の組み立ても、これに置き換える。
- Task 5 (文書) への申し送り: リテラルの比較の連なりは `Switch` の入れ子になり、深さは1つの欄の異なるリテラルの数になる。Perceus、生存解析、verifier は `Switch` の枝を再帰でたどるので、この深さは E0013 の入れ子の上限で抑えられない。異なるリテラルが非常に多い `match` (数万の枝) はスタックを使う。`docs/implementation/status.md` の「次の作業の注意点」の、決定木の再帰の深さの項目に足す。

- [ ] **Step 1: 変換の失敗するテストを書く**

`crates/eml_core_ir/tests/translate.rs` の末尾に、次の5つのテストを足す。`use` は今のままでよい。

```rust
#[test]
fn tuples_are_built_and_taken_apart_by_parameters_and_let() {
    // タプルの値はタグ 0 のコンストラクタの値で、タプルのパターンは枝が1つの `Switch` で分解する
    let text = "swap : (Int, String) -> (String, Int)\nswap (n, s) = (s, n)\n\nfirst : (Int, Int) -> Int\nfirst p =\n  let (a, _) = p\n  a\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r"
    fn swap(p0) {
      join j0(n1, s2) [] {
        let d5 = con #0(s2, n1)
        return d5
      }
      switch p0 {
        #0(n3, s4) ->
          jump j0(n3, s4)
      }
    }
    fn first(p0) {
      join j0(a1) [] {
        return a1
      }
      switch p0 {
        #0(a2, x3) ->
          jump j0(a2)
      }
    }
    fn main(p0) {
      return ()
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn a_tuple_column_is_a_single_constructor() {
    // タプルの欄を分解してから、その中の `Option` の欄で `Switch` する
    let text = "data Option a = | None | Some a\n\npick : (Option Int, Int) -> Int\npick p = match p with\n  | (Some n, _) -> n\n  | (None, k) -> k\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r"
    fn pick(p0) {
      join j0(n1) [] {
        return n1
      }
      join j1(k2) [] {
        return k2
      }
      switch p0 {
        #0(x3, k4) ->
          switch x3 {
            #0 ->
              jump j1(k4)
            #1(n5) ->
              jump j0(n5)
          }
      }
    }
    fn main(p0) {
      return ()
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn int_literals_compare_in_order_and_fall_back_to_the_rest() {
    // 異なるリテラルを上の行から順に比べ、最後の等しくない枝は残りの行列 (`_` の行) に進む
    let text = "describe : Int -> String\ndescribe n = match n with\n  | 0 -> \"zero\"\n  | 1 -> \"one\"\n  | -1 -> \"minus one\"\n  | _ -> \"many\"\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
    fn describe(n0) {
      join j0() [] {
        let s1 = const "zero"
        return s1
      }
      join j1() [] {
        let s2 = const "one"
        return s2
      }
      join j2() [] {
        let s3 = const "minus one"
        return s3
      }
      join j3() [] {
        let s4 = const "many"
        return s4
      }
      let t5 = prim ==(n0, 0)
      switch t5 {
        #0 ->
          let t6 = prim ==(n0, 1)
          switch t6 {
            #0 ->
              let t7 = prim ==(n0, -1)
              switch t7 {
                #0 ->
                  jump j3()
                #1 ->
                  jump j2()
              }
            #1 ->
              jump j1()
          }
        #1 ->
          jump j0()
      }
    }
    fn main(p0) {
      return ()
    }
    fn entry$main() {
      tailcall main(())
    }
    "#);
}

#[test]
fn string_literals_build_each_literal_for_its_comparison() {
    // `String` のリテラルは比べるたびに作る。変数の枝は、比べた出現そのものを受ける
    let text = "greet : String -> String\ngreet name = match name with\n  | \"en\" -> \"hello\"\n  | \"ja\" -> \"konnichiwa\"\n  | other -> other\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r#"
    fn greet(name0) {
      join j0() [] {
        let s1 = const "hello"
        return s1
      }
      join j1() [] {
        let s2 = const "konnichiwa"
        return s2
      }
      join j2(other3) [] {
        return other3
      }
      let s4 = const "en"
      let t5 = prim string==(name0, s4)
      switch t5 {
        #0 ->
          let s6 = const "ja"
          let t7 = prim string==(name0, s6)
          switch t7 {
            #0 ->
              jump j2(name0)
            #1 ->
              jump j1()
          }
        #1 ->
          jump j0()
      }
    }
    fn main(p0) {
      return ()
    }
    fn entry$main() {
      tailcall main(())
    }
    "#);
}

#[test]
fn a_literal_column_inside_a_tuple() {
    // タプルを分解した後、最初の行でいちばん左の調べる欄 (リテラル) を選ぶ。等しくない枝の `Bool` の欄には `True` の
    // 行がないので、残りの行列の join point を作る
    let text = "classify : (Int, Bool) -> Int\nclassify p = match p with\n  | (0, True) -> 1\n  | (_, False) -> 2\n  | (n, _) -> n\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r"
    fn classify(p0) {
      join j0() [] {
        return 1
      }
      join j1() [] {
        return 2
      }
      join j2(n1) [] {
        return n1
      }
      switch p0 {
        #0(n2, x3) ->
          let t4 = prim ==(n2, 0)
          switch t4 {
            #0 ->
              join j3() [n2] {
                jump j2(n2)
              }
              switch x3 {
                #0 ->
                  jump j1()
                #1 ->
                  jump j3()
              }
            #1 ->
              switch x3 {
                #0 ->
                  jump j1()
                #1 ->
                  jump j0()
              }
          }
      }
    }
    fn main(p0) {
      return ()
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}
```

- [ ] **Step 2: Perceus の失敗するテストを書く**

`crates/eml_core_ir/tests/perceus.rs` の末尾に足す。`simplify` の最初の B3 が `jump` の1つの枝の join point を戻すので、Perceus の後には join point が残らない。

```rust
#[test]
fn a_string_compared_twice_is_dupped_before_each_comparison() {
    // 比べる `prim` は出現の所有権を受け取るので、後の比較と枝でも使う `name0` を比べるたびに複製する。使わない枝は
    // 入口で捨てる
    let text = "greet : String -> String\ngreet name = match name with\n  | \"en\" -> \"hello\"\n  | \"ja\" -> \"konnichiwa\"\n  | other -> other\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r#"
    fn greet(name0) {
      let s4 = const "en"
      dup name0
      let t5 = prim string==(name0, s4)
      switch t5 {
        #0 ->
          let s6 = const "ja"
          dup name0
          let t7 = prim string==(name0, s6)
          switch t7 {
            #0 ->
              let other3 = name0
              return other3
            #1 ->
              decref name0
              let s2 = const "konnichiwa"
              return s2
          }
        #1 ->
          decref name0
          let s1 = const "hello"
          return s1
      }
    }
    fn main(p0) {
      return ()
    }
    fn entry$main() {
      tailcall main(())
    }
    "#);
}
```

- [ ] **Step 3: テストが失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test translate --test perceus`
Expected: 新しい6件が FAIL する。Task 1 と Task 2 が `translate/` に置いた `unreachable!` (タプルの式、タプルとリテラルのパターン) の panic で止まる。既存のテストは PASS のままである。

- [ ] **Step 4: タプルのタグと boxed の判定を足す**

`crates/eml_core_ir/src/lib.rs` の `FALSE` / `TRUE` の後に足す。

```rust
/// タプルの値のタグ。タプルは、コンストラクタが1つの `data` と同じオブジェクトで表す (docs/spec/core-ir.md)。
pub const TUPLE: u32 = 0;
```

`crates/eml_core_ir/src/translate/types.rs` の `boxed` の `Type::Record(_) | Type::Error => false` の腕を、次の2つに分ける。

```rust
        // 空のレコードは `Unit` で、値は `()` である。要素のあるレコード (タプル) はヒープのオブジェクトにする
        Type::Record(fields) => !fields.is_empty(),
        Type::Error => false,
```

関数の doc コメントの1文目「ヒープに置く値の型。」の後ろは今のままでよい。

- [ ] **Step 5: タプルの値と `lang_type` を足す**

`crates/eml_core_ir/src/translate/expr.rs` の `use eml_hir::{...}` に `TypeDefId` を、`use crate::{...}` に `TUPLE` を足す。`impl FnLowering<'_>` の `ty` の後に足す。

```rust
    /// 組み込みの型 (`String`、`Bool`) の、引数のない型構成子の型。
    pub(super) fn lang_type(&self, id: TypeDefId) -> Type {
        Type::Con {
            id,
            name: self.module.types[id].name.clone(),
            args: Vec::new(),
        }
    }
```

`atom` の `ExprKind::Literal(Literal::String(text))` の腕を、`lang_type` を使う形にする。

```rust
            ExprKind::Literal(Literal::String(text)) => {
                let index = self.program.strings.intern(text);
                let ty = self.lang_type(self.module.lang.string);
                self.bind(out, "s", &ty, Rhs::ConstString(index))
            }
```

Task 1 が `atom` に置いた `ExprKind::Tuple` の `unreachable!` の腕を、次の腕に置き換える。

```rust
            ExprKind::Tuple(elements) => {
                // 要素を左から評価し、コンストラクタが1つの `data` と同じ値にする (docs/spec/core-ir.md)
                let args = elements
                    .iter()
                    .map(|&element| self.atom(element, out))
                    .collect();
                let ty = self.ty(id);
                self.bind(out, "d", &ty, Rhs::Con { tag: TUPLE, args })
            }
```

- [ ] **Step 6: 決定木の欄の頭にタプルとリテラルを足す**

`crates/eml_core_ir/src/translate/pattern.rs` を次のように直す。モジュールの doc コメントの最後に1文を足す。

```rust
//! `match` と、`let`・ラムダ・等式の引数のパターンを、決定木にコンパイルする (docs/spec/core-ir.md)。同じ値を二度
//! 調べないように、行列の欄ごとに `Switch` する。各枝の本体は join point にして決定木の葉から jump する。共有の有無は
//! 数えず、jump が1つの枝は simplify の B3 がその位置に戻す。タプルはコンストラクタが1つの型として分解し、リテラルは
//! 比べる `prim` とその結果の `Switch` の連なりで調べる。
```

`use` を次にする。

```rust
use eml_hir::{Body, ConstructorId, ExprId, Literal, LocalId, MatchArm, PatId, PatKind, TypeDefKind};
use eml_types::Type;

use crate::{Arm, Atom, CExpr, CExprId, FALSE, JoinId, PrimOp, Rhs, TRUE, TUPLE, VarId};

use super::types::split_arrows;
use super::{Binding, Bindings, Exit, FnLowering};
```

`Head` と `head` と `has_constructor` を、次の4つの定義に置き換える (Task 1 が `head` と `has_constructor` に置いた `PatKind::Tuple` / `PatKind::Literal` の `unreachable!` も、ここでなくなる)。

```rust
/// 欄の先頭の形。変数は、その欄の出現を束縛するワイルドカードである。
enum Head<'a> {
    Any(Option<LocalId>),
    Con(ConstructorId, &'a [PatId]),
    /// タプルは、タグ 0 のコンストラクタが1つの型として分解する (docs/spec/core-ir.md)。
    Tuple(&'a [PatId]),
    Literal(&'a Literal),
}

fn head(body: &Body, cell: Cell) -> Head<'_> {
    let Cell::Pat(mut pat) = cell else {
        return Head::Any(None);
    };
    loop {
        match &body.pats[pat].kind {
            PatKind::Annot { pat: inner, .. } => pat = *inner,
            PatKind::Bind(local) => return Head::Any(Some(*local)),
            PatKind::Con { ctor, args } => return Head::Con(*ctor, args),
            PatKind::Tuple(elements) => return Head::Tuple(elements),
            PatKind::Literal(literal) => return Head::Literal(literal),
            // `()` は値が1つしかないので、ワイルドカードと同じに扱える
            PatKind::Wildcard | PatKind::Unit | PatKind::Missing => return Head::Any(None),
        }
    }
}

/// 欄を分解するコンストラクタ。特殊化とフィールドの名前で、`data` のコンストラクタとタプルを同じに扱う。
#[derive(Clone, Copy)]
enum Shape {
    Con(ConstructorId),
    Tuple,
}

/// 欄の先頭が `shape` のコンストラクタ (タプル) なら、その引数のパターン。
fn shape_args(head: Head<'_>, shape: Shape) -> Option<&[PatId]> {
    match (head, shape) {
        (Head::Con(other, args), Shape::Con(ctor)) if other == ctor => Some(args),
        (Head::Tuple(args), Shape::Tuple) => Some(args),
        _ => None,
    }
}

/// パターンが値を調べるか分解するか。どちらもしなければ、値をそのまま局所変数に対応させればよく、決定木は要らない。
pub(super) fn needs_decision_tree(body: &Body, pat: PatId) -> bool {
    match &body.pats[pat].kind {
        PatKind::Con { .. } | PatKind::Tuple(_) | PatKind::Literal(_) => true,
        PatKind::Annot { pat, .. } => needs_decision_tree(body, *pat),
        PatKind::Bind(_) | PatKind::Wildcard | PatKind::Unit | PatKind::Missing => false,
    }
}
```

`shape_args` の戻り値の寿命は `head` と同じである。コンパイラが省略できないと言ったら、`fn shape_args<'a>(head: Head<'a>, shape: Shape) -> Option<&'a [PatId]>` と書く。

`decide` を、欄を選んで変数のパターンを束縛するところまでにし、欄の種類ごとの処理を3つの関数に分ける。`decide` を次のコードで置き換える。

```rust
    /// 行列から決定木を作る。最初の行がすべてワイルドカードなら葉にする。そうでなければ、最初の行で値を調べる
    /// いちばん左の欄を選び、その欄の種類で分ける。`Switch` の直前に置く join point (残りの行列) と比較の束縛は `out`
    /// に積み、最後の命令を返す。行列は網羅性の検査を通っているので、空にならない。
    fn decide(
        &mut self,
        occurrences: &[Occurrence],
        mut rows: Vec<Row>,
        targets: &[Target],
        out: &mut Bindings,
    ) -> CExpr {
        let body = self.body;
        let first = rows.first().expect("type-checked patterns are exhaustive");
        let Some(column) = first
            .cells
            .iter()
            .position(|&cell| !matches!(head(body, cell), Head::Any(_)))
        else {
            return leaf(body, occurrences, rows.swap_remove(0), targets);
        };
        let cell = first.cells[column];
        let occurrence = occurrences[column].atom;
        // 変数のパターンはこの欄の出現を受け、以後はワイルドカードとして扱う
        for row in &mut rows {
            if let Head::Any(Some(local)) = head(body, row.cells[column]) {
                row.bound.push((local, occurrence));
            }
        }
        match head(body, cell) {
            Head::Con(ctor, _) => {
                self.switch_constructors(occurrences, &rows, column, ctor, targets, out)
            }
            Head::Tuple(elements) => {
                self.switch_tuple(occurrences, &rows, column, elements.len(), targets)
            }
            Head::Literal(_) => self.compare_literals(occurrences, &rows, column, targets, out),
            Head::Any(_) => unreachable!("the chosen column is not a wildcard"),
        }
    }

    /// コンストラクタの欄。型のすべてのコンストラクタの枝を持つ `Switch` にする。
    fn switch_constructors(
        &mut self,
        occurrences: &[Occurrence],
        rows: &[Row],
        column: usize,
        ctor: ConstructorId,
        targets: &[Target],
        out: &mut Bindings,
    ) -> CExpr {
        let body = self.body;
        let module = self.module;
        let occurrence = occurrences[column].clone();
        let TypeDefKind::Data { constructors } = &module.types[module.constructors[ctor].ty].kind
        else {
            unreachable!("constructor patterns belong to data types")
        };
        let mentions = |ctor: ConstructorId, row: &Row| match head(body, row.cells[column]) {
            Head::Con(other, _) => other == ctor,
            _ => false,
        };
        let rest = if constructors
            .iter()
            .all(|&ctor| rows.iter().any(|row| mentions(ctor, row)))
        {
            None
        } else {
            // 選んだ欄に現れないコンストラクタの枝は、どれもワイルドカードの行だけの同じ行列になる。部分木を枝の数だけ
            // 複製しないように、引数のない join point にして各枝から jump する
            let join = self.new_join();
            let mut remaining = occurrences.to_vec();
            remaining.remove(column);
            let default: Vec<Row> = rows
                .iter()
                .filter(|row| matches!(head(body, row.cells[column]), Head::Any(_)))
                .map(|row| row.replace(column, []))
                .collect();
            let mut inner = Vec::new();
            let last = self.decide(&remaining, default, targets, &mut inner);
            let code = self.seq(inner, last);
            out.push(Binding::Shared {
                join,
                params: Vec::new(),
                body: code,
            });
            Some(join)
        };
        let mut arms = Vec::new();
        for &ctor in constructors {
            let field_types = self.field_types(ctor, &occurrence.ty);
            let (fields, code) = if rows.iter().any(|row| mentions(ctor, row)) {
                self.branch(occurrences, rows, column, Shape::Con(ctor), field_types, targets)
            } else {
                let fields = field_types
                    .iter()
                    .map(|ty| self.new_var("x", ty))
                    .collect();
                let join = rest.expect("a constructor missing from the column goes to the rest");
                let jump = self.push(CExpr::Jump {
                    join,
                    args: Vec::new(),
                });
                (fields, jump)
            };
            arms.push(Arm {
                tag: module.constructors[ctor].tag,
                fields,
                body: code,
            });
        }
        CExpr::Switch {
            scrutinee: occurrence.atom,
            arms,
        }
    }

    /// タプルの欄。タグ 0 のコンストラクタが1つの型として、枝が1つの `Switch` で分解する (docs/spec/core-ir.md)。
    /// コンストラクタの集合はつねにそろっているので、残りの行列はない。
    fn switch_tuple(
        &mut self,
        occurrences: &[Occurrence],
        rows: &[Row],
        column: usize,
        arity: usize,
        targets: &[Target],
    ) -> CExpr {
        let occurrence = occurrences[column].clone();
        let field_types = tuple_field_types(&occurrence.ty, arity);
        let (fields, code) =
            self.branch(occurrences, rows, column, Shape::Tuple, field_types, targets);
        CExpr::Switch {
            scrutinee: occurrence.atom,
            arms: vec![Arm {
                tag: TUPLE,
                fields,
                body: code,
            }],
        }
    }

    /// `shape` の枝。フィールドを新しい出現として束縛し、行列を特殊化して決定木を続ける。フィールドは元の欄の位置に
    /// 並べる。左から深さ優先で欄を選ぶためである。
    fn branch(
        &mut self,
        occurrences: &[Occurrence],
        rows: &[Row],
        column: usize,
        shape: Shape,
        field_types: Vec<Type>,
        targets: &[Target],
    ) -> (Vec<VarId>, CExprId) {
        let body = self.body;
        let fields: Vec<VarId> = field_types
            .iter()
            .enumerate()
            .map(|(index, ty)| {
                let name = field_name(body, rows, column, shape, index);
                self.new_var(&name, ty)
            })
            .collect();
        let mut specialized_occurrences = occurrences[..column].to_vec();
        specialized_occurrences.extend(fields.iter().zip(&field_types).map(|(&var, ty)| {
            Occurrence {
                atom: Atom::Var(var),
                ty: ty.clone(),
            }
        }));
        specialized_occurrences.extend_from_slice(&occurrences[column + 1..]);
        let specialized: Vec<Row> = rows
            .iter()
            .filter_map(|row| match head(body, row.cells[column]) {
                Head::Any(_) => Some(row.replace(column, fields.iter().map(|_| Cell::Any))),
                other => shape_args(other, shape)
                    .map(|args| row.replace(column, args.iter().map(|&arg| Cell::Pat(arg)))),
            })
            .collect();
        let mut inner = Vec::new();
        let last = self.decide(&specialized_occurrences, specialized, targets, &mut inner);
        (fields, self.seq(inner, last))
    }

    /// リテラルの欄。上の行から現れる異なるリテラルの順に、出現と比べる `prim` とその結果の `Switch` を連ねる。等しい
    /// 枝はそのリテラルで特殊化した行列に、最後の等しくない枝は残りの行列に進む。残りの行列は最後の等しくない枝から
    /// だけ届くので、join point にせずにその位置に置く。リテラルは無限にあるので、網羅性の検査を通った行列では残りの
    /// 行列が空にならない (docs/spec/exhaustiveness.md)。
    ///
    /// 連なりは長くなりうるので、比較と等しい枝をループで先に作り、等しくない枝の入れ子を後ろから組み立てる。
    fn compare_literals(
        &mut self,
        occurrences: &[Occurrence],
        rows: &[Row],
        column: usize,
        targets: &[Target],
        out: &mut Bindings,
    ) -> CExpr {
        let body = self.body;
        let scrutinee = occurrences[column].atom;
        let mut remaining = occurrences.to_vec();
        remaining.remove(column);
        let mut literals: Vec<&Literal> = Vec::new();
        for row in rows {
            if let Head::Literal(literal) = head(body, row.cells[column])
                && !literals.contains(&literal)
            {
                literals.push(literal);
            }
        }
        let mut steps = Vec::new();
        for literal in literals {
            let mut compare = Vec::new();
            let equal = self.compare(scrutinee, literal, &mut compare);
            let specialized: Vec<Row> = rows
                .iter()
                .filter(|row| match head(body, row.cells[column]) {
                    Head::Literal(other) => other == literal,
                    Head::Any(_) => true,
                    Head::Con(..) | Head::Tuple(_) => false,
                })
                .map(|row| row.replace(column, []))
                .collect();
            let mut inner = Vec::new();
            let last = self.decide(&remaining, specialized, targets, &mut inner);
            let then = self.seq(inner, last);
            steps.push((compare, equal, then));
        }
        // どのリテラルにも等しくない値は、ワイルドカードの行だけの行列で調べる
        let default: Vec<Row> = rows
            .iter()
            .filter(|row| matches!(head(body, row.cells[column]), Head::Any(_)))
            .map(|row| row.replace(column, []))
            .collect();
        let mut inner = Vec::new();
        let last = self.decide(&remaining, default, targets, &mut inner);
        let mut otherwise = self.seq(inner, last);
        let (first_compare, first_equal, first_then) = steps.remove(0);
        for (compare, equal, then) in steps.into_iter().rev() {
            otherwise = self.seq(compare, if_equal(equal, then, otherwise));
        }
        out.extend(first_compare);
        if_equal(first_equal, first_then, otherwise)
    }

    /// 出現をリテラルと比べる `prim` の結果の `Bool`。比べる `prim` は引数の所有権を受け取るので、後で使う出現は
    /// Perceus が複製する。`String` のリテラルは比べるたびに作る。
    fn compare(&mut self, scrutinee: Atom, literal: &Literal, out: &mut Bindings) -> Atom {
        let (op, value) = match literal {
            Literal::Int(n) => (PrimOp::IntEq, Atom::Int(*n)),
            Literal::String(text) => {
                let index = self.program.strings.intern(text);
                let ty = self.lang_type(self.module.lang.string);
                let value = self.bind(out, "s", &ty, Rhs::ConstString(index));
                (PrimOp::StrEq, value)
            }
            Literal::Unit => unreachable!("`()` is a wildcard pattern, not a literal pattern"),
        };
        let ty = self.lang_type(self.module.lang.bool);
        self.bind(out, "t", &ty, Rhs::Prim(op, vec![scrutinee, value]))
    }
```

`field_types` は今のままにする。`leaf` の後の `field_name` を、`shape` を受ける形に置き換え、`tuple_field_types` と `if_equal` を足す。

```rust
/// フィールドの変数の名前。その位置を変数のパターンで受ける行があれば、その変数の名前にする。
fn field_name(body: &Body, rows: &[Row], column: usize, shape: Shape, index: usize) -> String {
    rows.iter()
        .find_map(|row| {
            let args = shape_args(head(body, row.cells[column]), shape)?;
            match head(body, Cell::Pat(args[index])) {
                Head::Any(Some(local)) => Some(body.locals[local].name.clone()),
                _ => None,
            }
        })
        .unwrap_or_else(|| "x".to_string())
}

/// タプルの要素の型。型検査はタプルを数字ラベルの閉じたレコードにし、ラベルの順に並べる (docs/spec/records.md)。
/// レコードでなければ置き換えずに型変数として扱う。型変数の値は boxed として扱うので、多めに RC の対象になるだけで
/// 正しく動く。
fn tuple_field_types(ty: &Type, arity: usize) -> Vec<Type> {
    match ty {
        Type::Record(fields) if fields.len() == arity => {
            fields.iter().map(|(_, field)| field.clone()).collect()
        }
        _ => vec![Type::Flexible; arity],
    }
}

/// 比べた結果の `Bool` で分ける `Switch`。`if` と同じく、`False` の枝を先に置く。
fn if_equal(equal: Atom, then: CExprId, otherwise: CExprId) -> CExpr {
    CExpr::Switch {
        scrutinee: equal,
        arms: vec![
            Arm {
                tag: FALSE,
                fields: Vec::new(),
                body: otherwise,
            },
            Arm {
                tag: TRUE,
                fields: Vec::new(),
                body: then,
            },
        ],
    }
}
```

`substitute` と `leaf` は今のままにする。`Type` が `Clone` なので `vec![Type::Flexible; arity]` はそのまま書ける。

- [ ] **Step 7: 呼び出し側を `needs_decision_tree` にする**

`crates/eml_core_ir/src/translate/mod.rs` の `use pattern::has_constructor;` を `use pattern::needs_decision_tree;` にし、`has_constructor(` の3か所 (`lower` の2か所と `stmts` の1か所) を `needs_decision_tree(` にする。`lower` の中のコメント「コンストラクタを含むパターンは名前のない引数で受け、本体の前で分解する」を「値を調べるか分解するパターンは名前のない引数で受け、本体の前で分解する」にする。

- [ ] **Step 8: 変換と Perceus のテストが通ることを確かめる**

Run: `cargo test -p eml_core_ir`
Expected: PASS。新しい6件と既存のテストのすべてが通る。`decide` の分け方を変えても、変数を作る順 (枝の join point の引数、コンストラクタの欄のフィールド、特殊化した行列) は変わらないので、段階4a のスナップショットは変わらない。各パスの後の `verify_scopes` と Perceus の後の `verify` も debug ビルドで通る。

`simplify` の B2 は、引数を1つ持ち本体がその引数で `Switch` する join point だけを見る。リテラルの比較の `Switch` の scrutinee は `let` で束縛した新しい変数 (`t5` など) で、join point の引数ではないので、B2 は働かない。`Bool` を返すリテラルの `match` を末尾にない `if` の条件にした場合は、`if` の join point に定数のタグが渡り、今までどおり B2 が働く。

- [ ] **Step 9: UI テストを書く**

`tests/ui/run/data/tuples.em`:

```haskell
-- Tuples are built, passed around and taken apart by `let`, `match`, lambda and equation parameters,
-- including nested tuples and tuples inside an `Option`.
data Option a =
  | None
  | Some a

swap : (Int, String) -> (String, Int)
swap (n, s) = (s, n)

sum_pair : (Int, Int) -> Int
sum_pair p =
  let (a, b) = p
  a + b

lookup : String -> Option (String, Int)
lookup key = if key == "two" then Some ("two", 2) else None

describe : Option (String, Int) -> String
describe o = match o with
  | Some (name, n) -> name ++ "=" ++ show_int n
  | None -> "missing"

nested : ((Int, Int), String) -> String
nested ((a, b), label) = label ++ show_int (a * b)

main : Unit -> <IO> Unit
main () =
  let (s, n) = swap (1, "one")
  println (s ++ " " ++ show_int n)
  println (show_int (sum_pair (3, 4)))
  println (describe (lookup "two"))
  println (describe (lookup "three"))
  println (nested ((6, 7), "product "))
  let add = fn (x, y) -> x + y
  println (show_int (add (10, 20)))
```

stdout:

```
one 1
7
two=2
missing
product 42
30
```

`tests/ui/run/data/literal_patterns.em`:

```haskell
-- Int and String literal patterns, including a negative number and a variable fallback. The String match runs
-- a thousand times, so every literal string built for a comparison must be freed.
describe : Int -> String
describe n = match n with
  | 0 -> "zero"
  | 1 -> "one"
  | -1 -> "minus one"
  | _ -> "many"

greet : String -> String
greet lang = match lang with
  | "en" -> "hello"
  | "ja" -> "konnichiwa"
  | other -> "? " ++ other

bonus : Int -> Int
bonus n = match greet (if n % 2 == 0 then "en" else "fr") with
  | "hello" -> 1
  | _ -> 0

count_hellos : Int -> Int -> Int
count_hellos n acc = if n == 0 then acc else count_hellos (n - 1) (acc + bonus n)

main : Unit -> <IO> Unit
main () =
  println (describe 0)
  println (describe 1)
  println (describe (-1))
  println (describe 42)
  println (greet "en")
  println (greet "ja")
  println (greet "fr")
  println (show_int (count_hellos 1000 0))
```

stdout:

```
zero
one
minus one
many
hello
konnichiwa
? fr
500
```

`count_hellos 1000 0` は、1 から 1000 までの偶数の `n` で `"en"` を渡して `"hello"` を受けるので 500 になる。奇数では `"? fr"` を受けて `_` の枝に進む。

Review Focus のために、タプルが multi-shot の handler を通る UI テストも足す。

`tests/ui/run/data/tuples_with_effects.em`:

```haskell
-- Tuples pass through a multi-shot handler: an operation clause takes its tuple argument apart, and each
-- resumption returns a tuple holding a string that the clause destructures.
effect Pick where
  multi pick : (String, String) -> String

both : Unit -> <Pick> (String, Int)
both () =
  let s = pick ("left", "right")
  (s ++ "!", 1)

collect : Unit -> (String, Int)
collect () =
  handle both () with
    | pick (a, b) k ->
      let (x, n) = resume k a
      let (y, m) = resume k b
      (x ++ " " ++ y, n + m)

main : Unit -> <IO> Unit
main () =
  let (text, count) = collect ()
  println text
  println (show_int count)
```

stdout:

```
left! right!
2
```

- [ ] **Step 10: UI テストを確かめて承認する**

Run: `cargo test -p eml_cli --test ui`
Expected: `ui::run` が3つの新しいスナップショットで失敗する。`.snap.new` を開き、stdout が上に書いたものと一致し、stderr が空であることを確かめてから `cargo insta accept` で承認する。もう一度 `cargo test -p eml_cli --test ui` を通す。`debug_heap` が有効なので、比較のたびに作る文字列が残ればリークとして失敗する。既存のスナップショットは変わらない。

- [ ] **Step 11: 全体を確かめる**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`
Expected: すべて PASS し、clippy の警告がない。`cargo fmt` の後に差分が出たら、その差分も含めてコミットする。

- [ ] **Step 12: コミットする**

```bash
git add crates/eml_core_ir/src/lib.rs crates/eml_core_ir/src/translate \
  crates/eml_core_ir/tests/translate.rs crates/eml_core_ir/tests/perceus.rs \
  tests/ui/run/data/tuples.em tests/ui/run/data/literal_patterns.em tests/ui/run/data/tuples_with_effects.em crates/eml_cli/tests/snapshots
git commit -m "Build tuple values and compile tuple and literal patterns

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014YGQm6f7Mtged1yVY4dmJG"
```

### Task 5: 文書を直す

spec の7節の表のとおり、決まった内容を `docs/spec/` と `docs/implementation/` に移す。作業用の spec と plan は、ブランチ全体のレビューの後に別のコミットで削除する。このタスクでは削除しない。

**Files:**
- Modify: `docs/spec/records.md`、`docs/spec/declarations.md`、`docs/spec/exhaustiveness.md`、`docs/spec/core-ir.md`、`docs/spec/diagnostics.md`
- Modify: `docs/implementation/architecture.md`、`docs/implementation/status.md`、`docs/implementation/testing.md`、`docs/implementation/test-changes.md`

**Interfaces:**
- Consumes: Task 1〜4 の実装。名前と振る舞いを文書に書くので、実装と食い違ったら実装の側を正とし、この節の文案を直す

- [ ] **Step 1: `yomiyasu:yomiyasu` スキルを読む**

文書はすべて日本語なので、書く前にスキルを読み、規則に従う。

- [ ] **Step 2: `docs/spec/records.md` を直す**

1. 「実行時の表現」の「マイルストーン1 のインタプリタでは、フィールドを名前で引く」の箇条を、次の2つに置き換える。

   ```markdown
   - タプルは、フィールドの位置が静的に決まるので、実行時にはコンストラクタが1つの `data` と同じオブジェクト (タグ 0、要素の順のフィールド) で表す。分解は `match` の分岐と同じ命令で行う ([Core IR とインタプリタ](core-ir.md))
   - 名前付きのレコードの実行時の表し方 (名前で引くか、位置で引くか) は、row 多相の射影と一緒に S2 で決める
   ```

2. 「実装の段階」の「タプルのレコードへの変換は HIR で行う」を、次のように直す。

   ```markdown
   - HIR はタプルを専用の形 (`ExprKind::Tuple`、`PatKind::Tuple`、`TypeRefKind::Tuple`) で持ち、型検査が数字ラベルの閉じたレコードに写す。名前付きのレコードを入れるときに、HIR のレコードの形と一緒に作り直す
   - マイルストーン1 では、タプルはパターンで分解する。射影 `t.0` は開いたレコードの row が要るので、名前付きのレコードと一緒に S2 で入れる
   ```

- [ ] **Step 3: `docs/spec/declarations.md` を直す**

標準の演算子の表の前の文「アドホック多相がないので、マイルストーン1 では型を1つに固定する。」を「アドホック多相がないので、マイルストーン1 では `==` と `!=` を除いて型を1つに固定する。」にする。表の `==` `!=` `<` `<=` `>` `>=` の行を、次の2行に置き換える。

```markdown
| `==` `!=` | `a -> a -> Bool`。`a` は `Int`、`String`、`Bool` のどれかでなければならない。型検査が引数の型から比べ方を決め、それ以外の型 (`data` の型、タプル、関数、シグネチャの型変数) は E2006 にする。将来の型クラス `Eq` の `Int`、`String`、`Bool` のインスタンスにあたる ([ロードマップ](../future/roadmap.md)) |
| `<` `<=` `>` `>=` | `Int -> Int -> Bool` |
```

- [ ] **Step 4: `docs/spec/exhaustiveness.md` を直す**

「検査パス」の「Int と String のリテラルは無限に値があるものとして扱う。」の箇条の前に、次の箇条を足す。

```markdown
- タプルは、要素の数だけのフィールドを持つコンストラクタが1つの型として扱う。漏れているパターンの例は `(Some _, None)` の形で示す
```

「Int と String のリテラルは…」の箇条の最後に「同じリテラルの2つ目の枝は到達しない枝である。リテラルの位置の漏れの例は `_` で示す。」を足す。

- [ ] **Step 5: `docs/spec/core-ir.md` を直す**

1. 「コンストラクタのタグは、`data` の宣言の順の番号である。」の箇条の後に、次の箇条を足す。

   ```markdown
   - タプルの値は、タグ 0 のコンストラクタの値と同じオブジェクトで表す。要素が2つ以上の閉じたレコードの型の変数はボックス化する。`Unit` (空のレコード) は即値の `()` である。
   ```

2. 「`match` は決定木にコンパイルする。」の箇条の「決定木は、最初の行でコンストラクタのパターンを持ついちばん左の列の値で `switch` し、型のすべてのコンストラクタの枝を作る。」を、次のように直す。

   ```markdown
   決定木は、最初の行でコンストラクタ、タプル、リテラルのどれかのパターンを持ついちばん左の列を選ぶ。コンストラクタとタプルの列は、その列の値で `switch` し、型のすべてのコンストラクタの枝を作る (タプルは枝が1つである)。リテラルの列は、列に上から現れる異なるリテラルの順に、値とリテラルを比べるプリミティブと、その結果の `Bool` での `switch` を連ねる。等しい枝はそのリテラルで特殊化した行列で続け、最後の等しくない枝は残りの行列で続ける。
   ```

3. 「命令」の表の後か、`Bool` の箇条の近くに、次の箇条を足す。

   ```markdown
   - `==` と `!=` は、型検査が決めた比べ方 (`Int`、`String`、`Bool`) に応じて、別々のプリミティブ (`IntEq`、`StrEq`、`BoolEq` と、それぞれの `Ne`) に変換する。比べるプリミティブも、ほかのプリミティブと同じく引数の所有権を受け取る。
   ```

- [ ] **Step 6: `docs/spec/diagnostics.md` を直す**

「割り当て済みの番号」の表の E2005 の行の後に、次の行を足す。

```markdown
| E2006 | `NOT_COMPARABLE` | `==` か `!=` で、`Int`、`String`、`Bool` のどれでもない型の値を比べた。演算子を primary にし、比べようとした型をメッセージに出す。note で比べられる型を示す |
```

- [ ] **Step 7: `docs/implementation/architecture.md` を直す**

1. 「`eml_types` の内部」に、次の箇条を足す。

   ```markdown
   - タプルの式、型、パターンは、数字ラベルの閉じたレコード (`Record([("0", A), ("1", B)])`) に写す。表示は、ラベルが 0 から連番の閉じたレコードを `(A, B)` と書く
   - `==` と `!=` は Prelude で `a -> a -> Bool` で、参照した位置を記録しておき、関数の本体の検査の後に `a` の型から比べ方 (`Equality`) を決める。決まった比べ方は `BodyTypes::equalities` に、呼ばれる側の式の ID で入れる。決まらなければ E2006 にする
   ```

2. 「`eml_core_ir`、`eml_runtime`、`eml_interp` の内部」の `pattern.rs` の説明に「列の頭はコンストラクタ、タプル、リテラルで、リテラルの列は比べるプリミティブと `Bool` の `switch` の連なりにする」を足す。boxed の箇条に「要素が2つ以上の閉じたレコードの型 (タプル) の変数も boxed にする」を足す。

- [ ] **Step 8: `docs/implementation/status.md` を直す**

1. 「名前解決以降の実装段階」の表の 4b の行の状態を「完了」にし、内容を「タプル (数字ラベルのレコード。射影は S2)、`Int` と `String` のリテラルのパターン、`Int`・`String`・`Bool` の `==` と `!=`」に直す。
2. 「各 crate の実装状況」の `eml_hir`、`eml_types`、`eml_core_ir`、`eml_interp` の行の「段階4a まで実装済み」を「段階4b まで実装済み」にし、次を足す。
   - `eml_hir`: 「タプルの式・型・パターン、リテラルのパターン」
   - `eml_types`: 「タプルの型、リテラルのパターン、`==` の比べ方の決定と E2006、網羅性のタプルとリテラル」
   - `eml_core_ir`: 「タプルの値と分解、決定木のリテラルの列、`StrEq` と `BoolEq`」
   - `eml_interp`: 「文字列と `Bool` の比較」
3. 「次の作業の注意点」から、「段階4b のタプルは、…」と「段階4b: リテラルのパターンは、…」の項目を消す。「段階4b 以降: 同じ名前の別の型を区別して表示しない。…」の「段階4b 以降」は「S2 まで」にする。
4. 「次の作業の注意点」に、次の項目を足す。

   ```markdown
   - S2: タプルの射影 `t.0` は、開いたレコードの row と一緒に、名前付きのレコードで入れる。今は E0004 である。名前付きのレコードの実行時の表し方もそのときに決める ([直積型とレコード](../spec/records.md))
   - 型クラスを入れるとき: `==` と `!=` は、今は `Int`、`String`、`Bool` だけで比べられる。`data` の型とタプルの等値と、多相な関数の中の `==` は、型クラスの `Eq` の制約で扱う。`String` の順序の比較 (`<` など) もそのときに決める
   - 段階6: `==` の演算子の参照 `(==)` とセクション `(== 1)` を入れるときは、比べ方を参照の位置の型で決め、比べ方ごとに包む関数を作る
   - 1つの列に異なるリテラルが多い `match` は、比べる `Switch` の入れ子がリテラルの数だけ深くなる。この深さは E0013 の入れ子の上限で抑えられないので、Perceus、生存解析、verifier の再帰が深くなる。非常に長い演算子の列と同じく、深さを抑える形 (比べる連なりを join point の本体の連なりにするなど) は後に回す
   - リテラルのパターンの比較は、比べるたびに `String` のリテラルを作り、比べるプリミティブが出現の所有権を受け取るので、Perceus が出現を `dup` する。借用の比較は、借用パラメータの最適化と一緒に見直す
   ```

5. 「spec に反映済みで、実装は後の段階で扱うもの」の「直積型の構造的なレコードへの統一」の項目の「タプルは段階4b、」を「タプルは段階4b で実装した。射影は S2、」に直す。
6. 「完了した作業」の表に、次の行を足す。

   ```markdown
   | 縦の貫通 段階4b | タプル (式、型、パターン)、`Int` と `String` のリテラルのパターン、`Int`・`String`・`Bool` の `==` と `!=` を通した。タプルは型検査で数字ラベルの閉じたレコードにし、実行時はコンストラクタが1つの `data` と同じに表す。リテラルのパターンは、決定木で比べるプリミティブと `Bool` の分岐の連なりにする。`==` の比べ方は型検査が引数の型から決め、決まらなければ E2006 にする |
   ```

- [ ] **Step 9: `docs/implementation/testing.md` と `test-changes.md` を直す**

1. `testing.md` の「今あるテストの地図」に、このブランチで足したテストのファイルを足す (`eml_types` の `tuples.rs` など。実際に足したファイルを `git diff --stat main` で確かめる)。
2. `test-changes.md` の「記録」の最後に、次の見出しと箇条を足す。

   ```markdown
   ### 縦の貫通 段階4b

   - タプルが E0004 でなくなったので、`tests/ui/check-fail/not-yet-supported/tuples.em` とそのスナップショットを削除し、まだ E0004 の射影を確かめる `projections.em` に置き換えた (種類1)
   - `eml_hir/tests/lower.rs` の `constructs_of_later_stages_are_not_yet_supported` は、入力のタプルのラムダを射影のラムダに、リテラルのパターンの `match` を `let ... in` に変えた (種類1)。タプルとリテラルのパターンが E0004 でなくなったためである。後の段階の構文の E0004 を、射影と `let ... in` で確かめ続ける
   - `eml_types/tests/check.rs` の `later_stage_constructs_add_no_type_errors` は、入力のタプルのパターンとタプルを射影に変え、期待値の E0004 の行をそれに合わせた (種類1)。E0004 の跡を型検査に通しても誤りを重ねないことを、射影で確かめ続ける
   - HIR の enum に種類を足したことによる、テストの中の網羅的な `match` の追随は、期待値を変えていない (種類3)
   ```

   実装中に、この表にない変更でユーザーの承認を得たものがあれば、それも足す。

- [ ] **Step 10: 文書のリンターを通す**

Run:

```bash
for f in docs/spec/records.md docs/spec/declarations.md docs/spec/exhaustiveness.md docs/spec/core-ir.md docs/spec/diagnostics.md docs/implementation/architecture.md docs/implementation/status.md docs/implementation/testing.md docs/implementation/test-changes.md; do
  python3 ~/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py "$f" | head -5
done
```

Expected: このタスクで足した文に、文末のコロン、ダッシュ、太字にならない書き方の指摘がない。英単語の前後の半角空白と箇条書きの比率の指摘は、このリポジトリの文書の書き方なので直さない。

- [ ] **Step 11: 全体の検査を通してコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add docs/spec docs/implementation
git commit -m "Document stage 4b: tuples, literal patterns and equality

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014YGQm6f7Mtged1yVY4dmJG"
```

