# 縦の貫通 段階5a Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 線形性の誤りを E3002〜E3005 に分けて `diagnostics.md` の表どおりの場所を指し、消費漏れに `drop x` の fix を付ける。組み込みの線形型 `File` と `open` / `read_all` / `close` を、HIR からインタプリタまで通す。

**Architecture:** 誤りの判定は今の「使用回数のパスが出した `Unr` の制約が `Lin` と矛盾する」仕組みのまま、制約の由来に使った位置と使わなかった経路を入れ、報告が由来ごとに番号を分ける (Task 1)。fix のために HIR のブロックに最後の文の行の情報を持たせる (Task 2)。ランタイムに `File` のオブジェクトを足し (Task 3)、`File` を型と組み込みとインタプリタに通す (Task 4)。UI テストで `File` の誤りを確かめ (Task 5)、文書を直す (Task 6)。

**Tech Stack:** Rust (edition 2024)、rowan 0.16、la-arena 0.3、insta 1.49。新しい外部 crate は足さない。

**Spec:** `docs/superpowers/specs/2026-10-05-stage-5a-linearity-file-design.md` (段階5a の設計)。規範は `docs/spec/` の `linearity.md`、`effects.md`、`diagnostics.md`、`types.md`、`runtime.md`、`core-ir.md`。

## Global Constraints

- 作業は `main` から切ったブランチ `stage-5a` で行う。タスクごとにコミットする。コミットメッセージは英語で、末尾に次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_016zHkGFL2SnpFfio7KVz8vU
  ```

- 外部 crate は増やさない
- コードのコメントは日本語で、何をするかではなく理由を書く。規則を指すときは `docs/` のパスを書く。日本語を書く前に `yomiyasu:yomiyasu` スキルを読み、その規則に従う (CLAUDE.md)。`.em` のテストの先頭のコメントは、既存のテストと同じく英語で書く
- 各段階は `fn stage(&In) -> (Out, Vec<Diagnostic>)` の純粋関数のままにする
- 診断の help / note は eml 自身の規則の説明に限る。他の言語の書き方を前提にしたヒントは入れない
- インタプリタとランタイムの値とフレームに `Rc` と `RefCell` を使わない。`unsafe` を書かない
- 既存のテストで期待値を変えてよいのは、spec の3節の「既存のテストの変更」の表にあるものだけである (`continuation_misuse.em` とそのスナップショット、`once_continuation_through_effect_argument.em` のスナップショットの note、`eml_types/tests/effects.rs` の E3001 のテストの番号・文言・note)。どれも Task 1 で変える。種類3 は期待値を変えずに追随する。表にないテストの期待値が変わったら、変えずに止まり、差分と理由をユーザーに示して承認を得る。設計を曲げてテストを守ることはしない
- 各タスクの最後に `cargo test`、`cargo clippy --all-targets`、`cargo fmt` を通す。clippy の警告を残さない
- インラインスナップショットと UI テストの期待値は、このプランのコードが出す形を書いてある。食い違ったら、まず実装がプランのコードと一致しているかを確かめる。プランの期待値の誤り (位置の数え違いなど) だと判断した場合は、理由をコミットメッセージに書いてから直す。UI テストの stdout と実行時エラーの文言が食い違ったら、それは本物の食い違いとして原因を調べる。新しい UI テストのスナップショットは、ステップに書いた stdout、診断の番号と文言、実行時エラーの文言に一致することを確かめてから承認する
- 診断の位置の表記は、1 始まりの行と、文字数で数えた列 (`2:7`) である
- 線形性の診断の note はすべて次の文にする (以下 NOTE と書く)

  ```
  linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
  ```

- 途中のタスクでも、ワークスペース全体をビルドできる状態に保つ

## Review Focus

- 消費漏れの fix の挿入位置より前で、同じ名前の変数を束縛し直している (`let j = k` の後に `let j = 1`)。`drop j` が別の `j` を指すので fix を付けない → Task 2 の `linearity.rs` の `no_fix_when_a_shadowing_binding_comes_first`
- ブロックの最後の文が、消費されない変数を束縛する `let` そのものである。その前に `drop` を入れると束縛より前になるので fix を付けない → Task 2 の `no_fix_when_the_binding_is_the_last_statement`
- 同じ `File` を2回 `read_all` する。2回目は現在の位置 (最後) から読むので空の文字列になる → Task 4 の `run/files/read_file.em`
- UTF-8 でないファイルを読む。実行時エラーにし、パスを文言に入れる → Task 4 の `run-fail/files/not_utf8.em`
- 型の誤りがある本体で線形な値を2回使う。誤りのある本体では E3002〜E3005 を連鎖させない → Task 1 の `linearity.rs` の `a_body_with_a_type_error_reports_no_linearity_errors`

---

### Task 1: 線形性の誤りを E3002〜E3005 に分ける

使用回数の表に使った位置と使わなかった経路を持たせ、Kind の制約の由来に入れる。報告は由来ごとに番号を分ける。テストの線形な値は `once` の `k` と、それを束縛した変数 `j` で作る (`File` は Task 4)。

**Files:**
- Modify: `crates/eml_types/src/usage.rs` (全体を書き換える)
- Modify: `crates/eml_types/src/kind.rs` (`KindReason`、`UnusedPath`)
- Modify: `crates/eml_types/src/check/report.rs` (`linear_misuse`)
- Modify: `crates/eml_types/src/lib.rs` (`codes`)
- Create: `crates/eml_types/tests/linearity.rs`
- Modify (種類1): `crates/eml_types/tests/effects.rs`、`tests/ui/check-fail/linearity/continuation_misuse.em`、`crates/eml_cli/tests/snapshots/ui__check_fail@linearity__continuation_misuse.em.snap`、`crates/eml_cli/tests/snapshots/ui__check_fail@linearity__once_continuation_through_effect_argument.em.snap`

**Interfaces:**
- Produces (`eml_types::codes`): `LINEAR_VALUE_USED_TWICE` (3002)、`LINEAR_VALUE_NOT_CONSUMED` (3003)、`LINEAR_VALUE_DISCARDED` (3004)、`CONTINUATION_NOT_HANDLED` (3005)
- Produces (`eml_types::kind`, crate 内):

  ```rust
  pub(crate) enum KindReason {
      UsedMoreThanOnce { name: String, first: TextRange, second: TextRange },
      NotUsed { name: String, path: UnusedPath },
      ContinuationNotUsed { name: String, clause: TextRange },
      Discarded,
      CapturedByClause(String),
      CapturedByReturnClause(String),
      CapturedByLambda,
      Passed(String),
      Unified,
  }
  pub(crate) enum UnusedPath { Branch(TextRange), NoElse(TextRange), ScopeEnd(TextRange) }
  ```

- Produces (`usage.rs`, Task 2 が広げる): `struct Use`、`enum Missing`、`Usage::count(local, use, scope: ExprId)`、`Usage::unused_path(missing, scope) -> UnusedPath`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/linearity.rs` を作る。

```rust
//! 線形性の診断 (docs/spec/diagnostics.md の「線形性の診断」)。番号、文言、指す場所を確かめる。

use eml_test_support::{check, full};

/// `once` の操作と、純粋な条件。どのテストも7行目から関数を書く。
const HEADER: &str =
    "effect Ask where\n  ask : Unit -> Int\n\nflag : Unit -> Bool\nflag () = True\n\n";

fn diagnostics(rest: &str) -> String {
    let checked = check(&format!("{HEADER}{rest}"));
    full(&checked.files, &checked.diagnostics)
}

#[test]
fn a_value_used_twice_points_at_both_uses() {
    let rest = "twice : Unit -> Int\ntwice () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        resume j 1 + resume j 2";
    insta::assert_snapshot!(diagnostics(rest), @r"
    E3002 12:29 `j` must be used exactly once, but it is used more than once
      12:29 used again here
      12:16 first used here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    ");
}

#[test]
fn a_branch_that_does_not_use_a_value() {
    let rest = "branch : Unit -> Int\nbranch () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        if flag () then resume j 1 else 0";
    insta::assert_snapshot!(diagnostics(rest), @r"
    E3003 11:13 `j` must be used exactly once, but some paths do not use it
      11:13 `j` is bound here
      12:41 this branch does not use `j`
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: pass `j` to `drop`
    ");
}

#[test]
fn an_omitted_else_does_not_use_a_value() {
    let rest = "omitted : Unit -> Int\nomitted () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        if flag () then drop j\n        0";
    insta::assert_snapshot!(diagnostics(rest), @r"
    E3003 11:13 `j` must be used exactly once, but some paths do not use it
      11:13 `j` is bound here
      12:9 the omitted `else` does not use `j`
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: pass `j` to `drop`
    ");
}

#[test]
fn a_match_arm_that_does_not_use_a_value() {
    let rest = "arm : Bool -> Int\narm b =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        match b with\n          | True -> resume j 1\n          | False -> 0";
    insta::assert_snapshot!(diagnostics(rest), @r"
    E3003 11:13 `j` must be used exactly once, but some paths do not use it
      11:13 `j` is bound here
      14:22 this branch does not use `j`
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: pass `j` to `drop`
    ");
}

#[test]
fn a_value_never_used_points_at_the_end_of_its_scope() {
    let rest = "unused : Unit -> Int\nunused () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        0";
    insta::assert_snapshot!(diagnostics(rest), @r"
    E3003 11:13 `j` must be used exactly once, but it is not used
      11:13 `j` is bound here
      12:10 `j` is not used before the end of this scope
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: pass `j` to `drop`
    ");
}

#[test]
fn a_shadowed_value_is_not_consumed() {
    let rest = "shadowed : Unit -> Int\nshadowed () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        let j = 1\n        j";
    insta::assert_snapshot!(diagnostics(rest), @r"
    E3003 11:13 `j` must be used exactly once, but it is not used
      11:13 `j` is bound here
      13:10 `j` is not used before the end of this scope
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: pass `j` to `drop`
    ");
}

#[test]
fn a_continuation_unused_on_some_paths_points_at_the_clause() {
    let rest = "partial : Unit -> Int\npartial () =\n  handle ask () with\n    | ask () k -> if flag () then resume k 1 else 0";
    insta::assert_snapshot!(diagnostics(rest), @r"
    E3005 10:5 the continuation `k` of a `once` operation must be resumed or dropped
      10:5 this clause
      10:14 `k` is bound here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: call `resume k v` or `drop k` on every path
    ");
}

#[test]
fn a_body_with_a_type_error_reports_no_linearity_errors() {
    let rest = "broken : Unit -> Int\nbroken () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        resume j \"no\" + resume j 2";
    let checked = check(&format!("{HEADER}{rest}"));
    let codes: Vec<String> = checked
        .diagnostics
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    assert_eq!(codes, ["E2001"]);
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --test linearity`
Expected: FAIL。番号がすべて E3001 で、`UsedMoreThanOnce` などのラベルが束縛の位置だけを指す。`a_body_with_a_type_error_reports_no_linearity_errors` は今も通る。

