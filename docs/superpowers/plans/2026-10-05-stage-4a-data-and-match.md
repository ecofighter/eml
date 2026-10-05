# 縦の貫通 段階4a Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 型引数を持つ `data`、コンストラクタ、`match` と入れ子のパターン、網羅性の検査を、HIR、型検査、Core IR、ランタイム、インタプリタの全体に通し、`Bool` を Prelude の `data` にする。

**Architecture:** 言語を変えない Core IR の準備 (join point の引数の並び) を先に済ませる (Task 1)。次に上流から積む。HIR に `data`、コンストラクタ、型の適用、パターン、`match` を入れ (Task 2)、型検査で型構成子に引数を持たせてパターンと `match` を検査し、`Bool` を Prelude に移す (Task 3)。網羅性の検査を独立したモジュールとして足す (Task 4)。ランタイムに `data` のオブジェクトを入れ、Core IR の `Switch` の枝にフィールドを束縛させる (Task 5)。最後に `match` を決定木にコンパイルし、B2 を引数のないタグに限る (Task 6)。文書は Task 7 で直す。

**Tech Stack:** Rust (edition 2024)、rowan 0.16、la-arena 0.3、insta 1.49。新しい外部 crate は足さない。

**Spec:** `docs/superpowers/specs/2026-10-05-stage-4a-data-and-match-design.md` (段階4a の設計)。規範は `docs/spec/` の `declarations.md` (`data`)、`expressions.md` (`match`)、`types.md`、`exhaustiveness.md`、`core-ir.md`、`runtime.md`、`diagnostics.md`。

## Global Constraints

