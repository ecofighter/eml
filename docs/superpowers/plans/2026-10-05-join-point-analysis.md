# join point の解析の整理と最適化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Core IR の join point に `captures` の欄を足して生存解析を関数ごとに1回にまとめ (段階A)、その上に join point を書き換える `simplify` のパスを置く (段階B)。

**Architecture:** 段階Aでは、まず `CExpr::Join` に `captures` を足して表示し (Task 1)、生存解析を作り直して Perceus が1回の解析で `dup` / `decref` と `saved` を決めるようにし (Task 2)、verifier が `captures` を宣言として確かめるようにする (Task 3)。段階Bでは、末尾にない `if` の条件を join point の範囲に入れ (Task 5)、`simplify` の骨組みと B5 / B4 を入れ (Task 6)、B2 / B3 を足す (Task 7)。文書は段階ごとに直す (Task 4、Task 8)。

**Tech Stack:** Rust (edition 2024)、la-arena 0.3、insta 1.49。新しい外部 crate は足さない。

**Spec:** `docs/superpowers/specs/2026-10-05-join-point-analysis-design.md`。規範は `docs/spec/core-ir.md`。

## Global Constraints

- 作業は `main` から切ったブランチ `join-point-captures` で行う。タスクごとにコミットする。コミットメッセージは英語で、末尾に次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01UrPZkaPH9AUX4GoNtt1ZtF
  ```

- 外部 crate は増やさない
- コードのコメントは日本語で、何をするかではなく理由を書く。規則を指すときは `docs/` のパスを書く。日本語を書く前に `yomiyasu:yomiyasu` スキルを読み、その規則に従う (CLAUDE.md)。`.em` のテストの先頭のコメントは、既存のテストと同じく英語で書く
- `Let` の連鎖と join point の本体の連なりは長くなりうる (文の `if` が2万個続くテストがある)。これをたどる処理は、ループか作業の列で書き、Rust の再帰を使わない。再帰してよいのは `Switch` の枝と join point の範囲だけである
- 既存のテストで変えてよいのは、各タスクのステップに書いたものだけである。種類2 (スナップショットの変化) はステップに書いた期待値に変え、種類3 は期待値を変えずに追随する。ほかのテストの期待値が変わったら、変えずに止まり、差分と理由をユーザーに示して承認を得る。設計を曲げてテストを守ることはしない
- Task 2 は振る舞いを変えないリファクタリングである。Task 2 の後、すべてのスナップショットは Task 1 の後と1文字も変わってはならない
- 各タスクの最後に `cargo test`、`cargo clippy --all-targets`、`cargo fmt` を通す。clippy の警告を残さない
- インラインスナップショットの期待値は、このプランのコードが出す形を書いてある。食い違ったら、まず実装がプランのコードと一致しているかを確かめる。プランの期待値の誤り (変数の番号など) だと判断した場合は、理由をコミットメッセージに書いてから直す。新しい UI テストのスナップショットは、`cargo insta review` で、ステップに書いた stdout に一致することを確かめてから承認する

## Review Focus

- `&&` や `||` を条件にした `if` の前で束縛した文字列が、条件の中の呼び出しをまたいで生き、`if` の後で使われる。`simplify` が呼び出しの位置を動かしても、Perceus が `saved` と `dup` / `decref` を決め直し、`debug_heap` でリークも解放済みアクセスもない → Task 7 の `run/short_circuit_conditions.em` (`kept`)
- B2 で切り出した枝が、条件の値そのものを値として使う (`let v = a && b` の後の `if v then not v else v`)。枝の中の引数の使用は、そのタグの定数に置き換わらなければならない → Task 7 の `split_arms_use_the_known_tag`
- `a && b && c` のように、同じ枝へ定数のタグの jump が2つ以上来る。どの jump も同じ枝の join point に向き、右辺の副作用の順は変わらない → Task 7 の `run/short_circuit_conditions.em` (`k`、`l`、`m`)
- 外側の join point の範囲の中で、内側の join point の本体が外側へ jump する。内側の `captures` は外側の `captures` を含まなければならない → Task 1 の `nested_join_points_capture_what_outer_join_points_need`
- `captures` に余分な変数がある。verifier は受け入れ、本体で decref すれば所有権の検査も通る (最小であることは不変条件ではない) → Task 3 の `an_unused_capture_released_by_the_body_is_accepted`

---

## 段階A: 解析の整理

### Task 1: `CExpr::Join` に `captures` を足して表示する

**Files:**
- Modify: `crates/eml_core_ir/src/lib.rs` (`CExpr::Join`、`CoreFn::captures`)
- Modify: `crates/eml_core_ir/src/pretty.rs` (`Join` の表示)
- Modify: `crates/eml_core_ir/src/lower.rs` (`seq` の `CExpr::Join`)
- Modify: `crates/eml_core_ir/src/liveness.rs` (`fill_captures`)
- Modify: `crates/eml_core_ir/src/perceus.rs` (`fill_captures` を呼び、`captures` を写す)
- Modify: `crates/eml_core_ir/src/verify.rs` (パターンに `..`)
- Test: `crates/eml_core_ir/tests/lower.rs`、`crates/eml_core_ir/tests/verify.rs`

**Interfaces:**
- Produces: `CExpr::Join { join: JoinId, param: VarId, captures: Vec<VarId>, body: CExprId, scope: CExprId }`。`captures` は `VarId` の昇順で、本体が使う外側の変数 (`param` を除く、RC の対象かどうかによらない)。
- Produces: `pub fn CoreFn::captures(&self, join: JoinId) -> &[VarId]`
- Produces: 表示 `join j0(t3) [s1] {`。`captures` が空なら `join j0(t3) [] {`。

- [ ] **Step 1: ブランチを切る**

```bash
git switch -c join-point-captures
```

- [ ] **Step 2: 既存のスナップショットを新しい表示に直し (種類2)、入れ子のテストを足す**

`crates/eml_core_ir/tests/lower.rs` の3件の `join` の行を次のように変える。ほかの行は変えない。

- `a_non_tail_if_keeps_strings_used_later`: `      join j0(t3) {` → `      join j0(t3) [s1] {`
- `ifs_in_a_condition_nest_join_points`: `      join j1(t2) {` → `      join j1(t2) [] {`、`        join j0(t3) {` → `        join j0(t3) [] {`
- `calls_save_the_variables_used_after_them`: `      join j0(t5) {` → `      join j0(t5) [t2] {`

同じファイルの `calls_save_the_variables_used_after_them` の後ろに、次のテストを足す。

```rust
#[test]
fn nested_join_points_capture_what_outer_join_points_need() {
    // 内側の join point の本体は外側の join point へ jump するので、外側の本体が使う `s2` も捕まえる
    let text = "label : Bool -> Bool -> String -> String\nlabel a b s =\n  let t =\n    if a then\n      let u = if b then s ++ \"!\" else \"plain\"\n      u ++ \"?\"\n    else s\n  t ++ s\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text), @r#"
    fn label(a0, b1, s2) {
      join j0(t9) [s2] {
        let t10 = prim ++(t9, s2)
        return t10
      }
      switch a0 {
        #0 ->
          dup s2
          jump j0(s2)
        #1 ->
          join j1(t6) [s2] {
            let s7 = const "?"
            let t8 = prim ++(t6, s7)
            jump j0(t8)
          }
          switch b1 {
            #0 ->
              let s5 = const "plain"
              jump j1(s5)
            #1 ->
              let s3 = const "!"
              dup s2
              let t4 = prim ++(s2, s3)
              jump j1(t4)
          }
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

Run: `cargo test -p eml_core_ir --test lower`
Expected: 4件が FAIL (表示に `[...]` がない)

- [ ] **Step 4: `CExpr::Join` に欄を足す**

`crates/eml_core_ir/src/lib.rs` の `CExpr::Join` を次にする。

```rust
    /// `scope` の中の `Jump` が `body` に入る。`param` は `Jump` が渡す値を受ける。末尾にない `if` の続きを、
    /// ヒープにフレームを積まずに実行するために使う (docs/spec/core-ir.md)。`body` の中からは `Jump` しない。
    Join {
        join: JoinId,
        param: VarId,
        /// 本体が使う外側の変数 (`param` を除く)。RC の対象かどうかによらずすべて入れ、`VarId` の昇順に並べる。
        /// Perceus の最初の解析が埋め直すので、変換や `simplify` は空のままでよい。
        captures: Vec<VarId>,
        body: CExprId,
        scope: CExprId,
    },
```

`impl CoreFn` の `join` の後ろに足す。

```rust
    /// join point の本体が使う外側の変数。
    pub fn captures(&self, join: JoinId) -> &[VarId] {
        match self.expr(self.joins[join.0 as usize]) {
            CExpr::Join { captures, .. } => captures,
            _ => unreachable!("the join index points at join points"),
        }
    }
```

- [ ] **Step 5: 表示する**

`crates/eml_core_ir/src/pretty.rs` の `CExpr::Join` の腕を次にする。

```rust
            CExpr::Join {
                join,
                param,
                captures,
                body,
                scope,
            } => {
                let captures: Vec<String> = captures.iter().map(|&v| var(function, v)).collect();
                writeln!(
                    out,
                    "{pad}join j{}({}) [{}] {{",
                    join.0,
                    var(function, *param),
                    captures.join(", ")
                )
                .unwrap();
                expr(program, function, *body, indent + 1, out);
                writeln!(out, "{pad}}}").unwrap();
                id = *scope;
            }
```

- [ ] **Step 6: 変換で空の欄を作る**

`crates/eml_core_ir/src/lower.rs` の `seq` の `CExpr::Join` に `captures: Vec::new(),` を足す。

```rust
                Binding::Join { join, param, scope } => {
                    let expr = self.push(CExpr::Join {
                        join,
                        param,
                        captures: Vec::new(),
                        body: id,
                        scope,
                    });
```

- [ ] **Step 7: 今の生存解析で欄を埋める**

`crates/eml_core_ir/src/liveness.rs` の末尾に足す。Task 2 で、この関数を新しい解析に置き換える。

```rust
/// join point の本体が使う外側の変数を `captures` に書く。
pub(crate) fn fill_captures(function: &mut CoreFn) {
    let all = vec![true; function.vars.len()];
    let joins = liveness(function, &all).joins;
    for (join, captured) in joins {
        let node = function.joins[join.0 as usize];
        if let CExpr::Join { captures, .. } = &mut function.exprs[node.0 as usize] {
            *captures = captured.into_iter().collect();
        }
    }
}
```

- [ ] **Step 8: Perceus の前に欄を埋め、作り直す式に写す**

`crates/eml_core_ir/src/perceus.rs` を次のように直す。

`use` を `use crate::liveness::{Liveness, Vars, fill_captures, liveness, tracked};` にし、`insert_rc` の先頭に足す。

```rust
fn insert_rc(function: &mut CoreFn) {
    fill_captures(function);
    let tracked = tracked(function);
```

`transform` の `CExpr::Join` の腕で `captures` を受け取り、`Step::Join` に持たせる。

```rust
                CExpr::Join {
                    join,
                    param,
                    captures,
                    body,
                    scope,
                } => {
                    // 範囲は今の所有から始まる。本体は、引数と、本体で使う変数を1つずつ所有して始まる
                    let scope = self.transform(*scope, &owned);
                    steps.push(Step::Join {
                        join: *join,
                        param: *param,
                        captures: captures.clone(),
                        scope,
                    });
```

組み立ての腕と `enum Step` も合わせる。

```rust
                Step::Join {
                    join,
                    param,
                    captures,
                    scope,
                } => {
                    code = self.push(CExpr::Join {
                        join,
                        param,
                        captures,
                        body: code,
                        scope,
                    });
                    self.joins[join.0 as usize] = Some(code);
                }
```

```rust
enum Step {
    Join {
        join: JoinId,
        param: VarId,
        captures: Vec<VarId>,
        scope: CExprId,
    },
```

`crates/eml_core_ir/src/verify.rs` の `CExpr::Join { join, param, body, scope }` のパターンに `..` を足す。`crates/eml_core_ir/src/liveness.rs` の `CExpr::Join { join, param, body, scope }` のパターンにも `..` を足す。

- [ ] **Step 9: 手書きの IR に欄を足す (種類3)**

`crates/eml_core_ir/tests/verify.rs` の4か所の `CExpr::Join { ... }` に `captures` を足す。期待値は変えない。

- `pick` の `exprs.push(CExpr::Join {`: `captures: vec![VarId(1)],`
- `a_jump_outside_its_join_scope_is_rejected`: `captures: vec![],`
- `a_jump_that_owns_too_much_is_rejected`: `captures: vec![],`
- `a_jump_after_a_call_needs_the_variables_of_the_join_body_in_scope`: `captures: vec![VarId(0)],`

- [ ] **Step 10: テストを通す**

Run: `cargo test`
Expected: PASS。`lower.rs` の4件は Step 2 の期待値どおり。

- [ ] **Step 11: clippy と fmt を通してコミットする**

```bash
cargo clippy --all-targets && cargo fmt
git add -A crates/eml_core_ir
git commit -m "Record the outer variables of join point bodies as captures

Add a captures field to CExpr::Join and print it as join j0(t3) [s1].
For now the existing liveness fills it right before Perceus, which
copies it into the rebuilt arena. Snapshots with join points gain the
captures (kind 2); hand-built IR in verify.rs follows (kind 3).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UrPZkaPH9AUX4GoNtt1ZtF"
```

---

### Task 2: 生存解析を1回にし、Perceus が `saved` まで決める

振る舞いを変えないリファクタリングである。テストは既存のスナップショットと UI テストで、どの期待値も1文字も変えない。

**Files:**
- Modify: `crates/eml_core_ir/src/liveness.rs` (`analyze` と `BlockLiveness` を足し、`fill_captures` を消す)
- Modify: `crates/eml_core_ir/src/perceus.rs` (全体を書き直す)
- Delete: `crates/eml_core_ir/src/saved.rs`
- Modify: `crates/eml_core_ir/src/lib.rs` (`mod saved;` を消し、`Rhs::call` の説明を直す)
- Modify: `crates/eml_core_ir/src/lower.rs` (`saved::record` を消す)
- Modify: `crates/eml_core_ir/src/verify.rs` (`needs` と `uses` を `captures` から求める)

**Interfaces:**
- Consumes: Task 1 の `CExpr::Join::captures`
- Produces: `pub(crate) fn analyze(function: &mut CoreFn) -> BlockLiveness` (`captures` を埋め直す)
- Produces: `BlockLiveness::entry(&self, id: CExprId) -> &Vars`、`BlockLiveness::captures(&self, join: JoinId) -> &Vars`、`BlockLiveness::at_end(&self, expr: &CExpr) -> Vars`
- 消すもの: 古い `liveness()`、`Liveness`、`fill_captures`。verifier は `captures` から `needs` と `uses` を求める

- [ ] **Step 1: 基準を確かめる**

Run: `cargo test`
Expected: PASS (Task 1 の後の状態)

- [ ] **Step 2: 解析を書く**

`crates/eml_core_ir/src/liveness.rs` を、`tracked` を残して次の内容に置き換える。古い `Liveness`、`Task`、`liveness`、`fill_captures` は消す。

```rust
//! 変数の生存 (docs/spec/core-ir.md)。`analyze` は、関数ごとに1回、すべての変数について生存を求め、join point の
//! `captures` を埋め直す。持つのはブロックの入口 (`Switch` の枝と join point の範囲) で生きている変数だけで、`Let`
//! ごとの集合は持たない。RC の対象だけの集合が要る側は、結果を RC の対象で絞る。生存は変数ごとに独立して決まるので、
//! 絞った結果は RC の対象だけで求めた結果と一致する。

use std::collections::{BTreeSet, HashMap};

use crate::{Atom, CExpr, CExprId, CoreFn, JoinId, Linearity, VarId};
```

```rust
pub(crate) struct BlockLiveness {
    /// `Switch` の枝と join point の範囲の入口で生きている変数。
    entries: HashMap<CExprId, Vars>,
    /// join point ごとの `captures`。
    captures: Vec<Vars>,
}

impl BlockLiveness {
    pub(crate) fn entry(&self, id: CExprId) -> &Vars {
        &self.entries[&id]
    }

    pub(crate) fn captures(&self, join: JoinId) -> &Vars {
        &self.captures[join.0 as usize]
    }

    /// 連鎖を終える式の直前で生きている変数。`Join` では、範囲の入口と同じである。
    pub(crate) fn at_end(&self, expr: &CExpr) -> Vars {
        match expr {
            CExpr::Return(atom) => var_of(atom).into_iter().collect(),
            CExpr::TailCall(call) => call.atoms().iter().filter_map(var_of).collect(),
            // 範囲の外への `Jump` は verifier が報告するので、ここでは求めていない `captures` を空として扱う
            CExpr::Jump { join, arg } => {
                let mut vars = self
                    .captures
                    .get(join.0 as usize)
                    .cloned()
                    .unwrap_or_default();
                vars.extend(var_of(arg));
                vars
            }
            CExpr::Switch { scrutinee, arms } => {
                let mut vars: Vars = var_of(scrutinee).into_iter().collect();
                for &(_, arm) in arms {
                    vars.extend(self.entry(arm).iter().copied());
                }
                vars
            }
            CExpr::Join { scope, .. } => self.entry(*scope).clone(),
            CExpr::Let { .. } | CExpr::Dup { .. } | CExpr::Decref { .. } => {
                unreachable!("a chain ends at a control expression")
            }
        }
    }
}

fn var_of(atom: &Atom) -> Option<VarId> {
    match atom {
        Atom::Var(var) => Some(*var),
        _ => None,
    }
}

/// 連鎖の始まりの種類。join point の本体の入口は表に持たず、`captures` に書く。
#[derive(Clone, Copy)]
enum Start {
    Function,
    Block,
    JoinBody { join: JoinId, param: VarId },
}

enum Step {
    Visit(CExprId, Start),
    Finish {
        start: CExprId,
        kind: Start,
        bindings: Vec<CExprId>,
        end: CExprId,
    },
}

/// `Let` の連鎖と join point の本体の連なりは長くなりうるので、再帰せずに作業の列で後順にたどる。join point の
/// 本体は範囲より先に求める。範囲の中の `Jump` と、範囲の中の join point の本体からの `Jump` が、行き先の
/// `captures` を要るためである。
pub(crate) fn analyze(function: &mut CoreFn) -> BlockLiveness {
    let mut live = BlockLiveness {
        entries: HashMap::new(),
        captures: vec![Vars::new(); function.joins.len()],
    };
    let mut work = vec![Step::Visit(function.body, Start::Function)];
    while let Some(step) = work.pop() {
        match step {
            Step::Visit(start, kind) => {
                let mut bindings = Vec::new();
                let mut id = start;
                while let CExpr::Let { body, .. }
                | CExpr::Dup { body, .. }
                | CExpr::Decref { body, .. } = function.expr(id)
                {
                    bindings.push(id);
                    id = *body;
                }
                work.push(Step::Finish {
                    start,
                    kind,
                    bindings,
                    end: id,
                });
                match function.expr(id) {
                    CExpr::Join {
                        join,
                        param,
                        body,
                        scope,
                        ..
                    } => {
                        work.push(Step::Visit(*scope, Start::Block));
                        work.push(Step::Visit(
                            *body,
                            Start::JoinBody {
                                join: *join,
                                param: *param,
                            },
                        ));
                    }
                    CExpr::Switch { arms, .. } => {
                        work.extend(arms.iter().map(|&(_, arm)| Step::Visit(arm, Start::Block)));
                    }
                    _ => {}
                }
            }
            Step::Finish {
                start,
                kind,
                bindings,
                end,
            } => {
                let mut vars = live.at_end(function.expr(end));
                for &id in bindings.iter().rev() {
                    match function.expr(id) {
                        CExpr::Let { var, rhs, .. } => {
                            vars.remove(var);
                            vars.extend(rhs.atoms().iter().filter_map(var_of));
                        }
                        CExpr::Dup { var, .. } | CExpr::Decref { var, .. } => {
                            vars.insert(*var);
                        }
                        _ => unreachable!("only bindings are collected"),
                    }
                }
                match kind {
                    Start::Function => {}
                    Start::Block => {
                        live.entries.insert(start, vars);
                    }
                    Start::JoinBody { join, param } => {
                        vars.remove(&param);
                        live.captures[join.0 as usize] = vars;
                    }
                }
            }
        }
    }
    for (join, &node) in function.joins.iter().enumerate() {
        if let CExpr::Join { captures, .. } = &mut function.exprs[node.0 as usize] {
            *captures = live.captures[join].iter().copied().collect();
        }
    }
    live
}
```

- [ ] **Step 3: Perceus を書き直す**

`crates/eml_core_ir/src/perceus.rs` の全体を次にする。所有の約束と `dup` / `decref` の置き場所は Task 1 の後と同じで、集合の求め方だけが変わる。

```rust
//! Perceus の `dup` / `decref` の挿入 (docs/spec/core-ir.md)。変数を使うことを所有権の移動として扱い、後でも使う
//! 変数を複製し、使わなくなった変数をできるだけ早く捨てる。対象は `Unr` でボックス化した変数だけである。呼び出しの
//! フレームに退避する変数 (`saved`) も、同じ生存の集合からここで決める。

use std::collections::BTreeMap;

use crate::liveness::{BlockLiveness, Vars, analyze, tracked};
use crate::{Atom, CExpr, CExprId, Call, CoreFn, JoinId, Program, Rhs, VarId};

/// 変換の後に、プログラム全体にかける。変換の途中の関数ごとではなく、独立したパスにする (docs/spec/core-ir.md)。
pub(crate) fn insert(program: &mut Program) {
    for function in &mut program.functions {
        insert_rc(function);
    }
}

fn insert_rc(function: &mut CoreFn) {
    let live = analyze(function);
    let tracked = tracked(function);
    let mut pass = Pass {
        old: &function.exprs,
        tracked: &tracked,
        live: &live,
        new: Vec::new(),
        joins: vec![None; function.joins.len()],
    };
    let owned: Vars = function
        .params
        .iter()
        .copied()
        .filter(|var| tracked[var.0 as usize])
        .collect();
    let body = pass.transform(function.body, &owned);
    let Pass { new, joins, .. } = pass;
    function.body = body;
    function.exprs = new;
    function.joins = joins
        .into_iter()
        .map(|join| join.expect("every join point is rebuilt"))
        .collect();
}

struct Pass<'a> {
    old: &'a [CExpr],
    tracked: &'a [bool],
    live: &'a BlockLiveness,
    new: Vec<CExpr>,
    joins: Vec<Option<CExprId>>,
}

/// 連鎖の今の段で所有している変数の求め方。
enum Segment {
    /// 連鎖の始まり (関数、`Switch` の枝、join point の範囲と本体の入口)。所有は入口の所有そのものである。
    Start(Vars),
    /// `Let` の後。後でも使う RC の対象はすべて所有し、使わなくなった変数は使った時点で手放しているので、所有は
    /// 生きている RC の対象と、直前に束縛した変数 (使わなくても次の段の前までは所有する) になる。
    After(VarId),
}

impl Pass<'_> {
    fn atom_var(&self, atom: &Atom) -> Option<VarId> {
        match atom {
            Atom::Var(var) if self.tracked[var.0 as usize] => Some(*var),
            _ => None,
        }
    }

    /// 値が使う変数を、使う回数の分だけ並べる。
    fn uses(&self, atoms: &[Atom]) -> Vec<VarId> {
        atoms
            .iter()
            .filter_map(|atom| self.atom_var(atom))
            .collect()
    }

    fn tracked_only(&self, vars: &Vars) -> Vars {
        vars.iter()
            .copied()
            .filter(|var| self.tracked[var.0 as usize])
            .collect()
    }

    fn owned(&self, segment: &Segment, live: &Vars) -> Vars {
        match segment {
            Segment::Start(owned) => owned.clone(),
            Segment::After(last) => {
                let mut owned = self.tracked_only(live);
                if self.tracked[last.0 as usize] {
                    owned.insert(*last);
                }
                owned
            }
        }
    }

    fn push(&mut self, expr: CExpr) -> CExprId {
        self.new.push(expr);
        CExprId(self.new.len() as u32 - 1)
    }

    /// `owned` は入口で所有している変数。どの経路も `Return`、`TailCall`、`Jump` で終わり、その時点で渡すもの以外は所有して
    /// いない。
    ///
    /// `Let` の連鎖と join point の本体の連なりは長くなりうるので、その向きはループで歩いて各段を記録し、最後に
    /// 逆順で組み立てる。逆順に組み立てるときに生きている変数の集合を1つだけ更新するので、`Let` ごとの集合を持たない。
    /// 再帰するのは `Switch` の枝と join point の範囲だけで、深さは E0013 の入れ子の制限で抑えられる。
    fn transform(&mut self, id: CExprId, owned: &Vars) -> CExprId {
        let old = self.old;
        let mut steps: Vec<Step> = Vec::new();
        let mut id = id;
        let mut segment = Segment::Start(owned.clone());
        let (mut code, mut live) = loop {
            let expr = &old[id.0 as usize];
            match expr {
                CExpr::Let { var, rhs, body } => {
                    // この束縛の前で捨てうるのは、連鎖の始まりなら入口の所有、そうでなければ直前に束縛した変数だけである
                    let released = match std::mem::replace(&mut segment, Segment::After(*var)) {
                        Segment::Start(owned) => owned,
                        Segment::After(last) => self
                            .tracked[last.0 as usize]
                            .then_some(last)
                            .into_iter()
                            .collect(),
                    };
                    steps.push(Step::Let {
                        var: *var,
                        rhs: rhs.clone(),
                        released,
                    });
                    id = *body;
                }
                CExpr::Join {
                    join,
                    param,
                    captures,
                    body,
                    scope,
                } => {
                    // 範囲は今の所有から始まる。本体は、引数と、`captures` のうち RC の対象を1つずつ所有して始まる
                    let before = self.live.at_end(expr);
                    let owned = self.owned(&segment, &before);
                    let scope = self.transform(*scope, &owned);
                    steps.push(Step::Join {
                        join: *join,
                        param: *param,
                        captures: captures.clone(),
                        scope,
                        before,
                    });
                    let mut owned = self.tracked_only(self.live.captures(*join));
                    if self.tracked[param.0 as usize] {
                        owned.insert(*param);
                    }
                    segment = Segment::Start(owned);
                    id = *body;
                }
                CExpr::Return(atom) => {
                    let live = self.live.at_end(expr);
                    let owned = self.owned(&segment, &live);
                    break (self.transform_return(*atom, &owned), live);
                }
                CExpr::TailCall(call) => {
                    let live = self.live.at_end(expr);
                    let owned = self.owned(&segment, &live);
                    break (self.transform_tail_call(call, &owned), live);
                }
                CExpr::Jump { join, arg } => {
                    let live = self.live.at_end(expr);
                    let owned = self.owned(&segment, &live);
                    break (self.transform_jump(*join, *arg, &owned), live);
                }
                CExpr::Switch { scrutinee, arms } => {
                    let live = self.live.at_end(expr);
                    let owned = self.owned(&segment, &live);
                    let arms = arms
                        .iter()
                        .map(|&(tag, arm)| (tag, self.transform(arm, &owned)))
                        .collect();
                    let code = self.push(CExpr::Switch {
                        scrutinee: *scrutinee,
                        arms,
                    });
                    break (code, live);
                }
                CExpr::Dup { .. } | CExpr::Decref { .. } => {
                    unreachable!("the pass runs once on code without RC instructions")
                }
            }
        };
        for step in steps.into_iter().rev() {
            match step {
                Step::Join {
                    join,
                    param,
                    captures,
                    scope,
                    before,
                } => {
                    code = self.push(CExpr::Join {
                        join,
                        param,
                        captures,
                        body: code,
                        scope,
                    });
                    self.joins[join.0 as usize] = Some(code);
                    live = before;
                }
                Step::Let { var, rhs, released } => {
                    // `live` は、この束縛の後で生きている変数である。呼び出しは、そのうち結果の変数以外を退避する
                    let rhs = match rhs {
                        Rhs::Call { call, .. } => Rhs::Call {
                            call,
                            saved: live.iter().copied().filter(|&v| v != var).collect(),
                        },
                        rhs => rhs,
                    };
                    let atoms = rhs.atoms();
                    let uses = self.uses(&atoms);
                    code = self.push(CExpr::Let {
                        var,
                        rhs,
                        body: code,
                    });
                    let later = |used: VarId| used != var && live.contains(&used);
                    code = self.release_and_duplicate(code, &released, &uses, later);
                    live.remove(&var);
                    live.extend(atoms.iter().filter_map(|atom| match atom {
                        Atom::Var(v) => Some(*v),
                        _ => None,
                    }));
                }
            }
        }
        code
    }

    /// `code` の前で、後で使わない変数を捨てる。`uses` は使うたびに所有権を1つ受け取るので、2回目以降の使用と、
    /// 後でも使う変数の分を複製する。
    fn release_and_duplicate(
        &mut self,
        mut code: CExprId,
        owned: &Vars,
        uses: &[VarId],
        later: impl Fn(VarId) -> bool,
    ) -> CExprId {
        for &dead in owned.iter().rev() {
            if !uses.contains(&dead) && !later(dead) {
                code = self.push(CExpr::Decref {
                    var: dead,
                    body: code,
                });
            }
        }
        let mut counts: BTreeMap<VarId, usize> = BTreeMap::new();
        for &used in uses {
            *counts.entry(used).or_default() += 1;
        }
        for (&used, &count) in counts.iter().rev() {
            let dups = count - usize::from(!later(used));
            for _ in 0..dups {
                code = self.push(CExpr::Dup {
                    var: used,
                    body: code,
                });
            }
        }
        code
    }

    fn transform_return(&mut self, atom: Atom, owned: &Vars) -> CExprId {
        let returned = self.atom_var(&atom);
        let mut code = self.push(CExpr::Return(atom));
        for &var in owned.iter().rev() {
            if Some(var) != returned {
                code = self.push(CExpr::Decref { var, body: code });
            }
        }
        code
    }

    /// 末尾呼び出しの後で使う変数はないので、呼び出しが使わない変数をすべて捨てる。
    fn transform_tail_call(&mut self, call: &Call, owned: &Vars) -> CExprId {
        let uses = self.uses(&call.atoms());
        let code = self.push(CExpr::TailCall(call.clone()));
        self.release_and_duplicate(code, owned, &uses, |_| false)
    }

    /// join point の本体は、`captures` のうち RC の対象をちょうど1つずつ所有して始まる。それ以外を捨て、渡す値を
    /// 本体でも使うなら複製する。
    fn transform_jump(&mut self, join: JoinId, arg: Atom, owned: &Vars) -> CExprId {
        let needs = self.tracked_only(self.live.captures(join));
        let passed = self.atom_var(&arg);
        let mut code = self.push(CExpr::Jump { join, arg });
        for &var in owned.iter().rev() {
            if !needs.contains(&var) && Some(var) != passed {
                code = self.push(CExpr::Decref { var, body: code });
            }
        }
        if let Some(var) = passed.filter(|var| needs.contains(var)) {
            code = self.push(CExpr::Dup { var, body: code });
        }
        code
    }
}

/// `transform` が連鎖を歩いた間に記録する1段分。
enum Step {
    Join {
        join: JoinId,
        param: VarId,
        captures: Vec<VarId>,
        scope: CExprId,
        /// join point の定義の直前 (範囲の入口) で生きている変数。
        before: Vars,
    },
    Let {
        var: VarId,
        rhs: Rhs,
        /// この束縛の前で捨てうる変数。
        released: Vars,
    },
}
```

- [ ] **Step 4: `saved.rs` を消す**

```bash
git rm crates/eml_core_ir/src/saved.rs
```

`crates/eml_core_ir/src/lib.rs` から `mod saved;` を消し、`Rhs::call` の説明を次にする。

```rust
    /// 退避する変数をまだ決めていない呼び出し。Perceus が、呼び出しの後で生きている変数で埋める。
```

`crates/eml_core_ir/src/lower.rs` の `use crate::{...}` から `saved` を消し、`lower` の `saved::record(&mut program);` の行を消す。

- [ ] **Step 5: verifier を `captures` に移す**

古い `liveness` を消したので、verifier の `needs` と `uses` を `captures` から求める。`captures` は、すべての変数で求めた古い `liveness` の `joins` と同じ集合なので、検査の結果は変わらない。

`crates/eml_core_ir/src/verify.rs` の `use` を `use crate::liveness::{Vars, tracked};` にする。`Checker` から `needs` と `uses` の欄とその説明を消し、`new` から2行の `liveness` の呼び出しと、初期化の `needs,` と `uses,` を消す。

`check` の `CExpr::Join` の腕のパターンで `captures` を受け取り (`CExpr::Join { join, param, captures, body, scope }`)、本体の所有を次にする。

```rust
                    // 本体は、`captures` のうち RC の対象を1つずつ所有し、引数を束縛して始まる
                    state.owned = captures
                        .iter()
                        .copied()
                        .filter(|var| self.tracked[var.0 as usize])
                        .map(|var| (var, 1))
                        .collect();
                    self.bind(&mut state, *param)?;
                    id = *body;
```

`check_jump` を次にする。`jump` の時点では、`state.joins` に入っているので、行き先の索引は正しい `Join` を指している。

```rust
    /// 渡す値を除き、行き先の join point の `captures` のうち RC の対象を、ちょうど1つずつ所有している。
    fn check_jump(&self, mut state: State, join: JoinId, arg: &Atom) -> Result<(), String> {
        if !state.joins.contains(&join) {
            return Err(format!("a jump to `j{}` is outside its scope", join.0));
        }
        let captures = self.function.captures(join);
        // 呼び出しの後に `Jump` する経路で、本体が使う変数を退避し忘れていないこと
        for &var in captures {
            self.visible(var)?;
        }
        self.consume(&mut state, arg)?;
        let needs: Vars = captures
            .iter()
            .copied()
            .filter(|var| self.tracked[var.0 as usize])
            .collect();
        let owned: Vec<VarId> = state
            .owned
            .iter()
            .flat_map(|(&var, &count)| std::iter::repeat_n(var, count as usize))
            .collect();
        if owned.iter().copied().collect::<Vars>() != needs || owned.len() != needs.len() {
            return Err(format!(
                "a jump to `j{}` owns {} but its join needs {}",
                join.0,
                self.names(&owned),
                self.names(&needs)
            ));
        }
        Ok(())
    }
```

`HashMap` を使わなくなったら `use` から外す。

- [ ] **Step 6: テストを通す**

Run: `cargo test`
Expected: PASS。スナップショットの変更はひとつもない (`cargo insta pending-snapshots` が空)。変わったら、`saved` の並びか `dup` / `decref` の位置の誤りなので、`Segment` と `released` の扱いを Step 3 と見比べる。

- [ ] **Step 7: clippy と fmt を通してコミットする**

```bash
cargo clippy --all-targets && cargo fmt
git add -A crates/eml_core_ir
git commit -m "Compute liveness once per function inside Perceus

Rewrite liveness as analyze, which fills join point captures and keeps
the live variables only at block entries. Perceus derives the owned
set inside a chain from the live set and the last binding, updates one
live set while building backwards, and fills saved from it, so the
separate saved pass is gone. The verifier reads the needs of a jump
from the captures. No snapshot changes.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UrPZkaPH9AUX4GoNtt1ZtF"
```

---

### Task 3: verifier が `captures` を宣言として確かめる

**Files:**
- Modify: `crates/eml_core_ir/src/verify.rs`
- Test: `crates/eml_core_ir/tests/verify.rs`

**Interfaces:**
- Consumes: Task 1 の `CoreFn::captures`、Task 2 の verifier (`needs` を `captures` から求める形)
- Produces: 新しい誤りのメッセージ `the captures of `j0` are not in increasing order`、`` `j0` captures `x2`, which is not in scope ``

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/verify.rs` の末尾に足す。

```rust
#[test]
fn a_join_body_that_uses_a_variable_missing_from_its_captures_is_rejected() {
    // f n = join j0(t) [] { let u = n + t; return u } in jump j0(1)
    let exprs = vec![
        CExpr::Return(var(2)),
        CExpr::Let {
            var: VarId(2),
            rhs: Rhs::Prim(PrimOp::IntAdd, vec![var(0), var(1)]),
            body: CExprId(0),
        },
        CExpr::Jump {
            join: JoinId(0),
            arg: Atom::Int(1),
        },
        CExpr::Join {
            join: JoinId(0),
            param: VarId(1),
            captures: vec![],
            body: CExprId(1),
            scope: CExprId(2),
        },
    ];
    let f = function("f", 1, vec![int("n"), int("t"), int("u")], exprs, &[3]);
    assert_eq!(
        check(vec![f]),
        Err("`n0` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn a_capture_out_of_scope_at_its_join_is_rejected() {
    // f n = join j0(t) [x] { return t } in jump j0(n)。`x` はどこでも束縛していない
    let exprs = vec![
        CExpr::Return(var(1)),
        CExpr::Jump {
            join: JoinId(0),
            arg: var(0),
        },
        CExpr::Join {
            join: JoinId(0),
            param: VarId(1),
            captures: vec![VarId(2)],
            body: CExprId(0),
            scope: CExprId(1),
        },
    ];
    let f = function("f", 1, vec![int("n"), int("t"), int("x")], exprs, &[2]);
    assert_eq!(
        check(vec![f]),
        Err("`j0` captures `x2`, which is not in scope in `f`".to_string())
    );
}

#[test]
fn captures_out_of_order_are_rejected() {
    let exprs = vec![
        CExpr::Return(var(2)),
        CExpr::Jump {
            join: JoinId(0),
            arg: Atom::Int(1),
        },
        CExpr::Join {
            join: JoinId(0),
            param: VarId(2),
            captures: vec![VarId(1), VarId(0)],
            body: CExprId(0),
            scope: CExprId(1),
        },
    ];
    let f = function("f", 2, vec![int("a"), int("b"), int("t")], exprs, &[2]);
    assert_eq!(
        check(vec![f]),
        Err("the captures of `j0` are not in increasing order in `f`".to_string())
    );
}

#[test]
fn an_unused_capture_released_by_the_body_is_accepted() {
    // f s = join j0(t) [s] { decref s; return t } in jump j0(1)。`captures` は最小でなくてよい
    let exprs = vec![
        CExpr::Return(var(1)),
        CExpr::Decref {
            var: VarId(0),
            body: CExprId(0),
        },
        CExpr::Jump {
            join: JoinId(0),
            arg: Atom::Int(1),
        },
        CExpr::Join {
            join: JoinId(0),
            param: VarId(1),
            captures: vec![VarId(0)],
            body: CExprId(1),
            scope: CExprId(2),
        },
    ];
    let f = function("f", 1, vec![string("s"), int("t")], exprs, &[3]);
    assert_eq!(check(vec![f]), Ok(()));
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test verify`
Expected: `a_join_body_that_uses_a_variable_missing_from_its_captures_is_rejected` と `captures_out_of_order_are_rejected` が FAIL (今の verifier は、本体の範囲を `captures` に絞らず、並びも確かめない)。`a_capture_out_of_scope_at_its_join_is_rejected` は、jump の時点の検査で「`x2` is used outside its scope」と報告するので、メッセージの違いで FAIL する。`an_unused_capture_released_by_the_body_is_accepted` は Task 2 の形ですでに PASS する (余分な `captures` を受け入れ続けることを守るテスト)

- [ ] **Step 3: verifier を直す**

`crates/eml_core_ir/src/verify.rs` を次のように直す。

`check` の `CExpr::Join` の腕を次にする。

```rust
                CExpr::Join {
                    join,
                    param,
                    captures,
                    body,
                    scope,
                } => {
                    if !self.defined_joins.insert(*join) {
                        return Err(format!("`j{}` is defined twice", join.0));
                    }
                    if function.joins.get(join.0 as usize) != Some(&id) {
                        return Err(format!("the join index does not point at `j{}`", join.0));
                    }
                    if !captures.is_sorted_by(|a, b| a < b) {
                        return Err(format!(
                            "the captures of `j{}` are not in increasing order",
                            join.0
                        ));
                    }
                    for &var in captures {
                        if !self.in_scope(var) {
                            return Err(format!(
                                "`j{}` captures `{}`, which is not in scope",
                                join.0,
                                self.name(var)
                            ));
                        }
                    }
                    // 範囲は今の状態から始まり、この join point に `Jump` できる
                    let mut scope_state = state.clone();
                    scope_state.joins.insert(*join);
                    self.check_branch(*scope, scope_state)?;
                    // 本体は、関数の本体と同じく、`captures` と引数だけが範囲にある状態から始まり、`captures` のうち
                    // RC の対象を1つずつ所有する。`captures` の書き漏れは、本体での範囲の誤りとして見つかる
                    self.epoch = self.next_epoch;
                    self.next_epoch += 1;
                    for &var in captures {
                        self.enter_scope(var);
                    }
                    state.owned = captures
                        .iter()
                        .copied()
                        .filter(|var| self.tracked[var.0 as usize])
                        .map(|var| (var, 1))
                        .collect();
                    self.bind(&mut state, *param)?;
                    id = *body;
                }
```

`visible` を次の2つにする。

```rust
    fn in_scope(&self, var: VarId) -> bool {
        self.stamps[var.0 as usize] == Some(self.epoch)
    }

    fn visible(&self, var: VarId) -> Result<(), String> {
        if self.in_scope(var) {
            Ok(())
        } else {
            Err(format!("`{}` is used outside its scope", self.name(var)))
        }
    }
```

`crates/eml_core_ir/src/verify.rs` の先頭の説明の「変数と join point の範囲」の後ろに、「join point の `captures` は宣言として扱い、生存解析には頼らない」という趣旨の1文を足す。

- [ ] **Step 4: テストを通す**

Run: `cargo test`
Expected: PASS。新しい4件も既存の verifier のテストも通る。

- [ ] **Step 5: clippy と fmt を通してコミットする**

```bash
cargo clippy --all-targets && cargo fmt
git add -A crates/eml_core_ir
git commit -m "Verify join point captures as declarations

A join body now starts with only its captures and parameter in scope,
so a capture missing from the list is reported as an out-of-scope use.
Captures must also be sorted and in scope where the join is defined.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UrPZkaPH9AUX4GoNtt1ZtF"
```

---

### Task 4: 段階Aの文書を直す

**Files:**
- Modify: `docs/spec/core-ir.md`
- Modify: `docs/implementation/architecture.md`
- Modify: `docs/implementation/testing.md`
- Modify: `docs/implementation/status.md`

- [ ] **Step 1: spec を直す**

`docs/spec/core-ir.md` を次のように直す。日本語を書く前に `yomiyasu:yomiyasu` を読む。

- 命令の表の制御の行を `` `join j(x) [captures] { 本体 }` (join point)、`jump j(v)`、末尾呼び出し `` にする。
- 「末尾にない分岐の値は join point で受ける。」の項の後ろに足す: 「join point は、本体が使う外側の変数 (`captures`、引数を除く) を持つ。RC の対象かどうかによらずすべて入れる。Perceus の最初の解析が埋めるので、変換は空のままでよい。」
- 「Perceus の挿入は、…」の項の、verifier の説明を次の趣旨に直す: 生存解析は Perceus の中で関数ごとに1回だけ行い、呼び出しの `saved` も Perceus が決める。verifier は生存解析を使わず、`captures` を宣言として扱う。`Join` の時点で `captures` が範囲にあり昇順であること、本体は `captures` と引数だけが範囲にある状態から始まること、`jump` の時点で `captures` が範囲にあり、そのうち RC の対象をちょうど1つずつ所有していることを確かめる。`captures` が最小であることは確かめない。

- [ ] **Step 2: architecture を直す**

`docs/implementation/architecture.md` の Core IR の項を直す。

- 「Perceus の挿入 (`perceus.rs`) は、…」の項: 生存解析 (`liveness.rs` の `analyze`) は関数ごとに1回で、ブロックの入口 (`Switch` の枝と join point の範囲) の集合と `captures` だけを持つこと、Perceus は連鎖を逆順に組み立てるときに生きている変数の集合を1つ更新して `dup` / `decref` と `saved` を決めることを書く。連鎖の途中の所有は「生きている RC の対象 + 直前に束縛した変数」から求めることも書く。
- 「verifier (`verify.rs`) は、…」の項: `captures` を宣言として扱い、生存解析を使わないことを足す。
- 「退避のパス (`saved.rs`) は、…」の項を消し、`saved` の説明 (RC の対象でない変数と、`jump` の先の join point の `captures` も含む。verifier が確かめること) を Perceus の項に移す。

- [ ] **Step 3: テストの変更を記録する**

`docs/implementation/testing.md` の「テストの変更の記録」の最後 (「縦の貫通 段階3b」の節の後ろ) に足す。

```markdown
### join point の解析の整理

- join point に `captures` の欄を足したので、`eml_core_ir/tests/lower.rs` の `a_non_tail_if_keeps_strings_used_later`、`ifs_in_a_condition_nest_join_points`、`calls_save_the_variables_used_after_them` の `join` の行に `[...]` が付いた (種類2)。`dup` と `decref` の位置と `saved` の並びは変わっていない
- `eml_core_ir/tests/verify.rs` の手書きの Core IR に `captures` を足した (種類3)。期待値は変えていない
```

- [ ] **Step 4: status を直す**

`docs/implementation/status.md` を直す。

- 「各 crate の実装状況」の `eml_core_ir` の行の「join point と末尾呼び出し、Perceus の独立したパスと verifier」を「join point (`captures` を持つ) と末尾呼び出し、関数ごとに1回の生存解析、Perceus の独立したパス (`saved` を含む) と verifier」にする。
- 「完了した作業」の表の最後に足す: `| join point の解析の整理 | join point に本体が使う外側の変数 (`captures`) を持たせ、生存解析を関数ごとに1回にした。Perceus が呼び出しの `saved` も決め、verifier は `captures` を宣言として確かめる |`

- [ ] **Step 5: コミットする**

```bash
git add docs
git commit -m "Document join point captures and single-pass liveness

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UrPZkaPH9AUX4GoNtt1ZtF"
```

---

## 段階B: `simplify`

### Task 5: 末尾にない `if` の条件を join point の範囲に入れる (B0)

**Files:**
- Modify: `crates/eml_core_ir/src/lower.rs` (`atom` の `ExprKind::If` の腕)
- Test: `crates/eml_core_ir/tests/lower.rs`

**Interfaces:**
- Produces: 末尾にない `if` の join point の範囲は、条件の計算を含む `if` 全体になる。条件が末尾にない `if` なら、その join point は外側の join point の範囲の中にでき、本体は条件の値の `Switch` から始まる (Task 7 の B2 がこれを使う)

- [ ] **Step 1: スナップショットを新しい形に直す (種類2)**

`crates/eml_core_ir/tests/lower.rs` の `calls_save_the_variables_used_after_them` の `fn around` の部分を次にする。`twice`、`main`、`entry$main` の部分は変えない。

```
    fn around(n0, s1) {
      let t2 = prim +(n0, 1)
      join j0(t5) [t2] {
        let t6 = prim show_int(t2)
        let t7 = prim ++(t5, t6)
        return t7
      }
      let t3 = prim >(n0, 0)
      switch t3 {
        #0 ->
          jump j0(s1)
        #1 ->
          let t4 = call twice(s1) [t2]
          jump j0(t4)
      }
    }
```

`ifs_in_a_condition_nest_join_points` の `fn choose` の部分を次にする。

```
    fn choose(a0, b1) {
      join j0(t3) [] {
        let t4 = prim +(t3, 1)
        return t4
      }
      join j1(t2) [] {
        switch t2 {
          #0 ->
            jump j0(2)
          #1 ->
            jump j0(1)
        }
      }
      switch a0 {
        #0 ->
          jump j1(#0)
        #1 ->
          jump j1(b1)
      }
    }
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test lower`
Expected: `calls_save_the_variables_used_after_them` と `ifs_in_a_condition_nest_join_points` が FAIL

- [ ] **Step 3: 変換を直す**

`crates/eml_core_ir/src/lower.rs` の `fn atom` の `ExprKind::If { .. }` の腕を次にする。

```rust
            ExprKind::If { .. } => {
                // 続きの式を join point の本体にし、`if` の値をその引数で受ける。条件の計算も範囲に入れる。条件が末尾に
                // ない `if` のとき、その join point が外側の join point の範囲の中にでき、枝から外側へ jump できる
                // (docs/spec/core-ir.md)
                let join = JoinId(self.joins.len() as u32);
                self.joins.push(None);
                let scope = self.tail(id, Exit::Jump(join));
                let ty = self.ty(id);
                let param = self.new_var("t", &ty);
                out.push(Binding::Join { join, param, scope });
                Atom::Var(param)
            }
```

`Binding::Join` の説明「`scope` (枝が `Jump` する `Switch`)」を「`scope` (条件の計算と、枝が `Jump` する `Switch`)」にする。

- [ ] **Step 4: テストを通す**

Run: `cargo test`
Expected: PASS。`lower.rs` の2件は Step 1 の期待値どおり。UI テストの出力は変わらない。

- [ ] **Step 5: clippy と fmt を通してコミットする**

```bash
cargo clippy --all-targets && cargo fmt
git add -A crates/eml_core_ir
git commit -m "Lower the condition of a non-tail if inside its join scope

Defining a join point does nothing at run time, so the whole if,
including its condition, now becomes the scope. A join point for a
non-tail if in a condition then lies inside the outer join scope, and
its body starts with the switch on the condition value. Two snapshots
move their bindings (kind 2).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UrPZkaPH9AUX4GoNtt1ZtF"
```

---

### Task 6: `simplify` の骨組み、小さな本体の転送 (B5)、使われない join point の削除 (B4)

**Files:**
- Create: `crates/eml_core_ir/src/simplify.rs`
- Modify: `crates/eml_core_ir/src/lib.rs` (`mod simplify;`)
- Modify: `crates/eml_core_ir/src/lower.rs` (Perceus の前に `simplify` を呼ぶ)
- Test: `crates/eml_core_ir/tests/lower.rs`

**Interfaces:**
- Consumes: Task 5 の変換の形
- Produces: `pub(crate) fn simplify(program: &mut Program)`。`struct Simplify<'a> { function: &'a mut CoreFn }` とメソッド `expr`、`set`、`reachable`、`jumps`、`parents`、`present`、`replace`、`forward_small_bodies`、`remove_unused`、`renumber`。自由関数 `children(&CExpr) -> Vec<CExprId>`、`replace_child(&mut CExpr, CExprId, CExprId)`、`movable(Atom, VarId) -> bool`。Task 7 は `unit_params` の欄、`push`、`split_known_tags`、`inline_single_jumps` を足す

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/lower.rs` の末尾に足す。

```rust
#[test]
fn a_join_point_that_returns_its_value_is_forwarded_and_removed() {
    // `let y = if ...` の後に `y` を返すだけなら、join point の本体を各 jump の位置に写し、join point を消す
    let text = "select : Bool -> Int\nselect c =\n  let y = if c then 1 else 2\n  y\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text), @r"
    fn select(c0) {
      switch c0 {
        #0 ->
          return 2
        #1 ->
          return 1
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
fn a_join_point_that_passes_its_value_on_is_forwarded() {
    // 内側の join point の本体は外側への jump だけなので、内側への jump を外側への jump にする
    let text = "nested : Bool -> Bool -> Int\nnested a b =\n  let y =\n    if a then\n      let z = if b then 1 else 2\n      z\n    else 3\n  y + 1\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text), @r"
    fn nested(a0, b1) {
      join j0(t3) [] {
        let t4 = prim +(t3, 1)
        return t4
      }
      switch a0 {
        #0 ->
          jump j0(3)
        #1 ->
          switch b1 {
            #0 ->
              jump j0(2)
            #1 ->
              jump j0(1)
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

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test lower`
Expected: 2件が FAIL (join point が残っている)

- [ ] **Step 3: `simplify.rs` を書く**

`crates/eml_core_ir/src/simplify.rs` を作る。

```rust
//! join point を書き換える最適化 (docs/spec/core-ir.md)。変換の後、Perceus の前に置く。RC の命令がまだないので、
//! 所有権を扱わずに書き換えられる。`captures` は古くなりうるが、Perceus の最初の解析が埋め直す。
//!
//! 書き換えは式のアリーナの上でその場で行う。木から外れた式はアリーナに残り、Perceus がアリーナを作り直すときに
//! 捨てる。そのため、jump の位置と親は、根からたどれる式だけで求める。

use crate::{Atom, CExpr, CExprId, CoreFn, JoinId, Program, VarId};

pub(crate) fn simplify(program: &mut Program) {
    for function in &mut program.functions {
        let mut pass = Simplify { function };
        pass.forward_small_bodies();
        pass.remove_unused();
        pass.renumber();
    }
}

struct Simplify<'a> {
    function: &'a mut CoreFn,
}

impl Simplify<'_> {
    fn expr(&self, id: CExprId) -> &CExpr {
        self.function.expr(id)
    }

    fn set(&mut self, id: CExprId, expr: CExpr) {
        self.function.exprs[id.0 as usize] = expr;
    }

    /// 根からたどれる式。長い連鎖で再帰しないように、作業の列でたどる。
    fn reachable(&self) -> Vec<CExprId> {
        let mut order = Vec::new();
        let mut work = vec![self.function.body];
        while let Some(id) = work.pop() {
            order.push(id);
            work.extend(children(self.expr(id)));
        }
        order
    }

    /// join point ごとの、木の中の jump の位置。
    fn jumps(&self) -> Vec<Vec<CExprId>> {
        let mut jumps = vec![Vec::new(); self.function.joins.len()];
        for id in self.reachable() {
            if let CExpr::Jump { join, .. } = self.expr(id) {
                jumps[join.0 as usize].push(id);
            }
        }
        jumps
    }

    /// 木の中の各式の親。根と、木から外れた式は `None` である。
    fn parents(&self) -> Vec<Option<CExprId>> {
        let mut parents = vec![None; self.function.exprs.len()];
        for id in self.reachable() {
            for child in children(self.expr(id)) {
                parents[child.0 as usize] = Some(id);
            }
        }
        parents
    }

    /// join point ごとの、木の中に定義が残っているか。
    fn present(&self) -> Vec<bool> {
        let mut present = vec![false; self.function.joins.len()];
        for id in self.reachable() {
            if let CExpr::Join { join, .. } = self.expr(id) {
                present[join.0 as usize] = true;
            }
        }
        present
    }

    /// 木の中の `old` を `new` で置き換える。`old` の親が `new` を指すようにし、親の表も直す。
    fn replace(&mut self, parents: &mut [Option<CExprId>], old: CExprId, new: CExprId) {
        match parents[old.0 as usize] {
            None => self.function.body = new,
            Some(parent) => replace_child(&mut self.function.exprs[parent.0 as usize], old, new),
        }
        parents[new.0 as usize] = parents[old.0 as usize];
    }

    /// B5: 本体が1命令だけの join point への jump を、その命令に置き換える。対象は、値を返す本体と、値を別の join point
    /// へ渡す本体である。別の join point は元の join point を範囲に含むので、jump の位置からも届く。
    fn forward_small_bodies(&mut self) {
        let jumps = self.jumps();
        for (index, sites) in jumps.iter().enumerate() {
            let CExpr::Join { param, body, .. } = self.expr(self.function.joins[index]) else {
                unreachable!("the join index points at join points")
            };
            let (param, body) = (*param, *body);
            let small = match self.expr(body) {
                CExpr::Return(value) if movable(*value, param) => self.expr(body).clone(),
                CExpr::Jump { arg, .. } if movable(*arg, param) => self.expr(body).clone(),
                _ => continue,
            };
            for &site in sites {
                let CExpr::Jump { arg, .. } = self.expr(site) else {
                    unreachable!("a jump site holds a jump")
                };
                let passed = *arg;
                let with = |atom: Atom| if atom == Atom::Var(param) { passed } else { atom };
                let copy = match &small {
                    CExpr::Return(value) => CExpr::Return(with(*value)),
                    CExpr::Jump { join, arg } => CExpr::Jump {
                        join: *join,
                        arg: with(*arg),
                    },
                    _ => unreachable!("only returns and jumps are small"),
                };
                self.set(site, copy);
            }
        }
    }

    /// B4: jump がなくなった join point を消し、範囲だけを残す。
    fn remove_unused(&mut self) {
        let jumps = self.jumps();
        let present = self.present();
        let mut parents = self.parents();
        for (index, sites) in jumps.iter().enumerate() {
            if !sites.is_empty() || !present[index] {
                continue;
            }
            let node = self.function.joins[index];
            let CExpr::Join { scope, .. } = self.expr(node) else {
                unreachable!("the join index points at join points")
            };
            let scope = *scope;
            self.replace(&mut parents, node, scope);
        }
    }

    /// 木に残った join point に、元の番号の順を保って 0 から番号を振り直し、索引を作り直す。消した join point が
    /// なければ番号は変わらない。
    fn renumber(&mut self) {
        let present = self.present();
        let mut numbers = vec![None; present.len()];
        let mut joins = Vec::new();
        for (index, &here) in present.iter().enumerate() {
            if here {
                numbers[index] = Some(JoinId(joins.len() as u32));
                joins.push(self.function.joins[index]);
            }
        }
        for id in self.reachable() {
            if let CExpr::Join { join, .. } | CExpr::Jump { join, .. } =
                &mut self.function.exprs[id.0 as usize]
            {
                *join = numbers[join.0 as usize].expect("a jump targets a join point in the tree");
            }
        }
        self.function.joins = joins;
    }
}

fn children(expr: &CExpr) -> Vec<CExprId> {
    match expr {
        CExpr::Let { body, .. } | CExpr::Dup { body, .. } | CExpr::Decref { body, .. } => {
            vec![*body]
        }
        CExpr::Join { body, scope, .. } => vec![*body, *scope],
        CExpr::Switch { arms, .. } => arms.iter().map(|&(_, arm)| arm).collect(),
        CExpr::Return(_) | CExpr::Jump { .. } | CExpr::TailCall(_) => Vec::new(),
    }
}

fn replace_child(expr: &mut CExpr, old: CExprId, new: CExprId) {
    let slots: Vec<&mut CExprId> = match expr {
        CExpr::Let { body, .. } | CExpr::Dup { body, .. } | CExpr::Decref { body, .. } => {
            vec![body]
        }
        CExpr::Join { body, scope, .. } => vec![body, scope],
        CExpr::Switch { arms, .. } => arms.iter_mut().map(|(_, arm)| arm).collect(),
        CExpr::Return(_) | CExpr::Jump { .. } | CExpr::TailCall(_) => Vec::new(),
    };
    let slot = slots
        .into_iter()
        .find(|slot| **slot == old)
        .expect("the parent points at the child");
    *slot = new;
}

/// 本体を jump の位置に写してよい値。引数は渡す値に置き換わり、定数はどこでも同じである。ほかの変数は、写すと
/// その変数の使用が増えるので写さない。
fn movable(atom: Atom, param: VarId) -> bool {
    match atom {
        Atom::Var(var) => var == param,
        _ => true,
    }
}
```

- [ ] **Step 4: パスをつなぐ**

`crates/eml_core_ir/src/lib.rs` の `mod saved;` があった位置の近くに `mod simplify;` を足す (`mod` はアルファベット順)。

`crates/eml_core_ir/src/lower.rs` の `use crate::{...}` に `simplify` を足し、`lower` の `perceus::insert(&mut program);` の前に足す。

```rust
    simplify::simplify(&mut program);
    perceus::insert(&mut program);
```

- [ ] **Step 5: テストを通す**

Run: `cargo test`
Expected: PASS。Step 1 の2件は期待値どおり。ほかのスナップショットと UI テストは変わらない。`eml_core_ir/tests/verify.rs` の `a_long_run_of_if_statements_is_verified_in_linear_time` と `eml_interp/tests/run.rs` の `long_sequence_of_if_statements_does_not_overflow_the_stack` も通る (`simplify` が長い連鎖を再帰せずにたどる)

- [ ] **Step 6: clippy と fmt を通してコミットする**

```bash
cargo clippy --all-targets && cargo fmt
git add -A crates/eml_core_ir
git commit -m "Add a simplify pass that forwards small join bodies

The pass runs after lowering and before Perceus and rewrites the arena
in place. A jump to a join point whose body only returns or passes on
its value or a constant becomes that instruction, join points left
without jumps are removed, and the remaining ones are renumbered in
their original order.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UrPZkaPH9AUX4GoNtt1ZtF"
```

---

### Task 7: 分かっているタグの jump (B2) と、jump が1つの join point の展開 (B3)

**Files:**
- Modify: `crates/eml_core_ir/src/simplify.rs`
- Modify: `crates/eml_core_ir/src/lib.rs` (`CExpr::atoms_mut`、`Rhs::atoms_mut`、`Call::atoms_mut`)
- Test: `crates/eml_core_ir/tests/lower.rs`
- Create: `tests/ui/run/short_circuit_conditions.em`
- Create: `crates/eml_cli/tests/snapshots/ui__run@short_circuit_conditions.em.snap` (insta が作る)

**Interfaces:**
- Consumes: Task 6 の `Simplify` とヘルパー
- Produces: `Simplify::split_known_tags`、`Simplify::inline_single_jumps`、`Simplify::substitute`、`Simplify::new_unit_param`。`pub(crate) fn CExpr::atoms_mut(&mut self) -> Vec<&mut Atom>`、`Rhs::atoms_mut`、`Call::atoms_mut`

- [ ] **Step 1: 振る舞いを守る UI テストを書く**

`tests/ui/run/short_circuit_conditions.em` を作る。`simplify` の前でも通るテストで、書き換えの後も同じ出力になることを確かめる。

```haskell
-- `&&` and `||` in `if` conditions evaluate the right operand only when the left one does not decide the result,
-- and keep the order of effects. A string bound before the conditions lives across the calls in them.
noisy : String -> Bool -> <IO> Bool
noisy name b =
  println name
  b

main : Unit -> <IO> Unit
main () =
  let kept = "kept"
  if noisy "a" False && noisy "b" True then println "and: yes" else println "and: no"
  if noisy "c" True || noisy "d" False then println "or: yes" else println "or: no"
  if noisy "e" True && noisy "f" True then println "both: yes" else println "both: no"
  if noisy "k" True && noisy "l" False && noisy "m" True then println "three: yes" else println "three: no"
  let n = if noisy "g" False || noisy "h" True then 1 else 2
  println (show_int n)
  println kept
```

Run: `cargo test -p eml_cli --test ui`
Expected: 新しいスナップショットが pending になって FAIL。`cargo insta review` で、stdout が次と一致することを確かめて承認する。

```
--- stdout ---
a
and: no
c
or: yes
e
f
both: yes
k
l
three: no
g
h
1
kept
--- stderr ---
```

- [ ] **Step 2: Core IR の失敗するテストを書き、`choose` を新しい形に直す (種類2)**

`crates/eml_core_ir/tests/lower.rs` の `ifs_in_a_condition_nest_join_points` の `fn choose` の部分を次にする (Task 5 の形からの変化)。

```
    fn choose(a0, b1) {
      join j0(t3) [] {
        let t4 = prim +(t3, 1)
        return t4
      }
      switch a0 {
        #0 ->
          jump j0(2)
        #1 ->
          let t2 = b1
          switch t2 {
            #0 ->
              jump j0(2)
            #1 ->
              jump j0(1)
          }
      }
    }
```

同じファイルの末尾に足す。

```rust
#[test]
fn known_tags_jump_straight_to_their_arm() {
    // `a && b` の偽は分かっているので、`a` が偽の枝は条件の値で分岐せずに `2` を返す
    let text = "both : Bool -> Bool -> Int\nboth a b = if a && b then 1 else 2\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text), @r"
    fn both(a0, b1) {
      switch a0 {
        #0 ->
          return 2
        #1 ->
          let t2 = b1
          switch t2 {
            #0 ->
              return 2
            #1 ->
              return 1
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
fn an_arm_reached_twice_stays_a_join_point() {
    // `||` の真の枝は2か所から来るので join point に残り、偽の枝は1か所からなので戻す
    let text = "either : Bool -> Bool -> String -> String\neither a b s = if a || b then s ++ \"!\" else s\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text), @r#"
    fn either(a0, b1, s2) {
      join j0(u7) [s2] {
        let s4 = const "!"
        let t5 = prim ++(s2, s4)
        return t5
      }
      switch a0 {
        #0 ->
          let t3 = b1
          switch t3 {
            #0 ->
              return s2
            #1 ->
              jump j0(())
          }
        #1 ->
          jump j0(())
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
fn split_arms_use_the_known_tag() {
    // 切り出した枝の中では、条件の値をその枝のタグに置き換える
    let text = "describe : Bool -> Bool -> Bool\ndescribe a b =\n  let v = a && b\n  if v then not v else v\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text), @r"
    fn describe(a0, b1) {
      switch a0 {
        #0 ->
          return #0
        #1 ->
          let t2 = b1
          switch t2 {
            #0 ->
              return #0
            #1 ->
              let t3 = prim not(#1)
              return t3
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

- [ ] **Step 3: テストが失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test lower`
Expected: `ifs_in_a_condition_nest_join_points`、`known_tags_jump_straight_to_their_arm`、`an_arm_reached_twice_stays_a_join_point`、`split_arms_use_the_known_tag` が FAIL

- [ ] **Step 4: 値を書き換える口を足す**

`crates/eml_core_ir/src/lib.rs` の `impl Rhs` の `atoms` の後ろに足す。

```rust
    /// 右辺が使う値を書き換える口。`atoms` と同じ順に並ぶ。
    pub(crate) fn atoms_mut(&mut self) -> Vec<&mut Atom> {
        match self {
            Rhs::Atom(atom) | Rhs::Drop(atom) => vec![atom],
            Rhs::Call { call, .. } => call.atoms_mut(),
            Rhs::MakeClosure(_, args) | Rhs::Prim(_, args) | Rhs::Io(_, args) => {
                args.iter_mut().collect()
            }
            Rhs::ConstString(_) => Vec::new(),
        }
    }
```

`impl Call` の `atoms` の後ろに足す。

```rust
    /// 呼び出しが使う値を書き換える口。`atoms` と同じ順に並ぶ。
    pub(crate) fn atoms_mut(&mut self) -> Vec<&mut Atom> {
        match self {
            Call::Direct(_, args) | Call::Perform { args, .. } => args.iter_mut().collect(),
            Call::Apply(callee, args) => std::iter::once(callee).chain(args.iter_mut()).collect(),
            Call::Handle {
                body, clauses, ret, ..
            } => std::iter::once(body)
                .chain(clauses.iter_mut())
                .chain(ret.iter_mut())
                .collect(),
            Call::Resume { k, arg } => vec![k, arg],
        }
    }
```

`CExpr` に `impl` を足す (`enum CExpr` の後ろ)。

```rust
impl CExpr {
    /// 式が直接使う値を書き換える口。子の式の値は含まない。
    pub(crate) fn atoms_mut(&mut self) -> Vec<&mut Atom> {
        match self {
            CExpr::Let { rhs, .. } => rhs.atoms_mut(),
            CExpr::Switch { scrutinee, .. } => vec![scrutinee],
            CExpr::Jump { arg, .. } => vec![arg],
            CExpr::Return(atom) => vec![atom],
            CExpr::TailCall(call) => call.atoms_mut(),
            CExpr::Join { .. } | CExpr::Dup { .. } | CExpr::Decref { .. } => Vec::new(),
        }
    }
}
```

- [ ] **Step 5: B2 と B3 を書く**

`crates/eml_core_ir/src/simplify.rs` の `use` を次にする。

```rust
use std::collections::HashSet;

use crate::{Atom, CExpr, CExprId, CoreFn, JoinId, Linearity, Program, Rhs, VarId, VarInfo};
```

`Simplify` に欄を足し、`simplify` の中で初期化して、書き換えの順を次にする。

```rust
struct Simplify<'a> {
    function: &'a mut CoreFn,
    /// B2 で作った join point の引数。`()` を受けるだけで本体では使わないので、戻すときに束縛を作らない。
    unit_params: HashSet<VarId>,
}
```

```rust
        let mut pass = Simplify {
            function,
            unit_params: HashSet::new(),
        };
        pass.split_known_tags();
        pass.forward_small_bodies();
        pass.inline_single_jumps();
        pass.remove_unused();
        pass.renumber();
```

先頭の説明の後ろに、順の理由を足す。

```rust
//! B2 (分かっているタグ)、B5 (小さな本体)、B3 (jump が1つ)、B4 (使われない) の順に1巡だけ回す。B5 を B4 より先に
//! 回すのは、B5 で jump がなくなった join point を、同じ巡の B4 で消すためである。
```

`impl Simplify` に足す。

```rust
    fn push(&mut self, expr: CExpr) -> CExprId {
        self.function.exprs.push(expr);
        CExprId(self.function.exprs.len() as u32 - 1)
    }

    fn new_unit_param(&mut self) -> VarId {
        self.function.vars.push(VarInfo {
            name: "u".to_string(),
            linearity: Linearity::Unr,
            boxed: false,
        });
        let var = VarId(self.function.vars.len() as u32 - 1);
        self.unit_params.insert(var);
        var
    }

    /// `root` の部分木で、`var` の使用を `atom` に置き換える。
    fn substitute(&mut self, root: CExprId, var: VarId, atom: Atom) {
        let mut work = vec![root];
        while let Some(id) = work.pop() {
            let expr = &mut self.function.exprs[id.0 as usize];
            for slot in expr.atoms_mut() {
                if *slot == Atom::Var(var) {
                    *slot = atom;
                }
            }
            work.extend(children(expr));
        }
    }

    fn known_tag(&self, site: CExprId) -> Option<u32> {
        match self.expr(site) {
            CExpr::Jump {
                arg: Atom::Tag(tag),
                ..
            } => Some(*tag),
            _ => None,
        }
    }

    /// B2: 本体が引数で分岐する join point に定数のタグを jump で渡していれば、各枝を join point に切り出し、定数の
    /// jump を枝へ直接向ける。`&&` と `||` を条件にした `if` がこの形になる (docs/spec/core-ir.md)。
    fn split_known_tags(&mut self) {
        let jumps = self.jumps();
        for (index, sites) in jumps.iter().enumerate() {
            let node = self.function.joins[index];
            let CExpr::Join {
                join,
                param,
                body,
                scope,
                ..
            } = self.expr(node).clone()
            else {
                unreachable!("the join index points at join points")
            };
            let CExpr::Switch {
                scrutinee: Atom::Var(scrutinee),
                arms,
            } = self.expr(body).clone()
            else {
                continue;
            };
            let known: Vec<u32> = sites.iter().filter_map(|&site| self.known_tag(site)).collect();
            let has_arm = |tag: &u32| arms.iter().any(|&(arm_tag, _)| arm_tag == *tag);
            if scrutinee != param || known.is_empty() || !known.iter().all(has_arm) {
                continue;
            }
            let mut arm_joins = Vec::new();
            for &(tag, arm) in &arms {
                self.substitute(arm, param, Atom::Tag(tag));
                let unit = self.new_unit_param();
                let arm_join = JoinId(self.function.joins.len() as u32);
                // 索引は、下で組み立てた `Join` の位置に直す
                self.function.joins.push(arm);
                arm_joins.push((tag, arm_join, unit, arm));
            }
            let dispatch = arm_joins
                .iter()
                .map(|&(tag, arm_join, _, _)| {
                    let jump = self.push(CExpr::Jump {
                        join: arm_join,
                        arg: Atom::Unit,
                    });
                    (tag, jump)
                })
                .collect();
            self.set(
                body,
                CExpr::Switch {
                    scrutinee: Atom::Var(param),
                    arms: dispatch,
                },
            );
            for &site in sites {
                if let Some(tag) = self.known_tag(site) {
                    let &(_, arm_join, _, _) = arm_joins
                        .iter()
                        .find(|&&(arm_tag, ..)| arm_tag == tag)
                        .expect("checked above");
                    self.set(
                        site,
                        CExpr::Jump {
                            join: arm_join,
                            arg: Atom::Unit,
                        },
                    );
                }
            }
            // 枝の join point を外側に並べ、元の join point をいちばん内側に置く。枝は元の join point の定義全体を
            // 範囲にするので、元の本体からも、範囲の中の定数の jump からも届く。元の位置には最初の枝の join point が入る
            let mut inner = self.push(CExpr::Join {
                join,
                param,
                captures: Vec::new(),
                body,
                scope,
            });
            self.function.joins[index] = inner;
            for (position, &(_, arm_join, unit, arm)) in arm_joins.iter().enumerate().rev() {
                let expr = CExpr::Join {
                    join: arm_join,
                    param: unit,
                    captures: Vec::new(),
                    body: arm,
                    scope: inner,
                };
                inner = if position == 0 {
                    self.set(node, expr);
                    node
                } else {
                    self.push(expr)
                };
                self.function.joins[arm_join.0 as usize] = inner;
            }
        }
    }

    /// B3: jump が1つだけの join point を、その jump の位置に戻す。jump の位置では、本体が使う外側の変数がすべて
    /// 範囲にある。
    fn inline_single_jumps(&mut self) {
        let jumps = self.jumps();
        let mut parents = self.parents();
        for (index, sites) in jumps.iter().enumerate() {
            let &[site] = sites.as_slice() else {
                continue;
            };
            let node = self.function.joins[index];
            let CExpr::Join { param, body, .. } = self.expr(node) else {
                unreachable!("the join index points at join points")
            };
            let (param, body) = (*param, *body);
            let CExpr::Jump { arg, .. } = self.expr(site) else {
                unreachable!("a jump site holds a jump")
            };
            let arg = *arg;
            if self.unit_params.contains(&param) {
                self.replace(&mut parents, site, body);
            } else {
                self.set(
                    site,
                    CExpr::Let {
                        var: param,
                        rhs: Rhs::Atom(arg),
                        body,
                    },
                );
                parents[body.0 as usize] = Some(site);
            }
            // jump が範囲そのものだった場合に備え、範囲は置き換えの後に読み直す
            let CExpr::Join { scope, .. } = self.expr(node) else {
                unreachable!("the join index points at join points")
            };
            let scope = *scope;
            self.replace(&mut parents, node, scope);
        }
    }
```

- [ ] **Step 6: テストを通す**

Run: `cargo test`
Expected: PASS。Step 2 の4件は期待値どおり。Step 1 の UI テストの stdout は変わらない。Task 1 の `nested_join_points_capture_what_outer_join_points_need`、`a_non_tail_if_keeps_strings_used_later`、`calls_save_the_variables_used_after_them` は変わらない (定数のタグも小さな本体もなく、どの join point にも jump が2つある)

- [ ] **Step 7: clippy と fmt を通してコミットする**

```bash
cargo clippy --all-targets && cargo fmt
git add -A crates/eml_core_ir tests/ui/run/short_circuit_conditions.em crates/eml_cli/tests/snapshots/ui__run@short_circuit_conditions.em.snap
git commit -m "Jump straight to the arm for known tags and inline single jumps

When a join body switches on its parameter and some jumps pass a
constant tag, each arm becomes its own join point and those jumps go
to the arm directly, with the parameter replaced by the tag inside the
arm. Join points with a single jump are then inlined at that jump.
The nested condition snapshot changes shape (kind 2).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UrPZkaPH9AUX4GoNtt1ZtF"
```

---

### Task 8: 段階Bの文書を直し、作業用の文書を消す

**Files:**
- Modify: `docs/spec/core-ir.md`
- Modify: `docs/implementation/architecture.md`
- Modify: `docs/implementation/testing.md`
- Modify: `docs/implementation/status.md`
- Delete: `docs/superpowers/specs/2026-10-05-join-point-analysis-design.md`
- Delete: `docs/superpowers/plans/2026-10-05-join-point-analysis.md`

- [ ] **Step 1: spec を直す**

`docs/spec/core-ir.md` を次のように直す。日本語を書く前に `yomiyasu:yomiyasu` を読む。

- 「末尾にない分岐の値は join point で受ける。」の項に足す: 末尾にない `if` は、条件の計算を含む全体を join point の範囲にする。join point の定義は実行時に何もしないので、評価の順は変わらない。条件が末尾にない `if` のとき、その join point が外側の join point の範囲の中にでき、枝から外側へ jump できる。
- 「マイルストーン1 で入れるパスは、Perceus の `dup` / `decref` の挿入だけにする。」を、`simplify` と Perceus の2つにする。`simplify` の項を足す: 変換の後、Perceus の前に置き、join point を書き換える。分かっているタグの jump を枝へ直接向ける (case-of-case)、本体が1命令の join point への jump をその命令にする、jump が1つの join point をその位置に戻す、jump のない join point を消す、の4つを、この順 (B2、B5、B3、B4) で1巡だけ行う。どの書き換えも、本体を「その jump に来たときだけ」実行される位置へ動かすだけなので、エフェクトの順と短絡評価は変わらない。

- [ ] **Step 2: architecture を直す**

`docs/implementation/architecture.md` の Core IR の項に足す。

- 変換の項: 末尾にない `if` は `tail` で条件ごと範囲を組み立てる。
- `simplify.rs` の項 (Perceus の項の前): アリーナの上でその場で書き換え、木から外れた式は Perceus が捨てること。jump の位置と親は根からたどれる式だけで求めること。B2 は枝ごとに新しい join point (引数は `()` を受ける `u`) を作り、枝の中の引数をタグの定数に置き換えること。最後に、残った join point に元の順で番号を振り直すこと。

- [ ] **Step 3: テストの変更を記録する**

`docs/implementation/testing.md` の「join point の解析の整理」の節に足す。

```markdown
- 末尾にない `if` の条件の計算を join point の範囲に入れたので、`eml_core_ir/tests/lower.rs` の `calls_save_the_variables_used_after_them` の `let t3 = prim >(n0, 0)` が `join` の定義の後ろに移り、`ifs_in_a_condition_nest_join_points` の2つの join point が入れ子でなく並んだ (種類2)。`saved` と `dup` / `decref` は変わっていない
- `simplify` を入れたので、`ifs_in_a_condition_nest_join_points` の条件の join point が消え、`a` が偽の枝は外側の join point へ直接 `jump j0(2)` するようになった (種類2)。join point の入れ子と `captures` は `nested_join_points_capture_what_outer_join_points_need` で確かめる
```

- [ ] **Step 4: status を直す**

`docs/implementation/status.md` を直す。

- 「各 crate の実装状況」の `eml_core_ir` の行に「join point を書き換える `simplify` (分かっているタグの jump、小さな本体の転送、jump が1つの join point の展開、使われない join point の削除)」を足す。
- 「完了した作業」の「join point の解析の整理」の行の内容に、「その上に join point を書き換える `simplify` を置き、`&&` と `||` を条件にした `if` が条件の値で分岐し直さないようにした」を足す。

- [ ] **Step 5: 作業用の文書を消してコミットする**

```bash
git rm docs/superpowers/specs/2026-10-05-join-point-analysis-design.md docs/superpowers/plans/2026-10-05-join-point-analysis.md
git add docs
git commit -m "Document the simplify pass and remove the working documents

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UrPZkaPH9AUX4GoNtt1ZtF"
```