- [ ] **Step 3: 番号を足す**

`crates/eml_types/src/lib.rs` の `codes` に足す。

```rust
    pub const LINEAR_VALUE_USED_TWICE: ErrorCode = ErrorCode(3002);
    pub const LINEAR_VALUE_NOT_CONSUMED: ErrorCode = ErrorCode(3003);
    pub const LINEAR_VALUE_DISCARDED: ErrorCode = ErrorCode(3004);
    pub const CONTINUATION_NOT_HANDLED: ErrorCode = ErrorCode(3005);
```

- [ ] **Step 4: 由来を広げる**

`crates/eml_types/src/kind.rs` の `KindOrigin` の doc を「制約が破れたときに E3001〜E3005 が指す場所と理由である」に直し、`KindReason` の先頭の2つを置き換え、`UnusedPath` を足す。

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KindReason {
    /// ある経路で2回以上使った変数。名前と、その経路で1回目と2回目に使った位置。
    UsedMoreThanOnce {
        name: String,
        first: TextRange,
        second: TextRange,
    },
    /// ある経路で使わなかった変数。
    NotUsed { name: String, path: UnusedPath },
    /// `once` の操作の節の `k` を、ある経路で `resume` も `drop` もしなかった。`clause` は節の範囲。
    ContinuationNotUsed { name: String, clause: TextRange },
    // ここから下は今のまま (`Discarded`、`CapturedByClause`、`CapturedByReturnClause`、`CapturedByLambda`、`Passed`、`Unified`)
}

/// 変数を使わなかった経路。E3003 の secondary が指す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnusedPath {
    /// `if` の枝、または `match` の枝の本体。
    Branch(TextRange),
    /// `else` のない `if`。
    NoElse(TextRange),
    /// どの経路でも使わなかった。範囲はスコープの終わりの長さ0の範囲である。
    ScopeEnd(TextRange),
}
```

- [ ] **Step 5: 使用回数のパスを書き換える**

`crates/eml_types/src/usage.rs` を次の内容にする。

```rust
//! 使用回数の数え上げ (docs/spec/linearity.md の「基本の規則」と「線形性の検査パス」)。線形な値の誤りは、ここで出した
//! `Unr` の制約が `Lin` と矛盾したときに見つかる。由来に使った位置と使わなかった経路を入れ、報告がそこを指す
//! (docs/spec/diagnostics.md の「線形性の診断」)。持ち越し規則は段階5b でこのパスに足す。

use std::collections::HashMap;

use eml_diagnostics::{TextRange, TextSize};
use eml_hir::{Body, ExprId, ExprKind, LocalId, PatId, PatKind, Res, Stmt};

use crate::check::BodyTyping;
use crate::kind::{Bound, KindOrigin, KindReason, UnusedPath};
use crate::table::Table;
use crate::ty::{Linearity, Multiplicity};

/// 制御フローの経路ごとの変数の使い方。
type Uses = HashMap<LocalId, Use>;

#[derive(Debug, Clone, Copy, Default)]
struct Use {
    /// 経路ごとの使用回数の最小と最大。2回以上は区別しないので2で頭打ちにする。
    min: u8,
    max: u8,
    /// どこかの経路で最初に使った位置。
    first: Option<TextRange>,
    /// ある経路で2回目に使った位置。
    second: Option<TextRange>,
    /// 使わなかった経路。`min` が0のときだけ持つ。
    missing: Option<Missing>,
}

/// 変数を使わなかった経路。
#[derive(Debug, Clone, Copy)]
enum Missing {
    /// `if` の枝か、`match` の枝の本体。
    Branch(ExprId),
    /// `else` のない `if`。
    NoElse(ExprId),
}

impl Use {
    fn once(at: TextRange) -> Use {
        Use {
            min: 1,
            max: 1,
            first: Some(at),
            ..Use::default()
        }
    }

    fn unused(missing: Missing) -> Use {
        Use {
            missing: Some(missing),
            ..Use::default()
        }
    }
}

pub(crate) fn constrain(body: &Body, typing: &BodyTyping, table: &mut Table) {
    // (今のコメントと `reliable` の計算をそのまま残す)
    let reliable = !body.has_errors
        && !body
            .exprs
            .iter()
            .any(|(_, expr)| matches!(expr.kind, ExprKind::Missing));
    let mut usage = Usage {
        body,
        typing,
        table,
        reliable,
    };
    let uses = usage.expr(body.root);
    for &param in &body.params {
        usage.check_pat(param, &uses, body.root);
    }
}

struct Usage<'a> {
    body: &'a Body,
    typing: &'a BodyTyping,
    table: &'a mut Table,
    reliable: bool,
}

impl Usage<'_> {
    fn expr(&mut self, id: ExprId) -> Uses {
        let body = self.body;
        let at = body.exprs[id].range;
        match &body.exprs[id].kind {
            ExprKind::Missing | ExprKind::Literal(_) => Uses::new(),
            ExprKind::Path(Res::Local(local)) => Uses::from([(*local, Use::once(at))]),
            ExprKind::Path(_) => Uses::new(),
            // 関数型の値を呼ぶことも、その値の1回の使用である (docs/spec/linearity.md の「基本の規則」)
            // 使用回数は評価の順によらないので、先に評価する引数 (`evaluate_first`) は区別しない
            ExprKind::Call { callee, args, .. } => {
                let mut uses = self.expr(*callee);
                for &arg in args {
                    let next = self.expr(arg);
                    sequence(&mut uses, next);
                }
                uses
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let mut uses = self.expr(*condition);
                let then_uses = self.expr(*then_branch);
                let otherwise = match else_branch {
                    Some(else_branch) => (self.expr(*else_branch), Missing::Branch(*else_branch)),
                    None => (Uses::new(), Missing::NoElse(id)),
                };
                sequence(
                    &mut uses,
                    join(vec![(then_uses, Missing::Branch(*then_branch)), otherwise]),
                );
                uses
            }
            ExprKind::Block { stmts, tail, .. } => {
                let mut uses = Uses::new();
                let mut bound = Vec::new();
                for stmt in stmts {
                    let next = match stmt {
                        Stmt::Let { pat, init, .. } => {
                            bound.push(*pat);
                            self.expr(*init)
                        }
                        Stmt::Expr(expr) => self.expr(*expr),
                    };
                    sequence(&mut uses, next);
                }
                if let Some(tail) = tail {
                    let next = self.expr(*tail);
                    sequence(&mut uses, next);
                }
                // ブロックの `let` は外から見えないので、ここで数え終える。外の `if` で枝を合わせると、片方の枝で
                // 束縛した変数が、もう片方の枝では「使わない」と数えられてしまうため
                for pat in bound {
                    self.check_pat(pat, &uses, id);
                    remove_bound(body, pat, &mut uses);
                }
                uses
            }
            ExprKind::Annot { expr, .. } => self.expr(*expr),
            // handle の本体と節は、捕まえた変数を先頭の引数に持つ関数に持ち上げる (docs/spec/core-ir.md)。ラムダと
            // 同じく、捕まえることを handle 式の位置での1回の使用に数え、中の使用を別に数える
            ExprKind::Handle {
                body: handled,
                effect,
                clauses,
                ret,
            } => {
                // 扱うエフェクトに `multi` の操作があれば、`k` を再開するたびに handler フレームを含む区間が写され、
                // `return` の節が何度も動きうる (docs/spec/linearity.md の「基本の規則」)
                let multi = effect.is_some_and(|effect| {
                    self.table.effect_multiplicity(effect) == Multiplicity::Multi
                });
                let mut uses = Uses::new();
                let inner = self.expr(*handled);
                let captured = self.captured_once(*handled, &[], inner, at);
                sequence(&mut uses, captured);
                if let Some(ret) = ret {
                    let inner = self.expr(ret.body);
                    let captured = self.captured_once(ret.body, &[ret.param], inner, at);
                    if multi {
                        let mut locals: Vec<LocalId> = captured.keys().copied().collect();
                        locals.sort();
                        for local in locals {
                            let name = body.locals[local].name.clone();
                            self.unr_local(local, KindReason::CapturedByReturnClause(name));
                        }
                    }
                    sequence(&mut uses, captured);
                }
                for clause in clauses {
                    let mut inner = self.expr(clause.body);
                    let bound: Vec<PatId> = clause.patterns().collect();
                    for &pat in &bound {
                        if Some(pat) == clause.k {
                            self.check_continuation(pat, &inner, clause.range, clause.body);
                        } else {
                            self.check_pat(pat, &inner, clause.body);
                        }
                        remove_bound(body, pat, &mut inner);
                    }
                    let mut captured: Vec<LocalId> = inner.keys().copied().collect();
                    captured.sort();
                    debug_assert_eq!(captured, body.captures(clause.body, &bound));
                    // 操作の節は、操作を起こすたびに呼ばれる。捕まえた変数は、何回使ってもよいものでなければならない
                    for &local in &captured {
                        let name = body.locals[local].name.clone();
                        self.unr_local(local, KindReason::CapturedByClause(name));
                    }
                    sequence(
                        &mut uses,
                        captured
                            .into_iter()
                            .map(|local| (local, Use::once(at)))
                            .collect(),
                    );
                }
                uses
            }
            ExprKind::Resume { k, arg } => {
                let mut uses = self.expr(*k);
                let next = self.expr(*arg);
                sequence(&mut uses, next);
                uses
            }
            // 枝は `if` の枝と同じく別の経路である。枝のパターンの変数は枝の外から見えないので、枝ごとに数え終える
            ExprKind::Match { scrutinee, arms } => {
                let mut uses = self.expr(*scrutinee);
                let mut branches = Vec::new();
                for arm in arms {
                    let mut inner = self.expr(arm.body);
                    self.check_pat(arm.pat, &inner, arm.body);
                    remove_bound(body, arm.pat, &mut inner);
                    branches.push((inner, Missing::Branch(arm.body)));
                }
                sequence(&mut uses, join(branches));
                uses
            }
            ExprKind::Tuple(elements) => {
                let mut uses = Uses::new();
                for &element in elements {
                    let next = self.expr(element);
                    sequence(&mut uses, next);
                }
                uses
            }
            ExprKind::Drop(value) => self.expr(*value),
            ExprKind::Lambda {
                params,
                body: lambda_body,
            } => {
                let mut inner = self.expr(*lambda_body);
                for &param in params {
                    self.check_pat(param, &inner, *lambda_body);
                    remove_bound(body, param, &mut inner);
                }
                // 残りは捕まえた変数である。捕まえることは外から見て1回の使用で、本体の中で1回でなければ `Unr`
                // にする。ラムダとその部分適用の線形性は、捕まえた値の Kind 以上になる (docs/spec/linearity.md)
                let mut captured: Vec<LocalId> = inner.keys().copied().collect();
                captured.sort();
                // 捕まえた変数の集合は、Core IR の変換が使う `lambda_captures` と同じでなければならない
                debug_assert_eq!(captured, body.lambda_captures(id));
                let mut captured_types = Vec::new();
                for &local in &captured {
                    self.count(local, inner[&local], *lambda_body);
                    if let Some(&ty) = self.typing.locals.get(local) {
                        captured_types.push(ty);
                    }
                }
                if let Some(&ty) = self.typing.exprs.get(id) {
                    self.with_origin(at, KindReason::CapturedByLambda, |table| {
                        table.closure_kinds(ty, params.len(), &captured_types)
                    });
                }
                captured
                    .into_iter()
                    .map(|local| (local, Use::once(at)))
                    .collect()
            }
        }
    }

    /// パターンが束縛した変数を数え終える。`scope` は変数が見える範囲の式で、どの経路でも使わなかったときに
    /// その終わりを指す。`_` で受けた値も使わない値なので、その型に `Unr` の制約を出す
    /// (docs/spec/linearity.md の「基本の規則」)。
    fn check_pat(&mut self, pat: PatId, uses: &Uses, scope: ExprId) {
        match &self.body.pats[pat].kind {
            PatKind::Bind(local) => {
                self.count(*local, uses.get(local).copied().unwrap_or_default(), scope);
            }
            PatKind::Wildcard => {
                if let Some(&ty) = self.typing.pats.get(pat) {
                    let range = self.body.pats[pat].range;
                    self.with_origin(range, KindReason::Discarded, |table| {
                        table.kind_at_most(ty, Bound::Const(Linearity::Unr))
                    });
                }
            }
            PatKind::Annot { pat, .. } => self.check_pat(*pat, uses, scope),
            PatKind::Con { args, .. } | PatKind::Tuple(args) => {
                for &arg in args {
                    self.check_pat(arg, uses, scope);
                }
            }
            PatKind::Unit | PatKind::Missing | PatKind::Literal(_) => {}
        }
    }

    /// 操作の節の `k`。ある経路で使わなければ、変数ではなく節を指す (docs/spec/diagnostics.md の「継続の扱い忘れ」)。
    fn check_continuation(&mut self, pat: PatId, uses: &Uses, clause: TextRange, scope: ExprId) {
        let PatKind::Bind(local) = &self.body.pats[pat].kind else {
            return self.check_pat(pat, uses, scope);
        };
        let local = *local;
        let used = uses.get(&local).copied().unwrap_or_default();
        if used.max < 2 && used.min == 0 {
            let name = self.body.locals[local].name.clone();
            self.unr_local(local, KindReason::ContinuationNotUsed { name, clause });
        } else {
            self.count(local, used, scope);
        }
    }

    /// 1回だけ動く部分 (handle の本体と `return` の節) の使用回数を、捕まえた変数の1回の使用にまとめる。`at` は
    /// handle 式の範囲で、捕まえた変数の使用の位置にする。
    fn captured_once(
        &mut self,
        root: ExprId,
        params: &[PatId],
        mut inner: Uses,
        at: TextRange,
    ) -> Uses {
        for &param in params {
            self.check_pat(param, &inner, root);
            remove_bound(self.body, param, &mut inner);
        }
        let mut captured: Vec<LocalId> = inner.keys().copied().collect();
        captured.sort();
        // 捕まえた変数の集合は、Core IR の変換が使う `captures` と同じでなければならない
        debug_assert_eq!(captured, self.body.captures(root, params));
        for &local in &captured {
            self.count(local, inner[&local], root);
        }
        captured
            .into_iter()
            .map(|local| (local, Use::once(at)))
            .collect()
    }

    /// 経路ごとの使用回数が1回でなければ、`Unr` の制約を出す。
    fn count(&mut self, local: LocalId, used: Use, scope: ExprId) {
        if (used.min, used.max) == (1, 1) {
            return;
        }
        let name = self.body.locals[local].name.clone();
        let reason = if used.max >= 2 {
            let first = used
                .first
                .expect("a variable used on some path has a first use");
            KindReason::UsedMoreThanOnce {
                name,
                first,
                second: used.second.unwrap_or(first),
            }
        } else {
            KindReason::NotUsed {
                name,
                path: self.unused_path(used.missing, scope),
            }
        };
        self.unr_local(local, reason);
    }

    fn unused_path(&self, missing: Option<Missing>, scope: ExprId) -> UnusedPath {
        let range = |id: ExprId| self.body.exprs[id].range;
        match missing {
            Some(Missing::Branch(branch)) => UnusedPath::Branch(range(branch)),
            Some(Missing::NoElse(if_expr)) => UnusedPath::NoElse(range(if_expr)),
            None => UnusedPath::ScopeEnd(TextRange::empty(self.scope_end(scope))),
        }
    }

    /// スコープの終わり。ブロックでは、最後の文の直後を指す。
    fn scope_end(&self, scope: ExprId) -> TextSize {
        let exprs = &self.body.exprs;
        if let ExprKind::Block { stmts, tail, .. } = &exprs[scope].kind {
            let last = tail.or_else(|| {
                stmts.last().map(|stmt| match stmt {
                    Stmt::Let { init, .. } => *init,
                    Stmt::Expr(expr) => *expr,
                })
            });
            if let Some(last) = last {
                return exprs[last].range.end();
            }
        }
        exprs[scope].range.end()
    }

    fn unr_local(&mut self, local: LocalId, reason: KindReason) {
        // (今のまま)
    }

    fn with_origin(
        &mut self,
        range: TextRange,
        reason: KindReason,
        constrain: impl FnOnce(&mut Table),
    ) {
        // (今のまま)
    }
}