- 作業は `main` から切ったブランチ `stage-4a` で行う。タスクごとにコミットする。コミットメッセージは英語で、末尾に次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_014YGQm6f7Mtged1yVY4dmJG
  ```

- 外部 crate は増やさない
- コードのコメントは日本語で、何をするかではなく理由を書く。規則を指すときは `docs/` のパスを書く。日本語を書く前に `yomiyasu:yomiyasu` スキルを読み、その規則に従う (CLAUDE.md)。`.em` のテストの先頭のコメントは、既存のテストと同じく英語で書く
- 各段階は `fn stage(&In) -> (Out, Vec<Diagnostic>)` の純粋関数のままにする
- 診断の help / note は eml 自身の規則の説明に限る。他の言語の書き方を前提にしたヒントは入れない
- インタプリタとランタイムの値とフレームに `Rc` と `RefCell` を使わない。`unsafe` を書かない
- 既存のテストで期待値を変えてよいのは、spec の6節の表にあるものだけである。種類1 (`data_declarations.em` の削除、`eml_hir/tests/lower.rs` の `constructs_of_later_stages_are_not_yet_supported`、`eml_hir/src/builtin.rs` の `Builtin::True` の行) はこのプランのステップに書いた形に変え、種類2 (`eml_core_ir/tests/simplify.rs` の2件) はステップに書いた期待値にし、種類3 は期待値を変えずに追随する。表にないテストの期待値が変わったら、変えずに止まり、差分と理由をユーザーに示して承認を得る。設計を曲げてテストを守ることはしない
- 各タスクの最後に `cargo test`、`cargo clippy --all-targets`、`cargo fmt` を通す。clippy の警告を残さない
- インラインスナップショットと UI テストの期待値は、このプランのコードが出す形を書いてある。食い違ったら、まず実装がプランのコードと一致しているかを確かめる。プランの期待値の誤り (位置の数え違い、局所変数や Core IR の変数の番号など) だと判断した場合は、理由をコミットメッセージに書いてから直す。新しい UI テストのスナップショットは、`cargo insta review` で、ステップに書いた stdout と診断の番号と文言に一致することを確かめてから承認する
- 診断の位置の表記は、1 始まりの行と、文字数で数えた列 (`2:7`) である
- 長い連鎖 (継続の区間、ヒープの子、`Let` の連鎖) をたどる処理はループで書く。Rust の再帰は、E0013 の入れ子の上限で深さが抑えられる木 (式、パターン、型、`Switch` の枝) に限る
- 途中のタスクでは、ワークスペース全体をビルドできる状態に保つ。Task 2 は、型検査でコンストラクタの式、`match`、コンストラクタのパターンを `Error` の型にして E0004 を出し、Core IR の変換に `unreachable!` を置く。Task 3 がこれを外す。Task 3 から Task 5 までは、引数を持つコンストラクタの値か `match` を含むプログラムを `eml run` すると、Core IR の変換で止まる。この間に、そうしたプログラムを実行するテストはない。仮の扱いには「後で実装する」という趣旨のコメントを書かない

## Review Focus

- 10万要素のリストを、末尾でない再帰で畳み込み、別のリストは先頭だけ読んで残りを丸ごと捨てる。解放も分解も Rust の再帰にならず、リークしない → Task 6 の `run/data/long_list.em`
- 関数をフィールドに持つコンストラクタを部分適用し、`match` で取り出して呼ぶ。フィールドの関数型の線形性は `Unr` に固定され、クロージャが捕まえた値も解放される → Task 6 の `run/data/function_fields.em`
- `match` の枝の中で作ったラムダが、パターンの変数を捕まえ、`match` の後で2回呼ばれる。枝の join point の引数が持ち上げの捕獲に正しく渡る → Task 6 の `run/data/closures_over_pattern_variables.em`
- 型引数を持つ再帰的な `data` の、3つのフィールドを持つ枝と、boxed な要素での多相な関数。部分木を作り直す挿入と、使わないフィールド (`_`) の解放 → Task 6 の `run/data/tree.em`
- handler の節の引数と `return` の節の引数に書いた反駁不可能なコンストラクタのパターン。節は持ち上げる関数なので、ラムダの引数と同じ経路で分解される → Task 6 の `run/data/clause_patterns.em` と `translate.rs` の `constructor_patterns_in_handler_clause_parameters`

---

### Task 1: join point の引数を並びにする

言語の振る舞いを変えずに、`CExpr::Join` の引数と `CExpr::Jump` の値を並びにする。B2 が作る join point を引数0個にし、`unit_params` をなくす。種類2の2件はここで変わる。`Switch` の枝の形は変えない (Task 5)。

**Files:**
- Modify: `crates/eml_core_ir/src/lib.rs` (`CExpr::Join`、`CExpr::Jump`、`CoreFn::join`、`CExpr::atoms_mut`)
- Modify: `crates/eml_core_ir/src/translate/mod.rs` (`Binding::Join`、`exit_with`、`seq`)
- Modify: `crates/eml_core_ir/src/translate/expr.rs:183-192` (末尾にない `if` の join point)
- Modify: `crates/eml_core_ir/src/simplify.rs` (`Simplify`、B2、B3、B5、`movable`)
- Modify: `crates/eml_core_ir/src/liveness.rs` (`at_end`、`Start::JoinBody`、`analyze`)
- Modify: `crates/eml_core_ir/src/perceus.rs` (`transform` の `Join` と `Jump`、`transform_jump`、`Step::Join`)
- Modify: `crates/eml_core_ir/src/verify.rs` (`Join` の引数の束縛、`check_jump`)
- Modify: `crates/eml_core_ir/src/pretty.rs`
- Modify: `crates/eml_interp/src/lib.rs:195-200` (`Jump`)
- Modify (種類3): `crates/eml_core_ir/tests/verify.rs` (`CExpr::Join` と `CExpr::Jump` を手で組む8か所)
- Modify (種類2): `crates/eml_core_ir/tests/simplify.rs` の `an_arm_reached_twice_stays_a_join_point`、`join_points_left_without_jumps_are_removed`
- Test: `crates/eml_core_ir/tests/verify.rs`、`crates/eml_core_ir/tests/simplify.rs`

**Interfaces:**
- Produces:
  - `CExpr::Join { join: JoinId, params: Vec<VarId>, captures: Vec<VarId>, body: CExprId, scope: CExprId }`
  - `CExpr::Jump { join: JoinId, args: Vec<Atom> }`
  - `CoreFn::join(&self, join: JoinId) -> (&[VarId], CExprId)`
  - translate の `Binding::Join { join: JoinId, params: Vec<VarId>, scope: CExprId }`
  - `pretty` の表示 `join j0(x, y) [c] {`、`jump j0(a, b)`、引数なしは `join j1() [] {`、`jump j1()`
  - verifier の誤り「a jump to `j0` passes 1 values, but its join takes 2」(引数の数の不一致)

**Interface notes (骨組みの Interfaces から変えた点と、前後のタスクとの取り決め):**

- verifier の引数の数の誤りの文言は、骨組みの「a jump passes N values to a join point that takes M」を、既存の直接呼び出しの文言 (`a direct call to `g` passes 2 arguments, but it takes 1`) に合わせて「a jump to `j0` passes 1 values, but its join takes 2」にした。後のタスクはこの文言に依存しない。
- 生存解析の内部の `Start::JoinBody` は、引数を持たずに `Join` の式の番号 (`node`) を持つ形にした。`Start` は `Copy` で、並びを持てないためである。外から見える名前は変わらない。

- [ ] **Step 1: ブランチを切る**

```bash
git switch -c stage-4a
```

- [ ] **Step 2: 手で組んだ Core IR を新しい形に書き換える (種類3)**

`crates/eml_core_ir/tests/verify.rs` で `CExpr::Join` と `CExpr::Jump` を組む箇所を、次の規則で機械的に書き換える。期待値 (`assert_eq!` の右辺) は1文字も変えない。

- `CExpr::Join { .., param: VarId(n), .. }` の `param: VarId(n)` を `params: vec![VarId(n)]` にする
- `CExpr::Jump { join: .., arg: X }` の `arg: X` を `args: vec![X]` にする
- `Call::Resume { k, arg }` の `arg` は Core IR の呼び出しの欄なので変えない (`handler_program` の `main$handle0$ask`)

対象は次の8か所である。

| 関数 | 書き換える式 |
|---|---|
| `pick` | `Jump` 2つ (`arg: var(2)`、`arg: var(1)`) と `Join` 1つ (`param: VarId(3)`) |
| `a_jump_outside_its_join_scope_is_rejected` | `Jump` 2つ (`Atom::Int(1)`、`Atom::Int(2)`) と `Join` 1つ (`VarId(0)`) |
| `a_jump_that_owns_too_much_is_rejected` | `Jump` 1つ (`Atom::Int(1)`) と `Join` 1つ (`VarId(1)`) |
| `a_jump_after_a_call_needs_the_variables_of_the_join_body_in_scope` | `Jump` 1つ (`var(3)`) と `Join` 1つ (`VarId(1)`) |
| `a_join_body_that_uses_a_variable_missing_from_its_captures_is_rejected` | `Jump` 1つ (`Atom::Int(1)`) と `Join` 1つ (`VarId(1)`) |
| `a_capture_out_of_scope_at_its_join_is_rejected` | `Jump` 1つ (`var(0)`) と `Join` 1つ (`VarId(1)`) |
| `captures_out_of_order_are_rejected` | `Jump` 1つ (`Atom::Int(1)`) と `Join` 1つ (`VarId(2)`) |
| `an_unused_capture_released_by_the_body_is_accepted` | `Jump` 1つ (`Atom::Int(1)`) と `Join` 1つ (`VarId(1)`) |

例として、`pick` の書き換えの前後は次のとおり。

```rust
// 前
        CExpr::Jump {
            join: JoinId(0),
            arg: var(2),
        },
// 後
        CExpr::Jump {
            join: JoinId(0),
            args: vec![var(2)],
        },

// 前
    exprs.push(CExpr::Join {
        join: JoinId(0),
        param: VarId(3),
        captures: vec![VarId(1)],
        body: CExprId(1),
        scope: CExprId(switch),
    });
// 後
    exprs.push(CExpr::Join {
        join: JoinId(0),
        params: vec![VarId(3)],
        captures: vec![VarId(1)],
        body: CExprId(1),
        scope: CExprId(switch),
    });
```

書き換え漏れがないことは、次の検索が `Call::Resume` の1行だけを出すことで確かめる。

```bash
grep -n "param:\|arg:" crates/eml_core_ir/tests/verify.rs
```

Expected: `arg: var(0),` の1行 (`Call::Resume` の中) だけ

- [ ] **Step 3: 失敗するテストを書く**

`crates/eml_core_ir/tests/verify.rs` の `a_jump_that_owns_too_little_is_rejected` の直後に、引数を2つ持つ join point の検査を足す。

```rust
/// `two s = let u = "s" in join j0(a, b) [] { let c = a ++ b; return c } in jump j0(s, u)` の Perceus の後の形。
/// `passed` を変えて、`jump` が渡す値の数を変える。
fn two_values(passed: Vec<Atom>) -> CoreFn {
    let exprs = vec![
        CExpr::Return(var(4)),
        concat(4, 2, 3, 0),
        CExpr::Jump {
            join: JoinId(0),
            args: passed,
        },
        CExpr::Let {
            var: VarId(1),
            rhs: Rhs::ConstString(0),
            body: CExprId(2),
        },
        CExpr::Join {
            join: JoinId(0),
            params: vec![VarId(2), VarId(3)],
            captures: vec![],
            body: CExprId(1),
            scope: CExprId(3),
        },
    ];
    let vars = vec![boxed("s"), boxed("u"), boxed("a"), boxed("b"), boxed("c")];
    function("two", 1, vars, exprs, &[4])
}

#[test]
fn a_join_point_with_two_parameters_is_accepted() {
    // 本体は2つの引数をどちらも所有して始まり、`jump` は渡す値の所有権を渡す
    assert_eq!(check(vec![two_values(vec![var(0), var(1)])]), Ok(()));
}

#[test]
fn a_jump_with_the_wrong_number_of_values_is_rejected() {
    assert_eq!(
        check(vec![two_values(vec![var(0)])]),
        Err("a jump to `j0` passes 1 values, but its join takes 2 in `two`".to_string())
    );
}
```

`crates/eml_core_ir/tests/simplify.rs` の種類2の2件の期待値を、B2 が引数0個の join point を作る形にする。B2 は変換と生存解析の後に、変数の表の末尾へ `u` の変数を足していただけなので、ほかの変数の番号は変わらない。

`an_arm_reached_twice_stays_a_join_point` の期待値を次にする。

```rust
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @r#"
    fn either(a0, b1, s2) {
      join j0() [s2] {
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
              jump j0()
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
```

`join_points_left_without_jumps_are_removed` の期待値を次にする。

```rust
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @r#"
    fn noisy(name0, b1) {
      let t2 = perform println(name0)
      return b1
    }
    fn main(p0) {
      let s1 = const "other"
      let s2 = const "a"
      let t3 = call noisy(s2, #1)
      join j0(t9) [] {
        let s10 = const "end"
        let t11 = perform println(s10)
        return t11
      }
      join j1() [] {
        let s6 = const "x"
        let t7 = perform println(s6)
        jump j0(t7)
      }
      switch t3 {
        #0 ->
          jump j1()
        #1 ->
          jump j1()
      }
    }
    fn entry$main() {
      tailcall main(())
    }
    "#);
```

- [ ] **Step 4: テストが失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test verify`
Expected: コンパイルエラー (`CExpr::Join` に `params` がない、`CExpr::Jump` に `args` がない)

- [ ] **Step 5: Core IR の型を変える**

`crates/eml_core_ir/src/lib.rs` の `CoreFn::join` を次にする。

```rust
    /// `Jump` の行き先の、join point の引数と本体。
    pub fn join(&self, join: JoinId) -> (&[VarId], CExprId) {
        match self.expr(self.joins[join.0 as usize]) {
            CExpr::Join { params, body, .. } => (params, *body),
            _ => unreachable!("the join index points at join points"),
        }
    }
```

`CExpr::Join` と `CExpr::Jump` を次にする。

```rust
    /// `scope` の中の `Jump` が `body` に入る。`params` は `Jump` が渡す値を順に受ける。末尾にない `if` の続きを、
    /// ヒープにフレームを積まずに実行するために使う (docs/spec/core-ir.md)。`body` の中からは `Jump` しない。
    /// 末尾にない `if` の join point は引数を1つ持ち、`simplify` の B2 が切り出す枝の join point は引数を持たない。
    Join {
        join: JoinId,
        params: Vec<VarId>,
        /// 本体が使う外側の変数 (`params` を除く)。RC の対象かどうかによらずすべて入れ、`VarId` の昇順に並べる。
        /// パスの中では古くなってよく、パスの間ではパイプラインが埋め直す (docs/spec/core-ir.md のパスの表)。
        captures: Vec<VarId>,
        body: CExprId,
        scope: CExprId,
    },
```

```rust
    Jump {
        join: JoinId,
        args: Vec<Atom>,
    },
```

`CExpr::atoms_mut` の `Jump` の腕を次にする。

```rust
            CExpr::Jump { args, .. } => args.iter_mut().collect(),
```

- [ ] **Step 6: 変換を直す**

`crates/eml_core_ir/src/translate/mod.rs` の `Binding::Join` を次にする。

```rust
    /// ここより後ろで組み立てる式を本体にし、`scope` (条件の計算と、枝が `Jump` する `Switch`) を範囲にする join point。
    Join {
        join: JoinId,
        params: Vec<VarId>,
        scope: CExprId,
    },
```

`exit_with` を次にする。

```rust
fn exit_with(exit: Exit, value: Atom) -> CExpr {
    match exit {
        Exit::Return => CExpr::Return(value),
        Exit::Jump(join) => CExpr::Jump {
            join,
            args: vec![value],
        },
    }
}
```

`FnLowering::seq` の `Binding::Join` の腕を次にする。

```rust
                Binding::Join {
                    join,
                    params,
                    scope,
                } => {
                    let expr = self.push(CExpr::Join {
                        join,
                        params,
                        captures: Vec::new(),
                        body: id,
                        scope,
                    });
                    self.joins[join.0 as usize] = Some(expr);
                    expr
                }
```

`crates/eml_core_ir/src/translate/expr.rs` の末尾にない `if` の join point (191行目) を次にする。

```rust
                out.push(Binding::Join {
                    join,
                    params: vec![param],
                    scope,
                });
```

- [ ] **Step 7: `simplify` を直す**

`crates/eml_core_ir/src/simplify.rs` の `use` と `simplify` と `Simplify` を次にする。B2 が作る join point は引数を持たないので、`()` を受けるだけの引数の表 (`unit_params`) と `new_unit_param` は削除する。

```rust
use crate::{Atom, CExpr, CExprId, CoreFn, JoinId, Program, Rhs, VarId};

pub(crate) fn simplify(program: &mut Program) {
    for function in &mut program.functions {
        let mut pass = Simplify { function };
        pass.split_known_tags();
        pass.forward_small_bodies();
        pass.inline_single_jumps();
        pass.remove_unused();
        pass.renumber();
    }
}

struct Simplify<'a> {
    function: &'a mut CoreFn,
}
```

`known_tag` を次にする。

```rust
    fn known_tag(&self, site: CExprId) -> Option<u32> {
        match self.expr(site) {
            CExpr::Jump { args, .. } => match args.as_slice() {
                [Atom::Tag(tag)] => Some(*tag),
                _ => None,
            },
            _ => None,
        }
    }
```

`split_known_tags` を次にする。

```rust
    /// B2: 本体が引数で分岐する join point に定数のタグを jump で渡していれば、各枝を引数のない join point に切り出し、
    /// 定数の jump を枝へ直接向ける。`&&` と `||` を条件にした `if` がこの形になる (docs/spec/core-ir.md)。
    fn split_known_tags(&mut self) {
        let jumps = self.jumps();
        for (index, sites) in jumps.iter().enumerate() {
            let node = self.function.joins[index];
            let CExpr::Join {
                join,
                params,
                body,
                scope,
                ..
            } = self.expr(node).clone()
            else {
                unreachable!("the join index points at join points")
            };
            let &[param] = params.as_slice() else {
                continue;
            };
            let CExpr::Switch {
                scrutinee: Atom::Var(scrutinee),
                arms,
            } = self.expr(body).clone()
            else {
                continue;
            };
            let known: Vec<u32> = sites
                .iter()
                .filter_map(|&site| self.known_tag(site))
                .collect();
            let has_arm = |tag: &u32| arms.iter().any(|&(arm_tag, _)| arm_tag == *tag);
            if scrutinee != param || known.is_empty() || !known.iter().all(has_arm) {
                continue;
            }
            let mut arm_joins = Vec::new();
            for &(tag, arm) in &arms {
                self.substitute(arm, param, Atom::Tag(tag));
                let arm_join = JoinId(self.function.joins.len() as u32);
                // 索引は、下で組み立てた `Join` の位置に直す
                self.function.joins.push(arm);
                arm_joins.push((tag, arm_join, arm));
            }
            let dispatch = arm_joins
                .iter()
                .map(|&(tag, arm_join, _)| {
                    let jump = self.push(CExpr::Jump {
                        join: arm_join,
                        args: Vec::new(),
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
                    let &(_, arm_join, _) = arm_joins
                        .iter()
                        .find(|&&(arm_tag, ..)| arm_tag == tag)
                        .expect("checked above");
                    self.set(
                        site,
                        CExpr::Jump {
                            join: arm_join,
                            args: Vec::new(),
                        },
                    );
                }
            }
            // 枝の join point を外側に並べ、元の join point をいちばん内側に置く。枝は元の join point の定義全体を
            // 範囲にするので、元の本体からも、範囲の中の定数の jump からも届く。元の位置には最初の枝の join point が入る
            let mut inner = self.push(CExpr::Join {
                join,
                params: vec![param],
                captures: Vec::new(),
                body,
                scope,
            });
            self.function.joins[index] = inner;
            for (position, &(_, arm_join, arm)) in arm_joins.iter().enumerate().rev() {
                let expr = CExpr::Join {
                    join: arm_join,
                    params: Vec::new(),
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
```

`inline_single_jumps` を次にする。引数がなければ本体をそのまま jump の位置に置き、引数があれば引数ごとに `let` を作る。

```rust
    /// B3: jump が1つだけの join point を、その jump の位置に戻す。引数は、jump が渡す値の束縛にする。jump の位置では、
    /// 本体が使う外側の変数がすべて範囲にある。
    fn inline_single_jumps(&mut self) {
        let jumps = self.jumps();
        let mut parents = self.parents();
        for (index, sites) in jumps.iter().enumerate() {
            let &[site] = sites.as_slice() else {
                continue;
            };
            let node = self.function.joins[index];
            let CExpr::Join { params, body, .. } = self.expr(node) else {
                unreachable!("the join index points at join points")
            };
            let (params, body) = (params.clone(), *body);
            let CExpr::Jump { args, .. } = self.expr(site) else {
                unreachable!("a jump site holds a jump")
            };
            let args = args.clone();
            match params.split_first() {
                None => self.replace(&mut parents, site, body),
                Some((&first, rest)) => {
                    // 2つ目からの引数の束縛を本体の前に積み、最初の引数の束縛を jump の位置に置く。新しく作った式も
                    // 後の置き換えで親をたどれるように、親の表を広げる
                    let mut inner = body;
                    for (&param, &arg) in rest.iter().zip(&args[1..]).rev() {
                        let binding = self.push(CExpr::Let {
                            var: param,
                            rhs: Rhs::Atom(arg),
                            body: inner,
                        });
                        parents.resize(self.function.exprs.len(), None);
                        parents[inner.0 as usize] = Some(binding);
                        inner = binding;
                    }
                    self.set(
                        site,
                        CExpr::Let {
                            var: first,
                            rhs: Rhs::Atom(args[0]),
                            body: inner,
                        },
                    );
                    parents[inner.0 as usize] = Some(site);
                }
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

`forward_small_bodies` を次にする。

```rust
    /// B5: 本体が1命令だけの join point への jump を、その命令に置き換える。対象は、値を返す本体と、値を別の join point
    /// へ渡す本体である。別の join point は元の join point を範囲に含むので、jump の位置からも届く。
    fn forward_small_bodies(&mut self) {
        let jumps = self.jumps();
        for (index, sites) in jumps.iter().enumerate() {
            let CExpr::Join { params, body, .. } = self.expr(self.function.joins[index]) else {
                unreachable!("the join index points at join points")
            };
            let (params, body) = (params.clone(), *body);
            let small = match self.expr(body) {
                CExpr::Return(value) if movable(*value, &params) => self.expr(body).clone(),
                CExpr::Jump { args, .. } if args.iter().all(|&arg| movable(arg, &params)) => {
                    self.expr(body).clone()
                }
                _ => continue,
            };
            for &site in sites {
                let CExpr::Jump { args: passed, .. } = self.expr(site) else {
                    unreachable!("a jump site holds a jump")
                };
                let passed = passed.clone();
                // 引数は、jump が同じ位置に渡す値に置き換える
                let with = |atom: Atom| match atom {
                    Atom::Var(var) => params
                        .iter()
                        .position(|&param| param == var)
                        .map_or(atom, |position| passed[position]),
                    _ => atom,
                };
                let copy = match &small {
                    CExpr::Return(value) => CExpr::Return(with(*value)),
                    CExpr::Jump { join, args } => CExpr::Jump {
                        join: *join,
                        args: args.iter().map(|&arg| with(arg)).collect(),
                    },
                    _ => unreachable!("only returns and jumps are small"),
                };
                self.set(site, copy);
            }
        }
    }
```

ファイル末尾の `movable` を次にする。

```rust
/// 本体を jump の位置に写してよい値。引数は渡す値に置き換わり、定数はどこでも同じである。ほかの変数は、写すと
/// その変数の使用が増えるので写さない。
fn movable(atom: Atom, params: &[VarId]) -> bool {
    match atom {
        Atom::Var(var) => params.contains(&var),
        _ => true,
    }
}
```

`substitute`、`remove_unused`、`renumber`、`children`、`replace_child` は変えない。

- [ ] **Step 8: 生存解析を直す**

`crates/eml_core_ir/src/liveness.rs` の `BlockLiveness::at_end` の `Jump` の腕を次にする。

```rust
            // 範囲の外への `Jump` は verifier が報告するので、ここでは求めていない `captures` を空として扱う
            CExpr::Jump { join, args } => {
                let mut vars = self
                    .captures
                    .get(join.0 as usize)
                    .cloned()
                    .unwrap_or_default();
                vars.extend(args.iter().filter_map(var_of));
                vars
            }
```

`Start` を次にする。`Start` は作業の列に写して積むので `Copy` のままにし、引数の並びは `Join` の式から読む。

```rust
/// 連鎖の始まりの種類。join point の本体の入口は表に持たず、`captures` に書く。`JoinBody` の `node` は `Join` の式で、
/// 本体の入口から除く引数をそこから読む。
#[derive(Clone, Copy)]
enum Start {
    Function,
    Block,
    JoinBody { join: JoinId, node: CExprId },
}
```

`analyze` の `Step::Visit` の `Join` の腕を次にする。

```rust
                    CExpr::Join {
                        join, body, scope, ..
                    } => {
                        work.push(Step::Visit(*scope, Start::Block));
                        work.push(Step::Visit(
                            *body,
                            Start::JoinBody {
                                join: *join,
                                node: id,
                            },
                        ));
                    }
```

`Step::Finish` の `Start::JoinBody` の腕を次にする。

```rust
                    Start::JoinBody { join, node } => {
                        let CExpr::Join { params, .. } = function.expr(node) else {
                            unreachable!("a join body starts at a join point")
                        };
                        for param in params {
                            vars.remove(param);
                        }
                        live.captures[join.0 as usize] = vars;
                    }
```

`use` から `VarId` が使われなくなったら外す (`Vars` の型の定義で使っているので、残る)。

- [ ] **Step 9: Perceus を直す**

`crates/eml_core_ir/src/perceus.rs` の `transform` の `CExpr::Join` の腕を次にする。

```rust
                CExpr::Join {
                    join,
                    params,
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
                        params: params.clone(),
                        captures: captures.clone(),
                        scope,
                        before,
                    });
                    let mut owned = self.tracked_only(self.live.captures(*join));
                    owned.extend(
                        params
                            .iter()
                            .copied()
                            .filter(|param| self.tracked[param.0 as usize]),
                    );
                    segment = Segment::Start(owned);
                    id = *body;
                }
```

`CExpr::Jump` の腕を次にする。

```rust
                CExpr::Jump { join, args } => {
                    let live = self.live.at_end(expr);
                    let owned = self.owned(&segment, &live);
                    break (self.transform_jump(*join, args, &owned), live);
                }
```

逆順に組み立てるループの `Step::Join` の腕を次にする。

```rust
                Step::Join {
                    join,
                    params,
                    captures,
                    scope,
                    before,
                } => {
                    code = self.push(CExpr::Join {
                        join,
                        params,
                        captures,
                        body: code,
                        scope,
                    });
                    self.joins[join.0 as usize] = Some(code);
                    live = before;
                }
```

`transform_jump` を次にする。値を1つ渡す場合の `dup` と `decref` の並びは、今の実装と同じになる (捨てる変数を逆順に積んでから、複製をその外側に積む)。

```rust
    /// join point の本体は、`captures` のうち RC の対象をちょうど1つずつ所有して始まる。それ以外を捨て、渡す値を
    /// 本体でも使うなら複製する。同じ変数を2つの引数に渡すときは、2つ目の分も複製する。
    fn transform_jump(&mut self, join: JoinId, args: &[Atom], owned: &Vars) -> CExprId {
        let needs = self.tracked_only(self.live.captures(join));
        let uses = self.uses(args);
        let code = self.push(CExpr::Jump {
            join,
            args: args.to_vec(),
        });
        self.release_and_duplicate(code, owned, &uses, |var| needs.contains(&var))
    }
```

ファイル末尾の `Step::Join` を次にする。

```rust
    Join {
        join: JoinId,
        params: Vec<VarId>,
        captures: Vec<VarId>,
        scope: CExprId,
        /// join point の定義の直前 (範囲の入口) で生きている変数。
        before: Vars,
    },
```

- [ ] **Step 10: verifier を直す**

`crates/eml_core_ir/src/verify.rs` の `check` の `Jump` の腕を次にする。

```rust
                CExpr::Jump { join, args } => return self.check_jump(state, *join, args),
```

`Join` の腕の分解に `params` を使い、最後の `self.bind(&mut state, *param)?;` を次にする。同じ変数を2つの引数にすると、`bind` が「bound twice」を報告する。

```rust
                    for &param in params {
                        self.bind(&mut state, param)?;
                    }
```

`check_jump` を次にする。引数の数は、範囲の検査の後、所有を数える前に確かめる。数が違うと所有の誤りが二次的に出るので、先に原因を報告するためである。

```rust
    /// 渡す値の数が行き先の引数の数と一致し、渡す値を除き、行き先の join point の `captures` のうち RC の対象を、
    /// ちょうど1つずつ所有している。
    fn check_jump(&self, mut state: State, join: JoinId, args: &[Atom]) -> Result<(), String> {
        if !state.joins.contains(&join) {
            return Err(format!("a jump to `j{}` is outside its scope", join.0));
        }
        let (params, _) = self.function.join(join);
        if args.len() != params.len() {
            return Err(format!(
                "a jump to `j{}` passes {} values, but its join takes {}",
                join.0,
                args.len(),
                params.len()
            ));
        }
        let captures = self.function.captures(join);
        // 呼び出しの後に `Jump` する経路で、本体が使う変数を退避し忘れていないこと
        for &var in captures {
            self.visible(var)?;
        }
        for arg in args {
            self.consume(&mut state, arg)?;
        }
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

`state.joins` に入るのは、定義の位置で索引が自分を指すことを確かめた join point だけなので、`self.function.join(join)` は索引の誤りで落ちない。

- [ ] **Step 11: 表示とインタプリタを直す**

`crates/eml_core_ir/src/pretty.rs` の `expr` の `Join` と `Jump` の腕を次にする。

```rust
            CExpr::Join {
                join,
                params,
                captures,
                body,
                scope,
            } => {
                let params: Vec<String> = params.iter().map(|&v| var(function, v)).collect();
                let captures: Vec<String> = captures.iter().map(|&v| var(function, v)).collect();
                writeln!(
                    out,
                    "{pad}join j{}({}) [{}] {{",
                    join.0,
                    params.join(", "),
                    captures.join(", ")
                )
                .unwrap();
                expr(program, function, *body, indent + 1, out);
                writeln!(out, "{pad}}}").unwrap();
                id = *scope;
            }
            CExpr::Jump { join, args } => {
                let args: Vec<String> = args.iter().map(|a| atom(function, a)).collect();
                writeln!(out, "{pad}jump j{}({})", join.0, args.join(", ")).unwrap();
                return;
            }
```

`crates/eml_interp/src/lib.rs` の `step` の `Jump` の腕を次にする。値をすべて読んでから束縛する。読み出しは move なので、束縛した引数を同じ `jump` の後の値で読み直さないためである。

```rust
            CExpr::Jump { join, args } => {
                // join point は同じ関数の中にあるので、環境をそのまま使い、フレームを積まない
                let values = self.atoms(args)?;
                let (params, body) = program.function(self.function).join(*join);
                for (param, value) in params.iter().zip(values) {
                    self.slots[param.0 as usize] = Some(value);
                }
                self.control = body;
            }
```

- [ ] **Step 12: テストが通ることを確かめる**

Run: `cargo test -p eml_core_ir`
Expected: PASS (`a_join_point_with_two_parameters_is_accepted`、`a_jump_with_the_wrong_number_of_values_is_rejected`、種類2の2件を含む)

Run: `cargo test`
Expected: PASS。種類2の2件のほかに、期待値が変わったテストはない。変わったら止まり、差分をユーザーに示す

- [ ] **Step 13: clippy と fmt を通す**

```bash
cargo clippy --all-targets
cargo fmt
```

Expected: 警告なし。`cargo fmt` の後に `cargo test` をもう一度通す

- [ ] **Step 14: コミットする**

```bash
git add crates/eml_core_ir crates/eml_interp
git commit -m "Give join points a list of parameters

B2 now splits arms into join points without parameters, so the unit
parameters it used to add are gone.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014YGQm6f7Mtged1yVY4dmJG"
```

### Task 2: HIR に `data`、コンストラクタ、型の適用、パターン、`match` を入れる

`data` の宣言を型の item とコンストラクタの item にし、型の適用 (`Option Int`)、コンストラクタの式とパターン、`match` を HIR に通す。コンストラクタの引数の個数の誤り (E1016)、同じ名前の2回の束縛 (E1017) を報告する。`Bool` はまだ組み込み (`Builtin::True` / `Builtin::False`) のままにする。型検査と Core IR は、新しい形を仮に扱う (Interface notes の5)。

決めたことは次のとおり。

- **E1017 の検査の範囲**: `BodyLowering` に `group_start` (束縛のスコープの位置) を持たせる。1つのパターンを変換するときは、その時点のスコープの長さを `group_start` にし、変数のパターンを変換するたびに `scope[group_start..]` に同じ名前がないかを見る。等式の引数の並びは、最初の引数の前に1回だけ `group_start` を決め、すべての引数を同じ組として変換する。入れ子のパターン (`Pair (Some x) x`) は、同じ組の中で変換されるので検出できる。ラムダの引数と handler の節の引数は、spec の範囲に合わせて引数ごとに別の組にする (今と同じく、後の名前が前の名前を隠す)。重複した束縛も局所変数にしてスコープに積むので、本体の名前は後の束縛を指す。
- **`Missing` のパターン**: コンストラクタが見つからないとき (E1001) と引数の個数が違うとき (E1016) は、パターン全体を `PatKind::Missing` にする。引数のパターンは、名前を引く前に変換して変数を束縛しておく。枝の本体でその変数を使っても、E1001 を連鎖させないためである。`Missing` の中の変数は `pat_bindings` に現れず、型検査では `Error` の型になる。
- **値の名前空間**: コンストラクタは `ValueItem::Constructor` として `ItemScope` のユーザーの値の表に入る。そのため `ItemScope::value` で引け、同じ名前の組み込みを隠す。パターンの先頭の名前は `ItemScope::constructor` で、コンストラクタだけから引く。このタスクでは `True` と `False` は組み込みのままなので、パターンに書くと E1001 になる。Task 3 で `Bool` を `data` にすると解ける。
- **中置のコンストラクタ**: 式の演算子の列の `:+` は、組み直した後に `ItemScope::constructor` で引き、2引数の呼び出しにする。fixity は今の表のまま (表になければ `infixl 9`) である。ユーザーが `::` をコンストラクタとして宣言すれば、表の `infixr 5` で組み直し、宣言がなければ今の E0004 (lists) のままにする。パターンの中置のコンストラクタは、文法が右に入れ子の木を作る (docs/spec/grammar.md の `pat`) が、HIR で平らにしてから式と同じ fixity で組み直す。そのため `a :+ b :+ c` は、式でもパターンでも `(a :+ b) :+ c` になる (Interface notes の9)。

**Files:**
- Modify: `crates/eml_syntax/src/ast.rs` (`DataItem`、`Alt`、`MatchExpr`、`MatchArm`、`ConPat`、`InfixConPat`、`AppType` の取り出し口)
- Create: `crates/eml_hir/src/lower/data.rs`
- Modify: `crates/eml_hir/src/hir.rs`、`lib.rs` (codes)、`lower/mod.rs`、`lower/scope.rs`、`lower/types.rs`、`lower/effect.rs`、`lower/expr.rs`、`lower/ops.rs`、`lower/prelude.rs`、`pretty.rs`
- Modify (仮の扱い): `crates/eml_types/src/scheme.rs`、`check/body.rs`、`crates/eml_core_ir/src/translate/expr.rs`
- Modify (最終の形): `crates/eml_types/src/usage.rs`、`check/mod.rs` (`has_error`)、`check/report.rs` (`callee_subject`)
- Modify (種類3): `crates/eml_types/src/ty.rs`、`crates/eml_types/src/table/tests.rs` (`TypeDef` の組み立て)
- Modify (種類1): `crates/eml_hir/tests/lower.rs` の `constructs_of_later_stages_are_not_yet_supported`
- Create: `tests/ui/check-fail/not-yet-supported/tuples.em` (`not-yet-supported/` を空にしないための新しいテスト)
- Delete (種類1): `tests/ui/check-fail/not-yet-supported/data_declarations.em`、`crates/eml_cli/tests/snapshots/ui__check_fail@not-yet-supported__data_declarations.em.snap`
- Create: `crates/eml_hir/tests/data.rs`、`tests/ui/check-fail/names/constructor_arity.em`、`tests/ui/check-fail/names/duplicate_binding.em`
- Test: `crates/eml_syntax/tests/ast.rs`

**Interfaces:**
- Consumes: なし (Task 1 とは独立)
- Produces (`eml_syntax::ast`):
  - `DataItem::name() -> Option<SyntaxToken>`、`DataItem::params() -> impl Iterator<Item = SyntaxToken>`、`DataItem::alts() -> AstChildren<Alt>`
  - `Alt::name() -> Option<SyntaxToken>` (前置のコンストラクタ)、`Alt::operator() -> Option<SyntaxToken>` (中置のコンストラクタの `CONOP`)、`Alt::fields() -> AstChildren<Type>`
  - `MatchExpr::scrutinee() -> Option<Expr>`、`MatchExpr::arms() -> AstChildren<MatchArm>`、`MatchArm::pat() -> Option<Pat>`、`MatchArm::body() -> Option<Expr>`
  - `ConPat::segments()`、`ConPat::args() -> AstChildren<Pat>`、`InfixConPat::lhs() / operator() / rhs()`
  - `AppType::segments()`、`AppType::args() -> AstChildren<Type>`
- Produces (`eml_hir`):

  ```rust
  pub type ConstructorId = Idx<Constructor>;

  pub struct TypeDef {
      pub name: String,
      pub generics: Generics,
      pub types: Arena<TypeRef>,
      pub kind: TypeDefKind,
  }
  impl TypeDef {
      pub fn builtin(name: &str) -> TypeDef;
  }

  pub enum TypeDefKind {
      Builtin,
      Data { constructors: Vec<ConstructorId> },
  }

  pub struct Constructor {
      pub name: String,
      pub range: TextRange,
      pub ty: TypeDefId,
      pub tag: u32,
      pub fields: Vec<TypeRefId>,
  }

  // Module に追加
  pub constructors: Arena<Constructor>,
  // Res に追加
  Constructor(ConstructorId),
  // TypeRefKind::Con を変更
  Con(TypeDefId, Vec<TypeRefId>),
  // PatKind に追加
  Con { ctor: ConstructorId, args: Vec<PatId> },
  // ExprKind に追加
  Match { scrutinee: ExprId, arms: Vec<MatchArm> },

  pub struct MatchArm {
      pub pat: PatId,
      pub body: ExprId,
  }
  ```

  - `eml_hir::codes::CONSTRUCTOR_ARITY` (E1016)、`eml_hir::codes::DUPLICATE_BINDING` (E1017)
  - `Body::walk_child_exprs` は `Match` の scrutinee と各枝の本体をソースの順に渡す。`Body::pat_bindings` は `PatKind::Con` の引数を左から順にたどる。`Body::captures` は枝のパターンの変数を束縛として扱う
  - HIR の表示: `data Option a` の行と `  | Some a` の行、`(match x#0 with | Some (Some n#1) -> n#1 | None -> 0)`、中置は `h#1 :: _`、等式とラムダの引数のコンストラクタのパターンは `(Some x#0)`、型の適用は `List (Option Int)`

**Interface notes (骨組みの Interfaces から変えた点と、前後のタスクとの取り決め):**

骨組みの Interfaces から、次の点を変えた、または足した。後のタスクはこの形を前提にする。

1. HIR の表示の `match` は、`handle` と同じく括弧で囲む。`(match x#0 with | Some y#1 -> y#1 | None -> 0)` になる。
2. `TypeLowering` の `define: bool` を `vars: Vars` (`Define` / `Signature` / `Data`) にする。`data` のフィールドで宣言にない型変数を書いたときに、ラベルを「not a parameter of this `data` declaration」にするため。呼び出し側は4か所 (`lower/mod.rs`、`prelude.rs`、`effect.rs`、`expr.rs`) で、`Define` か `Signature` を渡す。
3. `effect::lower_effects` は、型の名前空間のユーザーの名前の表 `declared: &mut HashMap<String, TextRange>` を引数で受ける。`data` とエフェクトの名前の重複も E1003 にするため。`ItemScope::define_type` は型引数の個数 `params` を受け、`ItemScope::type_params(id)` で引ける。
4. `TypeDef::builtin(name: &str) -> TypeDef` を足す。`eml_types` の単体テスト (`ty.rs`、`table/tests.rs`) の `TypeDef { name }` は、これを使う形に書き換える (種類3)。
5. 型検査の仮の扱いは、`Res::Constructor`、`ExprKind::Match`、`PatKind::Con` を `Error` の型にし、それぞれ E0004 を出す (「constructors are not supported yet」「`match` is not supported yet」「constructor patterns are not supported yet」)。誤りを出さずに通すと、`eml run` で Core IR の変換に届いてしまうためである。Task 3 はこの3つの E0004 を外す。Core IR の変換は、この2つの形に `unreachable!` を置く。
6. 使用回数のパス (`usage.rs`) の `Match` と `PatKind::Con` は、仮ではなく最終の形で入れる。Task 3 で変える必要はない。
7. Task 3 への申し送り: `Builtin::True` を名前で使うテストが、spec の種類1の表のほかに2つある。`crates/eml_types/tests/check.rs` の `assert!(!checked.typed.builtins.contains_key(&Builtin::True));` (350 行目付近) と、`crates/eml_hir/tests/structure.rs` の `the_prelude_has_a_signature_for_every_builtin_function` (`Builtin::True | Builtin::False` を除く行) である。`Builtin::True` をなくすと、この2つはコンパイルできなくなる。また、`Bool` を Prelude の `data` にすると、HIR の表示 (`pretty`) が `data Bool` を毎回出すので、`lang.bool` の宣言を表示から除く必要がある。
8. E1003 になるコンストラクタの重複は、コンストラクタどうしだけである。コンストラクタは大文字か `:` で始まり、関数と操作は小文字で始まるので、字句の上で重ならない。ユーザーの `data` は組み込みの型の名前 (`Int` など) を隠せる。値の名前空間で組み込みを隠せるのと同じ扱いで、E1003 にしない。
9. 中置のコンストラクタのパターンは、HIR で式の演算子の列と同じ規則で組み直す。パーサは右に入れ子にした木 (`INFIX_CON_PAT`) を作るが、CST は変えずに、HIR の変換で木を被演算子と演算子の列に平らにしてから組む。今は式と同じく `crate::builtin::fixity` の標準の表を引き、表にない演算子 (`:+` など) は `infixl 9` で左に組む。表の中で `:` で始まる演算子は `::` (`infixr 5`) だけなので、宣言した `::` はパターンでも式でも右に組まれる。fixity の宣言を読む段階6では、式とパターンの両方の経路が、宣言を含む同じ演算子の表を引くようにする。

- [ ] **Step 1: 構文木の取り出し口の失敗するテストを書く**

`crates/eml_syntax/tests/ast.rs` の末尾に次の3つのテストを足す。`use` に変更は要らない (`Item`、`Expr`、`Pat`、`Type`、`SyntaxKind::*` は読み込み済み)。

```rust
#[test]
fn data_declaration_parts() {
    let file = source("data List a = | Nil | Cons a (List a) | a :+ List a");
    let Some(Item::DataItem(data)) = file.items().next() else {
        panic!("expected a data declaration");
    };
    assert_eq!(data.name().unwrap().text(), "List");
    let params: Vec<String> = data.params().map(|token| token.text().to_string()).collect();
    assert_eq!(params, ["a"]);
    let alts: Vec<(Option<String>, Option<String>, Vec<SyntaxKind>)> = data
        .alts()
        .map(|alt| {
            (
                alt.name().map(|token| token.text().to_string()),
                alt.operator().map(|token| token.text().to_string()),
                alt.fields().map(|ty| ty.syntax().kind()).collect(),
            )
        })
        .collect();
    assert_eq!(
        alts,
        [
            (Some("Nil".to_string()), None, vec![]),
            (Some("Cons".to_string()), None, vec![VAR_TYPE, PAREN_TYPE]),
            (None, Some(":+".to_string()), vec![VAR_TYPE, APP_TYPE]),
        ]
    );
}

#[test]
fn match_arms_and_constructor_patterns() {
    let file = source("f o = match o with | Some (Pair a b) -> a | x :+ _ -> x");
    let equation = first_equation(&file);
    let Some(Expr::MatchExpr(expr)) = equation.body() else {
        panic!("expected a match");
    };
    assert!(matches!(expr.scrutinee(), Some(Expr::PathExpr(_))));
    let arms: Vec<_> = expr.arms().collect();
    assert_eq!(arms.len(), 2);
    let Some(Pat::ConPat(con)) = arms[0].pat() else {
        panic!("expected a constructor pattern");
    };
    let segments: Vec<String> = con.segments().map(|token| token.text().to_string()).collect();
    assert_eq!(segments, ["Some"]);
    let args: Vec<SyntaxKind> = con.args().map(|pat| pat.syntax().kind()).collect();
    assert_eq!(args, [PAREN_PAT]);
    assert!(matches!(arms[0].body(), Some(Expr::PathExpr(_))));
    let Some(Pat::InfixConPat(infix)) = arms[1].pat() else {
        panic!("expected an infix constructor pattern");
    };
    assert!(matches!(infix.lhs(), Some(Pat::BindPat(_))));
    assert_eq!(infix.operator().unwrap().text(), ":+");
    assert!(matches!(infix.rhs(), Some(Pat::WildcardPat(_))));
}

#[test]
fn type_application_parts() {
    let file = source("f : Option (List Int) -> Int");
    let Some(Item::Signature(signature)) = file.items().next() else {
        panic!("expected a signature");
    };
    let Some(Type::FnType(function)) = signature.ty() else {
        panic!("expected a function type");
    };
    let Some(Type::AppType(app)) = function.param() else {
        panic!("expected a type application");
    };
    let segments: Vec<String> = app.segments().map(|token| token.text().to_string()).collect();
    assert_eq!(segments, ["Option"]);
    let args: Vec<SyntaxKind> = app.args().map(|ty| ty.syntax().kind()).collect();
    assert_eq!(args, [PAREN_TYPE]);
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_syntax --test ast`
Expected: コンパイルエラー (`DataItem::name`、`MatchExpr::scrutinee`、`AppType::segments` などがない)

- [ ] **Step 3: 取り出し口を足す**

`crates/eml_syntax/src/ast.rs` の `impl DropExpr` の後ろに足す。

```rust
impl DataItem {
    pub fn name(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::UIDENT)
    }

    /// 型引数。選択肢は子のノードなので、直下のトークンだけを見る。
    pub fn params(&self) -> impl Iterator<Item = SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .filter(|token| token.kind() == SyntaxKind::LIDENT)
    }

    pub fn alts(&self) -> AstChildren<Alt> {
        support::children(&self.syntax)
    }
}

impl Alt {
    /// 前置のコンストラクタの名前。中置のコンストラクタの左辺の型の名前は子のノードの中にあるので、ここには現れない。
    pub fn name(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::UIDENT)
    }

    /// 中置のコンストラクタの演算子。
    pub fn operator(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::CONOP)
    }

    /// フィールドの型。中置のコンストラクタでは左右の2つである。
    pub fn fields(&self) -> AstChildren<Type> {
        support::children(&self.syntax)
    }
}

impl MatchExpr {
    pub fn scrutinee(&self) -> Option<Expr> {
        child_between(
            &self.syntax,
            Some(SyntaxKind::MATCH_KW),
            Some(SyntaxKind::WITH_KW),
        )
    }

    pub fn arms(&self) -> AstChildren<MatchArm> {
        support::children(&self.syntax)
    }
}

impl MatchArm {
    pub fn pat(&self) -> Option<Pat> {
        support::child(&self.syntax)
    }

    pub fn body(&self) -> Option<Expr> {
        child_between(&self.syntax, Some(SyntaxKind::THIN_ARROW), None)
    }
}

impl ConPat {
    pub fn segments(&self) -> impl Iterator<Item = SyntaxToken> {
        path_segments(&self.syntax)
    }

    pub fn args(&self) -> AstChildren<Pat> {
        support::children(&self.syntax)
    }
}

impl InfixConPat {
    pub fn lhs(&self) -> Option<Pat> {
        child_between(&self.syntax, None, Some(SyntaxKind::CONOP))
    }

    pub fn operator(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::CONOP)
    }

    pub fn rhs(&self) -> Option<Pat> {
        child_between(&self.syntax, Some(SyntaxKind::CONOP), None)
    }
}

impl AppType {
    /// 適用する型の名前。型引数は子のノードなので、直下のトークンだけが名前になる。
    pub fn segments(&self) -> impl Iterator<Item = SyntaxToken> {
        path_segments(&self.syntax)
    }

    pub fn args(&self) -> AstChildren<Type> {
        support::children(&self.syntax)
    }
}
```

- [ ] **Step 4: 構文木のテストが通ることを確かめる**

Run: `cargo test -p eml_syntax --test ast`
Expected: PASS

- [ ] **Step 5: HIR の失敗するテストを書く**

`crates/eml_hir/tests/data.rs` を作る。

```rust
//! `data` の宣言、コンストラクタ、型の適用、パターン、`match` の変換。

mod common;

use common::{diagnostics, lower_text};
use eml_hir::{Body, ExprKind, Module};

#[test]
fn data_declarations_become_items() {
    let text = "data Option a =\n  | None\n  | Some a\n\ndata List a = | Nil | a :+ List a\n\nf : Option Int -> List (Option Int)\nf o = Some 1 :+ Nil";
    insta::assert_snapshot!(lower_text(text), @r"
    data Option a
      | None
      | Some a
    data List a
      | Nil
      | a :+ List a
    f : Option Int -> List (Option Int)
    f o#0 = (:+ (Some 1) Nil)
    ");
}

#[test]
fn match_arms_and_constructor_patterns() {
    let text = "data Option a = | None | Some a\n\nf : Option (Option Int) -> Int\nf o = match o with\n  | Some (Some n) -> n\n  | Some None -> 1\n  | None -> 0\n\ng : Option Int -> Int\ng (Some x) =\n  let Some y = Some x\n  (fn (Some z) -> z) (Some y)";
    insta::assert_snapshot!(lower_text(text), @r"
    data Option a
      | None
      | Some a
    f : Option (Option Int) -> Int
    f o#0 = (match o#0 with | Some (Some n#1) -> n#1 | Some None -> 1 | None -> 0)
    g : Option Int -> Int
    g (Some x#0) = {
      let Some y#1 = (Some x#0)
      ((fn (Some z#2) -> z#2) (Some y#1))
    }
    ");
}

#[test]
fn infix_constructors_and_a_declared_cons() {
    // 宣言した `::` は、標準の演算子の表の `infixr 5` で組み直す
    let text = "data L = | E | Int :: L\n\nf : Int -> L\nf x = x :: x :: E\n\ng : L -> Int\ng l = match l with | h :: _ -> h | E -> 0";
    insta::assert_snapshot!(lower_text(text), @r"
    data L
      | E
      | Int :: L
    f : Int -> L
    f x#0 = (:: x#0 (:: x#0 E))
    g : L -> Int
    g l#0 = (match l#0 with | h#1 :: _ -> h#1 | E -> 0)
    ");
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
fn match_arms_bind_their_pattern_variables_only_in_the_arm() {
    let text = "data Option a = | None | Some a\n\nf : Option Int -> Int -> Int\nf o k = (fn u -> match o with | Some x -> x + k | None -> u) 0";
    let module = eml_test_support::lower_clean(text).module;
    let body = body(&module, "f");
    let names = |locals: Vec<eml_hir::LocalId>| -> Vec<String> {
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
    // 枝のパターンの `x` は捕まえる変数に入らない
    assert_eq!(names(body.lambda_captures(lambda)), ["o", "k"]);
    let (id, arms) = body
        .exprs
        .iter()
        .find_map(|(id, expr)| match &expr.kind {
            ExprKind::Match { arms, .. } => Some((id, arms)),
            _ => None,
        })
        .expect("the match");
    assert_eq!(names(body.pat_bindings(arms[0].pat)), ["x"]);
    let mut children = Vec::new();
    body.walk_child_exprs(id, |child| children.push(child));
    assert_eq!(children.len(), 3, "the scrutinee and the two arm bodies");
}

#[test]
fn infix_constructor_patterns_group_like_expressions() {
    // fixity の宣言がない `:+` は `infixl 9` なので、パターンも式も左に組む (docs/spec/declarations.md)
    let text = "data T = | L | T :+ Int\n\nf : T -> T\nf t = match t with | a :+ b :+ c -> a :+ b :+ c | L -> L";
    insta::assert_snapshot!(lower_text(text), @r"
    data T
      | L
      | T :+ Int
    f : T -> T
    f t#0 = (match t#0 with | (a#1 :+ b#2) :+ c#3 -> (:+ (:+ a#1 b#2) c#3) | L -> L)
    ");
}

#[test]
fn type_names_and_constructors_must_be_unique() {
    let text = "data A = | X | X\n\ndata B a a = | Y\n\ndata C = | X\n\ndata A = | Z\n\neffect C where\n  op : Unit -> Unit";
    assert_eq!(
        diagnostics(text),
        [
            "E1003 1:16 `X` is defined more than once",
            "E1003 3:10 `a` is defined more than once",
            "E1003 5:12 `X` is defined more than once",
            "E1003 7:6 `A` is defined more than once",
            "E1003 9:8 `C` is defined more than once",
        ]
    );
}

#[test]
fn field_types_and_type_arguments_are_checked() {
    let text = "data P a = | P a b (Int -> <e> Int)\n\ndata Q = | Q (P Int Int) P (Int Int)\n\nf : P -> Int\nf x = 1";
    assert_eq!(
        diagnostics(text),
        [
            "E1002 1:18 cannot find type variable `b`",
            "E1002 1:29 cannot find row variable `e`",
            "E1015 3:15 `P` takes 1 type argument, but 2 were given",
            "E1015 3:26 `P` takes 1 type argument, but 0 were given",
            "E1015 3:29 `Int` takes 0 type arguments, but 1 was given",
            "E1015 5:5 `P` takes 1 type argument, but 0 were given",
        ]
    );
}

#[test]
fn constructor_patterns_are_resolved_and_counted() {
    // 名前の引けないパターンの中の変数 (`a`) も束縛するので、枝の本体で E1001 を連鎖させない
    let text = "data Option a = | None | Some a\n\nf : Option Int -> Int\nf o = match o with\n  | Some x y -> x\n  | None z -> 0\n  | Nothing -> 0\n  | Some (Pair a b) -> a";
    assert_eq!(
        diagnostics(text),
        [
            "E1016 5:5 `Some` takes 1 argument, but 2 were given",
            "E1016 6:5 `None` takes 0 arguments, but 1 was given",
            "E1001 7:5 cannot find constructor `Nothing`",
            "E1001 8:11 cannot find constructor `Pair`",
        ]
    );
}

#[test]
fn a_name_is_bound_once_per_pattern_and_per_equation() {
    // 別の `let` の束縛は別の組なので、引数の `p` を隠してよい
    let text = "data Pair a b = | Pair a b\n\nf : Int -> Int -> Int\nf x x = x\n\ng : Pair Int Int -> Int\ng p =\n  let Pair y y = p\n  let p = y\n  p";
    assert_eq!(
        diagnostics(text),
        [
            "E1017 4:5 `x` is bound more than once",
            "E1017 8:14 `y` is bound more than once",
        ]
    );
}
```

`crates/eml_hir/tests/lower.rs` の `constructs_of_later_stages_are_not_yet_supported` を、次の形に置き換える (種類1)。入力から `data` の行を除き、`match` の枝をリテラルのパターンにする。行が1つ減るので、位置は1行ずつ上がる。

```rust
#[test]
fn constructs_of_later_stages_are_not_yet_supported() {
    let text = "f : Int -> Int\nf x =\n  let swap = fn (a, b) -> (b, a)\n  let plus = (+)\n  match x with | 0 -> x";
    insta::assert_snapshot!(lower_text(text), @"
    f : Int -> Int
    f x#0 = {
      let swap#1 = (fn <missing> -> <missing>)
      let plus#2 = <missing>
      (match x#0 with | <missing> -> x#0)
    }
    ---
    E0004 3:17 tuple patterns are not supported yet
    E0004 3:27 tuples are not supported yet
    E0004 4:14 operator references are not supported yet
    E0004 5:18 literal patterns are not supported yet
    ");
}
```

- [ ] **Step 6: テストが失敗することを確かめる**

Run: `cargo test -p eml_hir`
Expected: `data.rs` がコンパイルエラー (`ExprKind::Match` がない)。`lower.rs` の `constructs_of_later_stages_are_not_yet_supported` は、`data` と `match` の E0004 がまだ出るので FAIL

- [ ] **Step 7: HIR の型を変える**

`crates/eml_hir/src/hir.rs`:

`pub type TypeRefId = Idx<TypeRef>;` の後ろに `pub type ConstructorId = Idx<Constructor>;` を足す。

`Module` に `constructors` を足し、`types` の doc コメントを直す。

```rust
#[derive(Debug)]
pub struct Module {
    pub file: FileId,
    pub functions: Arena<Function>,
    /// 型の item。組み込みの `Int`、`String`、`Bool`、`Unit` と、`data` の宣言。
    pub types: Arena<TypeDef>,
    /// `data` の宣言のコンストラクタ。値の名前空間に置くトップレベルの値である (docs/spec/modules.md の「名前空間」)。
    pub constructors: Arena<Constructor>,
    /// エフェクトの item。組み込みの `IO` と、`effect` の宣言。
    pub effects: Arena<EffectDef>,
    /// エフェクトの操作。値の名前空間に置くトップレベルの値である (docs/spec/modules.md の「名前空間」)。
    pub operations: Arena<Operation>,
    /// Prelude の組み込みのシグネチャ。
    pub builtins: HashMap<Builtin, Signature>,
    pub lang: LangItems,
}
```

`TypeDef` を置き換え、`TypeDefKind` と `Constructor` を足す。

```rust
#[derive(Debug)]
pub struct TypeDef {
    pub name: String,
    /// 宣言の型引数。型引数は型だけで、row 変数は持たない (docs/spec/declarations.md の「`data` と `type`」)。
    pub generics: Generics,
    /// フィールドの型の注釈。`Constructor::fields` が指す。
    pub types: Arena<TypeRef>,
    pub kind: TypeDefKind,
}

impl TypeDef {
    /// 型引数もコンストラクタも持たない組み込みの型。
    pub fn builtin(name: &str) -> TypeDef {
        TypeDef {
            name: name.to_string(),
            generics: Generics::default(),
            types: Arena::new(),
            kind: TypeDefKind::Builtin,
        }
    }
}

#[derive(Debug)]
pub enum TypeDefKind {
    /// `Int`、`String` など、宣言を持たない組み込みの型。
    Builtin,
    /// 宣言した順のコンストラクタ。
    Data { constructors: Vec<ConstructorId> },
}

#[derive(Debug)]
pub struct Constructor {
    /// 中置のコンストラクタは演算子 (`:+`) が名前である。
    pub name: String,
    pub range: TextRange,
    pub ty: TypeDefId,
    /// 宣言の中の順の番号。Core IR のタグになる。
    pub tag: u32,
    /// フィールドの型。属する `TypeDef` の `types` と `generics` で解決する。
    pub fields: Vec<TypeRefId>,
}
```

`Generics` の doc コメントを「型変数と row 変数の表。関数と操作のシグネチャ、エフェクトと `data` の宣言が持つ。」に直す。

`Body::walk_child_exprs` の doc コメントの「段階4で `match` を足すときはここを直す。」を消し、`ExprKind::Drop(value) => f(*value),` の前に足す。

```rust
            ExprKind::Match { scrutinee, arms } => {
                f(*scrutinee);
                for arm in arms {
                    f(arm.body);
                }
            }
```

`collect_bindings` に `PatKind::Con` を足す。

```rust
    fn collect_bindings(&self, pat: PatId, out: &mut Vec<LocalId>) {
        match &self.pats[pat].kind {
            PatKind::Bind(local) => out.push(*local),
            PatKind::Annot { pat, .. } => self.collect_bindings(*pat, out),
            PatKind::Con { args, .. } => {
                for &arg in args {
                    self.collect_bindings(arg, out);
                }
            }
            PatKind::Missing | PatKind::Wildcard | PatKind::Unit => {}
        }
    }
```

`Body::captures` の `ExprKind::Handle { .. }` の腕の後ろに足す。

```rust
                ExprKind::Match { arms, .. } => {
                    for arm in arms {
                        bound.extend(self.pat_bindings(arm.pat));
                    }
                }
```

`ExprKind` の `Drop(ExprId),` の前に足し、`MatchArm` を足す。

```rust
    /// 枝のパターンが束縛する変数は、その枝の本体だけで見える (docs/spec/expressions.md の「`match`」)。
    Match {
        scrutinee: ExprId,
        arms: Vec<MatchArm>,
    },
```

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchArm {
    pub pat: PatId,
    pub body: ExprId,
}
```

`Res` に `Constructor(ConstructorId),` を `Operation(OperationId),` の後ろに足す。

`PatKind` の `Annot { .. }` の後ろに足す。

```rust
    /// 前置と中置のコンストラクタのパターン。引数の個数はフィールドの数と一致する。違えば E1016 を報告して `Missing`
    /// にする。
    Con {
        ctor: ConstructorId,
        args: Vec<PatId>,
    },
```

`TypeRefKind::Con` を変える。

```rust
    /// 型引数の個数は宣言と一致する。違えば E1015 を報告して `Error` にする。
    Con(TypeDefId, Vec<TypeRefId>),
```

`crates/eml_hir/src/lib.rs` の `codes` に足す。

```rust
    pub const CONSTRUCTOR_ARITY: ErrorCode = ErrorCode(1016);
    pub const DUPLICATE_BINDING: ErrorCode = ErrorCode(1017);
```

- [ ] **Step 8: `ItemScope` にコンストラクタと型引数の個数を持たせる**

`crates/eml_hir/src/lower/scope.rs`:

```rust
use crate::hir::{
    ConstructorId, EffectDef, EffectId, FunctionId, Generics, LangItems, OperationId, TypeDef,
    TypeDefId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ValueItem {
    Function(FunctionId),
    Operation(OperationId),
    Constructor(ConstructorId),
    Builtin(Builtin),
}
```

```rust
#[derive(Debug, Default)]
pub(super) struct ItemScope {
    /// ユーザーが定義した値 (関数、操作、コンストラクタ)。
    values: HashMap<String, ValueItem>,
    types: HashMap<String, TypeItem>,
    /// エフェクトの型引数の個数。row のエフェクトの型引数の個数を確かめるのに使う (E1015)。
    effect_params: HashMap<EffectId, usize>,
    /// 型の型引数の個数。型の適用の型引数の個数を確かめるのに使う (E1015)。
    type_params: HashMap<TypeDefId, usize>,
}
```

`define_type` を置き換え、コンストラクタの登録と引き方を足す。

```rust
    /// コンストラクタは大文字か `:` で始まり、関数と操作は小文字で始まるので、同じ名前のユーザーの値はない。
    pub(super) fn define_constructor(&mut self, name: &str, id: ConstructorId) {
        self.values
            .insert(name.to_string(), ValueItem::Constructor(id));
    }

    pub(super) fn define_type(&mut self, name: &str, id: TypeDefId, params: usize) {
        self.types.insert(name.to_string(), TypeItem::Type(id));
        self.type_params.insert(id, params);
    }

    pub(super) fn type_params(&self, id: TypeDefId) -> usize {
        self.type_params.get(&id).copied().unwrap_or(0)
    }

    /// パターンの先頭の名前は、コンストラクタだけから引く (docs/spec/modules.md の「名前の解決」)。
    pub(super) fn constructor(&self, name: &str) -> Option<ConstructorId> {
        match self.values.get(name) {
            Some(ValueItem::Constructor(id)) => Some(*id),
            _ => None,
        }
    }
```

`builtin_items` の型の登録を次のようにする。

```rust
    let mut ty = |name: &str| {
        let id = types.alloc(TypeDef::builtin(name));
        scope.define_type(name, id, 0);
        id
    };
```

`mod tests` の中に `TypeDef` や `define_type` を使う箇所があれば、同じ形に合わせる。

- [ ] **Step 9: 型の適用と、`data` のフィールドの型変数を変換する**

`crates/eml_hir/src/lower/types.rs`:

`TypeLowering` の `define` を `vars` にし、`Vars` を足す。

```rust
/// 型変数と row 変数の名前の引き方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Vars {
    /// シグネチャ。新しい変数の名前を表に入れる。
    Define,
    /// 本体の注釈。シグネチャの表にある名前だけを使える (docs/spec/types.md の「推論」)。
    Signature,
    /// `data` の宣言のフィールド。宣言の型引数だけを使える。宣言は row 変数を持たない
    /// (docs/spec/declarations.md の「`data` と `type`」)。
    Data,
}

pub(super) struct TypeLowering<'a> {
    pub file: FileId,
    pub types: &'a mut Arena<TypeRef>,
    pub generics: &'a mut Generics,
    pub items: &'a ItemScope,
    pub vars: Vars,
    pub diagnostics: &'a mut Vec<Diagnostic>,
}
```

`lower` の `PathType` と `AppType` の腕を置き換える。

```rust
            ast::Type::PathType(path) => {
                let segments: Vec<SyntaxToken> = path.segments().collect();
                self.applied(&segments, Vec::new(), range)
            }
```

```rust
            ast::Type::AppType(app) => {
                // 型引数を先に変換し、型の名前が誤っていても型引数の中の誤りを報告する
                let args = app
                    .args()
                    .map(|arg| {
                        let range = arg.range();
                        self.lower(Some(arg), range)
                    })
                    .collect();
                let segments: Vec<SyntaxToken> = app.segments().collect();
                self.applied(&segments, args, range)
            }
```

`fn path` を消し、`applied` と `arity_error` を足す。

```rust
    fn applied(
        &mut self,
        segments: &[SyntaxToken],
        args: Vec<TypeRefId>,
        range: TextRange,
    ) -> TypeRefKind {
        let [name] = segments else {
            return self.unsupported(range, "qualified names are not supported yet");
        };
        match self.items.type_item(name.text()) {
            Some(TypeItem::Type(id)) => {
                let expected = self.items.type_params(id);
                if args.len() != expected {
                    self.arity_error(name.text(), expected, args.len(), range);
                    return TypeRefKind::Error;
                }
                TypeRefKind::Con(id, args)
            }
            Some(TypeItem::Effect(_)) | None => {
                self.diagnostics.push(Diagnostic::error(
                    codes::UNDEFINED_TYPE,
                    format!("cannot find type `{}`", name.text()),
                    Label::new(self.file, range, "not found in this scope"),
                ));
                TypeRefKind::Error
            }
        }
    }

    /// 型とエフェクトの型引数の個数の誤り (E1015)。
    fn arity_error(&mut self, name: &str, expected: usize, given: usize, range: TextRange) {
        let given = match given {
            1 => "1 was given".to_string(),
            n => format!("{n} were given"),
        };
        self.diagnostics.push(Diagnostic::error(
            codes::TYPE_ARGUMENT_COUNT,
            format!("`{name}` takes {}, but {given}", type_arguments(expected)),
            Label::new(
                self.file,
                range,
                format!("expected {}", type_arguments(expected)),
            ),
        ));
    }
```

`row` の中の E1015 の組み立て (`let given = ...` から `self.diagnostics.push(...)` まで) を、次の1行に置き換える。文言は変わらない。

```rust
                    if args.len() != expected {
                        self.arity_error(name.text(), expected, args.len(), effect.range());
                        valid = false;
                        continue;
                    }
```

`type_var` と `row_var` の `if self.define {` を `if self.vars == Vars::Define {` にし、E1002 のラベルを `self.undeclared()` にする。

```rust
    /// 表にない変数の名前を書いたときのラベル。
    fn undeclared(&self) -> &'static str {
        match self.vars {
            Vars::Data => "not a parameter of this `data` declaration",
            Vars::Define | Vars::Signature => "not found in the signature",
        }
    }
```

```rust
        self.diagnostics.push(Diagnostic::error(
            codes::UNDEFINED_TYPE,
            format!("cannot find type variable `{text}`"),
            Label::new(self.file, range, self.undeclared()),
        ));
```

```rust
        self.diagnostics.push(Diagnostic::error(
            codes::UNDEFINED_TYPE,
            format!("cannot find row variable `{text}`"),
            Label::new(self.file, range, self.undeclared()),
        ));
```

`TypeLowering { .. define: true, .. }` を `vars: Vars::Define` に直す (`lower/mod.rs`、`lower/prelude.rs`、`lower/effect.rs`)。`lower/expr.rs` の `lower_type` は `vars: Vars::Signature` にする。各ファイルの `use super::types::TypeLowering;` を `use super::types::{TypeLowering, Vars};` にする。

`crates/eml_hir/src/lower/effect.rs` の `check_signature` と `mentions` を、`Con` の引数に合わせる。

```rust
            TypeRefKind::Con(..) | TypeRefKind::Fn { .. } => false,
```

```rust
fn mentions(types: &Arena<TypeRef>, id: TypeRefId, var: TypeVarId) -> bool {
    match &types[id].kind {
        TypeRefKind::Var(other) => *other == var,
        TypeRefKind::Fn { param, ret, .. } => {
            mentions(types, *param, var) || mentions(types, *ret, var)
        }
        TypeRefKind::Con(_, args) => args.iter().any(|&arg| mentions(types, arg, var)),
        TypeRefKind::Error => false,
    }
}
```

- [ ] **Step 10: `data` の宣言を変換する**

`crates/eml_hir/src/lower/data.rs` を作る。

```rust
//! `data` の宣言の変換 (docs/spec/declarations.md の「`data` と `type`」)。型は型の名前空間に、コンストラクタは
//! 値の名前空間に置く (docs/spec/modules.md の「名前空間」)。

use std::collections::HashMap;

use eml_diagnostics::{Diagnostic, FileId, TextRange};
use eml_syntax::ast;
use la_arena::Arena;

use super::duplicate;
use super::scope::ItemScope;
use super::types::{TypeLowering, Vars};
use crate::hir::{Constructor, Generics, TypeDef, TypeDefId, TypeDefKind, TypeVarDecl};

/// 型の名前と型引数だけを先に登録する。フィールドの型と操作のシグネチャが、後ろで宣言した型も引けるようにするため。
/// `declared` は型の名前空間のユーザーの名前で、エフェクトの宣言と共有して重複 (E1003) を見つける。
pub(super) fn declare_data(
    file: FileId,
    items: &[ast::DataItem],
    declared: &mut HashMap<String, TextRange>,
    scope: &mut ItemScope,
    types: &mut Arena<TypeDef>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<(TypeDefId, ast::DataItem)> {
    let mut lowered = Vec::new();
    for item in items {
        // 名前がなければパーサが報告済み
        let Some(name) = item.name() else {
            continue;
        };
        let mut generics = Generics::default();
        for param in item.params() {
            let text = param.text();
            let range = param.text_range();
            if let Some((_, first)) = generics.type_vars.iter().find(|(_, var)| var.name == text) {
                diagnostics.push(duplicate(file, text, first.range, range));
                continue;
            }
            generics.type_vars.alloc(TypeVarDecl {
                name: text.to_string(),
                range,
            });
        }
        let params = generics.type_vars.len();
        let range = name.text_range();
        let id = types.alloc(TypeDef {
            name: name.text().to_string(),
            generics,
            types: Arena::new(),
            kind: TypeDefKind::Data {
                constructors: Vec::new(),
            },
        });
        match declared.get(name.text()) {
            Some(&first) => diagnostics.push(duplicate(file, name.text(), first, range)),
            None => {
                declared.insert(name.text().to_string(), range);
                scope.define_type(name.text(), id, params);
            }
        }
        lowered.push((id, item.clone()));
    }
    lowered
}

/// フィールドの型を変換し、コンストラクタを値の名前空間に置く。タグは宣言の中の順の番号である。
pub(super) fn lower_constructors(
    file: FileId,
    data: &[(TypeDefId, ast::DataItem)],
    scope: &mut ItemScope,
    types: &mut Arena<TypeDef>,
    constructors: &mut Arena<Constructor>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut values: HashMap<String, TextRange> = HashMap::new();
    for (ty, item) in data {
        for alt in item.alts() {
            // 名前も演算子もなければパーサが報告済み
            let Some(name) = alt.name().or_else(|| alt.operator()) else {
                continue;
            };
            let def = &mut types[*ty];
            let mut lowering = TypeLowering {
                file,
                types: &mut def.types,
                generics: &mut def.generics,
                items: scope,
                vars: Vars::Data,
                diagnostics: &mut *diagnostics,
            };
            let fields = alt
                .fields()
                .map(|field| {
                    let range = field.range();
                    lowering.lower(Some(field), range)
                })
                .collect();
            let TypeDefKind::Data {
                constructors: declared,
            } = &mut def.kind
            else {
                unreachable!("`declare_data` makes data types")
            };
            let range = name.text_range();
            let id = constructors.alloc(Constructor {
                name: name.text().to_string(),
                range,
                ty: *ty,
                tag: declared.len() as u32,
                fields,
            });
            declared.push(id);
            match values.get(name.text()) {
                Some(&first) => diagnostics.push(duplicate(file, name.text(), first, range)),
                None => {
                    values.insert(name.text().to_string(), range);
                    scope.define_constructor(name.text(), id);
                }
            }
        }
    }
}
```

`crates/eml_hir/src/lower/effect.rs` の `lower_effects` に `declared` を引数で渡し、関数の中の `let mut declared: HashMap<String, TextRange> = HashMap::new();` を消す。

```rust
/// エフェクトの名前をすべて登録してから、操作のシグネチャを変換する。操作の引数の型の row で、後ろで宣言した
/// エフェクトも引けるようにするため。`declared` は `data` の宣言と共有する型の名前空間のユーザーの名前である。
pub(super) fn lower_effects(
    file: FileId,
    items: &[ast::EffectItem],
    declared: &mut HashMap<String, TextRange>,
    scope: &mut ItemScope,
    effects: &mut Arena<EffectDef>,
    operations: &mut Arena<Operation>,
    diagnostics: &mut Vec<Diagnostic>,
) {
```

`crates/eml_hir/src/lower/mod.rs`:

- `mod data;` を `mod effect;` の前に足す。
- `collect` が `data` の宣言も集めて返すようにする。

```rust
fn collect(
    file: FileId,
    source: &ast::SourceFile,
    diagnostics: &mut Vec<Diagnostic>,
) -> (Vec<Definition>, Vec<ast::DataItem>, Vec<ast::EffectItem>) {
    let mut definitions: Vec<Definition> = Vec::new();
    let mut by_name: HashMap<String, usize> = HashMap::new();
    let mut data = Vec::new();
    let mut effects = Vec::new();
```

  `ast::Item::DataItem(item) => diagnostics.push(Diagnostic::not_yet_supported(...)),` を `ast::Item::DataItem(item) => data.push(item),` にし、最後を `(definitions, data, effects)` にする。

- `lower` の先頭を次のようにする。

```rust
pub fn lower(file: FileId, source: &ast::SourceFile) -> (Module, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let (definitions, data_items, effect_items) = collect(file, source, &mut diagnostics);
    let mut functions = Arena::new();
    let mut scope = ItemScope::new();
    let mut types = Arena::new();
    let mut constructors = Arena::new();
    let mut effects = Arena::new();
    let mut operations = Arena::new();
    let lang = scope::builtin_items(&mut types, &mut effects, &mut scope);
    let builtins = prelude::lower_prelude(&scope);
    // 型の名前空間のユーザーの名前。`data` とエフェクトの間の重複も見つける
    let mut type_names = HashMap::new();
    let data = data::declare_data(
        file,
        &data_items,
        &mut type_names,
        &mut scope,
        &mut types,
        &mut diagnostics,
    );
    // 関数のシグネチャの row がユーザーのエフェクトを引けるように、エフェクトを先に変換する
    effect::lower_effects(
        file,
        &effect_items,
        &mut type_names,
        &mut scope,
        &mut effects,
        &mut operations,
        &mut diagnostics,
    );
    // フィールドの関数型の row がエフェクトを引けるように、コンストラクタはエフェクトの後に変換する
    data::lower_constructors(
        file,
        &data,
        &mut scope,
        &mut types,
        &mut constructors,
        &mut diagnostics,
    );
```

- `BodyLowering::new` の呼び出しに `&constructors` を `&operations` の後ろに足し、`Module { .. }` に `constructors,` を足す。

- [ ] **Step 11: パターン、`match`、コンストラクタの式を変換する**

`crates/eml_hir/src/lower/expr.rs`:

`BodyLowering` にフィールドを足す。

```rust
    pub(super) operations: &'a Arena<Operation>,
    /// コンストラクタのパターンの引数の個数を確かめる (E1016)。
    constructors: &'a Arena<Constructor>,
```

```rust
    /// 内側の束縛ほど後ろにある。後の `let` が前の同じ名前を隠す (docs/spec/expressions.md)。
    pub(super) scope: Vec<(String, LocalId)>,
    /// 今変換しているパターンの組が `scope` に積み始めた位置。組の中で同じ名前を2回束縛したら E1017 にする。
    group_start: usize,
```

`new` は `operations` の後ろに `constructors: &'a Arena<Constructor>` を受け、`constructors,` と `group_start: 0,` を入れる。

`lower_equation` の引数の変換を次のようにする。

```rust
        // 等式の引数の並びは、1つのパターンと同じく1つの組である (E1017)
        self.group_start = self.scope.len();
        let params = equation
            .params()
            .map(|pat| self.lower_pat_in_group(Some(pat), range))
            .collect();
```

`lower_expr` の `MatchExpr` の腕を置き換える。

```rust
            ast::Expr::MatchExpr(e) => self.lower_match(&e, range),
```

`lower_path` の `ValueItem` の対応に足す。

```rust
                    ValueItem::Constructor(id) => Res::Constructor(id),
```

`lower_match` を足す。

```rust
    fn lower_match(&mut self, expr: &ast::MatchExpr, range: TextRange) -> ExprId {
        let scrutinee = self.lower_expr(expr.scrutinee(), range);
        let arms = expr
            .arms()
            .map(|arm| {
                let arm_range = arm.range();
                let mark = self.scope.len();
                let pat = self.lower_pat(arm.pat(), arm_range);
                let body = self.lower_expr(arm.body(), arm_range);
                self.scope.truncate(mark);
                MatchArm { pat, body }
            })
            .collect();
        self.alloc(ExprKind::Match { scrutinee, arms }, range)
    }
```

`lower_pat` を、組を始める入口と、組の中で変換する本体に分ける。

```rust
    /// 1つのパターンを1つの組として変換する。
    pub(super) fn lower_pat(&mut self, pat: Option<ast::Pat>, fallback: TextRange) -> PatId {
        let outer = std::mem::replace(&mut self.group_start, self.scope.len());
        let id = self.lower_pat_in_group(pat, fallback);
        self.group_start = outer;
        id
    }

    fn lower_pat_in_group(&mut self, pat: Option<ast::Pat>, fallback: TextRange) -> PatId {
        let Some(pat) = pat else {
            return self.pats.alloc(Pat {
                kind: PatKind::Missing,
                range: fallback,
            });
        };
        let range = pat.range();
        let kind = match pat {
            ast::Pat::BindPat(bind) => match bind.name() {
                Some(name) => {
                    let name = name.text().to_string();
                    if let Some(&(_, first)) = self.scope[self.group_start..]
                        .iter()
                        .find(|(bound, _)| *bound == name)
                    {
                        let first = self.locals[first].range;
                        self.duplicate_binding(&name, first, range);
                    }
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
            ast::Pat::ParenPat(paren) => return self.lower_pat_in_group(paren.pat(), range),
            ast::Pat::ConPat(con) => {
                let args = con
                    .args()
                    .map(|arg| {
                        let arg_range = arg.range();
                        self.lower_pat_in_group(Some(arg), arg_range)
                    })
                    .collect();
                let segments: Vec<SyntaxToken> = con.segments().collect();
                match segments.as_slice() {
                    [name] => self.constructor_pat(name, args, range),
                    _ => self.unsupported_pat(range, "qualified names are not supported yet"),
                }
            }
            ast::Pat::InfixConPat(infix) => return self.lower_infix_pat(infix, range),
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

    /// パーサは中置のコンストラクタのパターンを右に入れ子の木で作る (docs/spec/grammar.md の `pat`)。式の演算子の列と
    /// 同じ fixity で組むため、木を被演算子と演算子の列に平らにしてから組み直す。fixity の宣言がない演算子は
    /// `infixl 9` なので (docs/spec/declarations.md)、`a :+ b :+ c` は式と同じく `(a :+ b) :+ c` になる。
    fn lower_infix_pat(&mut self, infix: ast::InfixConPat, range: TextRange) -> PatId {
        let mut operands = Vec::new();
        let mut operators = Vec::new();
        let mut current = infix;
        loop {
            operands.push(current.lhs());
            operators.push(
                current
                    .operator()
                    .expect("the parser makes an infix constructor pattern at its operator"),
            );
            match current.rhs() {
                // 括弧で囲んだ右辺は `ParenPat` なので、平らにせずに1つの被演算子のまま残る
                Some(ast::Pat::InfixConPat(next)) => current = next,
                rhs => {
                    operands.push(rhs);
                    break;
                }
            }
        }
        // 変数は左から順に束縛する。E1017 の組と局所変数の番号を、ソースの順にそろえるため
        let operands: Vec<PatId> = operands
            .into_iter()
            .map(|pat| self.lower_pat_in_group(pat, range))
            .collect();
        let mut position = 0;
        self.climb_pat(&operands, &operators, &mut position, 0)
    }

    /// 式の `climb` と同じ優先順位の上昇法である。表の中で `:` で始まる演算子は `::` だけなので、同じ優先順位で
    /// 結合の向きが違う並びは起きず、E1006 は出さない。
    fn climb_pat(
        &mut self,
        operands: &[PatId],
        operators: &[SyntaxToken],
        position: &mut usize,
        min_precedence: u8,
    ) -> PatId {
        let mut lhs = operands[*position];
        while let Some(operator) = operators.get(*position) {
            let (precedence, assoc) = fixity(operator.text()).unwrap_or((9, Assoc::Left));
            if precedence < min_precedence {
                break;
            }
            *position += 1;
            let next_min = if assoc == Assoc::Right {
                precedence
            } else {
                precedence + 1
            };
            let rhs = self.climb_pat(operands, operators, position, next_min);
            let whole = self.pats[lhs].range.cover(self.pats[rhs].range);
            let kind = self.constructor_pat(operator, vec![lhs, rhs], whole);
            lhs = self.pats.alloc(Pat { kind, range: whole });
        }
        lhs
    }

    /// 引数のパターンは呼び出し側が先に変換する。コンストラクタが決まらなくても引数の変数を束縛し、枝の本体で名前の
    /// 誤りを連鎖させないため。
    fn constructor_pat(
        &mut self,
        name: &SyntaxToken,
        args: Vec<PatId>,
        range: TextRange,
    ) -> PatKind {
        let Some(ctor) = self.items.constructor(name.text()) else {
            self.diagnostics.push(Diagnostic::error(
                codes::UNDEFINED_NAME,
                format!("cannot find constructor `{}`", name.text()),
                Label::new(self.file, name.text_range(), "not found in this scope"),
            ));
            return PatKind::Missing;
        };
        let expected = self.constructors[ctor].fields.len();
        if args.len() != expected {
            let given = match args.len() {
                1 => "1 was given".to_string(),
                n => format!("{n} were given"),
            };
            self.diagnostics.push(Diagnostic::error(
                codes::CONSTRUCTOR_ARITY,
                format!("`{}` takes {}, but {given}", name.text(), arguments(expected)),
                Label::new(self.file, range, format!("expected {}", arguments(expected))),
            ));
            return PatKind::Missing;
        }
        PatKind::Con { ctor, args }
    }

    fn duplicate_binding(&mut self, name: &str, first: TextRange, again: TextRange) {
        self.diagnostics.push(
            Diagnostic::error(
                codes::DUPLICATE_BINDING,
                format!("`{name}` is bound more than once"),
                Label::new(self.file, again, "bound again here"),
            )
            .with_secondary(Label::new(self.file, first, "first bound here")),
        );
    }
```

`use` に `use crate::builtin::{Assoc, fixity};` を足す。ファイルの末尾に足す。

```rust
fn arguments(n: usize) -> String {
    if n == 1 {
        "1 argument".to_string()
    } else {
        format!("{n} arguments")
    }
}
```

`crates/eml_hir/src/lower/ops.rs` の `binary` の最後の腕で、呼ばれる式を次のようにする。

```rust
            _ => {
                // 中置のコンストラクタも、演算子と同じく2引数の呼び出しにする
                let callee = if let Some(ctor) = self.items.constructor(op) {
                    self.alloc(ExprKind::Path(Res::Constructor(ctor)), op_range)
                } else {
                    match Builtin::binary_operator(op) {
                        Some(builtin) => {
                            self.alloc(ExprKind::Path(Res::Builtin(builtin)), op_range)
                        }
                        None if op == "::" => {
                            self.unsupported(op_range, "lists are not supported yet")
                        }
                        None => {
                            self.diagnostics.push(Diagnostic::error(
                                codes::UNDEFINED_NAME,
                                format!("cannot find operator `{op}`"),
                                Label::new(self.file, op_range, "not found in this scope"),
                            ));
                            self.alloc(ExprKind::Missing, op_range)
                        }
                    }
                };
```

`BodyLowering` の `items` は `pub(super)` なので、`ops.rs` から引ける。

- [ ] **Step 12: HIR の表示を足す**

`crates/eml_hir/src/pretty.rs`:

`pretty` の先頭 (エフェクトの前) に `data` の宣言を足す。

```rust
    for (_, def) in module.types.iter() {
        let TypeDefKind::Data { constructors } = &def.kind else {
            continue;
        };
        let params: Vec<&str> = def
            .generics
            .type_vars
            .iter()
            .map(|(_, var)| var.name.as_str())
            .collect();
        if params.is_empty() {
            writeln!(out, "data {}", def.name).unwrap();
        } else {
            writeln!(out, "data {} {}", def.name, params.join(" ")).unwrap();
        }
        let printer = Printer {
            module,
            generics: &def.generics,
        };
        for &ctor in constructors {
            writeln!(out, "  | {}", printer.constructor(def, &module.constructors[ctor])).unwrap();
        }
    }
```

`Printer` に足す。

```rust
    fn constructor(&self, def: &TypeDef, ctor: &Constructor) -> String {
        // 中置のコンストラクタは、宣言と同じく2つのフィールドの間に書く
        if ctor.name.starts_with(':') && ctor.fields.len() == 2 {
            return format!(
                "{} {} {}",
                self.ty_operand(&def.types, ctor.fields[0]),
                ctor.name,
                self.ty_operand(&def.types, ctor.fields[1])
            );
        }
        let mut text = ctor.name.clone();
        for &field in &ctor.fields {
            write!(text, " {}", self.ty_atom(&def.types, field)).unwrap();
        }
        text
    }

    /// 型の適用の引数の位置に置く型。型の適用と関数型は括弧で囲む。
    fn ty_atom(&self, types: &la_arena::Arena<TypeRef>, id: TypeRefId) -> String {
        let text = self.ty(types, id);
        match &types[id].kind {
            TypeRefKind::Fn { .. } => format!("({text})"),
            TypeRefKind::Con(_, args) if !args.is_empty() => format!("({text})"),
            _ => text,
        }
    }

    /// 関数型の引数や中置のコンストラクタの両側に置く型。関数型だけを括弧で囲む。
    fn ty_operand(&self, types: &la_arena::Arena<TypeRef>, id: TypeRefId) -> String {
        let text = self.ty(types, id);
        match &types[id].kind {
            TypeRefKind::Fn { .. } => format!("({text})"),
            _ => text,
        }
    }

    /// 引数を持つコンストラクタのパターンを括弧で囲む。等式とラムダの引数、コンストラクタの引数の位置で使う。
    fn pat_atom(&self, body: &Body, id: PatId) -> String {
        let text = self.pat(body, id);
        match &body.pats[id].kind {
            PatKind::Con { args, .. } if !args.is_empty() => format!("({text})"),
            _ => text,
        }
    }
```

`function` の引数の表示と、`ExprKind::Lambda` の引数の表示を `self.pat(body, param)` から `self.pat_atom(body, param)` にする。

`expr` の `ExprKind::Drop(value) => ...` の前に足す。

```rust
            ExprKind::Match { scrutinee, arms } => {
                let mut s = format!("(match {} with", self.expr(body, *scrutinee, indent));
                for arm in arms {
                    write!(
                        s,
                        " | {} -> {}",
                        self.pat(body, arm.pat),
                        self.expr(body, arm.body, indent)
                    )
                    .unwrap();
                }
                s + ")"
            }
```

`res` に足す。

```rust
            Res::Constructor(ctor) => self.module.constructors[ctor].name.clone(),
```

`pat` に足す。

```rust
            PatKind::Con { ctor, args } => {
                let name = &self.module.constructors[*ctor].name;
                let args: Vec<String> = args.iter().map(|&arg| self.pat_atom(body, arg)).collect();
                if name.starts_with(':') && args.len() == 2 {
                    format!("{} {name} {}", args[0], args[1])
                } else if args.is_empty() {
                    name.clone()
                } else {
                    format!("{name} {}", args.join(" "))
                }
            }
```

`ty` の `TypeRefKind::Con` の腕を置き換え、関数型の引数と row の型引数の括弧を、上の2つの関数で書く。

```rust
            TypeRefKind::Con(id, args) => {
                let mut text = self.module.types[*id].name.clone();
                for &arg in args {
                    write!(text, " {}", self.ty_atom(types, arg)).unwrap();
                }
                text
            }
```

```rust
            TypeRefKind::Fn { param, row, ret } => {
                let param_text = self.ty_operand(types, *param);
                let effect_names = |effects: &[EffectRef]| -> Vec<String> {
                    effects
                        .iter()
                        .map(|effect| {
                            let mut text = self.module.effects[effect.effect].name.clone();
                            for &arg in &effect.args {
                                write!(text, " {}", self.ty_atom(types, arg)).unwrap();
                            }
                            text
                        })
                        .collect()
                };
```

(row と結果の型の組み立ては今のまま。)

- [ ] **Step 13: 型検査と Core IR を新しい形に追随させる**

`crates/eml_types/src/scheme.rs` の `lower` の `Con` の腕を、引数を読まない形にする。

```rust
        TypeRefKind::Con(id, _) if *id == table.lang.unit => table.unit,
        TypeRefKind::Con(id, _) if *id == table.lang.int => table.int,
        TypeRefKind::Con(id, _) if *id == table.lang.string => table.string,
        TypeRefKind::Con(id, _) if *id == table.lang.bool => table.bool,
        TypeRefKind::Con(id, _) => table.alloc(TyShape::Con(*id)),
```

`crates/eml_types/src/check/mod.rs` の `has_error`:

```rust
        TypeRefKind::Con(_, args) => args.iter().any(|&arg| has_error(types, arg)),
```

`crates/eml_types/src/check/report.rs` の `callee_subject` に足す。

```rust
        ExprKind::Path(Res::Constructor(ctor)) => module.constructors[*ctor].name.as_str(),
```

`crates/eml_types/src/check/body.rs`:

`use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};` に `TextSize` を足す。

`infer_expr` の `ExprKind::Drop(value) => { .. }` の前に足す。

```rust
            ExprKind::Match { .. } => {
                let keyword = TextRange::at(expr.range.start(), TextSize::of("match"));
                self.diagnostics.push(Diagnostic::not_yet_supported(
                    self.file(),
                    keyword,
                    "`match` is not supported yet",
                ));
                self.table.error
            }
```

`value` の `Res::Builtin(Builtin::True | Builtin::False)` の腕の前に足す。

```rust
            Res::Constructor(_) => {
                self.diagnostics.push(Diagnostic::not_yet_supported(
                    self.file(),
                    range,
                    "constructors are not supported yet",
                ));
                return self.table.error;
            }
```

`bind_pat` に足す。

```rust
            PatKind::Con { .. } => {
                self.diagnostics.push(Diagnostic::not_yet_supported(
                    self.file(),
                    body.pats[pat].range,
                    "constructor patterns are not supported yet",
                ));
                let error = self.table.error;
                for local in body.pat_bindings(pat) {
                    self.typing.locals.insert(local, error);
                }
            }
```

`crates/eml_types/src/usage.rs`:

`expr` の `ExprKind::Drop(value) => self.expr(*value),` の前に足す。

```rust
            // 枝は `if` の枝と同じく別の経路である。枝のパターンの変数は枝の外から見えないので、枝ごとに数え終える
            ExprKind::Match { scrutinee, arms } => {
                let mut uses = self.expr(*scrutinee);
                let mut branches: Option<Uses> = None;
                for arm in arms {
                    let mut inner = self.expr(arm.body);
                    self.check_pat(arm.pat, &inner);
                    remove_bound(body, arm.pat, &mut inner);
                    branches = Some(match branches {
                        Some(other) => join(other, inner),
                        None => inner,
                    });
                }
                sequence(&mut uses, branches.unwrap_or_default());
                uses
            }
```

`check_pat` に足す。

```rust
            PatKind::Con { args, .. } => {
                for &arg in args {
                    self.check_pat(arg, uses);
                }
            }
```

`crates/eml_types/src/ty.rs` の `mod tests` と `crates/eml_types/src/table/tests.rs` の `TypeDef { name: name.to_string() }` を `TypeDef::builtin(name)` にする (種類3。期待値は変わらない)。

`crates/eml_core_ir/src/translate/expr.rs` の `atom` の `ExprKind::Path(Res::Operation(op)) => { .. }` の後ろに足す。

```rust
            ExprKind::Path(Res::Constructor(_)) | ExprKind::Match { .. } => {
                unreachable!("the type checker reports constructors and `match` as not yet supported")
            }
```

- [ ] **Step 14: HIR のテストが通ることを確かめる**

Run: `cargo test -p eml_syntax --test ast && cargo test -p eml_hir`
Expected: PASS (`data.rs` の9件、`lower.rs` の置き換えたテストを含む)

- [ ] **Step 15: UI テストを足し、`data_declarations.em` を消す**

`tests/ui/check-fail/names/constructor_arity.em`:

```haskell
-- E1016: a constructor pattern with more arguments than the constructor has fields.
data Option a = | None | Some a

first : Option Int -> Int
first (Some x y) = x

main : Unit -> <IO> Unit
main () = ()
```

`tests/ui/check-fail/names/duplicate_binding.em`:

```haskell
-- E1017: one equation binds `x` twice in its parameters.
add : Int -> Int -> Int
add x x = x

main : Unit -> <IO> Unit
main () = println (show_int (add 1 2))
```

どちらも、型検査の仮の扱い (Interface notes の5) の E0004 が出ない入力にしてある。`first` の引数のパターンは E1016 で `Missing` になり、`add` はコンストラクタを使わないためである。

種類1の削除をする。

```bash
git rm tests/ui/check-fail/not-yet-supported/data_declarations.em crates/eml_cli/tests/snapshots/ui__check_fail@not-yet-supported__data_declarations.em.snap
```

`not-yet-supported/` の UI テストがなくならないよう、段階4b に残すタプルの E0004 を確かめる新しいテスト `tests/ui/check-fail/not-yet-supported/tuples.em` を足す。

```haskell
-- E0004: tuples, which stage 4b implements.
main : Unit -> <IO> Unit
main () =
  let pair = (1, 2)
  println "done"
```

このテストのスナップショットは、`4:14` の `(1, 2)` を指す「[E0004] Error: tuples are not supported yet」の1件になる。

Run: `cargo test -p eml_cli --test ui`
Expected: 新しい3件のスナップショットが未承認で FAIL。`cargo insta review` で、次の形 (番号、文言、位置) であることを確かめてから承認する。罫線の細部は ariadne の出力に従う。

```
[E1016] Error: `Some` takes 1 argument, but 2 were given
   ╭─[ check-fail/names/constructor_arity.em:5:8 ]
   │
 5 │ first (Some x y) = x
   │        ────┬───  
   │            ╰───── expected 1 argument
───╯
```

```
[E1017] Error: `x` is bound more than once
   ╭─[ check-fail/names/duplicate_binding.em:3:7 ]
   │
 3 │ add x x = x
   │     ┬ ┬  
   │     │ ╰── bound again here
   │     ╰──── first bound here
───╯
```

承認した後にもう一度 `cargo test -p eml_cli --test ui` を実行し、PASS を確かめる。

- [ ] **Step 16: 全体を確かめる**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`
Expected: すべて PASS、clippy の警告なし。`cargo fmt` の後に差分が出たら、それもコミットに含める

- [ ] **Step 17: コミットする**

```bash
git add crates/eml_syntax crates/eml_hir crates/eml_types crates/eml_core_ir tests/ui/check-fail/names tests/ui/check-fail/not-yet-supported crates/eml_cli/tests/snapshots
git commit -m "$(cat <<'EOF'
Lower data declarations, constructors, patterns and match into HIR

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014YGQm6f7Mtged1yVY4dmJG
EOF
)"
```

### Task 3: 型構成子に引数を持たせ、パターンと `match` を型検査し、`Bool` を Prelude に移す

型の表の型構成子に型引数を持たせ、データ型の Kind を「Kind に効く型引数の join」で求める。コンストラクタのスキームを作って式と同じ経路で具体化し、パターンと `match` を検査する。Task 2 が入れた3つの仮の E0004 は外す。`Bool` は Prelude の `data Bool = | False | True` にし、`Builtin::True` / `Builtin::False` をなくす。Core IR は、引数のないコンストラクタを `Atom::Tag` にするところまで追随する。引数を持つコンストラクタの値は Task 5 で、`match` は Task 6 で変換する。

**Files:**
- Create: `crates/eml_types/src/data.rs`
- Modify: `crates/eml_types/src/lib.rs`、`ty.rs`、`table/mod.rs`、`table/unify.rs`、`table/kinds.rs`、`table/copy.rs`、`table/export.rs`、`table/row.rs`、`table/tests.rs` (種類3)、`scheme.rs`、`check/mod.rs`、`check/body.rs`、`check/report.rs`
- Modify (`Bool`): `crates/eml_hir/src/prelude.em`、`lower/prelude.rs`、`lower/scope.rs`、`lower/mod.rs`、`lower/expr.rs`、`lower/ops.rs`、`builtin.rs`、`hir.rs`、`pretty.rs`
- Modify: `crates/eml_core_ir/src/translate/expr.rs`、`translate/types.rs`、`translate/program.rs`
- Modify (種類1、spec の表にある): `crates/eml_hir/src/builtin.rs` の `names_are_looked_up_by_access` の `Builtin::True` の行
- Modify (種類1、spec の表に足す。Interface notes を参照): `crates/eml_types/tests/check.rs` の `builtin_schemes_are_exported`、`crates/eml_hir/tests/structure.rs` の `the_prelude_has_a_signature_for_every_builtin_function`
- Test: `crates/eml_types/tests/data.rs` (新規)、`crates/eml_hir/tests/data.rs` (Task 2 で作ったファイルに足す)、`tests/ui/check-fail/types/constructor_pattern_mismatch.em`、`tests/ui/check-fail/types/type_argument_count.em`

**Interfaces:**
- Consumes: Task 2 の HIR の形 (`TypeDef`、`TypeDef::builtin`、`TypeDefKind`、`Constructor`、`Module::constructors`、`Res::Constructor`、`TypeRefKind::Con(TypeDefId, Vec<TypeRefId>)`、`PatKind::Con`、`ExprKind::Match`、`MatchArm`)、`data::declare_data`、`data::lower_constructors`、`ValueItem::Constructor`、`ItemScope::define_type(name, id, params)`、`types::Vars`
- Produces:
  - `eml_types::Type::Con { id: TypeDefId, name: String, args: Vec<Type> }`。表示は `Option Int`、`List (Option a)`、`Option (Int -> Int)`
  - `eml_types::TypedModule::constructors: ArenaMap<ConstructorId, Scheme>`。スキームの型は `Some : a -> Option a` の形
  - 型の表の `TyShape::Con(TypeDefId, Vec<Ty>)` と、`Table::new(lang, types, constructors, effects, operations)`
  - `eml_hir::LangItems { int, string, bool, unit, io, true_ctor: ConstructorId, false_ctor: ConstructorId }`
  - `BodyLowering::new(file, items, effects, operations, constructors, lang, generics, diagnostics)`
  - `scope::builtin_items(...) -> BuiltinItems`、`scope::lang_items(builtin, &scope, &constructors) -> LangItems`、`prelude::lower_prelude(&mut scope, &mut types, &mut constructors) -> HashMap<Builtin, Signature>`
  - `Builtin::True` と `Builtin::False` はなくなる
  - Core IR の変換: 引数のないコンストラクタの `Res::Constructor` は `Atom::Tag(constructor.tag)` になる。引数を持つコンストラクタの値はまだ変換しない (Task 5)

**Interface notes (骨組みの Interfaces から変えた点と、前後のタスクとの取り決め):**

Task 2 の本文 (`plan-task-2.md`) の実際の形に合わせた。後のタスクはこの形を前提にする。

- Prelude の `data Bool` は、Task 2 の `data::declare_data` と `data::lower_constructors` で変換する。型の名前空間の重複の表 `declared` は、Prelude 専用の空の表を渡す。ユーザーの `declared` (`lower` の `type_names`) には Prelude の名前が入らないので、ユーザーの `data Bool` や `True` は E1003 にならない
- 名前の隠し方は、Task 2 の `ItemScope` のままでよい。`define_type` と `define_constructor` は表に `insert` するので、後で変換するユーザーの定義が Prelude の同じ名前を上書きして隠す。そのため、外側のスコープを作る `seal_prelude` は入れない。`LangItems` は、Prelude を変換した直後、ユーザーの定義が名前を上書きする前に `scope::lang_items` で作る
- `LangItems` に `true_ctor` / `false_ctor` を足す。`&&` と `||` の脱糖は、ユーザーが同じ名前のコンストラクタで隠しても Prelude の `Bool` を指す必要があるので、名前で引かずに lang item を使う。そのため `BodyLowering::new` に `lang: LangItems` を足す。最終の呼び出しは `BodyLowering::new(file, &scope, &effects, &operations, &constructors, lang, generics, &mut diagnostics)` である
- `builtin_items` は `Bool` を作らず、`BuiltinItems { int, string, unit, io }` を返す。型は Task 2 の `TypeDef::builtin(name)` で作る
- HIR の表示 (`pretty`) は、Task 2 で `data` の宣言を表示するようになった。Prelude の `Bool` (`module.lang.bool`) は表示から除く。除かないと、すべての HIR のスナップショットに `data Bool` の3行が増える
- `Table::new` の引数に `constructors: &Arena<Constructor>` を足す (Kind に効く型引数の位置を求めるため)
- Task 2 が型検査に入れた仮の E0004 (「constructors are not supported yet」「`match` is not supported yet」「constructor patterns are not supported yet」) は、Task 3 ですべて外す。Task 2 の使用回数のパス (`usage.rs`)、`has_error`、`callee_subject` はすでに最終の形なので、Task 3 では変えない
- Task 2 が Core IR の変換の `atom` に置いた `ExprKind::Path(Res::Constructor(_)) | ExprKind::Match { .. } => unreachable!(...)` は、2つの腕に分ける。`Res::Constructor` は引数のないコンストラクタを `Atom::Tag` にし、引数を持つコンストラクタは `assert!` で止める (Task 5 で置き換える)。`Match` は Task 6 で置き換えるまで `unreachable!` のままにし、文言だけを今の状態に合わせる。Task 3 で `match` の E0004 を外すので、Task 3 から Task 5 の間は、`match` か引数を持つコンストラクタの値を含むプログラムを `eml run` すると、Core IR の変換でこの panic に届く。このプランの中で、その間にそうしたプログラムを実行するテストはない
- Task 2 のテストで、仮の E0004 を期待するものはない。`crates/eml_hir/tests/data.rs` と `lower.rs` は HIR だけを見る。`check-fail/names/constructor_arity.em` は引数のパターンが E1016 で `Missing` になり、`duplicate_binding.em` はコンストラクタを使わないので、どちらも型検査の E0004 に届かない。そのため、仮の E0004 を外しても Task 2 のテストの期待値は変わらない
- 中置のコンストラクタの結合の向き (式は `infixl 9`、パターンは右結合) に、Task 3 は依存しない。Task 3 のテストは中置のコンストラクタを使わない
- spec の種類1の表にない既存のテストが2件変わる。どちらも `Builtin::True` がなくなるとコンパイルできなくなるテストで、親がユーザーの合意を得て spec の表と `test-changes.md` に足す必要がある (Task 2 の Interface notes の7と同じもの)
  - `crates/eml_types/tests/check.rs` の `builtin_schemes_are_exported`: 最後の `assert!(!checked.typed.builtins.contains_key(&Builtin::True))` とその前のコメントを、`TypedModule::constructors` に `True` のスキームがあることを確かめる行に置き換える
  - `crates/eml_hir/tests/structure.rs` の `the_prelude_has_a_signature_for_every_builtin_function`: `True` と `False` を除く分岐をなくし、すべての組み込みにシグネチャがあることを確かめる形にする。残る組み込みの期待値は変わらない

- [ ] **Step 1: 失敗する型検査のテストを書く**

`crates/eml_types/tests/data.rs` を作る。期待値の局所変数の番号は、引数、パターンの変数の順に振られる前提で書いてある。

```rust
mod common;

use common::check_text;

#[test]
fn applied_types_are_displayed_with_their_arguments() {
    let text = "data Option a =\n  | None\n  | Some a\n\ndata List a =\n  | Nil\n  | Cons a (List a)\n\nwrap : Int -> Option Int\nwrap n = Some n\n\nkeep : List (Option a) -> List (Option a)\nkeep xs = xs\n\nfuns : Option (Int -> Int) -> Option (Int -> Int)\nfuns f = f";
    insta::assert_snapshot!(check_text(text), @r"
    wrap : Int -> Option Int
      n#0 : Int
    keep : List (Option a) -> List (Option a)
      xs#0 : List (Option a)
    funs : Option (Int -> Int) -> Option (Int -> Int)
      f#0 : Option (Int -> Int)
    ");
}

#[test]
fn the_kind_of_a_data_type_is_the_kind_of_its_effective_arguments() {
    // `Option a` と再帰する `List a` は `a` の Kind を持つ。関数型のフィールドの矢印は `Unr` なので `Fun a` は `a` に
    // よらず、フィールドのない `Tag a` も `a` によらない
    let text = "data Option a =\n  | None\n  | Some a\n\ndata List a =\n  | Nil\n  | Cons a (List a)\n\ndata Fun a =\n  | Fun (a -> a)\n\ndata Tag a =\n  | Tag\n\nopt : Option a -> (Option a -> Option a -> b) -> b\nopt x g = g x x\n\nlist : List a -> (List a -> List a -> b) -> b\nlist x g = g x x\n\nfun : Fun a -> (Fun a -> Fun a -> b) -> b\nfun x g = g x x\n\ntag : Tag a -> (Tag a -> Tag a -> b) -> b\ntag x g = g x x";
    insta::assert_snapshot!(check_text(text), @r"
    opt : Option a -> (Option a -> Option a -> b) -> b
      kinds: a <= Unr
      x#0 : Option a
      g#1 : Option a -> Option a -> b
    list : List a -> (List a -> List a -> b) -> b
      kinds: a <= Unr
      x#0 : List a
      g#1 : List a -> List a -> b
    fun : Fun a -> (Fun a -> Fun a -> b) -> b
      x#0 : Fun a
      g#1 : Fun a -> Fun a -> b
    tag : Tag a -> (Tag a -> Tag a -> b) -> b
      x#0 : Tag a
      g#1 : Tag a -> Tag a -> b
    ");
}

#[test]
fn a_constructor_pattern_of_another_type_is_a_mismatch() {
    let text = "data Option a =\n  | None\n  | Some a\n\ndata List a =\n  | Nil\n  | Cons a (List a)\n\nfirst : Option Int -> Int\nfirst o = match o with\n  | Cons x _ -> x\n  | _ -> 0";
    insta::assert_snapshot!(check_text(text), @r"
    first : Option Int -> Int
      o#0 : Option Int
      x#1 : {error}
    ---
    E2001 11:5 mismatched types
      11:5 expected `Option Int`, found `List _`
      note: `Cons` is a constructor of `List`
    ");
}

#[test]
fn match_arms_have_one_type() {
    let text = "data Color =\n  | Red\n  | Green\n\nname : Color -> String\nname c = match c with\n  | Red -> \"red\"\n  | Green -> 2\n\nsize : Color -> Int\nsize c =\n  let n = match c with\n    | Red -> 1\n    | Green -> \"two\"\n  n";
    insta::assert_snapshot!(check_text(text), @r#"
    name : Color -> String
      c#0 : Color
    size : Color -> Int
      c#0 : Color
      n#1 : Int
    ---
    E2001 8:14 mismatched types
      8:14 expected `String`, found `Int`
      5:8 expected because of the signature of `name`
    E2001 14:16 mismatched types
      14:16 expected `Int`, found `String`
      13:14 the first arm has this type
    "#);
}

#[test]
fn constructors_are_values_and_can_be_partially_applied() {
    let text = "data Option a =\n  | None\n  | Some a\n\ndata Pair a b =\n  | Pair a b\n\napply : (a -> b) -> a -> b\napply f x = f x\n\nboxed : Int -> Option Int\nboxed n = apply Some n\n\npairs : Unit -> Pair Int String\npairs () =\n  let make = Pair 1\n  make \"one\"";
    insta::assert_snapshot!(check_text(text), @r"
    apply : (a -> b) -> a -> b
      f#0 : a -> b
      x#1 : a
    boxed : Int -> Option Int
      n#0 : Int
    pairs : Unit -> Pair Int String
      make#0 : String -> Pair Int String
    ");
}

#[test]
fn match_arms_are_paths_and_discarded_fields_are_unrestricted() {
    // `_` で捨てたフィールドと使わない変数の型は `Unr` になる。`match` の枝は別の経路なので、片方の枝だけで使う `d`
    // も `Unr` になる。各枝で1回ずつ使う `x` は制約を作らない
    let text = "data Option a =\n  | None\n  | Some a\n\ndata Box a =\n  | Box a\n\nskip : Box a -> Int\nskip b = match b with\n  | Box _ -> 1\n\nunused : Box a -> Int\nunused b = match b with\n  | Box x -> 1\n\npick : Option a -> a -> a\npick o d = match o with\n  | Some x -> x\n  | None -> d\n\nsame : Option a -> Option a\nsame o = match o with\n  | Some x -> Some x\n  | None -> None";
    insta::assert_snapshot!(check_text(text), @r"
    skip : Box a -> Int
      kinds: a <= Unr
      b#0 : Box a
    unused : Box a -> Int
      kinds: a <= Unr
      b#0 : Box a
      x#1 : a
    pick : Option a -> a -> a
      kinds: a <= Unr
      o#0 : Option a
      d#1 : a
      x#2 : a
    same : Option a -> Option a
      o#0 : Option a
      x#1 : a
    ");
}

#[test]
fn user_constructors_shadow_the_prelude() {
    // `True` はユーザーの `Answer` のコンストラクタを指す。`&&` の脱糖は Prelude の `False` を使うので、条件は `Bool` のまま
    let text = "data Answer =\n  | True\n  | No\n\nreply : Bool -> Answer\nreply b = if b && b then True else No";
    insta::assert_snapshot!(check_text(text), @r"
    reply : Bool -> Answer
      b#0 : Bool
    ");
}
```

`crates/eml_hir/tests/data.rs` (Task 2 で作ったファイル) の末尾に次のテストを足す。`use` には `eml_hir::TypeDefKind` を足す。

```rust
#[test]
fn bool_is_a_data_type_of_the_prelude() {
    let lowered = eml_test_support::lower_clean("f : Bool -> Bool\nf b = b && True");
    let module = &lowered.module;
    let TypeDefKind::Data { constructors } = &module.types[module.lang.bool].kind else {
        panic!("`Bool` is a data type");
    };
    let tags: Vec<(&str, u32)> = constructors
        .iter()
        .map(|&id| (module.constructors[id].name.as_str(), module.constructors[id].tag))
        .collect();
    assert_eq!(tags, [("False", 0), ("True", 1)]);
    assert_eq!(module.constructors[module.lang.false_ctor].name, "False");
    assert_eq!(module.constructors[module.lang.true_ctor].name, "True");
}
```

- [ ] **Step 2: 既存のテストを `Builtin::True` のない形に直す (種類1)**

`crates/eml_hir/src/builtin.rs` の `names_are_looked_up_by_access` から次の行を削除する (spec の表にある種類1)。

```rust
        assert_eq!(Builtin::from_name("True"), Some(Builtin::True));
```

`crates/eml_types/tests/check.rs` の `builtin_schemes_are_exported` の最後の2行 (コメントと `assert!`) を、次の2行に置き換える (Interface notes の種類1)。

```rust
    // コンストラクタは組み込みではなく、Prelude の `data Bool` のスキームとして書き出す
    assert_eq!(checked.typed.constructors[checked.module.lang.true_ctor].ty.to_string(), "Bool");
```

`crates/eml_hir/tests/structure.rs` の `the_prelude_has_a_signature_for_every_builtin_function` を次の形にする (Interface notes の種類1)。`Builtin` の `use` が使われなくなれば外す。

```rust
#[test]
fn the_prelude_has_a_signature_for_every_builtin_function() {
    let module = module("");
    for info in BUILTINS {
        assert!(module.builtins.contains_key(&info.builtin), "{}", info.name);
    }
}
```

- [ ] **Step 3: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --test data`
Expected: コンパイルエラー (`TypedModule` に `constructors` がない、`LangItems` に `true_ctor` がない)

- [ ] **Step 4: `Bool` を Prelude の `data` にする (`eml_hir`)**

`crates/eml_hir/src/prelude.em` の先頭のコメントの後に `data Bool` を足し、コメントを直す。

```haskell
-- 組み込みの型 `Bool` と、関数と演算子のシグネチャ。型は docs/spec/declarations.md の標準の演算子の表と、
-- docs/spec/effects.md の組み込みの IO に従う。関数の名前と見え方と引数の数は eml_hir::builtin::BUILTINS にある。
data Bool =
  | False
  | True

println : String -> <IO> Unit
```

`crates/eml_hir/src/builtin.rs` から `Builtin::True` と `Builtin::False` を消す。列挙子の2行と、`BUILTINS` の次の2行を削除する。

```rust
    info(Builtin::True, "True", Access::Named, 0),
    info(Builtin::False, "False", Access::Named, 0),
```

`crates/eml_hir/src/hir.rs` の `LangItems` に2つのフィールドを足す。

```rust
/// 処理系が名前ではなく役割で引く item。
#[derive(Debug, Clone, Copy)]
pub struct LangItems {
    pub int: TypeDefId,
    pub string: TypeDefId,
    pub bool: TypeDefId,
    pub unit: TypeDefId,
    pub io: EffectId,
    /// `&&` と `||` の脱糖が使う `Bool` のコンストラクタ。ユーザーが同じ名前のコンストラクタで隠しても、脱糖は
    /// Prelude のものを指す。
    pub true_ctor: ConstructorId,
    pub false_ctor: ConstructorId,
}
```

`Module::types` の doc コメントを「型の item。組み込みの `Int`、`String`、`Unit`、Prelude の `Bool`、ユーザーの `data` の宣言。」に直す。

`crates/eml_hir/src/lower/scope.rs` の `builtin_items` は `Bool` を作らずに、組み込みの4つの item を返すようにする。`LangItems` は Prelude を変換した後に `lang_items` で作る。`ItemScope` は Task 2 の形のまま変えない。後で変換するユーザーの定義は `insert` で同じ名前を上書きするので、Prelude の名前を E1003 にせずに隠せる。

```rust
/// Prelude より前に作る組み込みの item。`Bool` は Prelude の `data` である。
pub(super) struct BuiltinItems {
    pub int: TypeDefId,
    pub string: TypeDefId,
    pub unit: TypeDefId,
    pub io: EffectId,
}

/// 組み込みの型とエフェクトを item として登録する (docs/spec/declarations.md と docs/spec/effects.md)。
pub(super) fn builtin_items(
    types: &mut Arena<TypeDef>,
    effects: &mut Arena<EffectDef>,
    scope: &mut ItemScope,
) -> BuiltinItems {
    let mut ty = |name: &str| {
        let id = types.alloc(TypeDef::builtin(name));
        scope.define_type(name, id, 0);
        id
    };
    let (int, string, unit) = (ty("Int"), ty("String"), ty("Unit"));
    let io = effects.alloc(EffectDef {
        name: "IO".to_string(),
        generics: Generics::default(),
        operations: Vec::new(),
    });
    scope.define_effect("IO", io, 0);
    BuiltinItems {
        int,
        string,
        unit,
        io,
    }
}

/// Prelude を変換した直後、ユーザーの定義が同じ名前を上書きする前に呼ぶ。
pub(super) fn lang_items(
    builtin: BuiltinItems,
    scope: &ItemScope,
    constructors: &Arena<Constructor>,
) -> LangItems {
    let Some(TypeItem::Type(bool)) = scope.type_item("Bool") else {
        unreachable!("the Prelude declares `Bool`");
    };
    let constructor = |name: &str| match scope.constructor(name) {
        Some(id) => id,
        None => unreachable!("the Prelude declares `{name}`"),
    };
    let (false_ctor, true_ctor) = (constructor("False"), constructor("True"));
    // Core IR は `Bool` を、タグ 0 の `False` と 1 の `True` で表す (docs/spec/core-ir.md)
    assert_eq!(
        (constructors[false_ctor].tag, constructors[true_ctor].tag),
        (0, 1)
    );
    LangItems {
        int: builtin.int,
        string: builtin.string,
        bool,
        unit: builtin.unit,
        io: builtin.io,
        true_ctor,
        false_ctor,
    }
}
```

`use crate::hir::{...}` に `Constructor` を足し、使わなくなった `LangItems` 以外の名前を整理する。同じファイルの `types_and_effects_share_the_type_namespace` は `builtin_items` の戻り値の `int` と `io` だけを使うので、そのままコンパイルできる (変数名 `lang` も変えなくてよい)。

`crates/eml_hir/src/lower/prelude.rs` は、Task 2 の `declare_data` と `lower_constructors` で `data` を先に変換してから、シグネチャを変換する。

```rust
//! 組み込みの Prelude (`prelude.em`) の `data` とシグネチャを変換する。S2 で `Prelude` モジュールに移す。

use std::collections::HashMap;

use eml_diagnostics::FileId;
use eml_syntax::ast;
use la_arena::Arena;

use super::data::{declare_data, lower_constructors};
use super::scope::ItemScope;
use super::types::{TypeLowering, Vars};
use crate::builtin::Builtin;
use crate::hir::{Constructor, Generics, Signature, TypeDef};

const PRELUDE: &str = include_str!("../prelude.em");

pub(super) fn lower_prelude(
    scope: &mut ItemScope,
    types: &mut Arena<TypeDef>,
    constructors: &mut Arena<Constructor>,
) -> HashMap<Builtin, Signature> {
    let (parse, syntax_errors) = eml_syntax::parse(FileId::PRELUDE, PRELUDE);
    debug_assert!(syntax_errors.is_empty(), "{syntax_errors:?}");
    let tree = parse.tree();
    let mut diagnostics = Vec::new();
    // シグネチャが `Bool` を引けるように、`data` を先に変換する。重複を見る名前の表は Prelude だけのもので、ユーザーの
    // 定義との重複は E1003 にしない。ユーザーの定義は後で同じ名前を上書きし、Prelude の名前を隠す
    let data: Vec<ast::DataItem> = tree
        .items()
        .filter_map(|item| match item {
            ast::Item::DataItem(data) => Some(data),
            _ => None,
        })
        .collect();
    let declared = declare_data(
        FileId::PRELUDE,
        &data,
        &mut HashMap::new(),
        scope,
        types,
        &mut diagnostics,
    );
    lower_constructors(
        FileId::PRELUDE,
        &declared,
        scope,
        types,
        constructors,
        &mut diagnostics,
    );
    let mut signatures = HashMap::new();
    for item in tree.items() {
        let ast::Item::Signature(signature) = item else {
            continue;
        };
        let name = signature
            .name()
            .expect("every Prelude signature has a name");
        let builtin = Builtin::from_prelude_name(name.text())
            .expect("every Prelude signature names a builtin in the table");
        let range = signature.ty().map_or(signature.range(), |ty| ty.range());
        let mut signature_types = Arena::new();
        let mut generics = Generics::default();
        let ty = TypeLowering {
            file: FileId::PRELUDE,
            types: &mut signature_types,
            generics: &mut generics,
            items: scope,
            vars: Vars::Define,
            diagnostics: &mut diagnostics,
        }
        .lower(signature.ty(), range);
        signatures.insert(
            builtin,
            Signature {
                ty,
                range,
                types: signature_types,
                generics,
            },
        );
    }
    debug_assert!(diagnostics.is_empty(), "{diagnostics:?}");
    signatures
}
```

`crates/eml_hir/src/lower/mod.rs` の `lower` の先頭を次の形にする (Task 2 の形から、`builtin_items` から `lang_items` までの3行を変える)。

```rust
pub fn lower(file: FileId, source: &ast::SourceFile) -> (Module, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let (definitions, data_items, effect_items) = collect(file, source, &mut diagnostics);
    let mut functions = Arena::new();
    let mut scope = ItemScope::new();
    let mut types = Arena::new();
    let mut constructors = Arena::new();
    let mut effects = Arena::new();
    let mut operations = Arena::new();
    let builtin = scope::builtin_items(&mut types, &mut effects, &mut scope);
    let builtins = prelude::lower_prelude(&mut scope, &mut types, &mut constructors);
    // ユーザーの定義が `Bool`、`True`、`False` を隠す前に引く
    let lang = scope::lang_items(builtin, &scope, &constructors);
    // 型の名前空間のユーザーの名前。`data` とエフェクトの間の重複も見つける
    let mut type_names = HashMap::new();
```

(この後の `declare_data`、`lower_effects`、`lower_constructors` は Task 2 のまま。)

本体の変換の `BodyLowering::new` の呼び出しを次の形にする。

```rust
        let body = BodyLowering::new(
            file,
            &scope,
            &effects,
            &operations,
            &constructors,
            lang,
            generics,
            &mut diagnostics,
        )
        .lower_equation(&equation);
```

`crates/eml_hir/src/lower/expr.rs` の `BodyLowering` に、`constructors` の後ろにフィールドを足す。

```rust
    /// `&&` と `||` の脱糖が引く `Bool` のコンストラクタ。
    pub(super) lang: LangItems,
```

`new` は `constructors: &'a Arena<Constructor>` の後ろに `lang: LangItems` を受け、`lang,` を入れる。

`crates/eml_hir/src/lower/ops.rs` の `&&` と `||` の脱糖を lang item に替え、`Builtin` の `use` は `Builtin::binary_operator` のために残す。

```rust
            "&&" => {
                let otherwise = self.alloc(
                    ExprKind::Path(Res::Constructor(self.lang.false_ctor)),
                    op_range,
                );
```

```rust
            "||" => {
                let then = self.alloc(
                    ExprKind::Path(Res::Constructor(self.lang.true_ctor)),
                    op_range,
                );
```

`crates/eml_hir/src/pretty.rs` の `data` の宣言の表示 (Task 2 の Step 12) で、Prelude の `Bool` を飛ばす。

```rust
    for (id, def) in module.types.iter() {
        // Prelude の `Bool` は、どのモジュールにもあるので表示しない
        if id == module.lang.bool {
            continue;
        }
        let TypeDefKind::Data { constructors } = &def.kind else {
            continue;
        };
```

`Res::Constructor` の表示はコンストラクタの名前 (Task 2) なので、`&&` と `||` の脱糖の表示は今の `True` / `False` のまま変わらない。

- [ ] **Step 5: 型の表現に引数を持たせる (`ty.rs` と型の表)**

`crates/eml_types/src/ty.rs` の `Type::Con` に `args` を足す。

```rust
pub enum Type {
    /// 型構成子とその型引数。`Int` などの組み込みの型は引数を持たない。
    Con {
        id: TypeDefId,
        name: String,
        args: Vec<Type>,
    },
```

`contains_error` は引数の中も見る。`Type::Con { .. }` を最後の `false` の腕から外し、次の腕を足す。

```rust
            Type::Con { args, .. } => args.iter().any(Type::contains_error),
```

`Display` の `Type::Con` の腕と `atomic` を次のようにする。

```rust
            Type::Con { name, args, .. } => {
                f.write_str(name)?;
                for arg in args {
                    write!(f, " {}", atomic(arg))?;
                }
                Ok(())
            }
```

```rust
/// 型の適用の引数の位置に置く形。関数型、継続の型、引数を持つ型の適用は括弧で囲む。
fn atomic(ty: &Type) -> String {
    match ty {
        Type::Fn { .. } | Type::Cont { .. } => format!("({ty})"),
        Type::Con { args, .. } if !args.is_empty() => format!("({ty})"),
        _ => ty.to_string(),
    }
}
```

同じファイルの `tests` の `Type::Con { id: ..., name: ... }` の組み立てには `args: Vec::new()` を足す (種類3。`TypeDef` は Task 2 で `TypeDef::builtin` にしてある)。

`crates/eml_types/src/data.rs` を作る。

```rust
//! データ型の Kind に効く型引数の位置 (docs/spec/types.md の「Kind」)。データ型の Kind はフィールドの Kind の join
//! なので、`Option a` の Kind は `a` の Kind になる。どの型引数が効くかは宣言だけで決まるので、検査の前に1回だけ求める。

use eml_hir::{Constructor, TypeDef, TypeDefId, TypeDefKind, TypeRef, TypeRefId, TypeRefKind, TypeVarId};
use la_arena::{Arena, ArenaMap};

/// 型構成子ごとの、型引数の位置が Kind に効くかどうか。組み込みの型は型引数を持たないので空である。
pub(crate) fn effective_params(
    types: &Arena<TypeDef>,
    constructors: &Arena<Constructor>,
) -> ArenaMap<TypeDefId, Vec<bool>> {
    let mut effective: ArenaMap<TypeDefId, Vec<bool>> = types
        .iter()
        .map(|(id, def)| (id, vec![false; def.generics.type_vars.len()]))
        .collect();
    // 再帰する宣言 (`List a`) と相互再帰する宣言があるので、印が増えなくなるまで繰り返す。印は増えるだけなので止まる
    loop {
        let mut changed = false;
        for (id, def) in types.iter() {
            let TypeDefKind::Data { constructors: ctors } = &def.kind else {
                continue;
            };
            let mut found = Vec::new();
            for &ctor in ctors {
                for &field in &constructors[ctor].fields {
                    collect(&def.types, field, &effective, &mut found);
                }
            }
            for var in found {
                let index = u32::from(var.into_raw()) as usize;
                if !effective[id][index] {
                    effective[id][index] = true;
                    changed = true;
                }
            }
        }
        if !changed {
            return effective;
        }
    }
}

/// フィールドの型のうち、Kind に効く位置にある型変数。関数型の Kind はその矢印の線形性で決まり、フィールドの矢印は
/// `Unr` に固定するので、関数型の中は見ない。型変数の番号は `Generics` の並びの位置と同じである。
fn collect(
    types: &Arena<TypeRef>,
    id: TypeRefId,
    effective: &ArenaMap<TypeDefId, Vec<bool>>,
    out: &mut Vec<TypeVarId>,
) {
    match &types[id].kind {
        TypeRefKind::Var(var) => out.push(*var),
        TypeRefKind::Con(con, args) => {
            for (index, &arg) in args.iter().enumerate() {
                if effective[*con].get(index).copied().unwrap_or(false) {
                    collect(types, arg, effective, out);
                }
            }
        }
        TypeRefKind::Fn { .. } | TypeRefKind::Error => {}
    }
}
```

`crates/eml_types/src/lib.rs` に `mod data;` を足す。

`crates/eml_types/src/table/mod.rs` を次のように直す。

- `TyShape::Con(TypeDefId)` を `Con(TypeDefId, Vec<Ty>)` にする
- `Table` にフィールドを足す

  ```rust
      /// 型構成子ごとの、Kind に効く型引数の位置 (`crate::data`)。
      effective: ArenaMap<TypeDefId, Vec<bool>>,
  ```

- `Table::new` の引数に `constructors: &Arena<Constructor>` を `types` の次に足し、`effective: crate::data::effective_params(types, constructors),` で埋める。`use` に `Constructor` を足す
- 組み込みの型は引数なしで作る

  ```rust
        table.int = table.alloc(TyShape::Con(lang.int, Vec::new()));
        table.string = table.alloc(TyShape::Con(lang.string, Vec::new()));
        table.bool = table.alloc(TyShape::Con(lang.bool, Vec::new()));
  ```

`crates/eml_types/src/table/unify.rs`:

```rust
            (TyShape::Con(x, xs), TyShape::Con(y, ys)) if x == y && xs.len() == ys.len() => {
                for (x, y) in xs.iter().zip(&ys) {
                    self.unify(*x, *y)?;
                }
                Ok(())
            }
```

`occurs` は `TyShape::Con(_, args) => args.iter().any(|arg| self.occurs(var, *arg)),` を足し、最後の `false` の腕を `TyShape::Rigid(_) | TyShape::Error => false` にする。

`crates/eml_types/src/table/row.rs` の `row_occurs_in` も同じく、`TyShape::Con(_, args) => args.iter().any(|arg| self.row_occurs_in(var, *arg)),` を足して `false` の腕から `Con` を外す。

`crates/eml_types/src/table/copy.rs` の `copy_type`:

```rust
            TyShape::Con(id, args) if !args.is_empty() => {
                let args = args
                    .into_iter()
                    .map(|arg| self.copy_type(arg, subst))
                    .collect();
                self.alloc(TyShape::Con(id, args))
            }
            TyShape::Con(..) | TyShape::Var(_) | TyShape::Error => ty,
```

`crates/eml_types/src/table/kinds.rs`:

```rust
    /// 型の Kind の上界の候補。レコードとデータ型の Kind はフィールドの join なので、フィールドごとの境界を並べる
    /// (docs/spec/types.md)。データ型では、Kind に効く位置の型引数の境界を並べる。
    pub fn kind_bounds(&self, ty: Ty) -> Vec<Bound<Linearity>> {
        match self.shape(ty) {
            TyShape::Con(id, args) => {
                let mut bounds = vec![Bound::Const(Linearity::Unr)];
                for (&arg, &effective) in args.iter().zip(&self.effective[*id]) {
                    if effective {
                        bounds.extend(self.kind_bounds(arg));
                    }
                }
                bounds
            }
```

`kind_vars` は型引数もたどる。`TyShape::Con(_) | TyShape::Var(_) | TyShape::Error => {}` を次の2つの腕にする。型引数の rigid 変数の `μ` を多相化しないと、`Option a` を受ける関数の `a` の Kind が呼び出しの間で共有されてしまうためである。

```rust
                TyShape::Con(_, args) => work.extend(args.iter().rev().copied()),
                TyShape::Var(_) | TyShape::Error => {}
```

`crates/eml_types/src/table/export.rs` の `kind_names` も同じ2つの腕にする。`to_type` の `Con` の腕は次のようにする。

```rust
            TyShape::Con(id, args) => Type::Con {
                id,
                name: self.type_names[id].clone(),
                args: args
                    .into_iter()
                    .map(|arg| self.to_type(arg, solved))
                    .collect(),
            },
```

`crates/eml_types/src/table/tests.rs` の `new_table` を新しい `LangItems` と `Table::new` に合わせる (種類3。Task 2 で `TypeDef::builtin` を使う形にしてある)。

```rust
use eml_diagnostics::TextRange;
use eml_hir::{Constructor, EffectDef, Generics, LangItems, TypeDef};
use la_arena::Arena;

use super::*;

fn new_table() -> Table {
    let mut types = Arena::new();
    let mut effects = Arena::new();
    let mut constructors = Arena::new();
    let int = types.alloc(TypeDef::builtin("Int"));
    let string = types.alloc(TypeDef::builtin("String"));
    let bool = types.alloc(TypeDef::builtin("Bool"));
    let unit = types.alloc(TypeDef::builtin("Unit"));
    let mut constructor = |name: &str, tag| {
        constructors.alloc(Constructor {
            name: name.to_string(),
            range: TextRange::default(),
            ty: bool,
            tag,
            fields: Vec::new(),
        })
    };
    let false_ctor = constructor("False", 0);
    let true_ctor = constructor("True", 1);
    let lang = LangItems {
        int,
        string,
        bool,
        unit,
        io: effects.alloc(EffectDef {
            name: "IO".to_string(),
            generics: Generics::default(),
            operations: Vec::new(),
        }),
        true_ctor,
        false_ctor,
    };
    Table::new(lang, &types, &constructors, &effects, &Arena::new())
}
```

- [ ] **Step 6: 型の注釈とコンストラクタのスキームを型の表に変換する (`scheme.rs`)**

`crates/eml_types/src/scheme.rs` の `lower` の `outermost_unr: bool` を、矢印の線形性の決め方を表す enum にする。

```rust
/// 型の注釈の矢印の線形性の決め方。
#[derive(Clone, Copy)]
enum Arrows {
    /// Kind 変数にして推論する。
    Inferred,
    /// 一番外側の矢印だけを `Unr` にし、内側は推論する。トップレベルの関数のシグネチャに使う。
    OutermostUnr,
    /// すべて `Unr` にする。`data` のフィールドの型に使う。表面の構文で `m` を書けないので、操作の引数の型と同じく
    /// `Unr` に固定する (docs/spec/types.md の「Kind」)。
    Unr,
}

impl Arrows {
    /// 引数、戻り値、型引数の位置の決め方。
    fn inner(self) -> Arrows {
        match self {
            Arrows::Unr => Arrows::Unr,
            Arrows::Inferred | Arrows::OutermostUnr => Arrows::Inferred,
        }
    }
}
```

`lower_signature` は `Arrows::OutermostUnr`、`lower_type` は `Arrows::Inferred` で呼ぶ。`lower` は次の形にする。Task 2 の仮の `Con` の腕 (型引数を読まずに `TyShape::Con(*id)` を作る) は、この形で置き換わる。`lower_labels` にも `arrows: Arrows` を足し、型引数を `arrows` で変換する。

```rust
fn lower(
    table: &mut Table,
    types: &Arena<TypeRef>,
    rigids: &Rigids,
    id: TypeRefId,
    arrows: Arrows,
) -> Ty {
    match &types[id].kind {
        TypeRefKind::Error => table.error,
        // `Unit` は空のレコードである (docs/spec/records.md)
        TypeRefKind::Con(id, _) if *id == table.lang.unit => table.unit,
        TypeRefKind::Con(id, _) if *id == table.lang.int => table.int,
        TypeRefKind::Con(id, _) if *id == table.lang.string => table.string,
        TypeRefKind::Con(id, _) if *id == table.lang.bool => table.bool,
        TypeRefKind::Con(id, args) => {
            let mut lowered = Vec::new();
            for &arg in args {
                lowered.push(lower(table, types, rigids, arg, arrows.inner()));
            }
            table.alloc(TyShape::Con(*id, lowered))
        }
        TypeRefKind::Var(var) => rigids.tys[*var],
        TypeRefKind::Fn { param, row, ret } => {
            let param = lower(table, types, rigids, *param, arrows.inner());
            let ret = lower(table, types, rigids, *ret, arrows.inner());
            let row = match row {
                // 省略した row は空の row である (docs/spec/types.md の「関数型」)
                RowRef::Omitted => Row::pure(),
                RowRef::Closed { effects, .. } => Row::closed(lower_labels(
                    table,
                    types,
                    rigids,
                    effects,
                    arrows.inner(),
                )),
                RowRef::Open { effects, tail, .. } => Row {
                    labels: lower_labels(table, types, rigids, effects, arrows.inner()),
                    tail: Tail::Var(rigids.rows[*tail]),
                },
                // 未定義のエフェクトか、解決できない row 変数の跡。どのエフェクトも受け入れて、診断を連鎖させない
                RowRef::Error => Row::error(),
            };
            let lin = match arrows {
                Arrows::Inferred => table.fresh_arrow_lin(),
                Arrows::OutermostUnr | Arrows::Unr => ArrowLin::Known(Linearity::Unr),
            };
            table.function_with(param, lin, row, ret)
        }
    }
}
```

同じファイルにコンストラクタのスキームの型を作る関数を足す。`use` に `Constructor`、`TypeDef` を足す。

```rust
/// コンストラクタのスキームの型。`Some : a -> Option a` の形で、宣言の型引数で量化する。矢印の row は `<>` である。
/// 一番外側の矢印は、トップレベルの関数と同じく何度でも呼べるので `Unr` にし、内側の矢印は推論する。部分適用の
/// クロージャはそれまでの引数を捕まえるためである (docs/spec/types.md の「関数型」)。フィールドの型の中の矢印は
/// `Unr` に固定する。
pub(crate) fn lower_constructor(
    table: &mut Table,
    def: &TypeDef,
    constructor: &Constructor,
    rigids: &Rigids,
) -> Ty {
    let args = def
        .generics
        .type_vars
        .iter()
        .map(|(var, _)| rigids.tys[var])
        .collect();
    let mut ty = table.alloc(TyShape::Con(constructor.ty, args));
    for (index, &field) in constructor.fields.iter().enumerate().rev() {
        let field = lower(table, &def.types, rigids, field, Arrows::Unr);
        let lin = if index == 0 {
            ArrowLin::Known(Linearity::Unr)
        } else {
            table.fresh_arrow_lin()
        };
        ty = table.function_with(field, lin, Row::pure(), ty);
    }
    ty
}
```

- [ ] **Step 7: コンストラクタのスキームを作って書き出す (`check/mod.rs`、`lib.rs`)**

`crates/eml_types/src/lib.rs` の `TypedModule` にフィールドを足し、`builtins` の doc コメントの最後の文を消す。`use` に `ConstructorId` を足す。

```rust
    /// Prelude のシグネチャから作った組み込みのスキーム。Core IR が、組み込みを包む関数の変数を boxed にするかを
    /// 決めるのに使う。
    pub builtins: HashMap<Builtin, Scheme>,
    /// エフェクトの操作のスキーム。Core IR が、操作を包む関数の変数を boxed にするかを決めるのに使う。
    pub operations: ArenaMap<OperationId, Scheme>,
    /// コンストラクタのスキーム。`Some : a -> Option a` の形である。Core IR が、コンストラクタを包む関数の変数を
    /// boxed にするかを決めるのに使う。
    pub constructors: ArenaMap<ConstructorId, Scheme>,
```

`crates/eml_types/src/check/mod.rs`:

- `Table::new(module.lang, &module.types, &module.constructors, &module.effects, &module.operations)` にする
- 操作のスキームを作るループの後に、コンストラクタのスキームを作る

  ```rust
    // コンストラクタは本体を持たない値なので、組み込みと同じく作ってすぐ多相化する
    let mut constructors: ArenaMap<ConstructorId, Scheme> = ArenaMap::default();
    for (id, constructor) in module.constructors.iter() {
        let def = &module.types[constructor.ty];
        let rigids = Rigids::new(&mut table, &def.generics);
        let ty = lower_constructor(&mut table, def, constructor, &rigids);
        table.closure_kinds(ty, constructor.fields.len(), &[]);
        let mut scheme = Scheme::new(ty, &rigids);
        scheme.generalize(&table);
        constructors.insert(id, scheme);
    }
  ```

- `BodyCheck` を作るところに `constructors: &constructors,` を足す
- 操作のスキームを書き出すループの後に、同じ形でコンストラクタのスキームを `typed.constructors` に書き出す

  ```rust
    for (id, scheme) in constructors.iter() {
        typed.constructors.insert(
            id,
            crate::Scheme {
                ty: table.export(scheme.ty),
                constraints: kind_constraints(&table, scheme),
            },
        );
    }
  ```

- `use` に `ConstructorId` と `scheme::lower_constructor` を足す。`has_error` は Task 2 で型引数をたどる最終の形になっているので変えない

- [ ] **Step 8: 値、パターン、`match` を検査する (`check/body.rs`、`check/report.rs`)**

Task 2 が入れた3つの仮の E0004 をすべて外し、次の形に置き換える。外すのは次の3か所である。

- `infer_expr` の `ExprKind::Match { .. } => { .. }` の腕 (「`match` is not supported yet」)。下の `match_expr` を呼ぶ腕にする
- `value` の `Res::Constructor(_) => { .. }` の腕 (「constructors are not supported yet」)。下のスキームを具体化する腕にする
- `bind_pat` の `PatKind::Con { .. } => { .. }` の腕 (「constructor patterns are not supported yet」)。下の `constructor_pattern` を呼ぶ腕にする

Task 2 が足した `TextSize` の `use` は使われなくなるので外す。

`BodyCheck` にフィールドを足す。

```rust
    /// コンストラクタのスキーム。
    pub(super) constructors: &'a ArenaMap<ConstructorId, Scheme>,
```

`value` の `Res::Builtin(Builtin::True | Builtin::False) => self.table.bool,` の腕とその上のコメントを消し、Task 2 の仮の `Res::Constructor` の腕を次の腕に置き換える。関数と同じく具体化し、戻り値の側の row を開く。

```rust
            Res::Constructor(constructor) => {
                let name = module.constructors[constructor].name.clone();
                self.with_kind_origin(range, KindReason::Passed(name), |this| {
                    match this.constructors.get(constructor) {
                        Some(scheme) => scheme.instantiate(this.table),
                        None => this.table.error,
                    }
                })
            }
```

`check_expr` に `match` の腕を足す (`If` の腕の後)。

```rust
            ExprKind::Match { scrutinee, arms } => {
                self.match_expr(*scrutinee, arms, Expectation::Has(expected, origin));
                self.typing.exprs.insert(id, expected);
            }
```

`infer_expr` の Task 2 の仮の `ExprKind::Match` の腕を、次の腕に置き換える。

```rust
            ExprKind::Match { scrutinee, arms } => {
                self.match_expr(*scrutinee, arms, Expectation::None)
            }
```

`if_expr` の後に `match_expr` を足す。

```rust
    /// 各枝のパターンを scrutinee の型で検査する。期待する型があれば枝の本体をその型で検査し、なければ最初の枝の型を
    /// 推論して後の枝をそれに合わせる (`if` と同じ)。
    fn match_expr(
        &mut self,
        scrutinee: ExprId,
        arms: &[MatchArm],
        expectation: Expectation,
    ) -> Ty {
        let scrutinee_ty = self.infer_expr(scrutinee);
        for arm in arms {
            self.bind_pat(arm.pat, scrutinee_ty);
        }
        match expectation {
            Expectation::Has(expected, origin) => {
                for arm in arms {
                    self.check_expr(arm.body, expected, origin.clone());
                }
                expected
            }
            Expectation::None => {
                let Some((first, rest)) = arms.split_first() else {
                    return self.table.fresh_var();
                };
                let ty = self.infer_expr(first.body);
                let first_range = self.body.exprs[first.body].range;
                for arm in rest {
                    self.check_expr(arm.body, ty, Origin::MatchArms(first_range));
                }
                ty
            }
        }
    }
```

`bind_pat` の Task 2 の仮の `PatKind::Con` の腕を、次の腕に置き換える。

```rust
            PatKind::Con { ctor, args } => self.constructor_pattern(pat, *ctor, args, ty),
```

`bind_pat` の後に `constructor_pattern` を足す。

```rust
    /// コンストラクタのパターン。スキームを具体化し、結果の型を期待する型と単一化してから、引数のパターンを
    /// フィールドの型で検査する。HIR が引数の個数を確かめている (E1016) ので、引数とフィールドは同じ数である。
    fn constructor_pattern(&mut self, pat: PatId, ctor: ConstructorId, args: &[PatId], expected: Ty) {
        let range = self.body.pats[pat].range;
        let constructors = self.constructors;
        let mut ty = match constructors.get(ctor) {
            Some(scheme) => {
                self.with_kind_origin(range, KindReason::Unified, |this| scheme.instantiate(this.table))
            }
            None => self.table.error,
        };
        let mut fields = Vec::new();
        for _ in args {
            match self.table.shape(ty).clone() {
                TyShape::Fn { param, ret, .. } => {
                    fields.push(param);
                    ty = ret;
                }
                _ => fields.push(self.table.error),
            }
        }
        let unified = self.with_kind_origin(range, KindReason::Unified, |this| {
            this.table.unify(ty, expected)
        });
        if unified.is_err() {
            let constructor = &self.module.constructors[ctor];
            let origin = Origin::ConstructorPattern {
                constructor: constructor.name.clone(),
                ty: self.module.types[constructor.ty].name.clone(),
            };
            self.mismatch(range, expected, ty, &origin);
            // 型の合わないパターンの中は検査しない。網羅性の検査は、`Error` の型のパターンを含む `match` を飛ばす
            // (docs/spec/exhaustiveness.md)
            let error = self.table.error;
            self.typing.pats.insert(pat, error);
            fields = vec![error; args.len()];
        }
        for (&arg, field) in args.iter().zip(fields) {
            self.bind_pat(arg, field);
        }
    }
```

`use` に `ConstructorId`、`MatchArm` を足し、`Builtin` が使われなくなれば外す。

`crates/eml_types/src/check/report.rs` の `Origin` に2つの由来を足す。

```rust
    /// `match` の2つ目以降の枝。最初の枝の本体の範囲を持つ。
    MatchArms(TextRange),
    /// コンストラクタのパターン。コンストラクタと、それが作る型の名前を持つ。
    ConstructorPattern { constructor: String, ty: String },
```

`mismatch` の `match` に腕を足す。

```rust
            Origin::MatchArms(first) => diagnostic.with_secondary(Label::new(
                file,
                *first,
                "the first arm has this type",
            )),
            Origin::ConstructorPattern { constructor, ty } => {
                diagnostic.with_note(format!("`{constructor}` is a constructor of `{ty}`"))
            }
```

`callee_subject` のコンストラクタの腕は、Task 2 で足してあるので変えない。

- [ ] **Step 9: 使用回数のパスを確かめる (`usage.rs` は変えない)**

Task 2 が `usage.rs` に入れた `ExprKind::Match` の腕 (枝を別の経路として数え、枝のパターンの変数を枝ごとに数え終える) と `check_pat` の `PatKind::Con` の腕 (引数のパターンをたどる) は、最終の形である。Task 3 では変えない。Task 2 の時点では `PatKind::Con` の型が `Error` だったので制約が出なかったが、Step 8 で型が付くと、入れ子の `_` と使わない変数に `Unr` の制約が出る。Step 1 の `match_arms_are_paths_and_discarded_fields_are_unrestricted` がそれを確かめる。

- [ ] **Step 10: Core IR の変換を追随させる**

`crates/eml_core_ir/src/translate/types.rs` の `Lowering::Constructor(u32)` と、`lowering` の `Builtin::True` / `Builtin::False` の腕を消す。`use crate::{FALSE, IoOp, PrimOp, TRUE, VarInfo};` から `FALSE` と `TRUE` を外す。

`crates/eml_core_ir/src/translate/program.rs` の `Lowering::Constructor(_) => unreachable!(...)` の腕を消す。

`crates/eml_core_ir/src/translate/expr.rs`:

- 文字列リテラルの型 `Type::Con { id: string, name: ... }` に `args: Vec::new()` を足す
- `ExprKind::Path(Res::Builtin(builtin))` の腕は、`Lowering::Constructor` の分岐をなくして包む関数のクロージャだけにする

  ```rust
            ExprKind::Path(Res::Builtin(builtin)) => {
                let wrapper = self.program.wrapper(*builtin);
                let ty = self.ty(id);
                self.bind(out, "c", &ty, Rhs::MakeClosure(wrapper, Vec::new()))
            }
  ```

- Task 2 が `atom` に入れた `ExprKind::Path(Res::Constructor(_)) | ExprKind::Match { .. } => unreachable!(...)` の腕を、次の2つの腕に分ける。`Bool` は引数のないコンストラクタなので、今の `if` の `Switch` と同じタグになる (docs/spec/core-ir.md)。引数を持つコンストラクタの値は Task 5 で、`match` は Task 6 で置き換える

  ```rust
            ExprKind::Path(Res::Constructor(ctor)) => {
                let constructor = &self.module.constructors[*ctor];
                assert!(
                    constructor.fields.is_empty(),
                    "constructors with fields are not lowered to Core IR"
                );
                Atom::Tag(constructor.tag)
            }
            ExprKind::Match { .. } => unreachable!("`match` is not lowered to Core IR"),
  ```

- `call_builtin` の `Lowering::Constructor(_) => unreachable!(...)` の腕を消す

- [ ] **Step 11: テストが通ることを確かめる**

Run: `cargo test -p eml_types --test data && cargo test -p eml_hir --test data`
Expected: PASS

Run: `cargo test`
Expected: PASS。既存のスナップショット (HIR の表示の `True` / `False`、Core IR の `#0` / `#1`、UI テストの出力) は1バイトも変わらない。Task 2 のテスト (`eml_hir/tests/data.rs`、`check-fail/names/constructor_arity.em`、`check-fail/names/duplicate_binding.em`) も、仮の E0004 を期待していないので変わらない。変わったら止まり、差分をユーザーに示す。

- [ ] **Step 12: UI テストを足す**

`tests/ui/check-fail/types/constructor_pattern_mismatch.em`:

```haskell
-- E2001: a constructor pattern of another data type than the matched value.
data Option a =
  | None
  | Some a

data List a =
  | Nil
  | Cons a (List a)

first : List Int -> Int
first xs = match xs with
  | Some x -> x
  | _ -> 0

main : Unit -> <IO> Unit
main () = println (show_int (first Nil))
```

`tests/ui/check-fail/types/type_argument_count.em`:

```haskell
-- E1015 twice: a data type applied to too few and too many type arguments.
data Option a =
  | None
  | Some a

size : Option -> Int
size o = 0

pair : Option Int Int -> Int
pair o = 0

main : Unit -> <IO> Unit
main () = println "done"
```

Run: `cargo test -p eml_cli --test ui`
Expected: 新しいスナップショットが2つできて FAIL。`cargo insta review` で次を確かめてから承認する。
- `constructor_pattern_mismatch`: E2001 が1つで、`Some x` のパターン (12:5) を指し、ラベルが「expected `List Int`, found `Option _`」、note が「`Some` is a constructor of `Option`」
- `type_argument_count`: E1015 が2つで、`Option` (6:8) に「`Option` takes 1 type argument, but 0 were given」、`Option Int Int` (9:8) に「`Option` takes 1 type argument, but 2 were given」(文言は Task 2 の E1015 の文言に合わせる)

- [ ] **Step 13: 全体を確かめる**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて PASS、警告なし。`cargo fmt --check` が差分を出したら `cargo fmt` をかける。

- [ ] **Step 14: コミットする**

```bash
git add crates/eml_types crates/eml_hir crates/eml_core_ir tests/ui/check-fail/types crates/eml_cli/tests/snapshots
git commit -m "$(cat <<'EOF'
Type-check data types, patterns and match, and move Bool to the Prelude

Type constructors carry their type arguments, and the kind of a data
type is the join of the kinds of its effective type parameters.
Constructors get schemes and are instantiated like functions; patterns
and match arms are checked against the scrutinee type, and the usage
pass counts match arms as separate paths. Bool is now the Prelude's
`data Bool = | False | True`.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014YGQm6f7Mtged1yVY4dmJG
EOF
)"
```

### Task 4: 網羅性の検査

型推論と使用回数のパスの後に、型付き HIR の上で網羅性と到達可能性を調べるパス (`eml_types::exhaustive`) を足す。Maranget の usefulness で、網羅されていない `match` (E4001)、網羅されていない等式 (E4002)、反駁可能な `let` とラムダと節の引数 (E4003)、到達しない `match` の枝 (E4004、Warning) を報告する。漏れの例は note に最大3つ並べる。

**Files:**
- Create: `crates/eml_types/src/exhaustive.rs`
- Modify: `crates/eml_types/src/lib.rs` (`mod exhaustive`、codes の E4001〜E4004)
- Modify: `crates/eml_types/src/check/mod.rs` (`check_module` の最後で網羅性の検査を呼ぶ)
- Modify: `crates/eml_hir/src/hir.rs` (`Function::signature_name_range`)、`crates/eml_hir/src/lower/mod.rs` (値を入れる)
- Modify (種類3): `crates/eml_hir/src/lower/scope.rs` の単体テストなど、`Function { .. }` を手で組むコード (`signature_name_range: None` を足すだけで、期待値は変えない)
- Test: `crates/eml_types/tests/exhaustive.rs` (新規)
- Test: `tests/ui/check-fail/exhaustiveness/non_exhaustive_match.em`、`non_exhaustive_equation.em`、`refutable_pattern.em` (新規) とそのスナップショット

**Interfaces:**
- Consumes:
  - Task 2 の HIR: `TypeDefKind::Data { constructors }`、`Constructor { name, ty, .. }` (`fields.len()` が引数の数)、`PatKind::Con { ctor, args }`、`ExprKind::Match { scrutinee, arms }`、`MatchArm { pat, body }`
  - Task 3 の型: `TypedModule::bodies` の `BodyTypes::pats` と `BodyTypes::exprs`、`Type::contains_error`
- Produces:
  - `eml_types::codes::NON_EXHAUSTIVE_MATCH` (E4001)、`NON_EXHAUSTIVE_EQUATION` (E4002)、`REFUTABLE_PATTERN` (E4003)、`UNREACHABLE_ARM` (E4004、`Severity::Warning`)
  - `eml_types::check` が、型検査と使用回数のパスの後に網羅性の検査を動かし、その診断を返す診断の後ろに加える
  - `eml_hir::Function::signature_name_range: Option<TextRange>`
  - 診断の文言 (Task 6 と Task 7 が引く)

  | 番号 | メッセージ | primary のラベル | ほか |
  |---|---|---|---|
  | E4001 | ``"`match` does not cover every value"`` | `match` のキーワードに `"no arm matches some values"` | note ``"not covered: `None`"`` |
  | E4002 | ``"the equation of `f` does not cover every argument"`` | シグネチャの関数名に ``"`f` is not defined for some arguments"`` | secondary は等式の名前に `"this equation does not match every argument"`。note ``"not covered: `f None _`"`` |
  | E4003 | `"this pattern does not match every value"` | パターンに ``"`let` needs a pattern that matches every value"`` (`let`) か `"a parameter needs a pattern that matches every value"` (ラムダと節の引数) | note ``"not covered: `None`"`` |
  | E4004 | ``"unreachable `match` arm"`` | 枝のパターンに `"the arms above already match every value of this pattern"` | Warning |

  note の例は ``"not covered: `Green`, `Blue`, `Cyan`, and more"`` の形で、4つ以上あれば3つだけ並べて `, and more` を付ける。例の書き方は `None`、`Some _`、`Cons _ (Cons _ _)` で、引数を持つコンストラクタを引数の位置に置くときは括弧で囲む。中置のコンストラクタ (名前が `:` で始まる) は `_ :+ _` と書く

**Interface notes (骨組みの Interfaces から変えた点と、前後のタスクとの取り決め):**

- HIR の `Function` に `signature_name_range: Option<TextRange>` を足す (このタスクで足す)。E4002 の primary はシグネチャの関数名だが、今の HIR はシグネチャの名前の位置を持たない。`Function::name_range` は最初の等式の名前の位置なので、E4002 の secondary (等式の先頭) にはそれを使う。`Signature` に足さないのは、操作と Prelude のシグネチャには関数名の位置が要らないためである
- E4001 の primary は `match` のキーワードにする。Task 2 の `ExprKind::Match` はキーワードの範囲を持たないが、`match` 式の範囲はキーワードから始まる。そこで、式の範囲の先頭から `match` の5文字を `TextRange::at(range.start(), TextSize::of("match"))` で作る。HIR の形は変えない
- spec の E4003 は `let` の左辺とラムダの引数を挙げている。handler の節の引数と `return` の節の引数も、ラムダの引数と同じく値を1つ受けるだけの束縛なので、同じ E4003 で調べる。ラベルはラムダの引数と同じ文言にする

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/exhaustive.rs` を作る。`eml_types` の開発用の依存の `eml_test_support` (feature `types`) と、通常の依存の `eml_diagnostics` を使う。

```rust
//! 網羅性と到達可能性の検査 (docs/spec/exhaustiveness.md)。

use eml_diagnostics::{Severity, has_errors};
use eml_test_support::{check, full};

/// 型検査までの診断を、ラベルと note まで表示する。
fn diagnostics(text: &str) -> String {
    let checked = check(text);
    full(&checked.files, &checked.diagnostics)
}

#[test]
fn a_match_that_misses_a_constructor() {
    let text = "data Option a = | None | Some a\n\nf : Option Int -> Int\nf o = match o with\n  | Some n -> n";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4001 4:7 `match` does not cover every value
      4:7 no arm matches some values
      note: not covered: `None`
    ");
}

#[test]
fn a_bool_match_that_misses_true() {
    let text = "f : Bool -> Int\nf b = match b with\n  | False -> 0";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4001 2:7 `match` does not cover every value
      2:7 no arm matches some values
      note: not covered: `True`
    ");
}

#[test]
fn a_nested_pattern_reports_the_missing_inner_constructor() {
    let text = "data Option a = | None | Some a\n\nf : Option (Option Int) -> Int\nf o = match o with\n  | Some (Some n) -> n\n  | None -> 0";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4001 4:7 `match` does not cover every value
      4:7 no arm matches some values
      note: not covered: `Some None`
    ");
}

#[test]
fn a_recursive_type_reports_a_nested_example() {
    let text = "data List a = | Nil | Cons a (List a)\n\nf : List Int -> Int\nf xs = match xs with\n  | Nil -> 0\n  | Cons x Nil -> x";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4001 4:8 `match` does not cover every value
      4:8 no arm matches some values
      note: not covered: `Cons _ (Cons _ _)`
    ");
}

#[test]
fn more_than_three_examples_are_cut_short() {
    let text = "data Color = | Red | Green | Blue | Cyan | Magenta\n\nname : Color -> Int\nname c = match c with\n  | Red -> 0";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4001 4:10 `match` does not cover every value
      4:10 no arm matches some values
      note: not covered: `Green`, `Blue`, `Cyan`, and more
    ");
}

#[test]
fn an_arm_after_a_wildcard_is_unreachable() {
    let text = "data Option a = | None | Some a\n\nf : Option Int -> Int\nf o = match o with\n  | _ -> 0\n  | Some n -> n";
    let checked = check(text);
    insta::assert_snapshot!(full(&checked.files, &checked.diagnostics), @r"
    E4004 6:5 unreachable `match` arm
      6:5 the arms above already match every value of this pattern
    ");
    assert_eq!(checked.diagnostics[0].severity, Severity::Warning);
    assert!(!has_errors(&checked.diagnostics));
}

#[test]
fn a_repeated_arm_is_unreachable() {
    let text = "data Option a = | None | Some a\n\nf : Option Int -> Int\nf o = match o with\n  | None -> 0\n  | Some n -> n\n  | None -> 1";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4004 7:5 unreachable `match` arm
      7:5 the arms above already match every value of this pattern
    ");
}

#[test]
fn single_constructor_patterns_are_irrefutable() {
    let text = "data Box a = | Box a\n\nunbox : Box Int -> Int\nunbox (Box n) =\n  let Box m = Box n\n  let f = fn (Box k) -> k\n  f (Box m)";
    assert_eq!(diagnostics(text), "");
}

#[test]
fn refutable_let_and_lambda_patterns() {
    let text = "data Option a = | None | Some a\n\nf : Option Int -> Int\nf o =\n  let Some n = o\n  let g = fn (Some k) -> k\n  g (Some n)";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4003 5:7 this pattern does not match every value
      5:7 `let` needs a pattern that matches every value
      note: not covered: `None`
    E4003 6:15 this pattern does not match every value
      6:15 a parameter needs a pattern that matches every value
      note: not covered: `None`
    ");
}

#[test]
fn refutable_clause_parameters() {
    // 節は持ち上げる関数で、引数のパターンもラムダの引数と同じ経路で分解するので、同じ E4003 で調べる
    let text = "data Option a = | None | Some a\n\neffect Ask where\n  ask : Option Int -> Int\n\nf : Unit -> Int\nf () =\n  handle Some (ask (Some 1)) with\n    | ask (Some q) k -> resume k q\n    | return (Some r) -> r";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4003 9:12 this pattern does not match every value
      9:12 a parameter needs a pattern that matches every value
      note: not covered: `None`
    E4003 10:15 this pattern does not match every value
      10:15 a parameter needs a pattern that matches every value
      note: not covered: `None`
    ");
}

#[test]
fn a_refutable_equation_points_at_the_signature() {
    let text = "data Option a = | None | Some a\n\nget : Option Int -> Int -> Int\nget (Some n) m = n + m";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4002 3:1 the equation of `get` does not cover every argument
      3:1 `get` is not defined for some arguments
      4:1 this equation does not match every argument
      note: not covered: `get None _`
    ");
}

#[test]
fn a_scrutinee_with_an_error_type_is_not_checked() {
    let text = "data Option a = | None | Some a\n\nf : Int -> Int\nf x = match missing with\n  | Some n -> n";
    insta::assert_snapshot!(diagnostics(text), @r"
    E1001 4:13 cannot find value `missing`
      4:13 not found in this scope
    ");
}

#[test]
fn constructors_of_two_types_in_one_column_add_no_exhaustiveness_errors() {
    // `Nil` の型の誤りは E2001 で報告済みなので、混ざった列から E4xxx を連鎖させない
    let text = "data Option a = | None | Some a\ndata List a = | Nil | Cons a (List a)\n\nf : Option Int -> Int\nf o = match o with\n  | Nil -> 0\n  | None -> 1";
    let checked = check(text);
    let codes: Vec<String> = checked
        .diagnostics
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    assert!(codes.contains(&"E2001".to_string()), "{codes:?}");
    assert!(codes.iter().all(|code| !code.starts_with("E4")), "{codes:?}");
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --test exhaustive`
Expected: コンパイルは通る。`single_constructor_patterns_are_irrefutable`、`a_scrutinee_with_an_error_type_is_not_checked`、`constructors_of_two_types_in_one_column_add_no_exhaustiveness_errors` 以外の10件が、E4xxx の診断がないので FAIL する

- [ ] **Step 3: HIR にシグネチャの名前の位置を持たせる**

`crates/eml_hir/src/hir.rs` の `Function` に欄を足す。

```rust
#[derive(Debug)]
pub struct Function {
    pub name: String,
    /// 最初の等式の名前の位置。等式がなければシグネチャの名前の位置。
    pub name_range: TextRange,
    /// シグネチャの名前の位置。網羅されていない等式の診断 (E4002) は、ここを primary にする
    /// (docs/spec/diagnostics.md の「網羅性の診断」)。
    pub signature_name_range: Option<TextRange>,
    /// なければ `None` で、E1004 は報告済み。
    pub signature: Option<Signature>,
    /// 等式がなければ `None` で、E1005 は報告済み。
    pub body: Option<Body>,
}
```

`crates/eml_hir/src/lower/mod.rs` の `lower` で、`signature` を `map` で消費する前に名前の位置を取り出し、`Function` に入れる。

```rust
        let signature_name_range = signature.as_ref().map(|(_, _, range)| *range);
        let signature = signature.map(|(_, node, _)| {
            // (今のまま)
        });
        // ...
        let id = functions.alloc(Function {
            name: name.clone(),
            name_range,
            signature_name_range,
            signature,
            body: None,
        });
```

`Function { .. }` を手で組むほかの箇所 (`crates/eml_hir/src/lower/scope.rs` の単体テストなど) に `signature_name_range: None` を足す。`grep -rn "Function {" crates --include=*.rs` で漏れを探す。期待値は変えない (種類3)。

- [ ] **Step 4: codes を足す**

`crates/eml_types/src/lib.rs` の `mod` の並びに `mod exhaustive;` を足し、`codes` に4つを足す。

```rust
pub mod codes {
    use eml_diagnostics::ErrorCode;

    pub const LINEAR_VALUE_MISUSED: ErrorCode = ErrorCode(3001);
    pub const TYPE_MISMATCH: ErrorCode = ErrorCode(2001);
    pub const EFFECT_NOT_IN_ROW: ErrorCode = ErrorCode(2002);
    pub const MISSING_MAIN: ErrorCode = ErrorCode(2003);
    pub const INVALID_MAIN_TYPE: ErrorCode = ErrorCode(2004);
    pub const INFINITE_TYPE: ErrorCode = ErrorCode(2005);
    pub const NON_EXHAUSTIVE_MATCH: ErrorCode = ErrorCode(4001);
    pub const NON_EXHAUSTIVE_EQUATION: ErrorCode = ErrorCode(4002);
    pub const REFUTABLE_PATTERN: ErrorCode = ErrorCode(4003);
    /// 重大度は Warning である (docs/spec/diagnostics.md の「網羅性の診断」)。
    pub const UNREACHABLE_ARM: ErrorCode = ErrorCode(4004);
}
```

- [ ] **Step 5: 網羅性の検査を書く**

`crates/eml_types/src/exhaustive.rs` を作る。再帰するのはパターンの変換と usefulness の行列の処理だけで、どちらの深さもパターンの入れ子 (E0013 で抑えられる) と列の数で決まる。

```rust
//! パターンの網羅性と到達可能性の検査 (docs/spec/exhaustiveness.md)。型推論と使用回数のパスの後に、型付き HIR の
//! 上で別のパスとして動かす。アルゴリズムは Maranget の usefulness で、網羅されていないときは漏れている値の例を作る。

use std::iter;

use eml_diagnostics::{Diagnostic, Label, Severity, TextRange, TextSize};
use eml_hir::{
    Body, ConstructorId, ExprId, ExprKind, Function, MatchArm, Module, PatId, PatKind, Stmt,
    TypeDefKind, TypeDefId,
};

use crate::{BodyTypes, TypedModule, codes};

/// note に並べる漏れの例の数。1つ多く集めて、ほかにもあるかを知る。
const SHOWN: usize = 3;

const LET_LABEL: &str = "`let` needs a pattern that matches every value";
const PARAMETER_LABEL: &str = "a parameter needs a pattern that matches every value";

pub(crate) fn check(module: &Module, typed: &TypedModule) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (id, function) in module.functions.iter() {
        let (Some(body), Some(types)) = (&function.body, typed.bodies.get(id)) else {
            continue;
        };
        let mut pass = Exhaustive {
            module,
            body,
            types,
            diagnostics: Vec::new(),
        };
        pass.equation(function);
        pass.positions();
        // 式のアリーナの順はソースの順と一致しないので、関数の中で位置の順に並べる
        pass.diagnostics.sort_by_key(|d| d.primary.range.start());
        diagnostics.extend(pass.diagnostics);
    }
    diagnostics
}

/// 検査に使うパターン。変数、`_`、`()` はどれも何にでも合うので区別しない。漏れの例もこの形で作る。
#[derive(Debug, Clone)]
enum Pat {
    Wild,
    Con(ConstructorId, Vec<Pat>),
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
}

struct Exhaustive<'a> {
    module: &'a Module,
    body: &'a Body,
    types: &'a BodyTypes,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Exhaustive<'a> {
    /// 等式の引数の並びを1行の行列として調べる。spec の「引数のタプルに対する `match`」と同じ結果になる
    /// (docs/spec/exhaustiveness.md)。複数の等式は段階6で行を足す。
    fn equation(&mut self, function: &Function) {
        let Some(signature_name) = function.signature_name_range else {
            return;
        };
        let Some(row) = self.row(&self.body.params) else {
            return;
        };
        let width = row.len();
        let Ok(missing) = self.missing(&[row], width, SHOWN + 1) else {
            return;
        };
        if missing.is_empty() {
            return;
        }
        let name = &function.name;
        let examples: Vec<String> = missing
            .iter()
            .map(|args| {
                iter::once(name.clone())
                    .chain(args.iter().map(|arg| self.atomic(arg)))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        self.diagnostics.push(
            Diagnostic::error(
                codes::NON_EXHAUSTIVE_EQUATION,
                format!("the equation of `{name}` does not cover every argument"),
                Label::new(
                    self.module.file,
                    signature_name,
                    format!("`{name}` is not defined for some arguments"),
                ),
            )
            .with_secondary(Label::new(
                self.module.file,
                function.name_range,
                "this equation does not match every argument",
            ))
            .with_note(not_covered(&examples)),
        );
    }

    /// 本体の中の `match` と、値を1つ受けるだけの束縛のパターン。式はアリーナを順に見るので、木を再帰しない。
    fn positions(&mut self) {
        let body = self.body;
        for (_, expr) in body.exprs.iter() {
            match &expr.kind {
                ExprKind::Match { scrutinee, arms } => self.match_expr(expr.range, *scrutinee, arms),
                ExprKind::Block { stmts, .. } => {
                    for stmt in stmts {
                        if let Stmt::Let { pat, .. } = stmt {
                            self.irrefutable(*pat, LET_LABEL);
                        }
                    }
                }
                ExprKind::Lambda { params, .. } => {
                    for &param in params {
                        self.irrefutable(param, PARAMETER_LABEL);
                    }
                }
                // 節の引数もラムダの引数と同じく、値を1つ受けるだけの束縛である
                ExprKind::Handle { clauses, ret, .. } => {
                    for clause in clauses {
                        for pat in clause.patterns() {
                            self.irrefutable(pat, PARAMETER_LABEL);
                        }
                    }
                    if let Some(ret) = ret {
                        self.irrefutable(ret.param, PARAMETER_LABEL);
                    }
                }
                _ => {}
            }
        }
    }

    fn match_expr(&mut self, range: TextRange, scrutinee: ExprId, arms: &[MatchArm]) {
        // scrutinee の型の誤りは報告済みである。パターンの型が期待する型のまま残っていても、検査しない
        if self
            .types
            .exprs
            .get(scrutinee)
            .is_some_and(|ty| ty.contains_error())
        {
            return;
        }
        let Some(rows) = arms
            .iter()
            .map(|arm| self.pat(arm.pat).map(|pat| vec![pat]))
            .collect::<Option<Vec<Row>>>()
        else {
            return;
        };
        let mut unreachable = Vec::new();
        for (index, row) in rows.iter().enumerate() {
            match self.useful(&rows[..index], row) {
                Ok(true) => {}
                Ok(false) => unreachable.push(arms[index].pat),
                Err(Mixed) => return,
            }
        }
        let Ok(missing) = self.missing(&rows, 1, SHOWN + 1) else {
            return;
        };
        for pat in unreachable {
            let range = self.body.pats[pat].range;
            self.diagnostics.push(Diagnostic::new(
                codes::UNREACHABLE_ARM,
                Severity::Warning,
                "unreachable `match` arm",
                Label::new(
                    self.module.file,
                    range,
                    "the arms above already match every value of this pattern",
                ),
            ));
        }
        if missing.is_empty() {
            return;
        }
        // `match` 式の範囲は `match` のキーワードから始まる
        let keyword = TextRange::at(range.start(), TextSize::of("match"));
        let examples: Vec<String> = missing.iter().map(|row| self.show(&row[0])).collect();
        self.diagnostics.push(
            Diagnostic::error(
                codes::NON_EXHAUSTIVE_MATCH,
                "`match` does not cover every value",
                Label::new(self.module.file, keyword, "no arm matches some values"),
            )
            .with_note(not_covered(&examples)),
        );
    }

    fn irrefutable(&mut self, pat: PatId, label: &str) {
        let Some(row) = self.pat(pat).map(|pat| vec![pat]) else {
            return;
        };
        let Ok(missing) = self.missing(&[row], 1, SHOWN + 1) else {
            return;
        };
        if missing.is_empty() {
            return;
        }
        let examples: Vec<String> = missing.iter().map(|row| self.show(&row[0])).collect();
        self.diagnostics.push(
            Diagnostic::error(
                codes::REFUTABLE_PATTERN,
                "this pattern does not match every value",
                Label::new(self.module.file, self.body.pats[pat].range, label),
            )
            .with_note(not_covered(&examples)),
        );
    }

    fn row(&self, pats: &[PatId]) -> Option<Row> {
        pats.iter().map(|&pat| self.pat(pat)).collect()
    }

    /// `Missing` のパターンと、型が `Error` を含むパターンは `None` にする。構文や型の誤りを報告済みの位置から、
    /// 網羅性の診断を連鎖させない (docs/spec/types.md の「エラーの扱い」)。
    fn pat(&self, id: PatId) -> Option<Pat> {
        if self
            .types
            .pats
            .get(id)
            .is_some_and(|ty| ty.contains_error())
        {
            return None;
        }
        match &self.body.pats[id].kind {
            PatKind::Missing => None,
            PatKind::Bind(_) | PatKind::Wildcard | PatKind::Unit => Some(Pat::Wild),
            PatKind::Annot { pat, .. } => self.pat(*pat),
            PatKind::Con { ctor, args } => Some(Pat::Con(*ctor, self.row(args)?)),
        }
    }

    fn arity(&self, ctor: ConstructorId) -> usize {
        self.module.constructors[ctor].fields.len()
    }

    /// コンストラクタの型と、その型のすべてのコンストラクタ (宣言の順)。
    fn constructors_of(&self, ctor: ConstructorId) -> Result<(TypeDefId, &'a [ConstructorId]), Mixed> {
        let ty = self.module.constructors[ctor].ty;
        match &self.module.types[ty].kind {
            TypeDefKind::Data { constructors } => Ok((ty, constructors)),
            TypeDefKind::Builtin => Err(Mixed),
        }
    }

    fn column(&self, rows: &[Row]) -> Result<Column<'a>, Mixed> {
        let mut seen = Vec::new();
        let mut data: Option<(TypeDefId, &'a [ConstructorId])> = None;
        for row in rows {
            let Pat::Con(ctor, _) = &row[0] else {
                continue;
            };
            let (ty, all) = self.constructors_of(*ctor)?;
            match data {
                Some((known, _)) if known != ty => return Err(Mixed),
                _ => data = Some((ty, all)),
            }
            if !seen.contains(ctor) {
                seen.push(*ctor);
            }
        }
        Ok(match data {
            Some((_, all)) => Column::Data { seen, all },
            None => Column::Wild,
        })
    }

    /// 先頭の列がコンストラクタ `ctor` に合う行を、その引数を先頭に並べた行にする。
    fn specialize(&self, rows: &[Row], ctor: ConstructorId) -> Vec<Row> {
        let arity = self.arity(ctor);
        rows.iter()
            .filter_map(|row| {
                let (head, rest) = row.split_first()?;
                let mut out = match head {
                    Pat::Con(other, args) if *other == ctor => args.clone(),
                    Pat::Con(..) => return None,
                    Pat::Wild => vec![Pat::Wild; arity],
                };
                out.extend_from_slice(rest);
                Some(out)
            })
            .collect()
    }

    /// `vector` に合う値のうち、`rows` のどの行にも合わないものがあるか。到達しない枝を見つけるのに使う。
    fn useful(&self, rows: &[Row], vector: &[Pat]) -> Result<bool, Mixed> {
        let Some((head, rest)) = vector.split_first() else {
            return Ok(rows.is_empty());
        };
        let column = self.column(rows)?;
        match head {
            Pat::Con(ctor, args) => {
                if let Column::Data { all, .. } = column
                    && !all.contains(ctor)
                {
                    return Err(Mixed);
                }
                let mut next = args.clone();
                next.extend_from_slice(rest);
                self.useful(&self.specialize(rows, *ctor), &next)
            }
            Pat::Wild => match column {
                Column::Data { seen, all } if seen.len() == all.len() => {
                    for &ctor in all {
                        let mut next = vec![Pat::Wild; self.arity(ctor)];
                        next.extend_from_slice(rest);
                        if self.useful(&self.specialize(rows, ctor), &next)? {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                }
                _ => self.useful(&default_rows(rows), rest),
            },
        }
    }

    /// 長さ `width` の値の並びのうち、`rows` のどの行にも合わないものを `limit` 個まで作る。`_` の並びの usefulness を、
    /// 例を組み立てながら解く。先頭の列にその型のすべてのコンストラクタが現れていれば、コンストラクタごとに調べる。
    /// そうでなければ、先頭が `_` の行だけで残りの列を調べ、現れていないコンストラクタ (どれも現れていなければ `_`) を
    /// 先頭に付ける。
    fn missing(&self, rows: &[Row], width: usize, limit: usize) -> Result<Vec<Row>, Mixed> {
        if width == 0 {
            return Ok(if rows.is_empty() {
                vec![Vec::new()]
            } else {
                Vec::new()
            });
        }
        let mut found = Vec::new();
        match self.column(rows)? {
            Column::Data { seen, all } if seen.len() == all.len() => {
                for &ctor in all {
                    if found.len() >= limit {
                        break;
                    }
                    let arity = self.arity(ctor);
                    let inner = self.missing(
                        &self.specialize(rows, ctor),
                        arity + width - 1,
                        limit - found.len(),
                    )?;
                    for mut values in inner {
                        let rest = values.split_off(arity);
                        let mut row = vec![Pat::Con(ctor, values)];
                        row.extend(rest);
                        found.push(row);
                    }
                }
            }
            column => {
                let rest = self.missing(&default_rows(rows), width - 1, limit)?;
                let heads: Vec<Pat> = match column {
                    Column::Data { seen, all } => all
                        .iter()
                        .filter(|ctor| !seen.contains(ctor))
                        .map(|&ctor| Pat::Con(ctor, vec![Pat::Wild; self.arity(ctor)]))
                        .collect(),
                    Column::Wild => vec![Pat::Wild],
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

    fn show(&self, pat: &Pat) -> String {
        match pat {
            Pat::Wild => "_".to_string(),
            Pat::Con(ctor, args) => {
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
        }
    }

    /// 引数の位置に置く書き方。引数を持つコンストラクタは括弧で囲む。
    fn atomic(&self, pat: &Pat) -> String {
        match pat {
            Pat::Con(_, args) if !args.is_empty() => format!("({})", self.show(pat)),
            _ => self.show(pat),
        }
    }
}

/// 先頭の列が `_` の行の、残りの列。
fn default_rows(rows: &[Row]) -> Vec<Row> {
    rows.iter()
        .filter(|row| matches!(row[0], Pat::Wild))
        .map(|row| row[1..].to_vec())
        .collect()
}

fn not_covered(examples: &[String]) -> String {
    let shown: Vec<String> = examples
        .iter()
        .take(SHOWN)
        .map(|example| format!("`{example}`"))
        .collect();
    let more = if examples.len() > SHOWN { ", and more" } else { "" };
    format!("not covered: {}{more}", shown.join(", "))
}
```

`missing` の完全な列の分岐で `limit - found.len()` が 0 になる前に `break` するので、引き算は負にならない。内側の呼び出しが `limit` より多く返しても、最後の `truncate` で切る。

- [ ] **Step 6: 型検査の最後で網羅性の検査を呼ぶ**

`crates/eml_types/src/check/mod.rs` の `check_module` で、`typed.bodies` を埋めるループの後、`(typed, diagnostics)` を返す前に呼ぶ。`use` に `exhaustive` を足す (`use crate::{BodyTypes, TypedModule, codes, exhaustive, scc, usage};`)。

```rust
    for (id, typing) in bodies {
        // (今のまま)
        typed.bodies.insert(id, types);
    }
    // 網羅性は型推論と使用回数のパスの後に、書き出した型の上で調べる (docs/spec/exhaustiveness.md の「検査パス」)
    diagnostics.extend(exhaustive::check(module, &typed));
    (typed, diagnostics)
```

- [ ] **Step 7: テストが通ることを確かめる**

Run: `cargo test -p eml_types --test exhaustive`
Expected: 13件すべて PASS

- [ ] **Step 8: UI テストを足す**

`tests/ui/check-fail/exhaustiveness/` に3つのファイルを作る。

`non_exhaustive_match.em`:

```haskell
-- E4001: the `match` has no arm for `Blue`.
data Color = | Red | Green | Blue

name : Color -> String
name c = match c with
  | Red -> "red"
  | Green -> "green"

main : Unit -> <IO> Unit
main () = println (name Red)
```

`non_exhaustive_equation.em`:

```haskell
-- E4002: the only equation of `first` does not accept `Nil`.
data List a = | Nil | Cons a (List a)

first : List Int -> Int
first (Cons x _) = x

main : Unit -> <IO> Unit
main () = println (show_int (first (Cons 1 Nil)))
```

`refutable_pattern.em`:

```haskell
-- E4003: a `let` pattern and a lambda parameter that do not match `None`.
data Option a = | None | Some a

main : Unit -> <IO> Unit
main () =
  let Some n = Some 1
  let f = fn (Some m) -> m
  println (show_int (n + f (Some 2)))
```

Run: `cargo test -p eml_cli --test ui check_fail`
Expected: 新しい3件のスナップショットがないので FAIL する。`cargo insta review` で次の内容を確かめてから承認する。

- `non_exhaustive_match.em`: `[E4001] Error: `match` does not cover every value`、5:10 の `match` に `no arm matches some values`、note `not covered: `Blue``
- `non_exhaustive_equation.em`: `[E4002] Error: the equation of `first` does not cover every argument`、4:1 の `first` に `` `first` is not defined for some arguments``、5:1 の `first` に `this equation does not match every argument`、note `not covered: `first Nil``
- `refutable_pattern.em`: E4003 が2件。6:7 の `Some n` に `` `let` needs a pattern that matches every value``、7:15 の `Some m` に `a parameter needs a pattern that matches every value`。どちらも note `not covered: `None``

E4004 は Warning なので `check-fail/` には置けない。実行して stderr の Warning を確かめる UI テスト (`run/data/unreachable_arm.em`) は、`match` を Core IR にコンパイルする Task 6 で足す。

- [ ] **Step 9: 全体を確かめる**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`
Expected: すべて PASS し、clippy の警告がない。既存のテストの期待値は変わらない (`Function` の組み立ての追随は種類3)

- [ ] **Step 10: コミットする**

```bash
git add crates/eml_types/src/exhaustive.rs crates/eml_types/src/lib.rs crates/eml_types/src/check/mod.rs crates/eml_types/tests/exhaustive.rs crates/eml_hir/src tests/ui/check-fail/exhaustiveness crates/eml_cli/tests/snapshots
git commit -m "Check that patterns are exhaustive and match arms are reachable

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014YGQm6f7Mtged1yVY4dmJG"
```

### Task 5: `data` のオブジェクトと、フィールドを束縛する `Switch` の枝

ランタイムに `data` のオブジェクト (`Payload::Data`) を足し、Core IR の `Switch` の枝にフィールドを束縛させる。`Switch` は scrutinee を move で受け取り、一意なら取り出して箱を解放し、共有されていればフィールドを `dup` してから箱を `decref` する (`Heap::take_or_copy`)。コンストラクタの値は、引数がそろえば `Rhs::Con` で作り、足りなければコンストラクタを包む関数のクロージャにする。まだ `match` のコンパイルはないので、ソースから作れるのはコンストラクタの値だけである。`Switch` の分解は手書きの Core IR で確かめる。

**Files:**
- Modify: `crates/eml_runtime/src/heap.rs` (`Payload::Data`、`DescId::DATA`、`DESCRIPTORS`、`copy`、`children`)
- Modify: `crates/eml_runtime/src/heap/tests.rs`
- Modify: `crates/eml_core_ir/src/lib.rs` (`Arm`、`CExpr::Switch`、`Rhs::Con`)
- Modify: `crates/eml_core_ir/src/translate/types.rs` (boxed の判定と `var_info`)
- Modify: `crates/eml_core_ir/src/translate/program.rs` (`constructor_wrapper`、`lang` の除去)
- Modify: `crates/eml_core_ir/src/translate/expr.rs` (コンストラクタの値と呼び出し)
- Modify: `crates/eml_core_ir/src/translate/mod.rs` (`if` の `Switch`、`entry` と `var_info` の呼び出し)
- Modify: `crates/eml_core_ir/src/simplify.rs`、`liveness.rs`、`perceus.rs`、`verify.rs`、`pretty.rs`
- Modify: `crates/eml_interp/src/lib.rs`
- Modify (種類3): `crates/eml_core_ir/tests/verify.rs` の `pick` (`Switch` の枝を `Arm` にする)
- Create: `crates/eml_interp/tests/data.rs`
- Create: `tests/ui/run/data/constructors.em`
- Test: `crates/eml_runtime/src/heap/tests.rs`、`crates/eml_core_ir/tests/translate.rs`、`crates/eml_core_ir/tests/perceus.rs`、`crates/eml_core_ir/tests/verify.rs`、`crates/eml_interp/tests/data.rs`

**Interfaces:**
- Consumes:
  - Task 1 の `CExpr::Join { params, .. }`、`CExpr::Jump { args, .. }`
  - Task 2 の `Module::constructors`、`Constructor { name, tag, fields, .. }`、`TypeDefKind::Data { constructors }`、`Res::Constructor`
  - Task 3 の `TypedModule::constructors: ArenaMap<ConstructorId, Scheme>` と、引数のないコンストラクタを `Atom::Tag` にする変換
- Produces:

  ```rust
  // eml_runtime
  Payload::Data { tag: u32, fields: Vec<Value> }

  // eml_core_ir
  pub struct Arm {
      pub tag: u32,
      pub fields: Vec<VarId>,
      pub body: CExprId,
  }
  CExpr::Switch { scrutinee: Atom, arms: Vec<Arm> }
  Rhs::Con { tag: u32, args: Vec<Atom> }
  ```

  - `pretty` の表示は、フィールドのない枝が今の `#0 ->`、フィールドのある枝が `#1(h, t) ->`、値の確保が `let d1 = con #1(a, b)` (結果の変数の名前は `d`)
  - translate の `var_info(name: &str, ty: &Type, module: &Module) -> VarInfo`
  - `ProgramBuilder::constructor_wrapper(&mut self, module: &Module, ctor: ConstructorId) -> FnIdx` (関数の名前は `con$Some`)
  - インタプリタの `Switch` は、`Value::Tag` ならそのタグの枝に入り、`Payload::Data` なら `Heap::take_or_copy` で分解する。枝のフィールドの数が値と違えば `Fault::Internal("a switch arm binds a different number of fields than the value has")`

**Interface notes (骨組みの Interfaces から変えた点と、前後のタスクとの取り決め):**

- `ProgramBuilder::constructor_wrapper` の引数から `typed` を外し、`constructor_wrapper(&mut self, module: &Module, ctor: ConstructorId) -> FnIdx` にした。操作を包む関数 (`operation_wrapper`) と同じく、スキームの型は `ProgramBuilder::new` が `TypedModule::constructors` から写して持つ。
- `var_info(name, ty, module)` に合わせて、`ProgramBuilder` は `lang: LangItems` を持たなくなる。`ProgramBuilder::wrapper` と `ProgramBuilder::entry` は、先頭に `module: &Module` を受ける (`wrapper(&mut self, module: &Module, builtin: Builtin)`、`entry(&mut self, module: &Module, main: FnIdx, main_type: &Type)`)。どちらも `pub(super)` で、`translate/` の外には見えない。
- Perceus が枝のフィールドと scrutinee の move を扱う部分は Task 5 で書くが、ソースからその形 (フィールドを束縛する枝、RC の対象の scrutinee) を作れるのは `match` のコンパイル (Task 6) の後である。Task 5 では、verifier の手書きの IR のテストで「Perceus が作るべき形」を確かめ、インタプリタの手書きの IR のテストで実行を確かめる。Task 6 は `crates/eml_core_ir/tests/perceus.rs` に、少なくとも次の2つを足す。使わないフィールドを枝の入口で `decref` すること、枝の中でも scrutinee を使うときに `Switch` の前で `dup` すること (spec の `a_split_switch_still_unpacks_the_arm_with_fields` とは別に)
- Task 5 の B2 は、フィールドを束縛する枝を1つでも持つ `Switch` を対象にしない (`continue`)。引数のない枝だけを切り出す形への作り直しは Task 6 が行う。
- 引数のそろった `Rhs::Con` の結果の変数の名前は `d` にする (`let d3 = con #1(x1)`)。`data` の値を呼び出しの結果 (`t`) と見分けるためで、Task 6 のスナップショットもこの名前を前提にする。コンストラクタを包む関数の中の結果の変数も `d` にする。
- 新しい結合テストのファイル `crates/eml_interp/tests/data.rs` を作る。`docs/implementation/testing.md` の地図への追加は Task 7 で行う。
- verifier の新しい誤り「`d0` is not boxed, but a switch binds its fields」を足す。

- [ ] **Step 1: ランタイムの失敗するテストを書く**

`crates/eml_runtime/src/heap/tests.rs` の末尾 (`mark_shared_is_reserved_for_multicore` の後) に足す。

```rust
#[test]
fn a_data_object_releases_its_fields() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "a");
    let data = heap.alloc(Payload::Data {
        tag: 1,
        fields: vec![Value::Obj(s), Value::Int(3), Value::Tag(0)],
    });
    assert_eq!(
        heap.live_objects(),
        [("Data".to_string(), 1), ("String".to_string(), 1)]
    );
    heap.decref(data).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn take_or_copy_takes_a_unique_data_object_with_its_fields() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "a");
    let data = heap.alloc(Payload::Data {
        tag: 1,
        fields: vec![Value::Obj(s)],
    });
    assert_eq!(
        heap.take_or_copy(data),
        Ok(Payload::Data {
            tag: 1,
            fields: vec![Value::Obj(s)],
        })
    );
    // 箱は解放され、フィールドの参照は取り出した側に移る
    assert_eq!(heap.live_objects(), [("String".to_string(), 1)]);
    heap.decref(s).unwrap();
    assert!(heap.live_objects().is_empty());
}

#[test]
fn take_or_copy_copies_a_shared_data_object_and_dups_its_fields() {
    let mut heap = Heap::new();
    let s = string(&mut heap, "a");
    let data = heap.alloc(Payload::Data {
        tag: 1,
        fields: vec![Value::Obj(s), Value::Int(2)],
    });
    heap.dup(data).unwrap();
    let copy = heap.take_or_copy(data).unwrap();
    assert_eq!(
        copy,
        Payload::Data {
            tag: 1,
            fields: vec![Value::Obj(s), Value::Int(2)],
        }
    );
    // 元の箱と取り出したフィールドが、文字列の参照を1つずつ持つ
    heap.decref(data).unwrap();
    assert_eq!(heap.live_objects(), [("String".to_string(), 1)]);
    heap.decref(s).unwrap();
    assert!(heap.live_objects().is_empty());
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_runtime data`
Expected: コンパイルエラー (`Payload` に `Data` がない)

- [ ] **Step 3: `Payload::Data` を足す**

`crates/eml_runtime/src/heap.rs` を次のように直す。

`DescId` に記述子を足す。

```rust
impl DescId {
    const STRING: DescId = DescId(0);
    const FRAME: DescId = DescId(1);
    const CLOSURE: DescId = DescId(2);
    const CONTINUATION: DescId = DescId(3);
    const DATA: DescId = DescId(4);
}
```

`Descriptor` の doc コメントの最後の文「段階4で、ユーザーの `data` の記述子を登録できるようにする。」を「`data` のオブジェクトは、型によらず1つの記述子にする。型ごとのフィールドのレイアウトは、`Lin` の破棄処理と一緒に後で足す。」に置き換え、表に1行足す。

```rust
const DESCRIPTORS: [Descriptor; 5] = [
    Descriptor { name: "String" },
    Descriptor { name: "Frame" },
    Descriptor { name: "Closure" },
    Descriptor {
        name: "Continuation",
    },
    Descriptor { name: "Data" },
];
```

`Payload` に `Data` を足す。

```rust
    /// 引数を持つコンストラクタの値。フィールドはそれぞれ参照を1つずつ所有する。引数のないコンストラクタの値は
    /// ヒープに置かず、`Value::Tag` にする (docs/spec/runtime.md)。
    Data {
        tag: u32,
        fields: Vec<Value>,
    },
```

`Payload::desc` の `match` に `Payload::Data { .. } => DescId::DATA,` を足す。`copy` の `match` に次の腕を足す。

```rust
        Payload::Data { tag, fields } => Payload::Data {
            tag: *tag,
            fields: fields.clone(),
        },
```

`children` の `match` に次の腕を足す (`Payload::Closure` の腕の後)。

```rust
        Payload::Data { fields, .. } => work.extend(fields.iter().filter_map(object)),
```

- [ ] **Step 4: ランタイムのテストを通す**

Run: `cargo test -p eml_runtime`
Expected: PASS。`eml_interp` は `Payload` の網羅していない `match` があればコンパイルエラーになるが、インタプリタは Step 14 で直すので、ここでは `eml_runtime` だけを流す。

- [ ] **Step 5: Core IR の失敗するテストを書く**

`crates/eml_core_ir/tests/translate.rs` の末尾に足す。

```rust
#[test]
fn constructors_with_fields_build_values() {
    let text = "data Option a =\n  | None\n  | Some a\n\nwrap : Int -> Option Int\nwrap n = Some n\n\nnest : Unit -> Option (Option Int)\nnest () = Some (Some 1)\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r"
    fn wrap(n0) {
      let d1 = con #1(n0)
      return d1
    }
    fn nest(p0) {
      let d1 = con #1(1)
      let d2 = con #1(d1)
      return d2
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
fn a_constructor_used_as_a_function_value_is_wrapped() {
    let text = "data Pair a b =\n  | Pair a b\n\npairs : Int -> Pair Int Int\npairs n =\n  let make = Pair n\n  make 2\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r"
    fn pairs(n0) {
      let c1 = closure con$Pair(n0)
      tailcall apply c1(2)
    }
    fn main(p0) {
      return ()
    }
    fn con$Pair(p0, p1) {
      let d2 = con #0(p0, p1)
      return d2
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}
```

`crates/eml_core_ir/tests/perceus.rs` の末尾に足す。

```rust
#[test]
fn constructor_arguments_are_owned_by_the_value() {
    let text = "data Pair a b =\n  | Pair a b\n\ntwice : String -> Pair String String\ntwice s = Pair s s\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r"
    fn twice(s0) {
      dup s0
      let d1 = con #0(s0, s0)
      return d1
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

`crates/eml_core_ir/tests/verify.rs` を次のように直す (種類3と新しいテスト)。`use eml_core_ir::{...}` に `Arm` を足し、ファイルの先頭の補助関数の後に次の関数を足す。

```rust
fn arm(tag: u32, fields: &[u32], body: u32) -> Arm {
    Arm {
        tag,
        fields: fields.iter().map(|&field| VarId(field)).collect(),
        body: CExprId(body),
    }
}
```

`pick` の `Switch` の枝を `Arm` にする (種類3。期待値は変わらない)。

```rust
    exprs.push(CExpr::Switch {
        scrutinee: var(0),
        arms: vec![arm(0, &[], 3), arm(1, &[], then_arm)],
    });
```

ファイルの末尾に足す。どれも `f d = switch d { #0 -> …; #1(x) -> … }` の形で、`d` は引数、`x` は `#1` の枝のフィールドである。

```rust
#[test]
fn a_switch_that_binds_fields_is_accepted() {
    let exprs = vec![
        CExpr::Return(var(1)),
        CExpr::Return(Atom::Unit),
        CExpr::Switch {
            scrutinee: var(0),
            arms: vec![arm(0, &[], 1), arm(1, &[1], 0)],
        },
    ];
    let f = function("f", 1, vec![boxed("d"), boxed("x")], exprs, &[]);
    assert_eq!(check(vec![f]), Ok(()));
}

#[test]
fn an_unused_field_must_be_released() {
    let unused = |release: bool| {
        let mut exprs = vec![CExpr::Return(Atom::Unit), CExpr::Return(Atom::Unit)];
        let some = if release {
            exprs.push(CExpr::Decref {
                var: VarId(1),
                body: CExprId(0),
            });
            2
        } else {
            0
        };
        exprs.push(CExpr::Switch {
            scrutinee: var(0),
            arms: vec![arm(0, &[], 1), arm(1, &[1], some)],
        });
        function("f", 1, vec![boxed("d"), boxed("x")], exprs, &[])
    };
    assert_eq!(check(vec![unused(true)]), Ok(()));
    assert_eq!(
        check(vec![unused(false)]),
        Err("`x1` is still owned at the end of the function in `f`".to_string())
    );
}

/// 両方の枝が scrutinee の `d` を返す。`Switch` は `d` を move で受け取るので、枝で使うには前で複製する。
fn keep_scrutinee(dup: bool) -> CoreFn {
    let mut exprs = vec![
        CExpr::Return(var(0)),
        CExpr::Decref {
            var: VarId(1),
            body: CExprId(0),
        },
        CExpr::Return(var(0)),
        CExpr::Switch {
            scrutinee: var(0),
            arms: vec![arm(0, &[], 2), arm(1, &[1], 1)],
        },
    ];
    if dup {
        exprs.push(CExpr::Dup {
            var: VarId(0),
            body: CExprId(3),
        });
    }
    function("f", 1, vec![boxed("d"), boxed("x")], exprs, &[])
}

#[test]
fn a_scrutinee_used_in_an_arm_is_duplicated_before_the_switch() {
    assert_eq!(check(vec![keep_scrutinee(true)]), Ok(()));
    assert_eq!(
        check(vec![keep_scrutinee(false)]),
        Err("`d0` is used after it was moved in `f`".to_string())
    );
}

#[test]
fn a_field_is_in_scope_only_in_its_arm() {
    let exprs = vec![
        CExpr::Return(var(1)),
        CExpr::Return(var(1)),
        CExpr::Switch {
            scrutinee: var(0),
            arms: vec![arm(0, &[], 1), arm(1, &[1], 0)],
        },
    ];
    let f = function("f", 1, vec![boxed("d"), boxed("x")], exprs, &[]);
    assert_eq!(
        check_scopes(vec![f]),
        Err("`x1` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn a_switch_on_an_unboxed_variable_cannot_bind_fields() {
    let exprs = vec![
        CExpr::Return(var(1)),
        CExpr::Return(Atom::Unit),
        CExpr::Switch {
            scrutinee: var(0),
            arms: vec![arm(0, &[], 1), arm(1, &[1], 0)],
        },
    ];
    let f = function("f", 1, vec![unboxed("d"), boxed("x")], exprs, &[]);
    assert_eq!(
        check_scopes(vec![f]),
        Err("`d0` is not boxed, but a switch binds its fields in `f`".to_string())
    );
}
```

`crates/eml_interp/tests/data.rs` を作る。

```rust
//! 手で組んだ Core IR で、`data` の値の確保と、`Switch` の枝による分解を確かめる (docs/spec/core-ir.md)。共有された
//! 値の分解や、boxed な変数に入った引数のないコンストラクタは、ソースの `match` からは狙って作りにくいので、ここで
//! 組む。

use eml_core_ir::{Arm, Atom, CExpr, CExprId, CoreFn, IoOp, Rhs, VarId, VarInfo};
use eml_interp::{Fault, RuntimeError};
use eml_test_support::execute;
use eml_test_support::ir::{boxed, program, unboxed, var};

/// 引数のない入口の関数。`exprs` は子を親より先に並べ、最後の式を本体にする。
fn main_fn(vars: Vec<VarInfo>, exprs: Vec<CExpr>) -> CoreFn {
    CoreFn {
        name: "main".to_string(),
        params: Vec::new(),
        vars,
        body: CExprId(exprs.len() as u32 - 1),
        exprs,
        joins: Vec::new(),
    }
}

fn bind(var: u32, rhs: Rhs, body: u32) -> CExpr {
    CExpr::Let {
        var: VarId(var),
        rhs,
        body: CExprId(body),
    }
}

fn arm(tag: u32, fields: &[u32], body: u32) -> Arm {
    Arm {
        tag,
        fields: fields.iter().map(|&field| VarId(field)).collect(),
        body: CExprId(body),
    }
}

/// `let d = con #1(s)` を作り、`#1(x)` の枝で `x` を出力する。`shared` なら `Switch` の前で `d` を複製し、両方の
/// 枝で `d` を捨てる。
fn unpack(shared: bool) -> CoreFn {
    let vars = vec![boxed("s"), boxed("d"), boxed("x"), unboxed("o")];
    let exprs = if shared {
        vec![
            CExpr::Return(var(3)),
            CExpr::Decref {
                var: VarId(1),
                body: CExprId(0),
            },
            bind(3, Rhs::Io(IoOp::Println, vec![var(2)]), 1),
            CExpr::Return(Atom::Unit),
            CExpr::Decref {
                var: VarId(1),
                body: CExprId(3),
            },
            CExpr::Switch {
                scrutinee: var(1),
                arms: vec![arm(0, &[], 4), arm(1, &[2], 2)],
            },
            CExpr::Dup {
                var: VarId(1),
                body: CExprId(5),
            },
            bind(1, Rhs::Con { tag: 1, args: vec![var(0)] }, 6),
            bind(0, Rhs::ConstString(0), 7),
        ]
    } else {
        vec![
            CExpr::Return(var(3)),
            bind(3, Rhs::Io(IoOp::Println, vec![var(2)]), 0),
            CExpr::Return(Atom::Unit),
            CExpr::Switch {
                scrutinee: var(1),
                arms: vec![arm(0, &[], 2), arm(1, &[2], 1)],
            },
            bind(1, Rhs::Con { tag: 1, args: vec![var(0)] }, 3),
            bind(0, Rhs::ConstString(0), 4),
        ]
    };
    main_fn(vars, exprs)
}

#[test]
fn a_unique_value_is_unpacked_by_taking_its_fields() {
    let (stdout, result) = execute(program(vec![unpack(false)], 0, &["field"]), true);
    assert_eq!((stdout.as_str(), result), ("field\n", Ok(())));
}

#[test]
fn a_shared_value_is_unpacked_by_copying_its_fields() {
    let (stdout, result) = execute(program(vec![unpack(true)], 0, &["field"]), true);
    assert_eq!((stdout.as_str(), result), ("field\n", Ok(())));
}

#[test]
fn a_boxed_variable_holding_a_tag_takes_its_arm() {
    // 引数のないコンストラクタの値は、引数を持つコンストラクタのある型の変数 (boxed) にも即値で入る
    let vars = vec![boxed("d"), boxed("x"), boxed("s"), unboxed("o")];
    let exprs = vec![
        CExpr::Return(var(3)),
        bind(3, Rhs::Io(IoOp::Println, vec![var(2)]), 0),
        bind(2, Rhs::ConstString(0), 1),
        CExpr::Return(Atom::Unit),
        CExpr::Decref {
            var: VarId(1),
            body: CExprId(3),
        },
        CExpr::Switch {
            scrutinee: var(0),
            arms: vec![arm(0, &[], 2), arm(1, &[1], 4)],
        },
        bind(0, Rhs::Atom(Atom::Tag(0)), 5),
    ];
    let (stdout, result) = execute(program(vec![main_fn(vars, exprs)], 0, &["none"]), true);
    assert_eq!((stdout.as_str(), result), ("none\n", Ok(())));
}

#[test]
fn an_arm_with_a_different_number_of_fields_is_an_internal_error() {
    let vars = vec![boxed("s"), boxed("d"), boxed("x"), boxed("y")];
    let exprs = vec![
        CExpr::Return(Atom::Unit),
        CExpr::Return(Atom::Unit),
        CExpr::Switch {
            scrutinee: var(1),
            arms: vec![arm(0, &[], 0), arm(1, &[2, 3], 1)],
        },
        bind(1, Rhs::Con { tag: 1, args: vec![var(0)] }, 2),
        bind(0, Rhs::ConstString(0), 3),
    ];
    let (_, result) = execute(program(vec![main_fn(vars, exprs)], 0, &["field"]), true);
    assert_eq!(
        result,
        Err(RuntimeError::Fault {
            fault: Fault::Internal(
                "a switch arm binds a different number of fields than the value has"
            ),
            function: "main".to_string(),
        })
    );
}
```

- [ ] **Step 6: テストが失敗することを確かめる**

Run: `cargo test -p eml_core_ir`
Expected: コンパイルエラー (`eml_core_ir` に `Arm` がなく、`Rhs` に `Con` がない)

- [ ] **Step 7: Core IR の命令を変える**

`crates/eml_core_ir/src/lib.rs` の `CExpr::Switch` を次のようにし、`Arm` を足す。`Switch` の doc コメントは置き換える。

```rust
    /// タグで分岐する。`if` もここに変換する。scrutinee は move で受け取り、引数を持つコンストラクタの値なら分解して
    /// 枝のフィールドに入れる (docs/spec/core-ir.md)。
    Switch {
        scrutinee: Atom,
        arms: Vec<Arm>,
    },
```

```rust
/// `Switch` の枝。引数を持つコンストラクタの枝は、すべてのフィールドを順に束縛する。引数のないコンストラクタの枝の
/// `fields` は空である。枝は、フィールドの参照を1つずつ所有して始まる (docs/spec/core-ir.md)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arm {
    pub tag: u32,
    pub fields: Vec<VarId>,
    pub body: CExprId,
}
```

`Rhs` に `Con` を足す (`Drop` の前)。

```rust
    /// 引数を持つコンストラクタの値を作る。`args` の所有権は値に移る。引数のないコンストラクタの値は `Atom::Tag` で
    /// ある (docs/spec/core-ir.md)。
    Con { tag: u32, args: Vec<Atom> },
```

`Rhs::atoms` と `Rhs::atoms_mut` の、引数の並びを返す腕に `Con` を足す。

```rust
            Rhs::MakeClosure(_, args)
            | Rhs::Prim(_, args)
            | Rhs::Io(_, args)
            | Rhs::Con { args, .. } => args.clone(),
```

```rust
            Rhs::MakeClosure(_, args)
            | Rhs::Prim(_, args)
            | Rhs::Io(_, args)
            | Rhs::Con { args, .. } => args.iter_mut().collect(),
```

- [ ] **Step 8: 変換でコンストラクタの値を作る**

`crates/eml_core_ir/src/translate/types.rs` の boxed の判定と `var_info` を、HIR のコンストラクタを引く形にする。`use` は `eml_hir::{Module, TypeDefId, TypeDefKind}` にする (`LangItems` は使わなくなる)。

```rust
/// ヒープに置く値の型。`Unr` でボックス化した変数が RC の対象になる。関数値と型変数の値は、ヒープのクロージャや
/// 文字列かもしれない。インタプリタの `dup` / `decref` はヒープにない値を無視するので、多めに対象にしても正しく動く
/// (docs/spec/core-ir.md)。
fn boxed(ty: &Type, module: &Module) -> bool {
    match ty {
        Type::Con { id, .. } => *id == module.lang.string || has_fields(module, *id),
        Type::Fn { .. } | Type::Cont { .. } | Type::Rigid(_) | Type::Flexible => true,
        Type::Record(_) | Type::Error => false,
    }
}

/// 引数を持つコンストラクタが1つでもある `data` の値は、ヒープの箱かもしれない。同じ型の引数のないコンストラクタの
/// 値は即値のタグで同じ変数に入るが、`dup` と `decref` はそれを無視する (docs/spec/core-ir.md の boxed の判定)。
fn has_fields(module: &Module, id: TypeDefId) -> bool {
    match &module.types[id].kind {
        TypeDefKind::Data { constructors } => constructors
            .iter()
            .any(|&ctor| !module.constructors[ctor].fields.is_empty()),
        TypeDefKind::Builtin => false,
    }
}

pub(super) fn var_info(name: &str, ty: &Type, module: &Module) -> VarInfo {
    VarInfo {
        name: name.to_string(),
        linearity: Linearity::Unr,
        boxed: boxed(ty, module),
    }
}
```

`crates/eml_core_ir/src/translate/program.rs` を直す。

- `ProgramBuilder` から `lang: LangItems` を除き、次の2つのフィールドを足す。`use eml_hir::{...}` から `LangItems` を除き、`ConstructorId` を足す。

  ```rust
      /// コンストラクタのスキームの型。コンストラクタを包む関数の変数が boxed かどうかを決める。
      constructor_types: HashMap<ConstructorId, Type>,
      constructor_wrappers: HashMap<ConstructorId, FnIdx>,
  ```

- `ProgramBuilder::new` で `lang: module.lang,` を除き、次を足す。

  ```rust
              constructor_types: typed
                  .constructors
                  .iter()
                  .map(|(id, scheme)| (id, scheme.ty.clone()))
                  .collect(),
              constructor_wrappers: HashMap::new(),
  ```

- `operation_wrapper` の `var_info("p", ty, &self.lang)` を `var_info("p", ty, module)` にする。
- `entry` を `pub(super) fn entry(&mut self, module: &Module, main: FnIdx, main_type: &Type) -> FnIdx` にし、`var_info("f", main_type, &self.lang)` を `var_info("f", main_type, module)` にする。
- `wrapper` を `pub(super) fn wrapper(&mut self, module: &Module, builtin: Builtin) -> FnIdx` にし、`let lang = self.lang;` を除いて、`var_info("p", ty, &lang)` と `var_info("t", ty, &lang)` を `var_info("p", ty, module)` と `var_info("t", ty, module)` にする。
- `operation_wrapper` の後に `constructor_wrapper` を足す。

  ```rust
      /// コンストラクタを値や部分適用で使うときに、値を作って返すだけの関数を作る。コンストラクタごとに1つだけ作る。
      pub(super) fn constructor_wrapper(&mut self, module: &Module, ctor: ConstructorId) -> FnIdx {
          if let Some(&function) = self.constructor_wrappers.get(&ctor) {
              return function;
          }
          let constructor = &module.constructors[ctor];
          let arity = constructor.fields.len();
          let ty = self
              .constructor_types
              .get(&ctor)
              .expect("every constructor has a scheme");
          let (param_types, result_type) = split_arrows(ty, arity);
          let mut vars: Vec<VarInfo> = param_types
              .iter()
              .map(|ty| var_info("p", ty, module))
              .collect();
          vars.push(var_info("d", &result_type, module));
          let function = self.reserve(arity);
          self.constructor_wrappers.insert(ctor, function);
          let params: Vec<VarId> = (0..arity as u32).map(VarId).collect();
          let result = VarId(arity as u32);
          let core = CoreFn {
              name: format!("con${}", constructor.name),
              params: params.clone(),
              vars,
              body: CExprId(1),
              exprs: vec![
                  CExpr::Return(Atom::Var(result)),
                  CExpr::Let {
                      var: result,
                      rhs: Rhs::Con {
                          tag: constructor.tag,
                          args: params.into_iter().map(Atom::Var).collect(),
                      },
                      body: CExprId(0),
                  },
              ],
              joins: Vec::new(),
          };
          self.finish(function, core);
          function
      }
  ```

`crates/eml_core_ir/src/translate/mod.rs` を直す。

- `builder.entry(indices[main], main_type)` を `builder.entry(module, indices[main], main_type)` にする。
- `new_var` の `var_info(name, ty, &self.module.lang)` を `var_info(name, ty, self.module)` にする。
- `tail_expr` の `if` の `Switch` を `Arm` にする。`use crate::{...}` に `Arm` を足す。

  ```rust
                  CExpr::Switch {
                      scrutinee,
                      arms: vec![
                          Arm {
                              tag: FALSE,
                              fields: Vec::new(),
                              body: else_code,
                          },
                          Arm {
                              tag: TRUE,
                              fields: Vec::new(),
                              body: then_code,
                          },
                      ],
                  }
  ```

`crates/eml_core_ir/src/translate/expr.rs` を直す。`use eml_hir::{...}` に `ConstructorId` を足す。

- `self.program.wrapper(builtin)` と `self.program.wrapper(*builtin)` (`call_builtin` と `atom` の2か所) を、`self.program.wrapper(self.module, builtin)` と `self.program.wrapper(self.module, *builtin)` にする。
- `call_operation` の後に `call_constructor` を足す。

  ```rust
      /// 引数がフィールドの数にそろえば値を作り、足りなければコンストラクタを包む関数のクロージャにする。コンストラクタの
      /// 結果は `data` の値で関数ではないので、型検査を通った呼び出しで引数が余ることはない。
      fn call_constructor(
          &mut self,
          ctor: ConstructorId,
          args: Vec<Atom>,
          ty: &Type,
          out: &mut Bindings,
      ) -> Atom {
          let module = self.module;
          let constructor = &module.constructors[ctor];
          if args.len() < constructor.fields.len() {
              let wrapper = self.program.constructor_wrapper(module, ctor);
              return self.bind(out, "c", ty, Rhs::MakeClosure(wrapper, args));
          }
          self.bind(
              out,
              "d",
              ty,
              Rhs::Con {
                  tag: constructor.tag,
                  args,
              },
          )
      }
  ```

- `atom` の `ExprKind::Path(Res::Constructor(..))` の腕 (Task 3 で引数のないコンストラクタを `Atom::Tag` にした腕) を、次の形に置き換える。

  ```rust
              ExprKind::Path(Res::Constructor(ctor)) => {
                  let constructor = &self.module.constructors[*ctor];
                  if constructor.fields.is_empty() {
                      Atom::Tag(constructor.tag)
                  } else {
                      let wrapper = self.program.constructor_wrapper(self.module, *ctor);
                      let ty = self.ty(id);
                      self.bind(out, "c", &ty, Rhs::MakeClosure(wrapper, Vec::new()))
                  }
              }
  ```

- `atom` の `ExprKind::Call` の、呼ばれる式による場合分けに、`ExprKind::Path(Res::Operation(op))` の腕の後で次の腕を足す。引数のないコンストラクタは呼べない (型検査が報告する) ので、ここに来るのは引数を持つコンストラクタだけである。

  ```rust
                      ExprKind::Path(Res::Constructor(ctor)) => {
                          let args = self.call_args(args, first, out);
                          self.call_constructor(*ctor, args, &ty, out)
                      }
  ```

- [ ] **Step 9: `simplify` を `Arm` に合わせる**

`crates/eml_core_ir/src/simplify.rs` を直す。`use crate::{...}` に `Arm` を足す。

`children` と `replace_child` の `Switch` の腕を、枝の本体を返す形にする。

```rust
        CExpr::Switch { arms, .. } => arms.iter().map(|arm| arm.body).collect(),
```

```rust
        CExpr::Switch { arms, .. } => arms.iter_mut().map(|arm| &mut arm.body).collect(),
```

B2 (`split_known_tags`) は、フィールドを束縛する枝を1つでも持つ `Switch` を対象にしない。`let CExpr::Switch { scrutinee: Atom::Var(scrutinee), arms } = self.expr(body).clone() else { continue; };` の直後に足す。

```rust
            // フィールドを束縛する枝の中では、引数をタグの定数に置き換えられない。引数を持つコンストラクタの値は、
            // タグだけでは決まらないためである (docs/spec/core-ir.md の `simplify`)
            if arms.iter().any(|arm| !arm.fields.is_empty()) {
                continue;
            }
```

同じ関数の `has_arm` を `arms.iter().any(|arm| arm.tag == *tag)` にし、枝を切り出すループを `for arm in &arms { let (tag, arm) = (arm.tag, arm.body); … }` の形にする (ループの中身は変えない)。`dispatch` の各要素は、`(tag, jump)` の代わりに `Arm { tag, fields: Vec::new(), body: jump }` にする。`arm_joins` の要素の形は Task 1 のままにする。

- [ ] **Step 10: 生存解析で枝のフィールドを扱う**

`crates/eml_core_ir/src/liveness.rs` の `BlockLiveness::at_end` の `Switch` の腕を次にする。

```rust
            CExpr::Switch { scrutinee, arms } => {
                let mut vars: Vars = var_of(scrutinee).into_iter().collect();
                for arm in arms {
                    // フィールドは枝の入口で束縛するので、`Switch` の前では生きていない
                    vars.extend(
                        self.entry(arm.body)
                            .iter()
                            .copied()
                            .filter(|var| !arm.fields.contains(var)),
                    );
                }
                vars
            }
```

`analyze` の `CExpr::Switch { arms, .. }` の腕を次にする。

```rust
                    CExpr::Switch { arms, .. } => {
                        work.extend(arms.iter().map(|arm| Step::Visit(arm.body, Start::Block)));
                    }
```

- [ ] **Step 11: Perceus で scrutinee を move にし、枝にフィールドを所有させる**

`crates/eml_core_ir/src/perceus.rs` の `use crate::{...}` に `Arm` を足し、`transform` の `CExpr::Switch` の腕を次に置き換える。

```rust
                CExpr::Switch { scrutinee, arms } => {
                    let live = self.live.at_end(expr);
                    let mut owned = self.owned(&segment, &live);
                    // `Switch` は scrutinee を1回使う (move)。枝の中でも使うなら、`Switch` の前で複製し、枝はその分を
                    // 所有して始まる。どの枝も使わなければ、枝は scrutinee を所有しない (docs/spec/core-ir.md)
                    let consumed = self.atom_var(scrutinee);
                    let kept = consumed.filter(|var| {
                        arms.iter()
                            .any(|arm| self.live.entry(arm.body).contains(var))
                    });
                    if let (Some(var), None) = (consumed, kept) {
                        owned.remove(&var);
                    }
                    let arms = arms
                        .iter()
                        .map(|arm| {
                            // 枝はフィールドの参照を1つずつ所有して始まる。使わないフィールドは、連鎖の始まりの
                            // 所有として、最初の段で捨てる
                            let mut owned = owned.clone();
                            owned.extend(
                                arm.fields
                                    .iter()
                                    .copied()
                                    .filter(|var| self.tracked[var.0 as usize]),
                            );
                            Arm {
                                tag: arm.tag,
                                fields: arm.fields.clone(),
                                body: self.transform(arm.body, &owned),
                            }
                        })
                        .collect();
                    let mut code = self.push(CExpr::Switch {
                        scrutinee: *scrutinee,
                        arms,
                    });
                    if let Some(var) = kept {
                        code = self.push(CExpr::Dup { var, body: code });
                    }
                    break (code, live);
                }
```

`Switch` の前で生きている変数 (`live`) は、`at_end` が scrutinee と、各枝の入口でフィールドを除いて生きている変数を足したものなので、`owned` は scrutinee が RC の対象ならそれを含む。

- [ ] **Step 12: verifier で枝のフィールドを束縛として扱う**

`crates/eml_core_ir/src/verify.rs` を直す。`use crate::{...}` に `Arm` は要らない (`arm.fields` と `arm.body` を読むだけ)。

`check_branch` に、枝の入口で束縛する変数を受けさせる。

```rust
    /// 枝や範囲を確かめ、その中での範囲の変更を巻き戻す。`bindings` は入口で束縛する変数 (`Switch` の枝のフィールド)
    /// で、範囲はその枝の中だけである。
    fn check_branch(
        &mut self,
        id: CExprId,
        mut state: State,
        bindings: &[VarId],
    ) -> Result<(), String> {
        let mark = self.scope_log.len();
        let epoch = self.epoch;
        for &var in bindings {
            self.bind(&mut state, var)?;
        }
        self.check(id, state)?;
        for (var, stamp) in self.scope_log.drain(mark..).rev() {
            self.stamps[var.0 as usize] = stamp;
        }
        self.epoch = epoch;
        Ok(())
    }
```

`Join` の範囲を確かめる呼び出しは `self.check_branch(*scope, scope_state, &[])?;` にする。`check` の `CExpr::Switch` の腕を次にする。

```rust
                CExpr::Switch { scrutinee, arms } => {
                    if arms.iter().any(|arm| !arm.fields.is_empty()) {
                        self.fields_allowed(scrutinee)?;
                    }
                    self.consume(&mut state, scrutinee)?;
                    let mut tags = HashSet::new();
                    for arm in arms {
                        if !tags.insert(arm.tag) {
                            return Err(format!("a switch has two arms for tag {}", arm.tag));
                        }
                        self.check_branch(arm.body, state.clone(), &arm.fields)?;
                    }
                    return Ok(());
                }
```

`consume` の後に足す。

```rust
    /// フィールドを持つ値は boxed な変数に入る (docs/spec/core-ir.md の boxed の判定)。定数の scrutinee は、B2 が
    /// 引数をタグに置き換えた枝の中にできるので、フィールドを束縛する枝があってもよい。その枝には入らない。
    fn fields_allowed(&self, scrutinee: &Atom) -> Result<(), String> {
        match *scrutinee {
            Atom::Var(var) if !self.function.vars[var.0 as usize].boxed => Err(format!(
                "`{}` is not boxed, but a switch binds its fields",
                self.name(var)
            )),
            _ => Ok(()),
        }
    }
```

- [ ] **Step 13: 表示を直す**

`crates/eml_core_ir/src/pretty.rs` の `Switch` の腕を次にする。

```rust
            CExpr::Switch { scrutinee, arms } => {
                writeln!(out, "{pad}switch {} {{", atom(function, scrutinee)).unwrap();
                for arm in arms {
                    if arm.fields.is_empty() {
                        writeln!(out, "{pad}  #{} ->", arm.tag).unwrap();
                    } else {
                        let fields: Vec<String> =
                            arm.fields.iter().map(|&v| var(function, v)).collect();
                        writeln!(out, "{pad}  #{}({}) ->", arm.tag, fields.join(", ")).unwrap();
                    }
                    expr(program, function, arm.body, indent + 2, out);
                }
                writeln!(out, "{pad}}}").unwrap();
                return;
            }
```

`rhs_text` の `match` に足す (`Rhs::Drop` の前)。

```rust
        Rhs::Con { tag, args: a } => format!("con #{tag}({})", args(a)),
```

- [ ] **Step 14: インタプリタで値を作り、`Switch` で分解する**

`crates/eml_interp/src/lib.rs` の `step` の `CExpr::Switch` の腕を次にする。

```rust
            CExpr::Switch { scrutinee, arms } => {
                // scrutinee は move で受け取る。共有された値は `take_or_copy` がフィールドを複製して箱を手放すので、
                // どちらの場合も枝はフィールドの参照を1つずつ所有して始まる (docs/spec/core-ir.md)
                let (tag, fields) = match self.atom(scrutinee)? {
                    Value::Tag(tag) => (tag, Vec::new()),
                    Value::Obj(obj) => match self.heap.take_or_copy(obj).map_err(Fault::Heap)? {
                        Payload::Data { tag, fields } => (tag, fields),
                        _ => return Err(Fault::Internal("a switch on an object that is not data")),
                    },
                    _ => return Err(Fault::Internal("a switch on a value that is not a tag")),
                };
                let arm = arms
                    .iter()
                    .find(|arm| arm.tag == tag)
                    .ok_or(Fault::Internal("a switch without a matching arm"))?;
                if arm.fields.len() != fields.len() {
                    return Err(Fault::Internal(
                        "a switch arm binds a different number of fields than the value has",
                    ));
                }
                for (&field, value) in arm.fields.iter().zip(fields) {
                    self.slots[field.0 as usize] = Some(value);
                }
                self.control = arm.body;
            }
```

`bind` の `match rhs` に足す (`Rhs::MakeClosure` の腕の後)。

```rust
            Rhs::Con { tag, args } => {
                let fields = self.atoms(args)?;
                Value::Obj(self.heap.alloc(Payload::Data { tag: *tag, fields }))
            }
```

- [ ] **Step 15: テストを通す**

Run: `cargo test -p eml_runtime -p eml_core_ir -p eml_interp`
Expected: PASS。`constructors_with_fields_build_values`、`a_constructor_used_as_a_function_value_is_wrapped`、`constructor_arguments_are_owned_by_the_value` のスナップショットは、上に書いた形になる。既存の Core IR のスナップショットは、`if` の枝の表示が `#0 ->` / `#1 ->` のままなので変わらない。変わったら止まる。

- [ ] **Step 16: UI テストを足す**

`tests/ui/run/data/constructors.em` を作る。`match` はまだないので、値を作り、渡し、クロージャに捕まえ、使わずに捨てる。`debug_heap` が、箱とフィールドがすべて解放されることを確かめる。

```haskell
-- Values of constructors with fields live on the heap. They are passed around, captured by
-- closures, built through a partially applied constructor, and freed when no longer used.
data Option a =
  | None
  | Some a

data Pair a b =
  | Pair a b

keep : Option String -> Option String -> Option String
keep a b = a

label : Pair String (Option String) -> String
label p = "pair"

main : Unit -> <IO> Unit
main () =
  let some = Some "kept"
  let chosen = keep some None
  let held = fn u -> keep some u
  let picked = held (Some "other")
  let make = Pair "left"
  let p = make (Some "right")
  println (label p)
  let nested = Some (Some "nested")
  println "done"
```

期待する stdout:

```
pair
done
```

Run: `cargo test -p eml_cli --test ui`
Expected: `ui::run` が新しいスナップショットで失敗する。`cargo insta review` で、stdout が上と一致し、stderr が空であることを確かめて承認し、もう一度流して通す。

- [ ] **Step 17: 全体を通す**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`
Expected: すべて PASS。clippy の警告がない。

- [ ] **Step 18: コミットする**

```bash
git add -A
git commit -m "Add data objects and switch arms that bind constructor fields

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014YGQm6f7Mtged1yVY4dmJG"
```

### Task 6: `match` を決定木にコンパイルし、B2 を引数のないタグに限る

`match` と、`let`・ラムダ・等式の引数のコンストラクタのパターンを、決定木にコンパイルする。各枝の本体は join point にし、決定木の葉からパターン変数を引数にして jump する。simplify の B2 は引数のないタグに限り、その前に B3 を1回回す (Interface notes の食い違い)。最後に `run/data/` の UI テストで、HIR から実行までを通す。

**Files:**
- Create: `crates/eml_core_ir/src/translate/pattern.rs`
- Modify: `crates/eml_core_ir/src/translate/mod.rs` (`Binding::Shared`、`new_join`、`tail_after`、`match` と `let` と引数のパターン)
- Modify: `crates/eml_core_ir/src/translate/expr.rs` (`ty` の公開範囲、末尾にない `match` の join point)
- Modify: `crates/eml_core_ir/src/translate/program.rs` (`constructor_type`)
- Modify: `crates/eml_core_ir/src/simplify.rs` (B2 の限定、最初の B3)
- Test: `crates/eml_core_ir/tests/translate.rs` (5件)、`crates/eml_core_ir/tests/simplify.rs` (case-of-case の6件)、`crates/eml_core_ir/tests/perceus.rs` (3件。Task 5 から受け取った2件を含む)
- Test: `tests/ui/run/data/` の10個の `.em` と、そのスナップショット

**Interfaces:**
- Consumes: Task 1 の `CExpr::Join { params }`、`CExpr::Jump { args }`、`Binding::Join { join, params, scope }`。Task 2 の `ExprKind::Match`、`MatchArm`、`PatKind::Con`、`TypeDefKind::Data`、`Module::constructors`。Task 3 の `TypedModule::constructors`、`Type::Con { args }`、`BodyTypes::pats`。Task 5 の `Arm`、`CExpr::Switch { arms: Vec<Arm> }`、`Rhs::Con`、`var_info(name, ty, module)`、コンストラクタの値の変換
- Produces: 外から見える新しい名前はない。`translate/` の中の名前は Interface notes のとおり

**Interface notes (骨組みの Interfaces から変えた点と、前後のタスクとの取り決め):**

- 骨組みの Interfaces にない名前を `translate/` の中に3つ足す。どれも `pub(super)` で、外からは見えない
  - `Binding::Shared { join: JoinId, params: Vec<VarId>, body: CExprId }`。本体を先に組み立て、ここより後ろの式を範囲にする join point。`match` の枝と、決定木の残りの部分木に使う。今の `Binding::Join` は本体が「後ろの式」なので、この向きには使えない
  - `FnLowering::tail_after(bindings, expr, exit)`。引数のパターンの分解を、本体の前に置くために使う。`tail` はこれを空の並びで呼ぶ
  - `ProgramBuilder::constructor_type(ctor) -> &Type`。Task 5 が `ProgramBuilder::new` で `TypedModule::constructors` から写した `constructor_types` を引く
- `FnLowering::ty` (`translate/expr.rs`) を `pub(super)` にする。`pattern.rs` が scrutinee の型を引くため
- Task 3 への前提: コンストラクタのスキームの型は `f1 -> … -> fn -> T a1 … ak` の形で、宣言の型引数を `Type::Rigid(名前)` (宣言の `Generics::type_vars` の名前) で表す。フィールドの型の置き換えは、この名前で引く
- Task 5 との約束: 引数のそろったコンストラクタの値は、名前 `d` の変数に束縛する (`let d3 = con #1(x1)`)。このタスクのスナップショットはこの名前で書いてある
- simplify の順の変更 (ユーザーが承認済み): spec の変換は「各枝の本体をつねに join point にする」。このとき、`let o = if …` の後の `match o` や `match (if …) with` では、`if` の join point の本体が「枝の join point の並び + `Switch`」になり、本体が `Switch` でないので B2 が働かない。そこで simplify の最初に B3 を1回だけ回し (順は B3、B2、B5、B3、B4)、`jump` が1つの枝の join point を `Switch` の枝に戻してから B2 を見る。今の変換が作る join point は末尾にない `if` のものだけで、`jump` がつねに2つ以上あるので、既存のプログラムの結果は変わらない (既存の simplify のテストの期待値も変わらない)
- Task 7 への申し送り: `docs/spec/core-ir.md` の「パス」の節の `simplify` の項は、今「B2、B5、B3、B4 の順で1巡だけ」と書いてある。これを「B3、B2、B5、B3、B4 の順で1巡だけ。最初の B3 は、`match` の枝の join point を `Switch` の枝に戻し、B2 が本体の `Switch` を見られるようにする」に直す。B2 の説明も「引数のないコンストラクタの枝だけを切り出す」に直す
- handler の節と `return` の節の引数: どちらも `FnLowering::lift` から `FnLowering::lower` の `params` を通るので、ラムダの引数と同じ経路でコンストラクタのパターンを分解する。Task 4 が E4003 で反駁可能なパターンを止めるので、ここに来るのは反駁不可能なパターン (`Box n`) だけである。`translate.rs` の `constructor_patterns_in_handler_clause_parameters` で確かめる

- [ ] **Step 1: 変換の失敗するテストを書く**

`crates/eml_core_ir/tests/translate.rs` の末尾に、次の4つのテストを足す。

```rust
#[test]
fn a_match_compiles_to_a_decision_tree_with_arm_join_points() {
    // 各枝の本体を join point にし、選んだ欄に現れないコンストラクタ (`None`) は残りの行列の join point へ jump する
    let text = "data Option a = | None | Some a\n\nf : Option (Option Int) -> Int\nf o = match o with\n  | Some (Some n) -> n\n  | _ -> 0\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r"
    fn f(o0) {
      join j0(n1) [] {
        return n1
      }
      join j1() [] {
        return 0
      }
      join j2() [] {
        jump j1()
      }
      switch o0 {
        #0 ->
          jump j2()
        #1(x2) ->
          join j3() [] {
            jump j1()
          }
          switch x2 {
            #0 ->
              jump j3()
            #1(n3) ->
              jump j0(n3)
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
fn tail_and_non_tail_matches() {
    // 末尾にない `match` は、`if` と同じく値を受ける join point の範囲に入り、枝の本体はその join point へ jump する
    let text = "data Option a = | None | Some a\n\ng : Option Int -> Int\ng o =\n  let n = match o with\n    | Some m -> m\n    | None -> 0\n  n + 1\n\nh : Option Int -> Int\nh o = match o with | Some m -> m | None -> 0\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r"
    fn g(o0) {
      join j0(t3) [] {
        let t4 = prim +(t3, 1)
        return t4
      }
      join j1(m1) [] {
        jump j0(m1)
      }
      join j2() [] {
        jump j0(0)
      }
      switch o0 {
        #0 ->
          jump j2()
        #1(m2) ->
          jump j1(m2)
      }
    }
    fn h(o0) {
      join j0(m1) [] {
        return m1
      }
      join j1() [] {
        return 0
      }
      switch o0 {
        #0 ->
          jump j1()
        #1(m2) ->
          jump j0(m2)
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
fn constructor_patterns_in_let_lambda_and_equation_parameters() {
    // コンストラクタを含むパターンは、続きを本体にする join point の引数で変数を受け、枝が1つの決定木で分解する
    let text = "data Box a = | Box a\n\nby_equation : Box Int -> Int\nby_equation (Box n) = n\n\nby_let : Box Int -> Int\nby_let b =\n  let Box m = b\n  m + 1\n\nby_lambda : Box Int -> Int\nby_lambda b = (fn (Box k) -> k) b\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r"
    fn by_equation(p0) {
      join j0(n1) [] {
        return n1
      }
      switch p0 {
        #0(n2) ->
          jump j0(n2)
      }
    }
    fn by_let(b0) {
      join j0(m1) [] {
        let t3 = prim +(m1, 1)
        return t3
      }
      switch b0 {
        #0(m2) ->
          jump j0(m2)
      }
    }
    fn by_lambda(b0) {
      let c1 = closure by_lambda$lambda0()
      tailcall apply c1(b0)
    }
    fn main(p0) {
      return ()
    }
    fn by_lambda$lambda0(p0) {
      join j0(k1) [] {
        return k1
      }
      switch p0 {
        #0(k2) ->
          jump j0(k2)
      }
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}

#[test]
fn a_variable_pattern_after_a_switch_binds_the_scrutinee() {
    // `ys` は `xs` の `Switch` の後で、`xs` そのものを受ける。残りの行列の join point は `xs` を捕まえる
    let text = "data List a = | Nil | Cons a (List a)\n\nsize : List Int -> Int\nsize xs = 2\n\ndescribe : List Int -> Int\ndescribe xs = match xs with\n  | Cons _ Nil -> 1\n  | ys -> size ys\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r"
    fn size(xs0) {
      return 2
    }
    fn describe(xs0) {
      join j0() [] {
        return 1
      }
      join j1(ys1) [] {
        tailcall size(ys1)
      }
      join j2() [xs0] {
        jump j1(xs0)
      }
      switch xs0 {
        #0 ->
          jump j2()
        #1(x2, x3) ->
          join j3() [xs0] {
            jump j1(xs0)
          }
          switch x3 {
            #0 ->
              jump j0()
            #1(x4, x5) ->
              jump j3()
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
fn constructor_patterns_in_handler_clause_parameters() {
    // 操作の節と `return` の節はラムダと同じく関数に持ち上げるので、引数のコンストラクタのパターンも同じ経路で分解する
    let text = "data Box a = | Box a\n\neffect Give where\n  give : Box Int -> Int\n\nrun : Unit -> Int\nrun () =\n  handle Box (give (Box 1)) with\n    | give (Box n) k -> resume k n\n    | return (Box r) -> r\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Translate), @r"
    fn run(p0) {
      let c1 = closure run$handle0()
      let c2 = closure run$handle0$give()
      let c3 = closure run$handle0$return()
      tailcall handle Give(c1) {give: c2} return c3
    }
    fn main(p0) {
      return ()
    }
    fn run$handle0(p0) {
      let d1 = con #0(1)
      let t2 = perform Give.give(d1)
      let d3 = con #0(t2)
      return d3
    }
    fn run$handle0$give(p0, k1) {
      join j0(n2) [k1] {
        tailcall resume k1(n2)
      }
      switch p0 {
        #0(n3) ->
          jump j0(n3)
      }
    }
    fn run$handle0$return(p0) {
      join j0(r1) [] {
        return r1
      }
      switch p0 {
        #0(r2) ->
          jump j0(r2)
      }
    }
    fn entry$main() {
      tailcall main(())
    }
    ");
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test translate`
Expected: 新しい5つのテストが FAIL する。`match` とコンストラクタのパターンは、Task 2 の仮の扱いのまま Core IR に変換できない (変換の中の panic か、スナップショットの不一致)。既存のテストは PASS する。

- [ ] **Step 3: `translate/pattern.rs` を作る**

`crates/eml_core_ir/src/translate/pattern.rs` を次の内容で作る。

```rust
//! `match` と、`let`・ラムダ・等式の引数のパターンを、決定木にコンパイルする (docs/spec/core-ir.md)。同じ値を二度
//! 調べないように、行列の欄ごとに `Switch` する。各枝の本体は join point にして決定木の葉から jump する。共有の有無は
//! 数えず、jump が1つの枝は simplify の B3 がその位置に戻す。

use eml_hir::{Body, ConstructorId, ExprId, LocalId, MatchArm, PatId, PatKind, TypeDefKind};
use eml_types::Type;

use crate::{Arm, Atom, CExpr, JoinId, VarId};

use super::types::split_arrows;
use super::{Binding, Bindings, Exit, FnLowering};

/// 行列の欄。特殊化で増えたワイルドカードは、HIR のパターンを持たない。
#[derive(Clone, Copy)]
enum Cell {
    Pat(PatId),
    Any,
}

/// 欄の先頭の形。変数は、その欄の出現を束縛するワイルドカードである。
enum Head<'a> {
    Any(Option<LocalId>),
    Con(ConstructorId, &'a [PatId]),
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
            // `()` は値が1つしかないので、ワイルドカードと同じに扱える
            PatKind::Wildcard | PatKind::Unit | PatKind::Missing => return Head::Any(None),
        }
    }
}

/// パターンがコンストラクタを含むか。含まなければ、値をそのまま局所変数に対応させればよく、決定木は要らない。
pub(super) fn has_constructor(body: &Body, pat: PatId) -> bool {
    match &body.pats[pat].kind {
        PatKind::Con { .. } => true,
        PatKind::Annot { pat, .. } => has_constructor(body, *pat),
        PatKind::Bind(_) | PatKind::Wildcard | PatKind::Unit | PatKind::Missing => false,
    }
}

#[derive(Clone)]
struct Row {
    cells: Vec<Cell>,
    /// 葉で jump する先 (`Target`) の番号。
    target: usize,
    /// これまでに通った欄で、変数のパターンが受けた出現。
    bound: Vec<(LocalId, Atom)>,
}

impl Row {
    /// `column` の欄を `cells` で置き換える。左から深さ優先で欄を選ぶので、フィールドは元の欄の位置に並べる。
    fn replace(&self, column: usize, cells: impl IntoIterator<Item = Cell>) -> Row {
        let mut row = self.clone();
        row.cells.splice(column..=column, cells);
        row
    }
}

/// 葉の jump の行き先。`locals` は join point の引数の順 (`Body::pat_bindings` の順) である。
struct Target {
    join: JoinId,
    locals: Vec<LocalId>,
}

/// 調べる値と、その型。型は、フィールドの変数が boxed かどうかを決めるのに使う。
#[derive(Clone)]
struct Occurrence {
    atom: Atom,
    ty: Type,
}

impl FnLowering<'_> {
    /// `match` の値を `exit` に渡す最後の命令を返す。各枝の本体は join point にして、決定木の外側に置く。そのため、
    /// どの葉からも届く。
    pub(super) fn lower_match(
        &mut self,
        scrutinee: ExprId,
        arms: &[MatchArm],
        exit: Exit,
        out: &mut Bindings,
    ) -> CExpr {
        let value = self.atom(scrutinee, out);
        let ty = self.ty(scrutinee);
        let mut targets = Vec::new();
        for arm in arms {
            let join = self.new_join();
            let (locals, params) = self.bind_params(arm.pat);
            let body = self.tail(arm.body, exit);
            out.push(Binding::Shared { join, params, body });
            targets.push(Target { join, locals });
        }
        let rows = arms
            .iter()
            .enumerate()
            .map(|(target, arm)| Row {
                cells: vec![Cell::Pat(arm.pat)],
                target,
                bound: Vec::new(),
            })
            .collect();
        self.decide(&[Occurrence { atom: value, ty }], rows, &targets, out)
    }

    /// コンストラクタを含む `let` と引数のパターン。続きの式を本体にする join point の引数で変数を受け、枝が1つの
    /// `match` と同じ決定木で値を分解する。jump は1つなので、simplify の B3 がその位置に戻す。
    pub(super) fn destructure(&mut self, pat: PatId, value: Atom, ty: Type, out: &mut Bindings) {
        let join = self.new_join();
        let (locals, params) = self.bind_params(pat);
        let rows = vec![Row {
            cells: vec![Cell::Pat(pat)],
            target: 0,
            bound: Vec::new(),
        }];
        let targets = [Target { join, locals }];
        let mut tree = Vec::new();
        let last = self.decide(&[Occurrence { atom: value, ty }], rows, &targets, &mut tree);
        let scope = self.seq(tree, last);
        out.push(Binding::Join {
            join,
            params,
            scope,
        });
    }

    /// パターンが束縛する変数ごとに join point の引数の変数を作り、局所変数をそれに対応させる。
    fn bind_params(&mut self, pat: PatId) -> (Vec<LocalId>, Vec<VarId>) {
        let body = self.body;
        let locals = body.pat_bindings(pat);
        let params = locals
            .iter()
            .map(|&local| {
                let ty = self
                    .types
                    .locals
                    .get(local)
                    .cloned()
                    .expect("every local is typed");
                let var = self.new_var(&body.locals[local].name, &ty);
                self.locals.insert(local, Atom::Var(var));
                var
            })
            .collect();
        (locals, params)
    }

    /// 行列から決定木を作る。最初の行がすべてワイルドカードなら葉にする。そうでなければ、最初の行でコンストラクタを
    /// 持ついちばん左の欄で `Switch` する。`Switch` の直前に置く join point (残りの行列) は `out` に積み、最後の命令を
    /// 返す。行列は網羅性の検査を通っているので、空にならない。
    fn decide(
        &mut self,
        occurrences: &[Occurrence],
        mut rows: Vec<Row>,
        targets: &[Target],
        out: &mut Bindings,
    ) -> CExpr {
        let body = self.body;
        let module = self.module;
        let first = rows.first().expect("type-checked patterns are exhaustive");
        let Some(column) = first
            .cells
            .iter()
            .position(|&cell| matches!(head(body, cell), Head::Con(..)))
        else {
            return leaf(body, occurrences, rows.swap_remove(0), targets);
        };
        let Head::Con(ctor, _) = head(body, first.cells[column]) else {
            unreachable!("the chosen column holds a constructor")
        };
        let occurrence = occurrences[column].clone();
        // 変数のパターンはこの欄の出現を受け、以後はワイルドカードとして扱う
        for row in &mut rows {
            if let Head::Any(Some(local)) = head(body, row.cells[column]) {
                row.bound.push((local, occurrence.atom));
            }
        }
        let TypeDefKind::Data { constructors } = &module.types[module.constructors[ctor].ty].kind
        else {
            unreachable!("constructor patterns belong to data types")
        };
        let mentions = |ctor: ConstructorId, row: &Row| {
            matches!(head(body, row.cells[column]), Head::Con(other, _) if other == ctor)
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
            let fields: Vec<VarId> = field_types
                .iter()
                .enumerate()
                .map(|(index, ty)| {
                    let name = field_name(body, &rows, column, ctor, index);
                    self.new_var(&name, ty)
                })
                .collect();
            let code = if rows.iter().any(|row| mentions(ctor, row)) {
                let mut specialized_occurrences = occurrences[..column].to_vec();
                specialized_occurrences.extend(fields.iter().zip(&field_types).map(
                    |(&var, ty)| Occurrence {
                        atom: Atom::Var(var),
                        ty: ty.clone(),
                    },
                ));
                specialized_occurrences.extend_from_slice(&occurrences[column + 1..]);
                let specialized: Vec<Row> = rows
                    .iter()
                    .filter_map(|row| match head(body, row.cells[column]) {
                        Head::Con(other, args) if other == ctor => {
                            Some(row.replace(column, args.iter().map(|&arg| Cell::Pat(arg))))
                        }
                        Head::Con(..) => None,
                        Head::Any(_) => Some(row.replace(column, fields.iter().map(|_| Cell::Any))),
                    })
                    .collect();
                let mut inner = Vec::new();
                let last = self.decide(&specialized_occurrences, specialized, targets, &mut inner);
                self.seq(inner, last)
            } else {
                let join = rest.expect("a constructor missing from the column goes to the rest");
                self.push(CExpr::Jump {
                    join,
                    args: Vec::new(),
                })
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

    /// コンストラクタのフィールドの型。スキームの型引数を、調べる値の型の引数で置き換える。値の型が型構成子の適用で
    /// なければ置き換えず、型変数のままにする。型変数の値は boxed として扱うので、多めに RC の対象になるだけで正しく
    /// 動く (docs/spec/core-ir.md)。
    fn field_types(&self, ctor: ConstructorId, ty: &Type) -> Vec<Type> {
        let constructor = &self.module.constructors[ctor];
        let (fields, _) = split_arrows(
            self.program.constructor_type(ctor),
            constructor.fields.len(),
        );
        let names: Vec<String> = self.module.types[constructor.ty]
            .generics
            .type_vars
            .iter()
            .map(|(_, var)| var.name.clone())
            .collect();
        match ty {
            Type::Con { args, .. } if args.len() == names.len() => fields
                .iter()
                .map(|field| substitute(field, &names, args))
                .collect(),
            _ => fields,
        }
    }
}

/// 葉。最初の行の残りの変数を束縛し、その行の枝の join point へ、引数の順に出現を渡す。
fn leaf(body: &Body, occurrences: &[Occurrence], row: Row, targets: &[Target]) -> CExpr {
    let mut bound = row.bound;
    for (&cell, occurrence) in row.cells.iter().zip(occurrences) {
        if let Head::Any(Some(local)) = head(body, cell) {
            bound.push((local, occurrence.atom));
        }
    }
    let target = &targets[row.target];
    let args = target
        .locals
        .iter()
        .map(|local| {
            bound
                .iter()
                .find(|(bound, _)| bound == local)
                .map(|&(_, atom)| atom)
                .expect("every pattern variable is bound on the way to its leaf")
        })
        .collect();
    CExpr::Jump {
        join: target.join,
        args,
    }
}

/// フィールドの変数の名前。その位置を変数のパターンで受ける行があれば、その変数の名前にする。
fn field_name(body: &Body, rows: &[Row], column: usize, ctor: ConstructorId, index: usize) -> String {
    rows.iter()
        .find_map(|row| match head(body, row.cells[column]) {
            Head::Con(other, args) if other == ctor => match head(body, Cell::Pat(args[index])) {
                Head::Any(Some(local)) => Some(body.locals[local].name.clone()),
                _ => None,
            },
            _ => None,
        })
        .unwrap_or_else(|| "x".to_string())
}

/// スキームの型の中の型引数 (`names`) を `args` で置き換える。関数型と継続は中身によらず boxed で、パターンで
/// 分解もしないので、中を置き換えなくてよい。
fn substitute(ty: &Type, names: &[String], args: &[Type]) -> Type {
    match ty {
        Type::Rigid(name) => names
            .iter()
            .position(|candidate| candidate == name)
            .map_or_else(|| ty.clone(), |index| args[index].clone()),
        Type::Con {
            id,
            name,
            args: inner,
        } => Type::Con {
            id: *id,
            name: name.clone(),
            args: inner
                .iter()
                .map(|arg| substitute(arg, names, args))
                .collect(),
        },
        Type::Record(fields) => Type::Record(
            fields
                .iter()
                .map(|(label, field)| (label.clone(), substitute(field, names, args)))
                .collect(),
        ),
        Type::Fn { .. } | Type::Cont { .. } | Type::Flexible | Type::Error => ty.clone(),
    }
}
```

- [ ] **Step 4: 変換の骨組みに `match` とパターンをつなぐ**

`crates/eml_core_ir/src/translate/mod.rs` の先頭の `mod` に `mod pattern;` を足し、`use` に `pattern::has_constructor` を足す。

```rust
mod expr;
mod pattern;
mod program;
mod types;
```

```rust
use pattern::has_constructor;
```

`Binding` に `Shared` を足す。

```rust
/// 値を計算する束縛と、join point の開始の並び。`seq` が後ろから組み立てる。
enum Binding {
    Let(VarId, Rhs),
    /// ここより後ろで組み立てる式を本体にし、`scope` (条件の計算と、枝が `Jump` する `Switch`) を範囲にする join point。
    Join {
        join: JoinId,
        params: Vec<VarId>,
        scope: CExprId,
    },
    /// 本体を先に組み立てた join point。ここより後ろで組み立てる式を範囲にする。`match` の枝と、決定木の残りの
    /// 部分木に使う (`pattern.rs`)。
    Shared {
        join: JoinId,
        params: Vec<VarId>,
        body: CExprId,
    },
}
```

`FnLowering::lower` の引数の扱いを、コンストラクタを含むパターンを分解する形にする。

```rust
        let mut destructured = Vec::new();
        for (pat, ty) in params {
            // コンストラクタを含むパターンは名前のない引数で受け、本体の前で分解する
            let simple = pat.filter(|&pat| !has_constructor(body, pat));
            let local = simple.and_then(|pat| body.pat_bindings(pat).first().copied());
            let name = local.map_or("p", |local| body.locals[local].name.as_str());
            let var = self.new_var(name, ty);
            if let Some(local) = local {
                self.locals.insert(local, Atom::Var(var));
            }
            vars.push(var);
            if let Some(pat) = pat.filter(|&pat| has_constructor(body, pat)) {
                destructured.push((pat, var, ty.clone()));
            }
        }
        // 引数の変数をすべて作ってから分解する。関数の引数の番号を、分解で作る変数より前にそろえるため
        let mut bindings = Vec::new();
        for (pat, var, ty) in destructured {
            self.destructure(pat, Atom::Var(var), ty, &mut bindings);
        }
        let root = self.tail_after(bindings, root, Exit::Return);
```

(`let root = self.tail(root, Exit::Return);` の行を、上の最後の行で置き換える。)

`seq` に `Shared` を足し、join point の番号を取る `new_join` を足す。`tail` は `tail_after` を空の並びで呼ぶ形にする。

```rust
    fn new_join(&mut self) -> JoinId {
        let join = JoinId(self.joins.len() as u32);
        self.joins.push(None);
        join
    }

    fn seq(&mut self, bindings: Bindings, last: CExpr) -> CExprId {
        let mut id = self.push(last);
        for binding in bindings.into_iter().rev() {
            id = match binding {
                Binding::Let(var, rhs) => self.push(CExpr::Let { var, rhs, body: id }),
                Binding::Join {
                    join,
                    params,
                    scope,
                } => {
                    let expr = self.push(CExpr::Join {
                        join,
                        params,
                        captures: Vec::new(),
                        body: id,
                        scope,
                    });
                    self.joins[join.0 as usize] = Some(expr);
                    expr
                }
                Binding::Shared { join, params, body } => {
                    let expr = self.push(CExpr::Join {
                        join,
                        params,
                        captures: Vec::new(),
                        body,
                        scope: id,
                    });
                    self.joins[join.0 as usize] = Some(expr);
                    expr
                }
            };
        }
        id
    }

    /// 式の値を `exit` に渡すコード。値を返すだけの呼び出しは、呼び出し元のフレームを積まない末尾呼び出しにする。
    fn tail(&mut self, expr: ExprId, exit: Exit) -> CExprId {
        self.tail_after(Vec::new(), expr, exit)
    }

    /// `bindings` (引数のパターンの分解) の後に、式の値を `exit` に渡すコードを続ける。
    fn tail_after(&mut self, mut bindings: Bindings, expr: ExprId, exit: Exit) -> CExprId {
        let mut last = self.tail_expr(expr, exit, &mut bindings);
        if let CExpr::Return(Atom::Var(returned)) = last
            && let Some(Binding::Let(bound, Rhs::Call { .. })) = bindings.last()
            && *bound == returned
        {
            let Some(Binding::Let(_, Rhs::Call { call, .. })) = bindings.pop() else {
                unreachable!("checked above");
            };
            // 結果の変数は呼び出しの直前に作ったものなので、表から除いて番号を詰める
            debug_assert_eq!(returned.0 as usize, self.vars.len() - 1);
            self.vars.pop();
            last = CExpr::TailCall(call);
        }
        self.seq(bindings, last)
    }
```

`tail_expr` の `match` に、`ExprKind::Match` の腕を `ExprKind::Block` の前に足す。

```rust
            ExprKind::Match { scrutinee, arms } => self.lower_match(*scrutinee, arms, exit, out),
```

`stmts` の `Stmt::Let` を、コンストラクタを含むパターンを分解する形にする。

```rust
                Stmt::Let { pat, init, .. } => {
                    let value = self.atom(*init, out);
                    if has_constructor(self.body, *pat) {
                        let ty = self.ty(*init);
                        self.destructure(*pat, value, ty, out);
                    } else {
                        self.bind_pat(*pat, value);
                    }
                }
```

`crates/eml_core_ir/src/translate/expr.rs` の `fn ty` を `pub(super) fn ty` にする。`atom` の `ExprKind::If { .. }` の腕を、末尾にない `match` も受ける形にする。

```rust
            ExprKind::If { .. } | ExprKind::Match { .. } => {
                // 続きの式を join point の本体にし、`if` と `match` の値をその引数で受ける。条件と scrutinee の計算も範囲に
                // 入れる。条件が末尾にない `if` のとき、その join point が外側の join point の範囲の中にでき、枝から外側へ
                // jump できる (docs/spec/core-ir.md)
                let join = self.new_join();
                let scope = self.tail(id, Exit::Jump(join));
                let ty = self.ty(id);
                let param = self.new_var("t", &ty);
                out.push(Binding::Join {
                    join,
                    params: vec![param],
                    scope,
                });
                Atom::Var(param)
            }
```

`ProgramBuilder` は、Task 5 で足した `constructor_types` (コンストラクタのスキームの型) をすでに持つ。決定木がフィールドの変数の型を決めるのに、この表を引く口だけを `impl ProgramBuilder` に足す。

```rust
    pub(super) fn constructor_type(&self, ctor: ConstructorId) -> &Type {
        self.constructor_types
            .get(&ctor)
            .expect("every constructor has a scheme")
    }
```

Task 2 で `translate/` に入れた `ExprKind::Match` と `PatKind::Con` の仮の扱いが残っていれば、ここで消す。

- [ ] **Step 5: 変換のテストが通ることを確かめる**

Run: `cargo test -p eml_core_ir --test translate`
Expected: PASS。各パスの直後の `verify_scopes` も debug ビルドで通る。

- [ ] **Step 6: case-of-case の失敗するテストを書く**

`crates/eml_core_ir/tests/simplify.rs` の末尾に、spec の6節の6つのテストを足す。

```rust
#[test]
fn a_bool_match_in_a_condition_jumps_straight_to_the_branch() {
    // `match` の各枝が返す `True` と `False` は分かっているタグなので、`Bool` で分岐し直さずに `if` の枝へ直接進む
    let text = "data Option a = | None | Some a\n\npick : Option Int -> Int\npick o = if (match o with | Some _ -> True | None -> False) then 1 else 2\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @r"
    fn pick(o0) {
      switch o0 {
        #0 ->
          return 2
        #1(x1) ->
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
fn known_tags_of_a_larger_type_jump_straight_to_their_arm() {
    // 3つのタグのどれを渡す jump も、その枝へ直接向かう
    let text = "data Color = | Red | Green | Blue\n\ncode : Int -> Int\ncode n =\n  let c = if n == 0 then Red else if n == 1 then Green else Blue\n  match c with\n    | Red -> 10\n    | Green -> 20\n    | Blue -> 30\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @r"
    fn code(n0) {
      let t1 = prim ==(n0, 0)
      switch t1 {
        #0 ->
          let t2 = prim ==(n0, 1)
          switch t2 {
            #0 ->
              return 30
            #1 ->
              return 20
          }
        #1 ->
          return 10
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
fn a_mixed_switch_splits_only_the_arms_without_fields() {
    // `None` の枝だけを切り出す。`Some` の値を渡す jump は元の join point を通り、`Some _` の枝は `Switch` に残る
    let text = "data Option a = | None | Some a\n\npick : Bool -> Bool -> String -> String\npick a b s = match (if a then None else if b then Some s else Some \"x\") with\n  | None -> \"none\"\n  | Some _ -> \"some\"\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @r#"
    fn pick(a0, b1, s2) {
      join j1() [] {
        let s7 = const "none"
        return s7
      }
      join j0(t6) [] {
        switch t6 {
          #0 ->
            jump j1()
          #1(x9) ->
            let s8 = const "some"
            return s8
        }
      }
      switch a0 {
        #0 ->
          switch b1 {
            #0 ->
              let s4 = const "x"
              let d5 = con #1(s4)
              jump j0(d5)
            #1 ->
              let d3 = con #1(s2)
              jump j0(d3)
          }
        #1 ->
          jump j1()
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
fn arms_with_fields_keep_the_join_point_argument() {
    // フィールドを束縛する枝の中の `o` (join point の引数) は、タグの定数に置き換えない
    let text = "data Option a = | None | Some a\n\nh : Option Int -> Int -> Int\nh o y = y\n\npick : Bool -> Int -> Int\npick c x =\n  let o = if c then None else if x > 0 then Some x else Some 0\n  match o with\n    | None -> 0\n    | Some y -> h o y\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @r"
    fn h(o0, y1) {
      return y1
    }
    fn pick(c0, x1) {
      join j0(t5) [] {
        switch t5 {
          #0 ->
            return 0
          #1(y7) ->
            let y6 = y7
            tailcall h(t5, y6)
        }
      }
      switch c0 {
        #0 ->
          let t2 = prim >(x1, 0)
          switch t2 {
            #0 ->
              let d4 = con #1(0)
              jump j0(d4)
            #1 ->
              let d3 = con #1(x1)
              jump j0(d3)
          }
        #1 ->
          return 0
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
fn jumps_that_pass_constructed_values_are_left_alone() {
    // どの jump も `con` で作った値を渡すので、B2 は join point を変えない。引数を持つコンストラクタの case-of-case は
    // 使われない束縛を消すパスと一緒に入れる (docs/implementation/status.md)
    let text = "data Option a = | None | Some a\n\npick : Bool -> Int\npick c = match (if c then Some 1 else Some 2) with\n  | Some n -> n\n  | None -> 0\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @r"
    fn pick(c0) {
      join j0(t3) [] {
        switch t3 {
          #0 ->
            return 0
          #1(n5) ->
            let n4 = n5
            return n4
        }
      }
      switch c0 {
        #0 ->
          let d2 = con #1(2)
          jump j0(d2)
        #1 ->
          let d1 = con #1(1)
          jump j0(d1)
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
fn split_arms_of_a_data_type_use_the_known_tag() {
    // 切り出した引数のない枝の中では、`c` をその枝のタグに置き換える
    let text = "data Color = | Red | Green | Blue\n\npick : Bool -> Color\npick b =\n  let c = if b then Red else Green\n  match c with\n    | Red -> c\n    | Green -> Blue\n    | Blue -> c\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @r"
    fn pick(b0) {
      switch b0 {
        #0 ->
          return #2
        #1 ->
          return #0
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

`crates/eml_core_ir/tests/perceus.rs` の末尾に足す。

```rust
#[test]
fn a_split_switch_still_unpacks_the_arm_with_fields() {
    // B2 が `Switch` に残した `Some _` の枝は、フィールド `x9` を所有して始まり、使わないので入口で decref する
    let text = "data Option a = | None | Some a\n\npick : Bool -> Bool -> String -> String\npick a b s = match (if a then None else if b then Some s else Some \"x\") with\n  | None -> \"none\"\n  | Some _ -> \"some\"\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r#"
    fn pick(a0, b1, s2) {
      join j1() [] {
        let s7 = const "none"
        return s7
      }
      join j0(t6) [] {
        switch t6 {
          #0 ->
            jump j1()
          #1(x9) ->
            decref x9
            let s8 = const "some"
            return s8
        }
      }
      switch a0 {
        #0 ->
          switch b1 {
            #0 ->
              decref s2
              let s4 = const "x"
              let d5 = con #1(s4)
              jump j0(d5)
            #1 ->
              let d3 = con #1(s2)
              jump j0(d3)
          }
        #1 ->
          decref s2
          jump j1()
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
fn an_unused_field_is_decreffed_when_its_arm_starts() {
    // `Switch` が `o` を move で受け取り、`Some` の枝はフィールド `x1` を所有して始まる。使わないので入口で捨てる
    let text = "data Option a = | None | Some a\n\nflag : Option String -> Int\nflag o = match o with\n  | Some _ -> 0\n  | None -> 1\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r"
    fn flag(o0) {
      switch o0 {
        #0 ->
          return 1
        #1(x1) ->
          decref x1
          return 0
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
fn a_scrutinee_used_in_an_arm_is_dupped_before_the_switch() {
    // `ys` が受ける `xs` は枝の中でも使うので、`Switch` の前で複製する。その参照を使わない枝は入口側で捨てる
    let text = "data List a = | Nil | Cons a (List a)\n\nsize : List Int -> Int\nsize xs = 2\n\ndescribe : List Int -> Int\ndescribe xs = match xs with\n  | Cons _ Nil -> 1\n  | ys -> size ys\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text, Pass::Perceus), @r"
    fn size(xs0) {
      decref xs0
      return 2
    }
    fn describe(xs0) {
      join j0(ys1) [] {
        tailcall size(ys1)
      }
      dup xs0
      switch xs0 {
        #0 ->
          jump j0(xs0)
        #1(x2, x3) ->
          switch x3 {
            #0 ->
              decref xs0
              return 1
            #1(x4, x5) ->
              decref x5
              jump j0(xs0)
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

`a_scrutinee_used_in_an_arm_is_dupped_before_the_switch` は、`translate.rs` の `a_variable_pattern_after_a_switch_binds_the_scrutinee` と同じ入力である。simplify の後では、最初の B3 が `jump` が1つの join point (`Cons _ Nil` の枝と、2つの残りの行列) を戻すので、`ys` の枝の join point だけが残る。

- [ ] **Step 7: テストが失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test simplify --test perceus`
Expected: case-of-case の6つと `a_split_switch_still_unpacks_the_arm_with_fields` の7つが FAIL する。今の simplify は B3 を B2 の後にしか回さないので、`if` の join point の本体が枝の join point の並びのままで、B2 が働かない (`a_bool_match_in_a_condition…` 以外のスナップショットに `join j0(t…)` と `jump j0(#…)` が残る)。`jumps_that_pass_constructed_values_are_left_alone` も、B5 が先に枝を写すので `let n4 = n5` のない形になり一致しない。`an_unused_field_is_decreffed_when_its_arm_starts` と `a_scrutinee_used_in_an_arm_is_dupped_before_the_switch` は、今の順 (B2、B5、B3、B4) でも同じ形になるので、ここで PASS してよい。この2つは Task 5 の Perceus の規則 (枝の入口の decref と `Switch` の前の dup) をソースから確かめるためのテストで、Step 8 の後も PASS し続けることを確かめる。既存のテストは PASS する。

- [ ] **Step 8: B2 を引数のないタグに限り、最初に B3 を回す**

`crates/eml_core_ir/src/simplify.rs` の先頭の doc コメントと `simplify` を次のようにする。`use` に `Arm` を足す。

```rust
//! join point を書き換える最適化 (docs/spec/core-ir.md)。変換の後、Perceus の前に置く。RC の命令がまだないので、
//! 所有権を扱わずに書き換えられる。`captures` は古くなりうるが、このパスの後にパイプラインが埋め直す。
//!
//! B3 (jump が1つ)、B2 (分かっているタグ)、B5 (小さな本体)、B3、B4 (使われない) の順に1巡だけ回す。最初の B3 は、
//! `match` の枝の join point を `Switch` の枝に戻す。枝の join point が `if` の join point の本体と `Switch` の間に
//! 並んだままだと、B2 が本体の `Switch` を見つけられないためである。B5 を B4 より先に回すのは、B5 で jump がなくなった
//! join point を、同じ巡の B4 で消すためである。
//!
//! 書き換えは式のアリーナの上でその場で行う。木から外れた式はアリーナに残り、Perceus がアリーナを作り直すときに
//! 捨てる。そのため、jump の位置と親は、根からたどれる式だけで求める。
```

```rust
pub(crate) fn simplify(program: &mut Program) {
    for function in &mut program.functions {
        let mut pass = Simplify { function };
        pass.inline_single_jumps();
        pass.split_known_tags();
        pass.forward_small_bodies();
        pass.inline_single_jumps();
        pass.remove_unused();
        pass.renumber();
    }
}
```

`known_tag` と `split_known_tags` を次のようにする。

```rust
    fn known_tag(&self, site: CExprId) -> Option<u32> {
        match self.expr(site) {
            CExpr::Jump { args, .. } => match args.as_slice() {
                [Atom::Tag(tag)] => Some(*tag),
                _ => None,
            },
            _ => None,
        }
    }

    /// B2: 引数を1つだけ持ち、本体がその引数で分岐する join point に、定数のタグを jump で渡していれば、引数のない
    /// コンストラクタの枝を引数0個の join point に切り出し、定数の jump をその枝へ直接向ける。`&&` と `||` を条件にした
    /// `if` と、`Bool` を返す `match` を条件にした `if` がこの形になる (docs/spec/core-ir.md)。フィールドを束縛する枝は
    /// `Switch` に残し、引数も置き換えない。引数を持つコンストラクタの値はタグだけでは決まらないためである。
    fn split_known_tags(&mut self) {
        let jumps = self.jumps();
        for (index, sites) in jumps.iter().enumerate() {
            let node = self.function.joins[index];
            let CExpr::Join {
                join,
                params,
                body,
                scope,
                ..
            } = self.expr(node).clone()
            else {
                unreachable!("the join index points at join points")
            };
            let &[param] = params.as_slice() else {
                continue;
            };
            let CExpr::Switch {
                scrutinee: Atom::Var(scrutinee),
                arms,
            } = self.expr(body).clone()
            else {
                continue;
            };
            let known: Vec<u32> = sites
                .iter()
                .filter_map(|&site| self.known_tag(site))
                .collect();
            // 定数のタグは引数のないコンストラクタの値なので、フィールドのない枝に当たるはずである
            let nullary = |tag: u32| {
                arms.iter()
                    .any(|arm| arm.tag == tag && arm.fields.is_empty())
            };
            if scrutinee != param || known.is_empty() || !known.iter().all(|&tag| nullary(tag)) {
                continue;
            }
            let mut split = Vec::new();
            let mut dispatch = Vec::new();
            for arm in &arms {
                if !arm.fields.is_empty() {
                    dispatch.push(arm.clone());
                    continue;
                }
                self.substitute(arm.body, param, Atom::Tag(arm.tag));
                let arm_join = JoinId(self.function.joins.len() as u32);
                // 索引は、下で組み立てた `Join` の位置に直す
                self.function.joins.push(arm.body);
                split.push((arm.tag, arm_join, arm.body));
                let jump = self.push(CExpr::Jump {
                    join: arm_join,
                    args: Vec::new(),
                });
                dispatch.push(Arm {
                    tag: arm.tag,
                    fields: Vec::new(),
                    body: jump,
                });
            }
            self.set(
                body,
                CExpr::Switch {
                    scrutinee: Atom::Var(param),
                    arms: dispatch,
                },
            );
            for &site in sites {
                if let Some(tag) = self.known_tag(site) {
                    let &(_, arm_join, _) = split
                        .iter()
                        .find(|&&(arm_tag, ..)| arm_tag == tag)
                        .expect("checked above");
                    self.set(
                        site,
                        CExpr::Jump {
                            join: arm_join,
                            args: Vec::new(),
                        },
                    );
                }
            }
            // 枝の join point を外側に並べ、元の join point をいちばん内側に置く。枝は元の join point の定義全体を
            // 範囲にするので、元の本体からも、範囲の中の定数の jump からも届く。元の位置には最初の枝の join point が入る
            let mut inner = self.push(CExpr::Join {
                join,
                params: vec![param],
                captures: Vec::new(),
                body,
                scope,
            });
            self.function.joins[index] = inner;
            for (position, &(_, arm_join, arm)) in split.iter().enumerate().rev() {
                let expr = CExpr::Join {
                    join: arm_join,
                    params: Vec::new(),
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
```

`Arm` の `Clone` は Task 5 の `#[derive(Debug, Clone, PartialEq, Eq)]` を前提にする。

- [ ] **Step 9: simplify と Perceus のテストが通ることを確かめる**

Run: `cargo test -p eml_core_ir`
Expected: PASS。既存の simplify のテスト (`known_tags_jump_straight_to_their_arm` など) の期待値は変わらない。今の変換が作る join point は末尾にない `if` のもので、jump がつねに2つ以上あるので、最初の B3 は何もしない。

- [ ] **Step 10: `run/data/` の UI テストを書く**

次の10個のファイルを `tests/ui/run/data/` に作る。

`tests/ui/run/data/option.em`:

```haskell
-- Option values built by constructors and taken apart by `match`.
data Option a =
  | None
  | Some a

describe : Option Int -> String
describe o = match o with
  | None -> "none"
  | Some n -> "some " ++ show_int n

safe_div : Int -> Int -> Option Int
safe_div a b = if b == 0 then None else Some (a / b)

main : Unit -> <IO> Unit
main () =
  println (describe (safe_div 10 2))
  println (describe (safe_div 1 0))
  println (describe (Some 42))
```

stdout:

```
some 5
none
some 42
```

`tests/ui/run/data/list.em`:

```haskell
-- A recursive list: length and sum walk the structure, and every cell is freed.
data List a =
  | Nil
  | Cons a (List a)

length : List a -> Int
length xs = match xs with
  | Nil -> 0
  | Cons _ rest -> 1 + length rest

sum : List Int -> Int
sum xs = match xs with
  | Nil -> 0
  | Cons x rest -> x + sum rest

range : Int -> Int -> List Int
range lo hi = if lo > hi then Nil else Cons lo (range (lo + 1) hi)

main : Unit -> <IO> Unit
main () =
  let xs = range 1 10
  println (show_int (length xs))
  println (show_int (sum xs))
  println (show_int (sum (range 1 1000)))
```

stdout:

```
10
55
500500
```

`tests/ui/run/data/nested_patterns.em`:

```haskell
-- Nested constructor patterns, wildcards and a fallback arm share one decision tree.
data Option a =
  | None
  | Some a

data List a =
  | Nil
  | Cons a (List a)

first_two : List (Option Int) -> String
first_two xs = match xs with
  | Cons (Some a) (Cons (Some b) _) -> "both " ++ show_int (a + b)
  | Cons None (Cons (Some b) _) -> "second " ++ show_int b
  | Cons _ _ -> "other"
  | Nil -> "empty"

main : Unit -> <IO> Unit
main () =
  println (first_two (Cons (Some 1) (Cons (Some 2) Nil)))
  println (first_two (Cons None (Cons (Some 3) (Cons None Nil))))
  println (first_two (Cons (Some 4) (Cons None Nil)))
  println (first_two (Cons None Nil))
  println (first_two Nil)
```

stdout:

```
both 3
second 3
other
other
empty
```

`tests/ui/run/data/constructor_functions.em`:

```haskell
-- Constructors are values: they can be partially applied and passed to functions.
data Pair a b =
  | Pair a b

data Option a =
  | None
  | Some a

apply_to : (a -> b) -> a -> b
apply_to f x = f x

show_pair : Pair Int String -> String
show_pair p = match p with
  | Pair n s -> show_int n ++ " " ++ s

show_option : Option Int -> String
show_option o = match o with
  | None -> "none"
  | Some n -> show_int n

main : Unit -> <IO> Unit
main () =
  let with_one = Pair 1
  println (show_pair (with_one "one"))
  println (show_pair (apply_to (Pair 2) "two"))
  println (show_option (apply_to Some 3))
  let make = Some
  println (show_option (make 4))
```

stdout:

```
1 one
2 two
3
4
```

`tests/ui/run/data/infix_constructors.em`:

```haskell
-- An infix constructor declared with a `:` operator, in expressions and patterns. Without a fixity declaration
-- it is `infixl 9`, so a right-nested sequence needs parentheses.
data Seq a =
  | Done
  | a :> Seq a

total : Seq Int -> Int
total s = match s with
  | Done -> 0
  | x :> rest -> x + total rest

main : Unit -> <IO> Unit
main () = println (show_int (total (1 :> (2 :> (3 :> Done)))))
```

stdout:

```
6
```

`tests/ui/run/data/irrefutable_patterns.em`:

```haskell
-- Single-constructor patterns in a `let`, a lambda parameter and an equation parameter.
data Box a =
  | Box a

unbox : Box String -> String
unbox (Box s) = s

main : Unit -> <IO> Unit
main () =
  let Box n = Box 41
  println (show_int (n + 1))
  let bang = fn (Box s) -> s ++ "!"
  println (bang (Box "lambda"))
  println (unbox (Box "equation"))
```

stdout:

```
42
lambda!
equation
```

`tests/ui/run/data/unreachable_arm.em`:

```haskell
-- An arm after a catch-all is unreachable: a warning, and the program still runs.
data Color =
  | Red
  | Green

name : Color -> String
name c = match c with
  | _ -> "any"
  | Red -> "red"

main : Unit -> <IO> Unit
main () = println (name Green)
```

stdout は `any` の1行である。stderr には、9行目の `Red` のパターン (`9:5`) を指す E4004 の Warning が1件だけ出る。文言は Task 4 のものである。

`tests/ui/run/data/case_of_case.em`:

```haskell
-- Case-of-case: known tags jump straight to their arm, and arms with fields stay in the switch. Every path
-- frees what it built.
data Option a =
  | None
  | Some a

data Color =
  | Red
  | Green
  | Blue

describe : Option String -> String
describe o = if (match o with | Some _ -> True | None -> False) then "has a value" else "empty"

color_code : Int -> Int
color_code n =
  let c = if n == 0 then Red else if n == 1 then Green else Blue
  match c with
    | Red -> 10
    | Green -> 20
    | Blue -> 30

pick : Bool -> Bool -> String -> String
pick a b s = match (if a then None else if b then Some s else Some "x") with
  | None -> "none"
  | Some t -> "some " ++ t

main : Unit -> <IO> Unit
main () =
  println (describe (Some "v"))
  println (describe None)
  println (show_int (color_code 0 + color_code 1 + color_code 2))
  println (pick True True "s")
  println (pick False True "s")
  println (pick False False "s")
```

stdout:

```
has a value
empty
60
none
some s
some x
```

`tests/ui/run/data/shared_scrutinee.em`:

```haskell
-- The scrutinee is used again inside an arm and after the match, so the arm unpacks a shared value: it copies
-- the fields and keeps the cell.
data Option a =
  | None
  | Some a

show : Option String -> String
show o = match o with
  | Some s -> "Some " ++ s
  | None -> "None"

label : Option String -> String
label o =
  let first = match o with
    | Some s -> s ++ " / " ++ show o
    | None -> "none"
  first ++ " / " ++ show o

main : Unit -> <IO> Unit
main () =
  let o = Some "a"
  println (label o)
  println (label o)
  println (label None)
```

stdout:

```
a / Some a / Some a
a / Some a / Some a
none / None
```

`tests/ui/run/data/match_with_effects.em`:

```haskell
-- A non-tail `match` whose arms perform a multi-shot operation. The string saved across the operation, the
-- field bound by the arm and the arguments of the arm's join point survive every resumption.
effect Choice where
  multi choose : Unit -> Bool

data Option a =
  | None
  | Some a

pick : Option String -> <Choice> String
pick o =
  let prefix = "["
  let picked = match o with
    | Some s -> if choose () then s else s ++ "?"
    | None -> if choose () then "yes" else "no"
  prefix ++ picked ++ "]"

both : Option String -> String
both o =
  handle pick o with
    | choose () k -> resume k True ++ " " ++ resume k False

main : Unit -> <IO> Unit
main () =
  println (both (Some "a"))
  println (both None)
```

stdout:

```
[a] [a?]
[yes] [no]
```

Review Focus の5件を確かめる UI テストも、同じ `tests/ui/run/data/` に足す。

`tests/ui/run/data/long_list.em`:

```haskell
-- A list of 100000 cells is summed by a non-tail recursion, and another one is dropped whole when only its
-- head is read. Freeing the long tail must not recurse in Rust.
data List a =
  | Nil
  | Cons a (List a)

build : Int -> List Int -> List Int
build n acc = if n == 0 then acc else build (n - 1) (Cons n acc)

total : List Int -> Int
total xs = match xs with
  | Nil -> 0
  | Cons x rest -> x + total rest

first_or_zero : List Int -> Int
first_or_zero xs = match xs with
  | Nil -> 0
  | Cons x _ -> x

main : Unit -> <IO> Unit
main () =
  println (show_int (total (build 100000 Nil)))
  println (show_int (first_or_zero (build 100000 Nil)))
```

stdout:

```
5000050000
1
```

`tests/ui/run/data/function_fields.em`:

```haskell
-- Functions stored in constructor fields are taken out by `match` and called. A partially applied
-- constructor holds a closure that captures a local value.
data Op =
  | Unary (Int -> Int)
  | Binary (Int -> Int -> Int) Int

run : Op -> Int -> Int
run op x = match op with
  | Unary f -> f x
  | Binary g y -> g x y

main : Unit -> <IO> Unit
main () =
  let offset = 10
  let add = Binary (fn a -> fn b -> a + b + offset)
  println (show_int (run (Unary (fn n -> n * 2)) 21))
  println (show_int (run (add 5) 1))
```

stdout:

```
42
16
```

`tests/ui/run/data/closures_over_pattern_variables.em`:

```haskell
-- A lambda made inside a `match` arm captures pattern variables and is called twice after the match.
data Pair a b =
  | Pair a b

adder : Pair Int String -> (Int -> String)
adder p = match p with
  | Pair n label -> fn m -> label ++ " " ++ show_int (n + m)

main : Unit -> <IO> Unit
main () =
  let f = adder (Pair 1 "sum")
  println (f 2)
  println (f 40)
```

stdout:

```
sum 3
sum 41
```

`tests/ui/run/data/tree.em`:

```haskell
-- A polymorphic binary tree with three fields per node. Insertion rebuilds the path, the in-order walk
-- visits every node, and the size works for any element type.
data Tree a =
  | Leaf
  | Node (Tree a) a (Tree a)

insert : Int -> Tree Int -> Tree Int
insert x t = match t with
  | Leaf -> Node Leaf x Leaf
  | Node l v r -> if x < v then Node (insert x l) v r else Node l v (insert x r)

walk : Tree Int -> String
walk t = match t with
  | Leaf -> ""
  | Node l v r -> walk l ++ show_int v ++ ";" ++ walk r

size : Tree a -> Int
size t = match t with
  | Leaf -> 0
  | Node l _ r -> size l + 1 + size r

main : Unit -> <IO> Unit
main () =
  let t = insert 5 (insert 2 (insert 8 (insert 1 (insert 9 Leaf))))
  println (walk t)
  println (show_int (size (Node Leaf "only" Leaf)))
```

stdout:

```
1;2;5;8;9;
1
```

`main` の `t` は `walk` に1回だけ渡すので、`walk` が木を分解しながら解放する。`size` には `String` の要素の木を渡し、型引数を持つ `data` の多相な関数が boxed な要素でもリークしないことを確かめる。

`tests/ui/run/data/clause_patterns.em`:

```haskell
-- Constructor patterns in an operation clause parameter and in the return clause take boxes apart.
data Box a =
  | Box a

effect Ask where
  ask : Box Int -> Int

program : Unit -> <Ask> (Box Int)
program () =
  let a = ask (Box 20)
  let b = ask (Box 1)
  Box (a + b)

answer : Unit -> Int
answer () =
  handle program () with
    | ask (Box q) k -> resume k (q * 2)
    | return (Box r) -> r

main : Unit -> <IO> Unit
main () =
  println (show_int (answer ()))
```

stdout:

```
42
```

- [ ] **Step 11: UI テストを確かめて承認する**

Run: `cargo test -p eml_cli --test ui`
Expected: `ui::run` が15個の新しいスナップショットで失敗する。`cargo insta review` で、各ファイルの stdout が上に書いたものと一致することを確かめる。stderr は `unreachable_arm.em` だけが E4004 の Warning を1件持ち (位置 `9:5`)、ほかは空であることを確かめてから承認し、もう一度 `cargo test -p eml_cli --test ui` を通す。`debug_heap` が有効なので、リークがあれば `Leak` で失敗する。既存のスナップショットは変わらない。

- [ ] **Step 12: 全体を確かめる**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`
Expected: すべて PASS し、clippy の警告がない。`cargo fmt` の後に差分が出たら、その差分も含めてコミットする。

- [ ] **Step 13: コミットする**

```bash
git add crates/eml_core_ir/src/translate/pattern.rs crates/eml_core_ir/src/translate/mod.rs \
  crates/eml_core_ir/src/translate/expr.rs crates/eml_core_ir/src/translate/program.rs \
  crates/eml_core_ir/src/simplify.rs crates/eml_core_ir/tests/translate.rs \
  crates/eml_core_ir/tests/simplify.rs crates/eml_core_ir/tests/perceus.rs \
  tests/ui/run/data crates/eml_cli/tests/snapshots
git commit -m "Compile match to a decision tree and limit B2 to tags without fields

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014YGQm6f7Mtged1yVY4dmJG"
```

### Task 7: 文書を直す

spec の7節の表のとおり、決まった内容を `docs/spec/` と `docs/implementation/` に移す。作業用の spec と plan は、ブランチ全体のレビューの後に別のコミットで削除する (これまでの作業と同じ運用)。このタスクでは削除しない。

**Files:**
- Modify: `docs/spec/core-ir.md`、`docs/spec/runtime.md`、`docs/spec/types.md`、`docs/spec/diagnostics.md`、`docs/spec/exhaustiveness.md`
- Modify: `docs/implementation/architecture.md`、`docs/implementation/status.md`、`docs/implementation/test-changes.md`、`docs/implementation/testing.md`

**Interfaces:**
- Consumes: Task 1〜6 の実装 (名前と振る舞いを文書に書くので、実装と食い違ったら実装の側を正とし、この節の文案を直す)

- [ ] **Step 1: `yomiyasu:yomiyasu` スキルを読む**

文書はすべて日本語なので、書く前にスキルを読み、規則に従う。

- [ ] **Step 2: `docs/spec/core-ir.md` を直す**

1. 「Core IR」の命令の表を次の形にする。

   ```markdown
   | 種類 | 命令 |
   |---|---|
   | 値 | `let x = 呼び出し / クロージャの呼び出し / プリミティブ / コンストラクタ / クロージャ生成`、`match`、`return` |
   | 制御 | `join j(x1, .., xn) [captures] { 本体 }` (join point)、`jump j(v1, .., vn)`、末尾呼び出し |
   | 分岐 | `switch x { tag -> e; tag(y1, .., yn) -> e }` (引数を持つコンストラクタの枝はフィールドを束縛する) |
   | RC (`Unr` のヒープ値のみ) | `dup x`、`decref x` |
   | 線形値 | `drop x` (宣言された破棄処理を呼ぶ) |
   | エフェクト | `perform op args`、`handle body init { ops; return }`、`resume k v s`、`drop k` |
   ```

2. 「`Bool` は、タグ 0 (`False`) と 1 (`True`) の引数のないコンストラクタとして表し」の箇条を次の2つに置き換える。

   ```markdown
   - コンストラクタのタグは、`data` の宣言の順の番号である。引数のないコンストラクタの値は即値のタグで、引数を持つコンストラクタの値はヒープのオブジェクト (`Payload::Data`) である。引数を持つコンストラクタが1つでもある型の変数はボックス化し、引数を持つコンストラクタのない型 (`Bool` など) の変数はボックス化しない。`Bool` は Prelude の `data Bool = | False | True` で、`False` のタグが 0、`True` のタグが 1 になる。`if` は `match` と同じ分岐の命令に変換する。
   - コンストラクタを部分適用したときと関数値として使ったときは、コンストラクタの値を作るだけの関数で包む。
   ```

3. 「末尾にない分岐の値は join point で受ける」の箇条の後に、次の2つの箇条を足す。

   ```markdown
   - join point は引数の並びを持ち、`jump` は同じ数の値を渡す。末尾にない `if` と `match` の値を受ける join point は引数1つ、`match` の枝の本体の join point はパターンが束縛する変数を引数に持つ。
   - `match` は決定木にコンパイルする。各枝の本体をつねに join point にし、決定木の葉から `jump` する。決定木は、最初の行でコンストラクタのパターンを持ついちばん左の列の値で `switch` し、型のすべてのコンストラクタの枝を作る。その列のどの行にも現れないコンストラクタの枝は、`switch` の直前に置いた引数0個の join point へ `jump` する。`let`、ラムダの引数、等式の引数のパターンも、枝が1つの `match` として同じ経路でコンパイルする。`match` は網羅性の検査を通っているので、どの枝にも当たらない値の受け皿は作らない。
   ```

4. 「変数の読み出しは所有権の移動 (move) とし」の箇条の後に、次の箇条を足す。

   ```markdown
   - `switch` は scrutinee を1回使う (move)。枝は、`switch` の前に所有していた変数から scrutinee を除き、フィールドの変数を加えた状態から始まる。実行時は、scrutinee が即値のタグならその枝に入る。オブジェクトなら、一意のときはフィールドを取り出して箱を解放し、共有されているときはフィールドを `dup` してから箱を `decref` する。どちらでも、枝はフィールドの参照を1つずつ所有して始まる。枝の中でも scrutinee を使うなら、Perceus が `switch` の前に `dup` する。
   ```

5. 「パス」の `simplify` の箇条の、B2 の説明を次のように直す。

   ```markdown
   - `simplify` は、変換の後、Perceus の前に置き、join point を書き換える。分かっているタグの `jump` を枝へ直接向ける (case-of-case)、本体が1命令の join point への `jump` をその命令にする、`jump` が1つの join point をその位置に戻す、`jump` のない join point を消す、の4つを、B3、B2、B5、B3、B4 の順で1巡だけ行う。最初の B3 は、`jump` が1つの `match` の枝の join point を分岐の枝に戻し、B2 が分岐を見つけられるようにする。B2 は、引数を1つだけ持ち本体がその引数で分岐する join point に、定数のタグを渡す `jump` がある場合に限る。切り出すのは引数のないコンストラクタの枝だけで、引数0個の join point にし、その枝の中で引数をタグの定数に置き換える。フィールドを束縛する枝は分岐に残す。引数を持つコンストラクタの値はタグだけでは決まらないためである。どの書き換えも、本体を、その `jump` に来たときだけ実行される位置へ動かすだけなので、エフェクトの順と短絡評価は変わらない。
   ```

6. verifier の箇条の「`jump` では、`captures` が範囲にあり」の文の前に「`jump` の値の数が join point の引数の数と一致すること、」を足し、「Perceus より前の IR には、範囲と引数の数を確かめ」の文の後に「どちらの度合いも、`switch` の枝のフィールドを枝の範囲の束縛として、join point の引数を本体の範囲の束縛として扱う。」を足す。

- [ ] **Step 3: `docs/spec/runtime.md` を直す**

「ランタイムの API」の「クロージャも、同じ RC で管理するオブジェクトである」の箇条の後に、次の箇条を足す。

```markdown
- 引数を持つコンストラクタの値も、同じ RC で管理するオブジェクト (`Payload::Data`) である。タグとフィールドの並びを持ち、フィールドの値を子として所有する。記述子は、どの `data` の型でも同じものにする。`match` の分岐でフィールドを取り出す側は、共有されたオブジェクトの複製と同じ手続き (`take_or_copy`) を使う ([Core IR とインタプリタ](core-ir.md))。
```

「オブジェクトのヘッダ」の「記述子はペイロードの種類から決める」の箇条はそのままにする。`Lin` のフィールドを持つ `data` の破棄処理を記述子に足すのは段階5である。

- [ ] **Step 4: `docs/spec/types.md` を直す**

「データ型の Kind は、フィールドの Kind の上限 (join) で推論する。」の箇条を次のように直す。

```markdown
- データ型の Kind は、フィールドの Kind の上限 (join) で推論する。型引数を持つ `data` では、宣言ごとに Kind に効く型引数の位置を求める。フィールドに直接書いた型引数と、別の `data` の効く引数の位置に書いた型引数が効く。再帰する宣言があるので、すべての宣言で不動点を求める。`Option a` の Kind は `a` の Kind になる。関数型の中の型引数は効かない。フィールドに書いた関数型の線形性は、表面の構文で `m` を書けないので `Unr` に固定する (操作の引数の型と同じ扱い。見直しは [実装の現在地](../implementation/status.md) に記録する)。
- コンストラクタの型は、宣言の型引数で量化したスキーム (`Some : a -> Option a`) である。矢印の row は `<>`、線形性は `Unr` である。
```

- [ ] **Step 5: `docs/spec/diagnostics.md` と `docs/spec/exhaustiveness.md` を直す**

1. 「割り当て済みの番号」の前置きの文の「E3xxx の残りと E4xxx の番号は、線形性と網羅性の検査を実装するときに割り当てる。」を「E4xxx は `eml_types::codes` に置く。E3xxx の残りの番号は、線形性の検査を実装するときに割り当てる。」にする。
2. 表の E1015 の行を「`data` の型の適用や row の中のエフェクトの型引数の個数が、宣言と違う」に直し、E1015 の後に次の2行、E3001 の後に次の4行を足す。

   ```markdown
   | E1016 | `CONSTRUCTOR_ARITY` | パターンのコンストラクタの引数の個数が、宣言のフィールドの数と違う |
   | E1017 | `DUPLICATE_BINDING` | 1つのパターン、または1つの等式の引数の並びで、同じ変数名を2回束縛した。2つ目の束縛を primary、1つ目を secondary にする |
   ```

   ```markdown
   | E4001 | `NON_EXHAUSTIVE_MATCH` | 網羅されていない `match` |
   | E4002 | `NON_EXHAUSTIVE_EQUATION` | 網羅されていない等式 |
   | E4003 | `REFUTABLE_PATTERN` | 反駁可能な `let` の左辺、ラムダの引数、handler の節の引数と `return` の節の引数のパターン |
   | E4004 | `UNREACHABLE_ARM` | 到達しない枝 (Warning) |
   ```

3. 「網羅性の診断」の表に番号の列を足し、`match` と等式の行の「fix で枝の追加を提案する」「fix で最後の等式の後への等式の追加を提案する」の後に「(fix は段階5の線形性の fix と一緒に入れる)」を足す。漏れの例の数の規則として、表の後に次の文を足す。

   ```markdown
   「反駁可能な `let` / ラムダの引数のパターン」の行を「反駁可能な `let` / ラムダの引数 / handler の節と `return` の節の引数のパターン」にする。節はラムダと同じく持ち上げる関数で、引数のパターンも同じ経路でコンパイルするためである。

   漏れているパターンの例は、note に最大3つ並べる。3つより多ければ、残りがあることを書き添える。到達しない等式の Warning は、複数の等式を実装する段階6で入れる。
   ```

4. `docs/spec/exhaustiveness.md` の「検査パス」に、次の箇条を足す。

   ```markdown
   - 変数、`_`、`()` はどれもワイルドカードとして扱う。`Unit` の値は1つしかないためである。型を明示したパターンは、中のパターンとして扱う。
   - 型が `Error` を含むパターンと scrutinee は検査しない。診断を連鎖させないためである ([診断](diagnostics.md) の「連鎖する診断の抑止」)。
   ```

- [ ] **Step 6: `docs/implementation/architecture.md` を直す**

1. 「`eml_hir` の内部」の型とエフェクトの item の箇条で、「組み込みの `Int`、`String`、`Bool`、`Unit` と `IO` を変換のはじめに登録し」を「組み込みの `Int`、`String`、`Unit` と `IO` を変換のはじめに登録し、Prelude の `data Bool` を変換してから」にし、次の箇条を足す。

   ```markdown
   - `data` の宣言は `lower/data.rs` で変換する。型の名前をすべて登録してから宣言を変換するので、宣言どうしは再帰と相互再帰ができる。`TypeDef` は型引数 (`generics`)、フィールドの型の注釈のアリーナ (`types`)、種類 (`TypeDefKind::{Builtin, Data}`) を持つ。コンストラクタは `Module::constructors` (`ConstructorId`) に置き、属する型、タグ (宣言の順)、フィールドの型を持つ。コンストラクタは値の名前空間に入り、式では `Res::Constructor` に解決する。`Bool` の2つのコンストラクタは lang item (`LangItems::true_ctor`、`false_ctor`) で、`&&` と `||` の脱糖が使う
   - パターンは `PatKind::Con` (コンストラクタと引数のパターン) を持つ。1つのパターンと1つの等式の引数の並びで同じ変数名を2回束縛すると E1017 にする。`match` は `ExprKind::Match` で、枝 (`MatchArm`) のパターンの変数は、その枝の本体だけで見える
   ```

2. 「`eml_hir` の内部」の名前の解決の箇条の `Res::{Local, Function, Operation, Builtin}` を `Res::{Local, Function, Operation, Constructor, Builtin}` にする。
3. 「`eml_types` の内部」で、「型構成子は `TyShape::Con(TypeDefId)`」の箇条を「型構成子は `TyShape::Con(TypeDefId, Vec<Ty>)` で、型の適用の引数を持つ。外に出す型は `Type::Con { id, name, args }`」に直し、「`True` と `False` は、段階4で `data Bool` にするまで lang item の `Bool` の型である」の文を消す。次の箇条を足す。

   ```markdown
   - データ型の Kind に効く型引数の位置は `data.rs` で、すべての `data` の宣言について不動点で求める。`Table::kind_bounds` は、`Con` の効く位置の引数の境界を並べる。コンストラクタのスキームは関数と同じ経路で作り、`TypedModule::constructors` に置く
   - パターンは期待する型を受けて検査する。`match` は scrutinee の型で各枝のパターンを検査し、枝の本体を `if` の枝と同じく検査する。使用回数のパスは `match` の枝を別の経路として扱う
   - 網羅性の検査は `exhaustive.rs` にある。型推論と使用回数のパスの後に、型付き HIR の上で Maranget の usefulness を使って検査し、漏れているパターンの例を作る。コンストラクタの集合はパターンの型の型構成子から引く
   ```

4. 「`eml_core_ir`、`eml_runtime`、`eml_interp` の内部」で次を直す。
   - `translate/` の箇条に「`pattern.rs` は `match` と、`let`・ラムダ・等式の引数のパターンを決定木にコンパイルする」を足す
   - boxed の箇条の「変換の種類 (`Prim`、`Perform`、`Compose`、`Constructor`)」を「変換の種類 (`Prim`、`Io`、`Compose`)」に直し、「`data` の型の変数は、引数を持つコンストラクタが1つでもあれば boxed にする」を足す
   - 「ラムダは、捕まえた変数を先頭の引数に持つ関数に持ち上げる」の箇条に「コンストラクタを値として使うときは、値を作るだけの関数 (`con$名前`) で包む」を足す
   - `simplify` の箇条の順の説明を「B3、B2、B5、B3、B4 の順に1巡だけ回す。最初の B3 は、`match` の枝の join point を `Switch` の枝に戻し、B2 が `Switch` を見つけられるようにする」に直し、B2 の説明を「B2 (分かっているタグの `jump`) は、引数のないコンストラクタの枝ごとに引数0個の join point を作り、枝の中の join point の引数をそのタグの定数に置き換える。フィールドを束縛する枝は `Switch` に残す」に直す
   - Perceus の箇条の「join point の本体は、`captures` のうち RC の対象をちょうど1つずつ所有して始まり」を「join point の本体は、`captures` と引数のうち RC の対象をちょうど1つずつ所有して始まり」にし、「`Switch` は scrutinee を消費し、枝はフィールドのうち RC の対象を所有して始まる」を足す
   - クロージャの箇条の後に「引数を持つコンストラクタの値は `Payload::Data` (タグとフィールド) で、`Switch` は `Heap::take_or_copy` で分解する」を足す

- [ ] **Step 7: `docs/implementation/status.md` を直す**

1. 「名前解決以降の実装段階」の表の段階4の行を、次の2行に置き換える。

   ```markdown
   | 4a | `data` (型引数、中置のコンストラクタ、型の適用)、コンストラクタの値、`match` と入れ子のパターン、網羅性の検査、`Bool` を Prelude の `data` にする | 完了 |
   | 4b | タプル (数字ラベルのレコード)、`Int` と `String` のリテラルのパターン、`String` などの等値の扱いの決定 | 未着手 |
   ```

2. 「各 crate の実装状況」の `eml_hir`、`eml_types`、`eml_core_ir`、`eml_runtime`、`eml_interp` の行の「段階3b まで実装済み」を「段階4a まで実装済み」にし、それぞれの行の最後に次を足す。
   - `eml_hir`: 「`data` の宣言とコンストラクタ、型の適用、コンストラクタのパターン、`match`、E1016、E1017。`Bool` は Prelude の `data`」
   - `eml_types`: 「型構成子の引数、データ型の Kind、コンストラクタのスキーム、パターンと `match` の検査、網羅性の検査 (E4001〜E4004)」。同じ行の「線形性と網羅性の検査は未実装」を「線形性の検査は未実装」に直す
   - `eml_core_ir`: 「`Switch` の枝のフィールドの束縛、`Rhs::Con`、join point の引数の並び、`match` の決定木 (`translate/pattern.rs`)。B2 は引数のないタグに限る」。同じ行の `simplify` の説明はそのままにする
   - `eml_runtime`: 「`data` のオブジェクト (`Payload::Data`)」
   - `eml_interp`: 「コンストラクタの値と `Switch` の分解」
3. 「次の作業の注意点」から次の項目を消す。
   - 「段階4: HIR の `Module` に `data` の item を足し、…」
   - 「段階4で boxed な値を `Switch` の scrutinee にするとき、…」
   - 「段階4: `simplify` の B2 は、枝の中の join point の引数をそのタグの定数に置き換える。…」
   - 「段階4: `match` のコンパイルは `translate/pattern.rs` に置き、…」
   - 「段階4: 引数名の重複 (`f x x = x`) は、…」
4. 「段階4のタプルは、…」の項目の「段階4のタプル」を「段階4b のタプル」に直す。
5. 「次の作業の注意点」に次の項目を足す。

   ```markdown
   - 段階4b: リテラルのパターンは、決定木の `Switch` ではなく、値を比べるプリミティブと分岐で調べる。`String` の比べ方は等値の扱いと一緒に決める。網羅性の検査は、Int と String のリテラルを無限に値があるものとして扱う ([網羅性](../spec/exhaustiveness.md))
   - 後で入れる最適化: 引数を持つコンストラクタの case-of-case (`jump` が `let x = Con(...)` の変数を渡す場合に、枝の join point へ直接向ける)。使われなくなった `let x = Con(...)` を消すパスがないと、箱の確保と `dup` と解放が増えるので、そのパスと一緒に入れる
   - 網羅されていない `match` と等式の fix (枝や等式の追加の提案) は、段階5の線形性の診断の fix (分解パターンへの書き換え) と一緒に入れる
   - フィールドに書いた関数型の線形性を `Unr` に固定した。操作の引数の型と同じく、線形なクロージャを `data` に入れられるようにするかを後で見直す
   - 決定木の列は、最初の行でコンストラクタのパターンを持ついちばん左の列を選ぶ。行列によっては部分木が大きくなりうる。必要になったら列の選び方を見直す
   - 段階5: `Lin` のフィールドを持つ `data` は、宣言がつねに `Lin` になる。Kind に効く型引数の位置の計算 (`eml_types::data`) に定数の境界を足し、記述子に破棄処理を足す
   ```

6. 「spec に反映済みで、実装は後の段階で扱うもの」から「コンストラクタは値の名前空間に置く。…段階4で実装する。」の文の「段階4で実装する。」を消し、「網羅されていない等式の診断は、…段階4で実装する」の項目を「到達しない等式の Warning は段階6で実装する」に直す。「直積型の構造的なレコードへの統一」の項目の「タプルは段階4」を「タプルは段階4b」にする。
7. 「完了した作業」の表に次の行を足す。

   ```markdown
   | 縦の貫通 段階4a | 型引数を持つ `data`、コンストラクタ、`match` と入れ子のパターン、網羅性の検査を通した。`Bool` を Prelude の `data` にした。Core IR の join point に引数の並びを持たせ、`Switch` の枝がフィールドを束縛して scrutinee を move で分解するようにした。`match` は決定木にコンパイルし、`simplify` の B2 を引数のないタグに限った |
   ```

8. 冒頭の「2026-10-05 時点のリポジトリの状態」の日付を、作業を終えた日に直す。

- [ ] **Step 8: `docs/implementation/test-changes.md` に記録する**

「記録」の最後に、次の見出しと箇条を足す。

```markdown
### 縦の貫通 段階4a

- `data` の宣言が E0004 でなくなったので、`tests/ui/check-fail/not-yet-supported/data_declarations.em` とそのスナップショットを削除した (種類1)。`data` を使うプログラムが通ることは `tests/ui/run/data/` のテストで確かめる
- `eml_hir/tests/lower.rs` の `constructs_of_later_stages_are_not_yet_supported` は、入力から `data` の行を除き、`match` の枝をリテラルのパターン (`| 0 -> x`) に変えた (種類1)。`data` と `match` が E0004 でなくなったためである。リテラルのパターンは段階4b まで E0004 なので、後の段階の構文を E0004 にすることを `match` の行で確かめ続ける
- `eml_hir/src/builtin.rs` の単体テストから、`Builtin::True` を引く行を削除した (種類1)。`True` と `False` は Prelude の `data Bool` のコンストラクタになり、組み込みの表から外れた
- 同じ理由で、`eml_types/tests/check.rs` の `builtin_schemes_are_exported` の「`builtins` に `True` がない」の assert を、「`TypedModule::constructors` に `True` のスキームがある」の assert に置き換え、`eml_hir/tests/structure.rs` の `the_prelude_has_a_signature_for_every_builtin_function` から `True` と `False` を除く分岐をなくした (種類1)。残る組み込みの期待値は変わらない
- `not-yet-supported/` の UI テストがなくならないよう、段階4b に残すタプルの E0004 を確かめる `tests/ui/check-fail/not-yet-supported/tuples.em` を足した (新しいテスト)
- `eml_core_ir/tests/simplify.rs` の `an_arm_reached_twice_stays_a_join_point` と `join_points_left_without_jumps_are_removed` の期待値で、B2 が作る join point の引数がなくなった (`join j0(u7)` が `join j0()` に、`jump j0(())` が `jump j0()` に) (種類2)。join point が引数の並びを持つようになり、`()` を受けるだけの引数が要らなくなったためである。ほかの行は変わっていない
- 手で組んだ Core IR のテスト (`CExpr::Join`、`CExpr::Jump`、`CExpr::Switch` の組み立て) と、`Type::Con` と `TypeRefKind::Con` を組み立てるテストは、欄の形の変更に合わせて書き換えた。期待値は変えていない (種類3)
```

実装中に種類1と種類2の表にない変更が出て、ユーザーの承認を得た場合は、その変更もここに足す。

- [ ] **Step 9: `docs/implementation/testing.md` を直す**

1. 「今あるテストの地図」の表に、このブランチで足したテストのファイルを足す。
   - `eml_hir`: `data.rs` (`data` の宣言、コンストラクタ、型の適用、パターン、`match`)
   - `eml_types`: `data.rs` (型構成子の引数、データ型の Kind、パターンと `match` の型検査)、`exhaustive.rs` (網羅性の検査と漏れの例)
   - `eml_interp`: `data.rs` (手書きの Core IR による `data` の値と `Switch` の分解)
   - `eml_syntax` の `ast.rs` の説明に「`data`、`match`、コンストラクタのパターン、型の適用の取り出し口」を足す
2. 「分類」の `run/` の箇条に「`data/`: `data`、コンストラクタ、`match`、パターン」を足し、`check-fail/` の箇条の「網羅性 (E4xxx) の検査を実装したら `exhaustiveness/` を足す」を「`exhaustiveness/` (E4xxx)」にする。

- [ ] **Step 10: 文書のリンターを通す**

Run:

```bash
for f in docs/spec/core-ir.md docs/spec/runtime.md docs/spec/types.md docs/spec/diagnostics.md docs/spec/exhaustiveness.md docs/implementation/architecture.md docs/implementation/status.md docs/implementation/test-changes.md docs/implementation/testing.md; do
  python3 ~/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py "$f" | head -5
done
```

Expected: このタスクで足した文に、文末のコロン、ダッシュ、太字にならない書き方の指摘がない。英単語の前後の半角空白と箇条書きの比率の指摘は、このリポジトリの文書の書き方なので直さない。

- [ ] **Step 11: 全体の検査を通してコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る (文書だけの変更なので、前のタスクの状態のまま)

```bash
git add docs/spec docs/implementation
git commit -m "Document stage 4a: data, match and exhaustiveness

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014YGQm6f7Mtged1yVY4dmJG"
```