fn remove_bound(body: &Body, pat: PatId, uses: &mut Uses) {
    for local in body.pat_bindings(pat) {
        uses.remove(&local);
    }
}

/// 続けて実行する2つの部分の使い方を足す。2回目の位置は、経路の上で早いものを選ぶ。
fn sequence(uses: &mut Uses, next: Uses) {
    for (local, b) in next {
        let a = uses.entry(local).or_default();
        let min = (a.min + b.min).min(2);
        let max = (a.max + b.max).min(2);
        let second = a
            .second
            .or(if a.first.is_some() { b.first } else { None })
            .or(b.second);
        *a = Use {
            min,
            max,
            first: a.first.or(b.first),
            second,
            missing: if min == 0 { a.missing.or(b.missing) } else { None },
        };
    }
}

/// 分岐の枝の使い方を合わせる。枝に現れない変数は、その枝では使われない。使った位置は回数の多い枝から取り、
/// 使わなかった経路は前の枝から取る。
fn join(branches: Vec<(Uses, Missing)>) -> Uses {
    let mut locals: Vec<LocalId> = branches
        .iter()
        .flat_map(|(uses, _)| uses.keys().copied())
        .collect();
    locals.sort();
    locals.dedup();
    let mut out = Uses::new();
    for local in locals {
        let mut joined: Option<Use> = None;
        for (uses, path) in &branches {
            let used = uses
                .get(&local)
                .copied()
                .unwrap_or(Use::unused(*path));
            joined = Some(match joined {
                None => used,
                Some(j) => {
                    let min = j.min.min(used.min);
                    let (first, second) = if used.max > j.max {
                        (used.first, used.second)
                    } else {
                        (j.first, j.second)
                    };
                    let missing = match (min, j.min) {
                        (0, 0) => j.missing,
                        (0, _) => used.missing,
                        _ => None,
                    };
                    Use {
                        min,
                        max: j.max.max(used.max),
                        first,
                        second,
                        missing,
                    }
                }
            });
        }
        if let Some(joined) = joined {
            out.insert(local, joined);
        }
    }
    out
}
```

`unr_local` と `with_origin` の本体と、`constrain` の `reliable` の上のコメントは今のコードをそのまま残す。

- [ ] **Step 6: 報告を由来ごとに分ける**

`crates/eml_types/src/check/report.rs` の `linear_misuse` を次に置き換える。`use crate::kind::{KindOrigin, KindReason, UnusedPath};` に直す。

```rust
const LINEAR_NOTE: &str = "linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once";

/// 線形な値の誤った使い方。破れた Kind の制約の由来から番号と指す場所を決める (docs/spec/diagnostics.md の
/// 「線形性の診断」)。表に当たらない由来 (受け渡し、単一化、捕獲) は E3001 にする。
pub(super) fn linear_misuse(file: FileId, origin: &KindOrigin) -> Diagnostic {
    match &origin.reason {
        KindReason::UsedMoreThanOnce {
            name,
            first,
            second,
        } => Diagnostic::error(
            codes::LINEAR_VALUE_USED_TWICE,
            format!("`{name}` must be used exactly once, but it is used more than once"),
            Label::new(file, *second, "used again here"),
        )
        .with_secondary(Label::new(file, *first, "first used here"))
        .with_note(LINEAR_NOTE),
        KindReason::NotUsed { name, path } => {
            let (how, label) = match path {
                UnusedPath::Branch(range) => (
                    "some paths do not use it",
                    Label::new(file, *range, format!("this branch does not use `{name}`")),
                ),
                UnusedPath::NoElse(range) => (
                    "some paths do not use it",
                    Label::new(
                        file,
                        *range,
                        format!("the omitted `else` does not use `{name}`"),
                    ),
                ),
                UnusedPath::ScopeEnd(range) => (
                    "it is not used",
                    Label::new(
                        file,
                        *range,
                        format!("`{name}` is not used before the end of this scope"),
                    ),
                ),
            };
            Diagnostic::error(
                codes::LINEAR_VALUE_NOT_CONSUMED,
                format!("`{name}` must be used exactly once, but {how}"),
                Label::new(file, origin.range, format!("`{name}` is bound here")),
            )
            .with_secondary(label)
            .with_note(LINEAR_NOTE)
            .with_help(format!("pass `{name}` to `drop`"))
        }
        KindReason::Discarded => Diagnostic::error(
            codes::LINEAR_VALUE_DISCARDED,
            "a linear value cannot be discarded with `_`",
            Label::new(file, origin.range, "this pattern discards it"),
        )
        .with_note(LINEAR_NOTE)
        .with_help("bind it to a name and pass the name to `drop`"),
        KindReason::ContinuationNotUsed { name, clause } => Diagnostic::error(
            codes::CONTINUATION_NOT_HANDLED,
            format!("the continuation `{name}` of a `once` operation must be resumed or dropped"),
            Label::new(file, *clause, "this clause"),
        )
        .with_secondary(Label::new(
            file,
            origin.range,
            format!("`{name}` is bound here"),
        ))
        .with_note(LINEAR_NOTE)
        .with_help(format!("call `resume {name} v` or `drop {name}` on every path")),
        _ => captured_or_passed(file, origin),
    }
}

/// E3001。違反した制約の由来を指す。
fn captured_or_passed(file: FileId, origin: &KindOrigin) -> Diagnostic {
    let (message, label) = match &origin.reason {
        KindReason::CapturedByClause(name) => (
            format!("`{name}` must be used exactly once, but an operation clause captures it"),
            format!("`{name}` is bound here"),
        ),
        KindReason::CapturedByReturnClause(name) => (
            format!(
                "`{name}` must be used exactly once, but the `return` clause of a handler with a `multi` operation captures it"
            ),
            format!("`{name}` is bound here"),
        ),
        KindReason::CapturedByLambda => (
            "a lambda that captures a linear value is used where it may be called any number of times"
                .to_string(),
            "this lambda".to_string(),
        ),
        KindReason::Passed(name) => (
            format!(
                "a linear value is passed to `{name}`, which may use it more than once or not at all"
            ),
            format!("`{name}` is used here"),
        ),
        KindReason::Unified
        | KindReason::UsedMoreThanOnce { .. }
        | KindReason::NotUsed { .. }
        | KindReason::ContinuationNotUsed { .. }
        | KindReason::Discarded => (
            "a linear value is used where an unrestricted value is expected".to_string(),
            "this expression".to_string(),
        ),
    };
    let mut diagnostic = Diagnostic::error(
        codes::LINEAR_VALUE_MISUSED,
        message,
        Label::new(file, origin.range, label),
    )
    .with_note(LINEAR_NOTE);
    match &origin.reason {
        KindReason::CapturedByClause(_) => {
            diagnostic = diagnostic
                .with_note("an operation clause runs each time its operation is performed");
        }
        KindReason::CapturedByReturnClause(_) => {
            diagnostic = diagnostic.with_note(
                "the `return` clause runs each time a continuation of a `multi` operation is resumed",
            );
        }
        _ => {}
    }
    diagnostic
}
```

- [ ] **Step 7: 新しいテストが通ることを確かめる**

Run: `cargo test -p eml_types --test linearity`
Expected: PASS (8件)

- [ ] **Step 8: 種類1の期待値を直す**

`cargo test` を走らせ、落ちるのが spec の表にあるテストだけであることを確かめる。

1. `crates/eml_types/tests/effects.rs` の `a_continuation_of_a_once_operation_must_be_used_exactly_once` の期待値の `---` より下を次にする。

```
    E3002 7:39 `k` must be used exactly once, but it is used more than once
      7:39 used again here
      7:26 first used here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    E3005 12:5 the continuation `k` of a `once` operation must be resumed or dropped
      12:5 this clause
      12:14 `k` is bound here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: call `resume k v` or `drop k` on every path
    E3004 17:14 a linear value cannot be discarded with `_`
      17:14 this pattern discards it
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: bind it to a name and pass the name to `drop`
    E3001 22:14 `k` must be used exactly once, but an operation clause captures it
      22:14 `k` is bound here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      note: an operation clause runs each time its operation is performed
```

2. 同じファイルのほかの E3001 のテスト (`a_closure_capturing_a_continuation_cannot_be_used_twice`、`a_continuation_cannot_pass_through_a_polymorphic_operation_parameter`、`the_return_clause_of_a_multi_handler_cannot_capture_a_linear_value`) は、note の行の `such as the continuation` を `such as files, the continuation` に直す。番号と位置は変わらない。
3. `tests/ui/check-fail/linearity/continuation_misuse.em` の先頭のコメントの `E3001` を `E3002` に直す。
4. `cargo insta test -p eml_cli --test ui` を走らせ、`cargo insta review` で2件のスナップショットを確かめて承認する。`continuation_misuse.em` は `[E3002] Error: `k` must be used exactly once, but it is used more than once` で、8:39 に「used again here」、8:26 に「first used here」、新しい note がある。`once_continuation_through_effect_argument.em` は note の行だけが変わる。

- [ ] **Step 9: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add -A crates/eml_types crates/eml_cli/tests/snapshots tests/ui/check-fail/linearity
git commit -m "Report linear misuse as E3002 to E3005 with the places the table names"
```

---

### Task 2: 消費漏れに `drop x` の fix を付ける

HIR のブロックに最後の文の行の情報を持たせ、E3003 に `drop x` の行を入れる fix を付ける。

**Files:**
- Modify: `crates/eml_syntax/src/ast.rs` (`Stmt::line_indent`)
- Modify: `crates/eml_hir/src/hir.rs` (`LineStart`、`ExprKind::Block::last_line`)、`crates/eml_hir/src/lower/expr.rs` (`lower_block`)
- Modify (種類3): `ExprKind::Block { stmts, tail }` を分解している箇所に `..` を足す (`crates/eml_hir/src/hir.rs`、`pretty.rs`、`crates/eml_types/src/check/body.rs`、`exhaustive.rs`、`crates/eml_core_ir/src/translate/expr.rs`、`translate/mod.rs`、`crates/eml_hir/tests/tuples.rs`)
- Modify: `crates/eml_diagnostics/src/lib.rs` (`Diagnostic::with_fix`)
- Modify: `crates/eml_types/src/kind.rs`、`usage.rs`、`check/report.rs`
- Modify: `crates/eml_test_support/src/lib.rs` (`fixes`)
- Test: `crates/eml_hir/tests/structure.rs`、`crates/eml_types/tests/linearity.rs`

**Interfaces:**
- Consumes: Task 1 の `KindReason::NotUsed`、`Usage::count`、`Usage::unused_path`
- Produces (`eml_syntax::ast`): `Stmt::line_indent(&self) -> Option<u32>`
- Produces (`eml_hir`):

  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub struct LineStart {
      pub offset: TextSize,
      pub indent: u32,
  }
  // ExprKind::Block に追加
  last_line: Option<LineStart>,
  ```

- Produces (`eml_diagnostics`): `Diagnostic::with_fix(self, edits: Vec<TextEdit>) -> Self`
- Produces (`eml_test_support`): `pub fn fixes(files: &SourceFiles, diagnostics: &[Diagnostic]) -> String`
- Produces (`eml_types::kind`): `KindReason::NotUsed { name, path: UnusedPath, fix: Option<DropFix> }`、`pub(crate) struct DropFix { pub offset: TextSize, pub indent: u32 }`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_hir/tests/structure.rs` に足す。`use eml_hir::{...}` に `LineStart` を足す。

```rust
#[test]
fn a_block_records_where_its_last_line_starts() {
    // fix が最後の文の前に行を入れるので、その位置と字下げを持つ (docs/spec/diagnostics.md の「線形性の診断」)
    let module = module("f : Int -> Int\nf x =\n  let y = x\n  y");
    let body = function(&module, "f").body.as_ref().expect("a body");
    let ExprKind::Block { last_line, .. } = &body.exprs[body.root].kind else {
        panic!("the body is a block");
    };
    assert_eq!(
        *last_line,
        Some(LineStart {
            offset: 35.into(),
            indent: 2,
        })
    );
}
```

`crates/eml_test_support/src/lib.rs` の `full` の後に足す。

```rust
/// fix のある診断ごとに、先頭の行に続けて編集を `開始..終了 "置き換える文字列"` の形で並べる。fix のない診断は出さない。
pub fn fixes(files: &SourceFiles, diagnostics: &[Diagnostic]) -> String {
    let mut out = String::new();
    for d in diagnostics {
        let Some(edits) = &d.fix else { continue };
        writeln!(out, "{} {}", d.code, position(files, &d.primary)).unwrap();
        for edit in edits {
            let start = files.line_col(edit.file, edit.range.start());
            let end = files.line_col(edit.file, edit.range.end());
            writeln!(out, "  {start}..{end} {:?}", edit.replacement).unwrap();
        }
    }
    out
}
```

`crates/eml_types/tests/linearity.rs` の `use` を `use eml_test_support::{check, fixes, full};` にし、次を足す。

```rust
fn fix_text(rest: &str) -> String {
    let checked = check(&format!("{HEADER}{rest}"));
    fixes(&checked.files, &checked.diagnostics)
}

#[test]
fn the_fix_inserts_drop_before_the_last_statement_of_the_scope() {
    let rest = "unused : Unit -> Int\nunused () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        0";
    insta::assert_snapshot!(fix_text(rest), @r#"
    E3003 11:13
      12:9..12:9 "drop j\n        "
    "#);
}

#[test]
fn the_fix_inserts_drop_into_a_branch_that_is_a_block() {
    let rest = "arm : Bool -> Int\narm b =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        match b with\n          | True -> resume j 1\n          | False ->\n              let n = 0\n              n";
    insta::assert_snapshot!(fix_text(rest), @r#"
    E3003 11:13
      16:15..16:15 "drop j\n              "
    "#);
}

#[test]
fn no_fix_for_a_branch_on_one_line() {
    let rest = "branch : Unit -> Int\nbranch () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        if flag () then resume j 1 else 0";
    assert_eq!(fix_text(rest), "");
}

#[test]
fn no_fix_when_a_shadowing_binding_comes_first() {
    let rest = "shadowed : Unit -> Int\nshadowed () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n        let j = 1\n        j";
    assert_eq!(fix_text(rest), "");
}

#[test]
fn no_fix_when_the_binding_is_the_last_statement() {
    let rest = "last : Unit -> Unit\nlast () =\n  handle ask () with\n    | ask () k ->\n        let j = k\n    | return x -> ()";
    assert_eq!(fix_text(rest), "");
    assert!(diagnostics(rest).starts_with("E3003 11:13"));
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_hir --test structure a_block_records && cargo test -p eml_types --test linearity`
Expected: コンパイルエラー (`LineStart` がない、`last_line` がない)

- [ ] **Step 3: 型付き AST に字下げを足す**

`crates/eml_syntax/src/ast.rs` の `Block` の impl の後に足す。

```rust
impl Stmt {
    /// 文が行の最初のトークンで始まるとき、その行の字下げ (空白の数)。fix が文の前に行を入れるのに使う。trivia は
    /// 囲むノードに付くので、文の最初のトークンの直前のトークンが、前の行の終わりからの空白である。タブは字句の段階で
    /// 誤りなので、空白だけを数えればよい (docs/spec/lexical.md)。
    pub fn line_indent(&self) -> Option<u32> {
        let first = self.syntax().first_token()?;
        let previous = first.prev_token()?;
        if previous.kind() != SyntaxKind::WHITESPACE {
            return None;
        }
        let (_, indent) = previous.text().rsplit_once('\n')?;
        Some(indent.len() as u32)
    }
}
```

- [ ] **Step 4: HIR のブロックに行の情報を持たせる**

`crates/eml_hir/src/hir.rs` に足し、`ExprKind::Block` に欄を足す。`TextSize` は `eml_diagnostics::TextSize` を使う。

```rust
/// ブロックの最後の文の先頭と、その行の字下げ。消費漏れの fix が `drop x` の行を入れる (docs/spec/diagnostics.md の
/// 「線形性の診断」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineStart {
    pub offset: TextSize,
    pub indent: u32,
}
```

```rust
    Block {
        stmts: Vec<Stmt>,
        tail: Option<ExprId>,
        /// 最後の文が行の最初のトークンで始まるときだけ持つ。
        last_line: Option<LineStart>,
    },
```

`crates/eml_hir/src/lower/expr.rs` の `lower_block` の最後を次にする。

```rust
        self.scope.truncate(mark);
        let last_line = all.last().and_then(|stmt| {
            Some(LineStart {
                offset: stmt.range().start(),
                indent: stmt.line_indent()?,
            })
        });
        self.alloc(
            ExprKind::Block {
                stmts,
                tail,
                last_line,
            },
            range,
        )
```

`ExprKind::Block { stmts, tail }` と書いている分解と構築を、`cargo build --all-targets` のエラーに従って `..` か `last_line` 付きに直す (種類3)。HIR の表示 (`pretty.rs`) は `last_line` を出さない。

- [ ] **Step 5: HIR のテストが通ることを確かめる**

Run: `cargo test -p eml_hir`
Expected: PASS

- [ ] **Step 6: fix の builder を足す**

`crates/eml_diagnostics/src/lib.rs` の `with_help` の後に足す。

```rust
    pub fn with_fix(mut self, edits: Vec<TextEdit>) -> Self {
        self.fix = Some(edits);
        self
    }
```

- [ ] **Step 7: 由来に fix を持たせる**

`crates/eml_types/src/kind.rs` の `NotUsed` を次にし、`DropFix` を足す。

```rust
    /// ある経路で使わなかった変数。`fix` は `drop x` の行を入れる先である。
    NotUsed {
        name: String,
        path: UnusedPath,
        fix: Option<DropFix>,
    },
```

```rust
/// `drop x` の行を入れる位置と、その行の字下げ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DropFix {
    pub offset: TextSize,
    pub indent: u32,
}
```

`crates/eml_types/src/usage.rs` の `count` の `NotUsed` の分岐と、新しい `drop_fix` を次にする。

```rust
        } else {
            let fix = match used.missing {
                Some(Missing::Branch(branch)) => self.drop_fix(local, branch),
                Some(Missing::NoElse(_)) => None,
                None => self.drop_fix(local, scope),
            };
            KindReason::NotUsed {
                name,
                path: self.unused_path(used.missing, scope),
                fix,
            }
        };
```

```rust
    /// 経路の式がブロックで、その最後の文が行の先頭で始まるとき、その前に `drop x` の行を入れる。最後の文が束縛より前
    /// (束縛する `let` そのもの) のときと、間に同じ名前の束縛があるときは付けない。後者では、入れた `drop x` が
    /// シャドーイングした別の変数を指してしまう。
    fn drop_fix(&self, local: LocalId, target: ExprId) -> Option<DropFix> {
        let ExprKind::Block {
            last_line: Some(line),
            ..
        } = &self.body.exprs[target].kind
        else {
            return None;
        };
        let binding = &self.body.locals[local];
        if line.offset < binding.range.end() {
            return None;
        }
        let shadowed = self.body.locals.iter().any(|(other, data)| {
            other != local
                && data.name == binding.name
                && data.range.start() > binding.range.end()
                && data.range.start() < line.offset
        });
        if shadowed {
            return None;
        }
        Some(DropFix {
            offset: line.offset,
            indent: line.indent,
        })
    }
```

`crates/eml_types/src/check/report.rs` の `NotUsed` の分岐で fix を付ける。`use eml_diagnostics::TextEdit;` を足す。

```rust
        KindReason::NotUsed { name, path, fix } => {
            // (Task 1 の `how` と `label` の計算をそのまま残す)
            let diagnostic = Diagnostic::error(
                codes::LINEAR_VALUE_NOT_CONSUMED,
                format!("`{name}` must be used exactly once, but {how}"),
                Label::new(file, origin.range, format!("`{name}` is bound here")),
            )
            .with_secondary(label)
            .with_note(LINEAR_NOTE)
            .with_help(format!("pass `{name}` to `drop`"));
            match fix {
                Some(fix) => diagnostic.with_fix(vec![TextEdit {
                    file,
                    range: TextRange::empty(fix.offset),
                    replacement: format!("drop {name}\n{}", " ".repeat(fix.indent as usize)),
                }]),
                None => diagnostic,
            }
        }
```

`captured_or_passed` の網羅の並びの `KindReason::NotUsed { .. }` はそのままでよい。

- [ ] **Step 8: テストが通ることを確かめる**

Run: `cargo test -p eml_types --test linearity`
Expected: PASS (13件)

- [ ] **Step 9: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る。既存のテストの期待値は変わらない (種類3 の追随だけ)

```bash
git add -A crates
git commit -m "Suggest inserting drop for a linear value that is not consumed"
```

---

### Task 3: ランタイムに `File` のオブジェクトを足す

**Files:**
- Create: `crates/eml_runtime/src/file.rs`
- Modify: `crates/eml_runtime/src/lib.rs`、`crates/eml_runtime/src/heap.rs`
- Test: `crates/eml_runtime/src/heap/tests.rs`

**Interfaces:**
- Produces (`eml_runtime`):

  ```rust
  pub struct FileHandle {
      pub path: String,
      pub reader: Box<dyn Read + Send + Sync>,
  }
  impl FileHandle { pub fn new(path: String, reader: Box<dyn Read + Send + Sync>) -> FileHandle }
  // Payload に追加
  File(FileHandle),
  // HeapError に追加
  NotCopyable,
  ```

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_runtime/src/heap/tests.rs` の末尾に足す。

```rust
use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::FileHandle;

/// 捨てられたことを旗で知らせる読み出し口。
struct Flagged(Arc<AtomicBool>);

impl Read for Flagged {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Ok(0)
    }
}

impl Drop for Flagged {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn file(heap: &mut Heap, dropped: &Arc<AtomicBool>) -> ObjRef {
    heap.alloc(Payload::File(FileHandle::new(
        "a.txt".to_string(),
        Box::new(Flagged(dropped.clone())),
    )))
}

#[test]
fn releasing_a_file_drops_its_reader() {
    let mut heap = Heap::new();
    let dropped = Arc::new(AtomicBool::new(false));
    let f = file(&mut heap, &dropped);
    assert_eq!(heap.live_objects(), [("File".to_string(), 1)]);
    heap.decref(f).unwrap();
    assert!(dropped.load(Ordering::SeqCst));
    assert!(heap.live_objects().is_empty());
}

#[test]
fn a_file_inside_data_is_released_with_it() {
    let mut heap = Heap::new();
    let dropped = Arc::new(AtomicBool::new(false));
    let f = file(&mut heap, &dropped);
    let pair = heap.alloc(Payload::Data {
        tag: 0,
        fields: vec![Value::Obj(f), Value::Int(1)],
    });
    heap.decref(pair).unwrap();
    assert!(dropped.load(Ordering::SeqCst));
    assert!(heap.live_objects().is_empty());
}

#[test]
fn a_shared_file_is_not_copied() {
    let mut heap = Heap::new();
    let dropped = Arc::new(AtomicBool::new(false));
    let f = file(&mut heap, &dropped);
    heap.dup(f).unwrap();
    assert!(matches!(heap.take_or_copy(f), Err(HeapError::NotCopyable)));
    assert!(!dropped.load(Ordering::SeqCst));
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_runtime`
Expected: コンパイルエラー (`FileHandle` がない)

- [ ] **Step 3: `FileHandle` を作る**

`crates/eml_runtime/src/file.rs`:

```rust
//! `File` のオブジェクトの中身 (docs/spec/runtime.md)。破棄処理はオブジェクトの解放そのもので、読み出し口を捨てると
//! OS のファイルが閉じる。読み出し口を trait object にするのは、解放で捨てられることを単体テストで確かめるためである。

use std::fmt;
use std::io::Read;

pub struct FileHandle {
    /// `open` に渡したパス。実行時エラーの文言に使う。
    pub path: String,
    /// マルチコアに備え、どのスレッドで捨ててもよいものに限る (docs/spec/runtime.md)。
    pub reader: Box<dyn Read + Send + Sync>,
}

impl FileHandle {
    pub fn new(path: String, reader: Box<dyn Read + Send + Sync>) -> FileHandle {
        FileHandle { path, reader }
    }
}

impl fmt::Debug for FileHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileHandle")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// `Payload` の比較はテストで中身を確かめるためにあり、読み出し口は比べられないのでパスだけを比べる。
impl PartialEq for FileHandle {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}
```

`crates/eml_runtime/src/lib.rs` に `mod file;` と `pub use file::FileHandle;` を足す。

- [ ] **Step 4: ヒープに `File` を足す**

`crates/eml_runtime/src/heap.rs`:

1. `use crate::FileHandle;` を足す。
2. `Payload` に変種を足す。

```rust
    /// 組み込みの線形型 `File` の値。写さない (`take_or_copy` は `NotCopyable` を返す)。
    File(FileHandle),
```

3. `DescId` に `const FILE: DescId = DescId(5);` を、`DESCRIPTORS` を6要素にして末尾に `Descriptor { name: "File" }` を足す。`Payload::desc` に `Payload::File(_) => DescId::FILE,` を足す。`DESCRIPTORS` の上の doc の「`Lin` の破棄処理を足す」の記述は、「`Lin` の破棄処理はオブジェクトの解放で済む。`File` の読み出し口は、解放で捨てると閉じる」に直す。
4. `HeapError` に `NotCopyable` を足し、`Display` に `HeapError::NotCopyable => f.write_str("a file cannot be copied"),` を足す。
5. `take_or_copy` の `is_unique` の確認の直後に足す。

```rust
        // `File` は線形で、型検査が共有させない。共有されていたら処理系の誤りなので、写さずに止める
        if matches!(self.object(obj)?.payload, Payload::File(_)) {
            return Err(HeapError::NotCopyable);
        }
```

6. `copy` に `Payload::File(_) => unreachable!("`take_or_copy` refuses to copy a file"),` を足す。
7. `children` の最後の分岐を `Payload::Frame(Frame::Io) | Payload::Str(_) | Payload::File(_) => {}` にする。

- [ ] **Step 5: テストが通ることを確かめる**

Run: `cargo test -p eml_runtime`
Expected: PASS

- [ ] **Step 6: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add -A crates/eml_runtime
git commit -m "Add file objects that close when they are released"
```

---

### Task 4: `File` を型検査からインタプリタまで通す

**Files:**
- Modify: `crates/eml_hir/src/hir.rs` (`LangItems::file`)、`lower/scope.rs` (`BuiltinItems::file`)、`builtin.rs`、`prelude.em`
- Modify: `crates/eml_types/src/data.rs`、`table/mod.rs`、`table/kinds.rs`
- Modify: `crates/eml_core_ir/src/lib.rs` (`IoOp`)、`translate/types.rs`、`pretty.rs`
- Modify: `crates/eml_interp/src/lib.rs`
- Modify: `crates/eml_cli/tests/ui.rs`
- Create: `tests/ui/run/files/input.txt`、`read_file.em`、`drop_file.em`、`file_in_data.em`、`file_in_closure.em`、`file_in_handle_body.em`
- Create: `tests/ui/run-fail/files/missing_file.em`、`not_utf8.em`、`not_utf8.txt`
- Test: `crates/eml_hir/tests/effects.rs`、`crates/eml_types/tests/linearity.rs`、`crates/eml_core_ir/tests/translate.rs`

**Interfaces:**
- Consumes: Task 3 の `FileHandle`、`Payload::File`
- Produces (`eml_hir`): `LangItems::file: TypeDefId`、`Builtin::{Open, ReadAll, Close}`
- Produces (`eml_core_ir`): `IoOp::{Open, ReadAll, Close}`
- Produces (`eml_interp`): `RunConfig::file_root: PathBuf`、`RunConfig::with_file_root(self, root: PathBuf) -> Self`、`Fault::{FileOpen { path: String, reason: &'static str }, FileRead { path: String, reason: &'static str }, FileNotUtf8 { path: String }}`

- [ ] **Step 1: 型の側の失敗するテストを書く**

`crates/eml_types/tests/linearity.rs` に足す。

```rust
/// HEADER を付けずに検査する。
fn plain(text: &str) -> String {
    let checked = check(text);
    full(&checked.files, &checked.diagnostics)
}

#[test]
fn a_file_read_and_closed_is_clean() {
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let f = open \"a.txt\"\n  let (f, text) = read_all f\n  close f\n  println text";
    assert_eq!(plain(text), "");
}

#[test]
fn a_file_closed_twice() {
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let f = open \"a.txt\"\n  close f\n  close f";
    insta::assert_snapshot!(plain(text), @r"
    E3002 5:9 `f` must be used exactly once, but it is used more than once
      5:9 used again here
      4:9 first used here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    ");
}

#[test]
fn data_with_a_file_field_is_linear() {
    let text = "data Handle = Handle File\n\nmain : Unit -> <IO> Unit\nmain () =\n  let h = Handle (open \"a.txt\")\n  drop h\n  drop h";
    insta::assert_snapshot!(plain(text), @r"
    E3002 7:8 `h` must be used exactly once, but it is used more than once
      7:8 used again here
      6:8 first used here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    ");
}

#[test]
fn recursive_and_mutually_recursive_data_with_a_file_are_linear() {
    let text = "data Files =\n  | Nil\n  | More File Files\n\ndata A =\n  | NoA\n  | SomeA B\n\ndata B = B File\n\nmain : Unit -> <IO> Unit\nmain () =\n  let fs = More (open \"a.txt\") Nil\n  let a = SomeA (B (open \"b.txt\"))\n  ()";
    let codes: Vec<String> = check(text)
        .diagnostics
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    assert_eq!(codes, ["E3003", "E3003"]);
}

#[test]
fn a_file_cannot_be_discarded_from_a_tuple() {
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let (_, text) = read_all (open \"a.txt\")\n  println text";
    insta::assert_snapshot!(plain(text), @r"
    E3004 3:8 a linear value cannot be discarded with `_`
      3:8 this pattern discards it
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: bind it to a name and pass the name to `drop`
    ");
}

#[test]
fn a_file_cannot_go_where_a_value_is_used_twice() {
    let text = "pair : a -> (a, a)\npair x = (x, x)\n\nmain : Unit -> <IO> Unit\nmain () =\n  let (f, g) = pair (open \"a.txt\")\n  close f\n  close g";
    insta::assert_snapshot!(plain(text), @r"
    E3001 6:16 a linear value is passed to `pair`, which may use it more than once or not at all
      6:16 `pair` is used here
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
    ");
}

#[test]
fn no_fix_for_a_function_body_on_one_line() {
    let text = "consume : File -> Int\nconsume f = 0";
    insta::assert_snapshot!(plain(text), @r"
    E3003 2:9 `f` must be used exactly once, but it is not used
      2:9 `f` is bound here
      2:14 `f` is not used before the end of this scope
      note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: pass `f` to `drop`
    ");
    let checked = check(text);
    assert_eq!(fixes(&checked.files, &checked.diagnostics), "");
}
```

`crates/eml_hir/tests/effects.rs` に足す。

```rust
#[test]
fn file_operations_cannot_be_handled() {
    let text = "f : Unit -> Int\nf () =\n  handle 1 with\n    | open p k -> resume k 1";
    assert!(
        diagnostics(text).contains(&"E1009 4:7 `IO` cannot be handled".to_string()),
        "{:?}",
        diagnostics(text)
    );
}
```

`crates/eml_core_ir/tests/translate.rs` に足す。

```rust
#[test]
fn file_operations_are_performed_on_the_io_handler() {
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let f = open \"a.txt\"\n  let (f, s) = read_all f\n  close f\n  println s";
    let ir = core_text(text, Pass::Translate);
    for op in ["perform open(", "perform read_all(", "perform close("] {
        assert!(ir.contains(op), "{ir}");
    }
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --test linearity && cargo test -p eml_hir --test effects && cargo test -p eml_core_ir --test translate`
Expected: FAIL (`File` と `open` が名前解決できない)

- [ ] **Step 3: HIR に `File` と3つの組み込みを足す**

1. `crates/eml_hir/src/hir.rs` の `LangItems` に `pub file: TypeDefId,` を足す (doc: 組み込みの線形型。Kind はつねに `Lin` である)。
2. `crates/eml_hir/src/lower/scope.rs`: `BuiltinItems` に `pub file: TypeDefId,` を足し、`builtin_items` で `let (int, string, unit, file) = (ty("Int"), ty("String"), ty("Unit"), ty("File"));` にし、返り値と `lang_items` の `LangItems` に `file` を渡す。`builtin_items` の doc に「`File` は組み込みの線形型 (docs/spec/effects.md)」を足す。
3. `crates/eml_hir/src/builtin.rs`: `Builtin` に `Open`、`ReadAll`、`Close` を足し (`Println` の後)、`BUILTINS` の `println` の行の後に足す。

```rust
    info(Builtin::Open, "open", Access::Named, 1),
    info(Builtin::ReadAll, "read_all", Access::Named, 1),
    info(Builtin::Close, "close", Access::Named, 1),
```

`is_io_operation` を `matches!(self, Builtin::Println | Builtin::Open | Builtin::ReadAll | Builtin::Close)` にし、doc の「段階5で `open` などを足す」を消す。

4. `crates/eml_hir/src/prelude.em` の `println` の行の後に足す。

```haskell
open : String -> <IO> File
read_all : File -> <IO> (File, String)
close : File -> <IO> Unit
```

- [ ] **Step 4: `File` とそれを含む `data` を `Lin` にする**

`crates/eml_types/src/data.rs` を次の内容にする (`collect` の `Tuple` と `Fn` のコメントは今のものを残す)。

```rust
//! データ型の Kind (docs/spec/types.md の「Kind」)。データ型の Kind はフィールドの Kind の join なので、`Option a` の
//! Kind は `a` の Kind になる。フィールドに定数の `Lin` の型 (`File` など) があれば、型引数によらず `Lin` になる。
//! どちらも宣言だけで決まるので、検査の前に1回だけ求める。

use eml_hir::{
    Constructor, LangItems, TypeDef, TypeDefId, TypeDefKind, TypeRef, TypeRefId, TypeRefKind,
    TypeVarId,
};
use la_arena::{Arena, ArenaMap};

/// 型構成子の Kind の決まり方。
#[derive(Debug, Clone)]
pub(crate) struct DataKind {
    /// 型引数の位置ごとに、Kind に効くかどうか。組み込みの型は型引数を持たないので空である。
    pub params: Vec<bool>,
    /// 定数の `Lin` の型を、関数型の外のフィールドに含む。組み込みでは `File` だけが真である。
    pub lin: bool,
}

pub(crate) fn data_kinds(
    types: &Arena<TypeDef>,
    constructors: &Arena<Constructor>,
    lang: &LangItems,
) -> ArenaMap<TypeDefId, DataKind> {
    let mut kinds: ArenaMap<TypeDefId, DataKind> = types
        .iter()
        .map(|(id, def)| {
            let kind = DataKind {
                params: vec![false; def.generics.type_vars.len()],
                lin: id == lang.file,
            };
            (id, kind)
        })
        .collect();
    // 再帰する宣言 (`List a`) と相互再帰する宣言があるので、印が増えなくなるまで繰り返す。印は増えるだけなので止まる
    loop {
        let mut changed = false;
        for (id, def) in types.iter() {
            let TypeDefKind::Data {
                constructors: ctors,
            } = &def.kind
            else {
                continue;
            };
            let mut found = Found::default();
            for &ctor in ctors {
                for &field in &constructors[ctor].fields {
                    collect(&def.types, field, &kinds, &mut found);
                }
            }
            for var in found.vars {
                let index = u32::from(var.into_raw()) as usize;
                if !kinds[id].params[index] {
                    kinds[id].params[index] = true;
                    changed = true;
                }
            }
            if found.lin && !kinds[id].lin {
                kinds[id].lin = true;
                changed = true;
            }
        }
        if !changed {
            return kinds;
        }
    }
}

#[derive(Default)]
struct Found {
    vars: Vec<TypeVarId>,
    lin: bool,
}

/// フィールドの型のうち、Kind に効く位置にある型変数と、定数の `Lin` の型。関数型の Kind はその矢印の線形性で決まり、
/// フィールドの矢印は `Unr` に固定するので、関数型の中は見ない。型変数の番号は `Generics` の並びの位置と同じである。
fn collect(
    types: &Arena<TypeRef>,
    id: TypeRefId,
    kinds: &ArenaMap<TypeDefId, DataKind>,
    out: &mut Found,
) {
    match &types[id].kind {
        TypeRefKind::Var(var) => out.vars.push(*var),
        TypeRefKind::Con(con, args) => {
            if kinds[*con].lin {
                out.lin = true;
            }
            for (index, &arg) in args.iter().enumerate() {
                if kinds[*con].params.get(index).copied().unwrap_or(false) {
                    collect(types, arg, kinds, out);
                }
            }
        }
        // タプルの Kind は要素の Kind の join なので、要素に書いた型引数はすべて効く (docs/spec/records.md の「Kind」)
        TypeRefKind::Tuple(elements) => {
            for &element in elements {
                collect(types, element, kinds, out);
            }
        }
        TypeRefKind::Fn { .. } | TypeRefKind::Error => {}
    }
}
```

`crates/eml_types/src/table/mod.rs` の `effective: ArenaMap<TypeDefId, Vec<bool>>` を `data_kinds: ArenaMap<TypeDefId, DataKind>` にし、`Table::new` で `data_kinds: crate::data::data_kinds(types, constructors, &lang),` にする (`lang` を move する行より前に置く)。

`crates/eml_types/src/table/kinds.rs` の `kind_bounds` の `TyShape::Con` の分岐を次にする。doc に「`File` を含むデータ型は定数の `Lin` である」を足す。

```rust
            TyShape::Con(id, args) => {
                let kind = &self.data_kinds[*id];
                if kind.lin {
                    return vec![Bound::Const(Linearity::Lin)];
                }
                let mut bounds = vec![Bound::Const(Linearity::Unr)];
                for (&arg, &effective) in args.iter().zip(&kind.params) {
                    if effective {
                        bounds.extend(self.kind_bounds(arg));
                    }
                }
                bounds
            }
```

- [ ] **Step 5: Core IR に3つの操作を足す**

1. `crates/eml_core_ir/src/lib.rs` の `IoOp` を次にする。

```rust
pub enum IoOp {
    Println,
    Open,
    ReadAll,
    Close,
}

impl IoOp {
    /// 表示での名前。Prelude の名前と同じである。
    pub fn name(self) -> &'static str {
        match self {
            IoOp::Println => "println",
            IoOp::Open => "open",
            IoOp::ReadAll => "read_all",
            IoOp::Close => "close",
        }
    }
}
```

2. `crates/eml_core_ir/src/pretty.rs` の `Rhs::Io(IoOp::Println, a) => format!("perform println({})", args(a)),` を `Rhs::Io(op, a) => format!("perform {}({})", op.name(), args(a)),` にし、使わなくなった `IoOp` の import を外す。
3. `crates/eml_core_ir/src/translate/types.rs` の `lowering` に足す。

```rust
        Builtin::Open => Lowering::Io(IoOp::Open),
        Builtin::ReadAll => Lowering::Io(IoOp::ReadAll),
        Builtin::Close => Lowering::Io(IoOp::Close),
```

`boxed` の `Type::Con` の分岐を `*id == module.lang.string || *id == module.lang.file || has_fields(module, *id)` にし、doc に「`File` はヒープのオブジェクトである」を足す。

- [ ] **Step 6: 型と Core IR のテストが通ることを確かめる**

Run: `cargo test -p eml_types --test linearity && cargo test -p eml_hir --test effects && cargo test -p eml_core_ir --test translate`
Expected: PASS。この時点では `eml_interp` が `IoOp` の新しい変種を扱わないのでワークスペースのビルドが通らないことがある。そのときは Step 7 まで進めてから確かめる

- [ ] **Step 7: インタプリタで3つの操作を実行する**

`crates/eml_interp/src/lib.rs`:

1. `RunConfig` に欄と builder を足す。

```rust
pub struct RunConfig {
    pub debug_heap: bool,
    /// `open` の相対パスの基準 (docs/spec/effects.md の「組み込みの `IO`」)。既定の空のパスはカレントディレクトリを指す。
    pub file_root: PathBuf,
}

impl RunConfig {
    // (with_debug_heap は今のまま)

    pub fn with_file_root(mut self, root: PathBuf) -> Self {
        self.file_root = root;
        self
    }
}
```

2. `Fault` に変種を足し、`Display` に文言を足す。

```rust
    /// `open` がファイルを開けなかった。理由は `ErrorKind` から決めた固定の文言で、OS の文言は環境ごとに違うので使わない。
    FileOpen { path: String, reason: &'static str },
    FileRead { path: String, reason: &'static str },
    FileNotUtf8 { path: String },
```

```rust
            Fault::FileOpen { path, reason } => write!(f, "cannot open `{path}`: {reason}"),
            Fault::FileRead { path, reason } => write!(f, "cannot read `{path}`: {reason}"),
            Fault::FileNotUtf8 { path } => write!(f, "`{path}` is not valid UTF-8"),
```

3. `Machine` に `file_root: &'p Path` を足し、`Machine::new(program, out, file_root)` で受け取る。`run` は `Machine::new(&program, out, &config.file_root)` で作る。
4. `bind` の `Rhs::Io(IoOp::Println, args) => { ... }` を `Rhs::Io(op, args) => { let args = self.atoms(args)?; self.io(*op, &args)? }` にし、`println` の処理を次の `io` に移す。`io` は `take_string` の近くに置く。

```rust
    /// `IO` はユーザーが handle できず、最下部の handler が必ずすぐに再開するので、継続を遡らずにその場で実行する
    /// (docs/spec/core-ir.md)。
    fn io(&mut self, op: IoOp, args: &[Value]) -> Result<Value, Fault> {
        match op {
            IoOp::Println => {
                let text = self.take_string(args[0])?;
                self.out
                    .write_str(&format!("{text}\n"))
                    .map_err(|error| Fault::Output(error.to_string()))?;
                Ok(Value::Unit)
            }
            IoOp::Open => {
                let path = self.take_string(args[0])?;
                // 絶対パスなら `join` がそのパスを返す
                let file = std::fs::File::open(self.file_root.join(&path)).map_err(|error| {
                    Fault::FileOpen {
                        path: path.clone(),
                        reason: io_reason(error.kind()),
                    }
                })?;
                let handle = FileHandle::new(path, Box::new(file));
                Ok(Value::Obj(self.heap.alloc(Payload::File(handle))))
            }
            // 受け取った `File` の参照を、そのまま返す組に移す (docs/spec/effects.md の「組み込みの `IO`」)
            IoOp::ReadAll => {
                let Value::Obj(file) = args[0] else {
                    return Err(Fault::Internal("`read_all` on a value that is not a file"));
                };
                let read = match self.heap.get_mut(file).map_err(Fault::Heap)? {
                    Payload::File(handle) => {
                        let mut bytes = Vec::new();
                        match handle.reader.read_to_end(&mut bytes) {
                            Ok(_) => String::from_utf8(bytes).map_err(|_| Fault::FileNotUtf8 {
                                path: handle.path.clone(),
                            }),
                            Err(error) => Err(Fault::FileRead {
                                path: handle.path.clone(),
                                reason: io_reason(error.kind()),
                            }),
                        }
                    }
                    _ => Err(Fault::Internal("`read_all` on a value that is not a file")),
                };
                let text = self.heap.alloc(Payload::Str(read?));
                Ok(Value::Obj(self.heap.alloc(Payload::Data {
                    tag: TUPLE,
                    fields: vec![Value::Obj(file), Value::Obj(text)],
                })))
            }
            // 破棄処理はオブジェクトの解放で、読み出し口を捨てると閉じる (docs/spec/runtime.md)
            IoOp::Close => {
                if let Value::Obj(file) = args[0] {
                    self.heap.decref(file).map_err(Fault::Heap)?;
                }
                Ok(Value::Unit)
            }
        }
    }
```

ファイルの末尾に足す。

```rust
fn io_reason(kind: std::io::ErrorKind) -> &'static str {
    match kind {
        std::io::ErrorKind::NotFound => "not found",
        std::io::ErrorKind::PermissionDenied => "permission denied",
        _ => "I/O error",
    }
}
```

import に `std::io::Read`、`std::path::{Path, PathBuf}`、`eml_core_ir::TUPLE`、`eml_runtime::FileHandle` を足す。

5. `crates/eml_cli/tests/ui.rs` の `compile_and_execute` の `config` を次にする。

```rust
    // 入力のファイルはテストの隣に置く (docs/implementation/testing.md の「UI テスト」)
    let root = path.parent().expect("a test file has a directory").to_path_buf();
    let config = RunConfig::default()
        .with_debug_heap(true)
        .with_file_root(root);
```

- [ ] **Step 8: UI テストを書く**

`tests/ui/run/files/input.txt` を、末尾の改行なしで作る。

```bash
printf 'hello from a file' > tests/ui/run/files/input.txt
```

`tests/ui/run/files/read_file.em`:

```haskell
-- Reads a file next to this test. The second `read_all` starts where the first stopped, so it reads nothing.
main : Unit -> <IO> Unit
main () =
  let f = open "input.txt"
  let (f, first) = read_all f
  let (f, rest) = read_all f
  close f
  println first
  println ("[" ++ rest ++ "]")
```

stdout は `hello from a file\n[]\n`。

`tests/ui/run/files/drop_file.em`:

```haskell
-- `drop` releases a file like `close` does.
main : Unit -> <IO> Unit
main () =
  let f = open "input.txt"
  drop f
  println "dropped"
```

stdout は `dropped\n`。

`tests/ui/run/files/file_in_data.em`:

```haskell
-- A `data` value with a file field is linear and is taken apart to reach the file.
data Handle = Handle String File

main : Unit -> <IO> Unit
main () =
  let h = Handle "input" (open "input.txt")
  match h with
    | Handle name f ->
        let (f, text) = read_all f
        close f
        println (name ++ ": " ++ text)
```

stdout は `input: hello from a file\n`。

`tests/ui/run/files/file_in_closure.em`:

```haskell
-- A lambda that captures a file is linear and can be called once.
main : Unit -> <IO> Unit
main () =
  let f = open "input.txt"
  let finish = fn () -> close f
  finish ()
  println "closed"
```

stdout は `closed\n`。

`tests/ui/run/files/file_in_handle_body.em`:

```haskell
-- The body of a handle runs at most once, so it can capture a file.
effect Ask where
  ask : Unit -> Int

use_file : File -> <Ask, IO> Int
use_file f =
  close f
  ask ()

main : Unit -> <IO> Unit
main () =
  let f = open "input.txt"
  let n =
    handle use_file f with
      | ask () k -> resume k 41
  println (show_int (n + 1))
```

stdout は `42\n`。

`tests/ui/run-fail/files/missing_file.em`:

```haskell
-- Opening a file that does not exist stops the program with a runtime error.
main : Unit -> <IO> Unit
main () =
  let f = open "missing.txt"
  close f
```

実行時エラーは ``cannot open `missing.txt`: not found in `main` ``。

`tests/ui/run-fail/files/not_utf8.txt` を作る。

```bash
printf '\xff\xfe' > tests/ui/run-fail/files/not_utf8.txt
```

`tests/ui/run-fail/files/not_utf8.em`:

```haskell
-- Reading a file that is not UTF-8 stops the program with a runtime error.
main : Unit -> <IO> Unit
main () =
  let f = open "not_utf8.txt"
  let (f, text) = read_all f
  close f
  println text
```

実行時エラーは ``` `not_utf8.txt` is not valid UTF-8 in `main` ```。

- [ ] **Step 9: UI テストを走らせてスナップショットを確かめる**

Run: `cargo insta test -p eml_cli --test ui` の後、`cargo insta review`
Expected: 新しいスナップショットが7件。stdout と実行時エラーが Step 8 に書いたものと一致することを確かめてから承認する。既存のスナップショットは変わらない

- [ ] **Step 10: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add -A crates tests/ui/run/files tests/ui/run-fail/files
git commit -m "Add File with open, read_all and close"
```

---

### Task 5: `File` の線形性の誤りの UI テスト

**Files:**
- Create: `tests/ui/check-fail/linearity/file_closed_twice.em`、`file_not_closed_on_a_branch.em`、`file_never_closed.em`、`file_discarded.em`、`file_shadowed.em`、`continuation_not_handled.em`
- Create: それぞれのスナップショット (`crates/eml_cli/tests/snapshots/`)

**Interfaces:**
- Consumes: Task 1〜4 のすべて

- [ ] **Step 1: テストを書く**

`file_closed_twice.em`:

```haskell
-- E3002: a file is linear, so closing it twice is an error.
main : Unit -> <IO> Unit
main () =
  let f = open "input.txt"
  close f
  close f
```

`file_not_closed_on_a_branch.em`:

```haskell
-- E3003: every branch must consume a file; the `else` branch forgets to close it.
main : Unit -> <IO> Unit
main () =
  let f = open "input.txt"
  if True then close f else println "kept"
```

`file_never_closed.em`:

```haskell
-- E3003: a file that is never closed or dropped is an error.
main : Unit -> <IO> Unit
main () =
  let f = open "input.txt"
  println "forgot"
```

`file_discarded.em`:

```haskell
-- E3004: a file cannot be discarded with `_`.
main : Unit -> <IO> Unit
main () =
  let (_, text) = read_all (open "input.txt")
  println text
```

`file_shadowed.em`:

```haskell
-- E3003: shadowing a file that was not consumed leaves it unconsumed.
main : Unit -> <IO> Unit
main () =
  let f = open "a.txt"
  let f = open "b.txt"
  close f
```

`continuation_not_handled.em`:

```haskell
-- E3005: a clause of a `once` operation must resume or drop its continuation on every path.
effect Ask where
  ask : Unit -> Int

main : Unit -> <IO> Unit
main () =
  let n =
    handle ask () with
      | ask () k -> 0
  println (show_int n)
```

- [ ] **Step 2: スナップショットを確かめる**

Run: `cargo insta test -p eml_cli --test ui` の後、`cargo insta review`
Expected: 新しいスナップショットが6件。それぞれ先頭のコメントの番号の診断が1件だけで、指す場所が次のとおりであることを確かめてから承認する。
- `file_closed_twice.em`: primary 6:9 (used again here)、secondary 5:9 (first used here)
- `file_not_closed_on_a_branch.em`: primary 4:7 (`f` is bound here)、secondary 5:29 (this branch does not use `f`)、help
- `file_never_closed.em`: primary 4:7、secondary 5:19 (`f` is not used before the end of this scope)、help
- `file_discarded.em`: primary 4:8、help
- `file_shadowed.em`: primary 4:7、secondary 6:10、help
- `continuation_not_handled.em`: primary 9:7 (this clause)、secondary 9:16 (`k` is bound here)、help

- [ ] **Step 3: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add tests/ui/check-fail/linearity crates/eml_cli/tests/snapshots
git commit -m "Add UI tests for linear misuse of files and continuations"
```

---

### Task 6: 文書を直す

コードは変えない。日本語を書く前に `yomiyasu:yomiyasu` スキルを読む。

**Files:**
- Modify: `docs/spec/diagnostics.md`、`linearity.md`、`types.md`、`effects.md`、`runtime.md`、`core-ir.md`
- Modify: `docs/implementation/architecture.md`、`testing.md`、`status.md`、`test-changes.md`

- [ ] **Step 1: spec を直す**

1. `docs/spec/diagnostics.md`
   - 割り当て済みの番号の表の E3001 の行の後に足す。

     ```
     | E3002 | `LINEAR_VALUE_USED_TWICE` | 線形な値を、ある経路で2回以上使った。2回目に使った位置を primary、1回目を secondary にする |
     | E3003 | `LINEAR_VALUE_NOT_CONSUMED` | 線形な値を、ある経路で使わなかった。束縛した位置を primary、使わなかった枝、省いた `else`、またはスコープの終わりを secondary にする。help で `drop` を提案し、使わなかった経路がブロックなら、その最後の文の前に `drop x` の行を入れる fix を付ける |
     | E3004 | `LINEAR_VALUE_DISCARDED` | 線形な値を `_` で受けた。パターンを指す |
     | E3005 | `CONTINUATION_NOT_HANDLED` | `once` の操作の節の `k` を、ある経路で `resume` も `drop` もしなかった。節を primary、`k` の束縛を secondary にする |
     ```

   - E3001 の行の説明を「線形な値の誤った使い方のうち、E3002〜E3005 に当たらないもの (関数への受け渡し、型の単一化、ラムダや節の捕獲)。違反した Kind の制約の由来を指す。`multi` の操作を持つ handler の `return` の節が捕まえた場合を含む」にする。
   - 「E3001 は `eml_types::codes` に置く。」と「E3xxx の残りの番号は、線形性の検査を実装するときに割り当てる。」を「E3xxx は `eml_types::codes` に置く。持ち越し規則の番号は段階5b で、射影と更新の番号は S2 で割り当てる。」にする。
   - 「番号を割り当てていない診断」の表の E3xxx の行に「`multi` の呼び出しをまたぐ線形な変数 (段階5b)」を足す。
   - 網羅性の診断の表の E4001 と E4002 の「(fix は段階5の線形性の fix と一緒に入れる)」を「(fix は、どの型にもなる仮置きの式を言語に入れるときに一緒に入れる)」にする。
2. `docs/spec/linearity.md` の「段階3a では、使用回数のパスが出した制約と……段階5で入れる。」の段落を次にする。「使用回数のパスは、使った位置と使わなかった経路を制約の由来に入れる。制約が破れたら、由来の種類から E3002 (二重使用)、E3003 (消費されていない)、E3004 (`_`)、E3005 (継続の扱い忘れ) を選び、どれにも当たらない由来 (受け渡し、単一化、捕獲) は E3001 にする。」
3. `docs/spec/types.md` の「Kind」の節で `data` の Kind を説明している箇所に、「フィールドに定数の `Lin` の型 (`File`、またはそれを含む `data`) が関数型の外に現れる `data` は、型引数によらず `Lin` である」を足す。
4. `docs/spec/effects.md` の組み込みの `IO` の表の後に足す。
   - `open` の相対パスは、実行の設定 (`RunConfig::file_root`) の基準ディレクトリから解釈する。CLI の `run` はカレントディレクトリを基準にする。絶対パスはそのまま開く。
   - `read_all` は現在の位置から最後までを UTF-8 として読み、同じ `File` と組にして返す。
   - `close f` と `drop f` は、どちらも `File` の破棄処理 (オブジェクトの解放) を呼ぶ。
   - 開けない、読めない、UTF-8 でないときは実行時エラーにする ([Core IR とインタプリタ](core-ir.md))。
5. `docs/spec/runtime.md` に `File` のオブジェクトを書く。ペイロード `File` は読み出し口と `open` に渡したパスを持ち、記述子は `File` である。`Lin` の破棄処理はオブジェクトの解放で、読み出し口を捨てると OS のファイルが閉じる。`File` は写さず、共有された `File` を写そうとすると `NotCopyable` の誤りになる。
6. `docs/spec/core-ir.md` の `IO` の命令の説明に `open`、`read_all`、`close` を足し、実行時エラーの一覧に3つの文言 (``cannot open `<path>`: <reason>``、``cannot read `<path>`: <reason>``、``` `<path>` is not valid UTF-8 ```) と、`reason` が `not found`、`permission denied`、`I/O error` のどれかであることを足す。

- [ ] **Step 2: implementation の文書を直す**

1. `docs/implementation/architecture.md` の `eml_types` の使用回数のパスの説明に「使った位置と使わなかった経路を Kind の制約の由来に入れ、報告が由来から E3001〜E3005 を選ぶ」を、HIR の説明に「ブロックは fix のために最後の文の行の情報 (`LineStart`) を持つ」を、`eml_interp` の説明に「`RunConfig::file_root` が `open` の基準ディレクトリである」を足す。
2. `docs/implementation/testing.md` の UI テストの節に「実行テストは `.em` のあるディレクトリを `open` の基準ディレクトリにする。入力のファイルはテストの隣に置く。`*.em` だけがテストとして数えられる」を足す。
3. `docs/implementation/test-changes.md` の末尾に足す。

   ```markdown
   ### 縦の貫通 段階5a

   - 線形性の誤りを E3002〜E3005 に分けたので、`tests/ui/check-fail/linearity/continuation_misuse.em` が E3001 から E3002 になり、2回目と1回目の `resume k` を指すようになった。先頭のコメントの番号も直した (種類1)
   - `eml_types/tests/effects.rs` の `a_continuation_of_a_once_operation_must_be_used_exactly_once` で、二重使用が E3002、使わない経路が E3005、`_` が E3004 になり、文言と指す場所が変わった (種類1)。節の捕獲は E3001 のまま
   - 線形性の診断の note に `files` を加えたので、`once_continuation_through_effect_argument.em` のスナップショットと、`effects.rs` のほかの E3001 のテストの note の行が変わった (種類1)
   - `ExprKind::Block` に `last_line` を足したことによる分解の追随と、`KindReason` の形の変更の追随は、期待値を変えていない (種類3)
   ```

4. `docs/implementation/status.md`
   - 実装段階の表の段階5の行を2行に分ける。「5a | 線形性の誤りの E3002〜E3005 と `drop x` の fix、組み込みの `File` と `open` / `read_all` / `close`、`Lin` のフィールドを持つ `data` | 完了」「5b | 持ち越し規則、中断時の後始末の確認 | 未着手」。
   - 各 crate の実装状況の `eml_types`、`eml_runtime`、`eml_interp`、`eml_hir` の行を段階5a まで実装済みにし、足した内容を1文ずつ足す。
   - 「次の作業の注意点」から、5a で済んだ項目を消す: 「`Lin` のフィールドを持つ `data` は……記述子に破棄処理を足す」、「使用回数のパス (`eml_types::usage`) は……このパスに足す」のうち枝ごとの一致と E3xxx の部分 (持ち越し規則は 5b として残す)、「Kind の制約の違反は、段階3a で E3001 にした……確かめ直す」(指し方は 5a で入れた)。
   - 「網羅されていない `match` と等式の fix は、段階5の線形性の診断の fix と一緒に入れる」を「網羅されていない `match` と等式の fix は、どの型にもなる仮置きの式を言語に入れるときに一緒に入れる」にする。
   - 「段階5: Core IR の `VarInfo::linearity` は、今はつねに `Unr` である……」を「Core IR の `VarInfo::linearity` は、今はつねに `Unr` である。`File` も RC で数え、`read_all` と `close` は一意性を求めないので正しく動く。`Lin` の変数を Perceus の対象から外すのは、借用と reuse の最適化と一緒に見直す」にする。
   - 「段階5: 操作の節のクロージャは……`File` が入ったら、handler フレームの解放で `Lin` の値の破棄処理を呼ぶことを確かめる」の「段階5」を「段階5b」にする。ほかの「段階5:」で始まる持ち越し規則の項目 (`σ` の上限、`multi_over_once.em`、`return` の節が2回動く問題) も「段階5b:」にする。
   - 「完了した作業」の表に「縦の貫通 段階5a | 線形性の誤りを E3002〜E3005 に分け、`diagnostics.md` の表どおりの場所を指すようにした。消費漏れに `drop x` の fix を付けた。組み込みの線形型 `File` と `open` / `read_all` / `close` を通し、破棄処理をオブジェクトの解放にした。`File` を含む `data` を `Lin` にした」を足す。

- [ ] **Step 3: 文書を確かめてコミットする**

`python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py <直したファイル>` を走らせ、新しく書いた文の指摘を見直す (既存の文書と同じ書き方の、英単語の前後の空白と箇条書きの比率の指摘は残してよい)。

```bash
git add docs
git commit -m "Document stage 5a: linearity diagnostics and File"
```
