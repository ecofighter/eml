# 段階6a: 糖衣構文と E1xxx の残り 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 複数の等式、演算子の定義と fixity、セクション、`use`、`let ... in`、パターンの型の明示を HIR からインタプリタまで通し、`simplify` に分かっているコンストラクタの書き換えを足す。

**Architecture:** 構文の糖衣はすべて HIR で既存の形 (`match`、ラムダ、ブロック、呼び出し) に脱糖し、型検査以降には新しい式を足さない。fixity は Prelude と ユーザーの宣言から `ItemScope` が持つ表にする。等式の脱糖で生じるタプルの確保は、Core IR の `simplify` に足す一般の書き換え (K1、DCE) で消す。

**Tech Stack:** Rust (edition 2024)、rowan、la-arena、insta。

**Spec:** `docs/superpowers/specs/2026-10-05-stage-6a-syntax-sugar-design.md`

## Global Constraints

- コードのコメントと文書は日本語で書き、`yomiyasu:yomiyasu` の規則に従う。コメントは「なぜ」を書き、`docs/` を引くときはパスを書く
- 新しい診断の番号: E1018 `NON_CONSECUTIVE_EQUATIONS`、E1019 `SIGNATURE_NOT_ADJACENT`、E1020 `EQUATION_ARITY_MISMATCH`、E1021 `DUPLICATE_FIXITY`、E1022 `FIXITY_WITHOUT_DEFINITION`、E1023 `INVALID_SECTION`、E1024 `USE_AT_END_OF_BLOCK`、E4005 `UNREACHABLE_EQUATION` (Warning)
- `simplify` の順は F、B3、K1、B2、B5、B3、B4、DCE の1巡で、不動点まで繰り返さない
- 隠れた変数の名前は、等式の引数が `$0`、`$1`、…、演算子の参照が `$a`、`$b`、セクションが `$x`
- `run/` の UI テストの出力と `debug_heap` の検査は変えない。テストの変更は spec の節6の表にあるものだけで、ほかの期待値が変わったら作業を止めて相談する (CLAUDE.md の Testing)
- 種類1と種類2のテストの変更は `docs/implementation/test-changes.md` に記録する
- 各タスクの終わりに `cargo test`、`cargo clippy --all-targets`、`cargo fmt` を通してからコミットする
- `git diff` を確かめるときは `git diff --no-ext-diff` を使う (difftastic が設定されている)

## Review Focus

- `simplify` の K1 と B2 が、別の枝で束縛した変数や join point の引数を「分かっている値」と取り違えないこと。変数はちょうど1回束縛され (verifier の `bound twice`)、使用は束縛の範囲にあるので、関数全体の表で正しい。Task 1 と Task 2 のテストは、join point の引数で `switch` する形と、枝が値全体を使う形を含める
- DCE が、実行時エラーを起こす右辺 (`/`、`%`、オーバーフローする演算) や `Drop` を消さないこと。Task 1 に、使われない `1 / 0` が実行時エラーのまま残る run-fail テストを置く
- 等式の脱糖で、等式ごとの引数の数が違うとき (E1020)、引数が0個の値に等式が2つあるとき、シグネチャのない等式 (E1004) のときに、パニックせず、網羅性の診断を連鎖させないこと。Task 6 と Task 7 のテストで押さえる
- ユーザーが `&&` などの脱糖用の演算子や `+` を定義したとき、その定義が使われ、fixity が `infixl 9` になること。Task 5 のテストで押さえる
- セクションの被演算子に前置の `-` や括弧の中の演算子があるとき、E1023 を誤って出さないこと。Task 9 のテストで押さえる

---

## ファイルの構成

| ファイル | 役目 | タスク |
|---|---|---|
| `crates/eml_core_ir/src/simplify.rs` | F、K1、B2 の拡張、DCE を足す | 1、2、3 |
| `crates/eml_core_ir/src/lib.rs` | `PrimOp::may_fail` | 1 |
| `crates/eml_syntax/src/ast.rs` | `FixityItem`、`OpRef`、`LeftSection`、`RightSection`、`UseStmt`、`LetExpr` のアクセサ | 4、9、10 |
| `crates/eml_syntax/src/grammar/patterns.rs`、`grammar/expressions.rs` | `apat` で `(pat : type)` を受け付ける | 11 |
| `crates/eml_hir/src/prelude.em`、`lower/prelude.rs` | 標準の演算子の fixity の宣言 | 4 |
| `crates/eml_hir/src/lower/scope.rs` | `Fixity` と fixity の表 | 4 |
| `crates/eml_hir/src/lower/mod.rs` | fixity の宣言の収集、演算子の定義、等式の並びの検査 | 4、5、6 |
| `crates/eml_hir/src/lower/ops.rs` | 表を引く組み直し、`binary` の名前解決の順 | 4、5 |
| `crates/eml_hir/src/lower/expr.rs` | 等式の脱糖、`use`、`let ... in`、パターンの型の明示、引数の組 | 6、10、11、12 |
| `crates/eml_hir/src/lower/section.rs` (新規) | 演算子の参照とセクションの脱糖、E1023 | 9 |
| `crates/eml_hir/src/lower/handler.rs` | 節の引数の組 | 12 |
| `crates/eml_hir/src/hir.rs` | `MatchSource`、`Function::equation_ranges` | 6 |
| `crates/eml_hir/src/builtin.rs` | `fixity()` を消す | 4 |
| `crates/eml_types/src/exhaustive.rs` | 等式の E4002 と E4005 | 7 |
| `crates/eml_types/src/check/body.rs`、`check/report.rs` | 等式の連鎖の修正、パターンの型の明示の照合 | 8、11 |
| `docs/spec/*.md`、`docs/implementation/*.md` | 文書 | 3、13 |

---

### Task 1: K1 と DCE (分かっているコンストラクタの `switch` と、使われない純粋な束縛)

**Files:**
- Modify: `crates/eml_core_ir/src/simplify.rs`
- Modify: `crates/eml_core_ir/src/lib.rs` (`PrimOp`)
- Test: `crates/eml_core_ir/tests/simplify.rs`
- Create: `tests/ui/run/data/known_constructors.em`、`tests/ui/run-fail/basics/unused_division_by_zero.em`

**Interfaces:**
- Produces: `Simplify::known_constructors(&self) -> HashMap<VarId, (u32, Vec<Atom>)>`、自由関数 `known_value(known: &HashMap<VarId, (u32, Vec<Atom>)>, atom: Atom) -> Option<(u32, Vec<Atom>)>` (Task 2 が使う)、`PrimOp::may_fail(self) -> bool`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/simplify.rs` の末尾に足す。

```rust
/// `name` の関数だけの表示。関数は列0の `fn` で始まり、列0の `}` で終わる。
fn function(core: &str, name: &str) -> String {
    let start = core
        .find(&format!("fn {name}("))
        .expect("the function");
    let rest = &core[start..];
    let end = rest.find("\n}\n").map_or(rest.len(), |end| end + 2);
    rest[..end].to_string()
}

#[test]
fn a_match_on_a_tuple_literal_builds_no_tuple() {
    // `(a, b)` を作ってすぐ分解するので、K1 が `switch` を枝にし、DCE が使われなくなった `con` を消す
    let text = "pair : Int -> Int -> Int\npair a b = match (a, b) with\n  | (x, y) -> x + y\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    let pair = function(&core_text(text, Pass::Simplify), "pair");
    assert!(!pair.contains("con #"), "{pair}");
    assert!(!pair.contains("switch"), "{pair}");
}

#[test]
fn a_match_on_a_constructed_value_takes_its_arm() {
    let text = "data Option a = | None | Some a\n\nunwrap : Int -> Int\nunwrap x = match Some x with\n  | Some y -> y\n  | None -> 0\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    let unwrap = function(&core_text(text, Pass::Simplify), "unwrap");
    assert!(!unwrap.contains("con #"), "{unwrap}");
    assert!(!unwrap.contains("switch"), "{unwrap}");
}

#[test]
fn unused_bindings_that_cannot_fail_are_removed() {
    let text = "f : Int -> Int\nf x =\n  let p = (x, x)\n  let s = \"unused\"\n  1\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    let f = function(&core_text(text, Pass::Simplify), "f");
    assert!(!f.contains("con #"), "{f}");
    assert!(!f.contains("const"), "{f}");
}

#[test]
fn unused_bindings_that_can_fail_are_kept() {
    // `/` はゼロ除算で実行時エラーになるので、使われなくても消さない
    let text = "f : Int -> Int\nf x =\n  let q = x / 0\n  1\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    let f = function(&core_text(text, Pass::Simplify), "f");
    assert!(f.contains("prim /("), "{f}");
}

#[test]
fn a_switch_on_a_join_point_argument_is_not_known() {
    // join point の引数の値は jump ごとに違うので、K1 は `switch` を残す
    let text = "data Option a = | None | Some a\n\npick : Bool -> Int\npick c =\n  let o = if c then Some 1 else None\n  let n = match o with\n    | Some v -> v\n    | None -> 0\n  n + 1\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    let pick = function(&core_text(text, Pass::Simplify), "pick");
    assert!(pick.contains("switch c0"), "{pick}");
}
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_core_ir --test simplify`
Expected: `a_match_on_a_tuple_literal_builds_no_tuple`、`a_match_on_a_constructed_value_takes_its_arm`、`unused_bindings_that_cannot_fail_are_removed` が FAIL (`con #` か `switch` か `const` が残る)。ほかは PASS。

- [ ] **Step 3: `PrimOp::may_fail` を足す**

`crates/eml_core_ir/src/lib.rs` の `impl PrimOp` に足す。

```rust
    /// 実行時エラーを起こしうるプリミティブ。整数の演算はオーバーフローとゼロ除算で止まる (docs/spec/declarations.md の
    /// 標準の演算子の表)。`simplify` の DCE は、これらを使われなくても消さない。
    pub fn may_fail(self) -> bool {
        matches!(
            self,
            PrimOp::IntAdd
                | PrimOp::IntSub
                | PrimOp::IntMul
                | PrimOp::IntDiv
                | PrimOp::IntMod
                | PrimOp::IntNeg
        )
    }
```

- [ ] **Step 4: K1 と DCE を `simplify.rs` に足す**

`use` を次にする。

```rust
use std::collections::HashMap;

use crate::{Arm, Atom, CExpr, CExprId, CoreFn, JoinId, Program, Rhs, VarId};
```

`simplify` の呼び出しの並びを次にする (F は Task 3 で足す)。

```rust
        pass.inline_single_jumps();
        pass.switch_known_constructors();
        pass.split_known_tags();
        pass.forward_small_bodies();
        pass.inline_single_jumps();
        pass.remove_unused();
        pass.remove_dead_bindings();
        pass.renumber();
```

モジュールの先頭のコメントの順の説明を、「B3 (jump が1つ)、K1 (分かっているコンストラクタの `switch`)、B2 (分かっているタグ)、B5 (小さな本体)、B3、B4 (使われない)、DCE (使われない純粋な束縛) の順に1巡だけ回す。」に直し、K1 を最初の B3 の後に置く理由 (「B3 が join point を戻すときに作る引数の束縛 `let t = d` をたどって `d` の `con` まで届くため」) と、DCE を最後に置く理由 (「K1 と B2 が使わなくした `con` をまとめて消すため」) を足す。

`impl Simplify` に足す。

```rust
    /// 分かっているコンストラクタの値。`let v = con #k(a…)`、引数のないタグの束縛、別名 (`let v = u`) をたどった先である。
    /// 変数は関数の中で1回だけ束縛され (verifier が確かめる)、使用はつねに束縛の範囲にあるので、関数全体で1つの表でよい。
    fn known_constructors(&self) -> HashMap<VarId, (u32, Vec<Atom>)> {
        let mut direct = HashMap::new();
        let mut aliases = HashMap::new();
        for id in self.reachable() {
            let CExpr::Let { var, rhs, .. } = self.expr(id) else {
                continue;
            };
            match rhs {
                Rhs::Con { tag, args } => {
                    direct.insert(*var, (*tag, args.clone()));
                }
                Rhs::Atom(Atom::Tag(tag)) => {
                    direct.insert(*var, (*tag, Vec::new()));
                }
                Rhs::Atom(Atom::Var(other)) => {
                    aliases.insert(*var, *other);
                }
                _ => {}
            }
        }
        let mut known = direct.clone();
        for (&var, &first) in &aliases {
            // 別名は束縛より前の変数しか指さないので、たどっても輪にならない
            let mut target = first;
            while let Some(&next) = aliases.get(&target) {
                target = next;
            }
            if let Some(value) = direct.get(&target) {
                known.insert(var, value.clone());
            }
        }
        known
    }

    /// K1: `switch` の値が分かっているコンストラクタなら、その枝で置き換え、枝のフィールドの変数を値に置き換える。
    /// 値の束縛は `switch` を支配するので、値に使う変数は `switch` の位置で範囲にある。
    fn switch_known_constructors(&mut self) {
        let known = self.known_constructors();
        let mut parents = self.parents();
        for id in self.reachable() {
            let CExpr::Switch { scrutinee, arms } = self.expr(id).clone() else {
                continue;
            };
            let Some((tag, values)) = known_value(&known, scrutinee) else {
                continue;
            };
            let Some(arm) = arms
                .iter()
                .find(|arm| arm.tag == tag && arm.fields.len() == values.len())
            else {
                continue;
            };
            for (&field, &value) in arm.fields.iter().zip(&values) {
                self.substitute(arm.body, field, value);
            }
            self.replace(&mut parents, id, arm.body);
        }
    }

    /// DCE: 使われない変数の `let` のうち、右辺が実行時に何も起こさないものを消す。前順の逆にたどるので、内側の束縛を
    /// 先に消し、それで使われなくなった外側の束縛 (`con` の引数など) も同じ巡で消せる。
    fn remove_dead_bindings(&mut self) {
        let order = self.reachable();
        let mut uses = vec![0usize; self.function.vars.len()];
        for &id in &order {
            for atom in used_atoms(self.expr(id)) {
                if let Atom::Var(var) = atom {
                    uses[var.0 as usize] += 1;
                }
            }
        }
        let mut parents = self.parents();
        for &id in order.iter().rev() {
            let CExpr::Let { var, rhs, body } = self.expr(id).clone() else {
                continue;
            };
            if uses[var.0 as usize] > 0 || !pure(&rhs) {
                continue;
            }
            for atom in rhs.atoms() {
                if let Atom::Var(used) = atom {
                    uses[used.0 as usize] -= 1;
                }
            }
            self.replace(&mut parents, id, body);
        }
    }
```

ファイルの末尾の自由関数に足す。

```rust
/// `atom` が分かっているコンストラクタの値なら、そのタグとフィールドの値。
fn known_value(known: &HashMap<VarId, (u32, Vec<Atom>)>, atom: Atom) -> Option<(u32, Vec<Atom>)> {
    match atom {
        Atom::Tag(tag) => Some((tag, Vec::new())),
        Atom::Var(var) => known.get(&var).cloned(),
        Atom::Int(_) | Atom::Unit => None,
    }
}

/// 式が直接使う値。
fn used_atoms(expr: &CExpr) -> Vec<Atom> {
    let mut copy = expr.clone();
    copy.atoms_mut().into_iter().map(|atom| *atom).collect()
}

/// 消してもよい右辺。値を作るだけで、エフェクトも実行時エラーも起こさない。`con` と `MakeClosure` が所有権を受け取る
/// 値は、消すと Perceus がその値の生存の終わりに `decref` を入れるので、解放が早まるだけである。
fn pure(rhs: &Rhs) -> bool {
    match rhs {
        Rhs::Atom(_) | Rhs::ConstString(_) | Rhs::Con { .. } | Rhs::MakeClosure(..) => true,
        Rhs::Prim(op, _) => !op.may_fail(),
        Rhs::Call { .. } | Rhs::Io(..) | Rhs::Drop(_) => false,
    }
}
```

- [ ] **Step 5: テストを通す**

Run: `cargo test -p eml_core_ir`
Expected: 新しい5件が PASS。既存のスナップショットのうち、spec の節6の表にある `simplify.rs` の3件と `perceus.rs` の1件以外が変わったら、作業を止めて相談する。この時点では表の4件もまだ変わらないはずである (B2 の拡張は Task 2)。

- [ ] **Step 6: UI テストを足す**

`tests/ui/run/data/known_constructors.em`:

```haskell
-- A tuple or a constructor built only to be matched is taken apart without being allocated.
data Option a =
  | None
  | Some a

add : Int -> Int -> Int
add a b = match (a, b) with
  | (x, y) -> x + y

unwrap : Int -> Int
unwrap x = match Some x with
  | Some y -> y
  | None -> 0

main : Unit -> <IO> Unit
main () =
  println (show_int (add 1 2))
  println (show_int (unwrap 7))
```

`tests/ui/run-fail/basics/unused_division_by_zero.em`:

```haskell
-- An unused division by zero still stops the program: removing unused bindings keeps the ones that can fail.
main : Unit -> <IO> Unit
main () =
  let unused = 1 / 0
  println "unreachable"
```

Run: `cargo test -p eml_cli --test ui`、続けて `cargo insta review`
Expected: `known_constructors` の stdout は `3` と `7` の2行。`unused_division_by_zero` はゼロ除算の実行時エラーで、stdout は空。どちらもこの内容なら受け入れる。

- [ ] **Step 7: 全体を確かめてコミット**

```bash
cargo test && cargo clippy --all-targets && cargo fmt
git add crates/eml_core_ir tests/ui/run/data/known_constructors.em tests/ui/run-fail/basics/unused_division_by_zero.em crates/eml_cli/tests/snapshots
git commit -m "Take the arm of a switch on a known constructor and remove unused pure bindings"
```

---

### Task 2: B2 をフィールドを持つコンストラクタに広げる

**Files:**
- Modify: `crates/eml_core_ir/src/simplify.rs` (`split_known_tags` を置き換え、`known_tag` を消す)
- Test: `crates/eml_core_ir/tests/simplify.rs`、`crates/eml_core_ir/tests/perceus.rs`
- Modify: `tests/ui/run/data/known_constructors.em`

**Interfaces:**
- Consumes: Task 1 の `known_constructors`、`known_value`
- Produces: `Simplify::fresh_like(&mut self, var: VarId) -> VarId`、`Simplify::uses(&self, root: CExprId, var: VarId) -> bool`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/simplify.rs` に足す。

```rust
#[test]
fn a_jump_that_passes_a_constructed_value_goes_to_its_arm() {
    // どの jump も `Some` の値を渡すので、`Some n` の枝をフィールドを引数に取る join point にし、`con` は消える
    let text = "data Option a = | None | Some a\n\npick : Bool -> Int\npick c =\n  let n = match (if c then Some 1 else Some 2) with\n    | Some n -> n\n    | None -> 0\n  n + 1\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    let pick = function(&core_text(text, Pass::Simplify), "pick");
    assert!(!pick.contains("con #"), "{pick}");
}

#[test]
fn an_arm_that_uses_the_whole_value_also_receives_it() {
    // 変数の枝 `x` は scrutinee そのものを受けるので、切り出した join point に値も渡し、`con` は残る。
    // 新しい join point の引数は新しい変数にするので、verifier の「2回束縛」にならない
    let text = "data Option a = | None | Some a\n\nsize : Option Int -> Int\nsize o = 1\n\npick : Bool -> Int\npick c =\n  let n = match (if c then Some 1 else None) with\n    | None -> 0\n    | x -> size x\n  n + 1\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    let pick = function(&core_text(text, Pass::Simplify), "pick");
    assert!(pick.contains("con #1(1)"), "{pick}");
    // Perceus の後の verifier も通る
    core_text(text, Pass::Perceus);
}
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_core_ir --test simplify`
Expected: `a_jump_that_passes_a_constructed_value_goes_to_its_arm` が FAIL (`con #1(1)` などが残る)。もう1件は今の実装でも PASS しうる。PASS ならそのまま残す (B2 の拡張で壊さないことを確かめるテストになる)。

- [ ] **Step 3: `split_known_tags` を置き換える**

`known_tag` を消し、`split_known_tags` の全体を次に置き換える。doc コメントは B2 の新しい規則に合わせる。

```rust
    /// B2: 引数を1つだけ持ち、本体がその引数で分岐する join point に、分かっているコンストラクタの値を渡す jump があれば、
    /// その値の枝を join point に切り出し、jump をその枝へ直接向ける (docs/spec/core-ir.md)。引数のない枝は今までどおり
    /// すべて切り出し、枝の中の引数をタグの定数に置き換える。フィールドを持つ枝は、分かっている値が届くものだけを、
    /// フィールドを引数に取る join point にし、jump はフィールドの値を渡す。枝が値全体も使うなら、値も最後の引数で渡す。
    /// 切り出した join point の引数は新しい変数にする。元の枝のフィールドと join point の引数は、どちらも束縛だからである。
    fn split_known_tags(&mut self) {
        let known = self.known_constructors();
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
            if scrutinee != param {
                continue;
            }
            let values: Vec<Option<(u32, Vec<Atom>)>> = sites
                .iter()
                .map(|&site| match self.expr(site) {
                    CExpr::Jump { args, .. } => match args.as_slice() {
                        [arg] => known_value(&known, *arg),
                        _ => None,
                    },
                    _ => None,
                })
                .collect();
            let fits = |(tag, fields): &(u32, Vec<Atom>)| {
                arms.iter()
                    .any(|arm| arm.tag == *tag && arm.fields.len() == fields.len())
            };
            if values.iter().all(Option::is_none) || !values.iter().flatten().all(fits) {
                continue;
            }
            let targeted: Vec<u32> = values.iter().flatten().map(|(tag, _)| *tag).collect();
            // (タグ, 切り出した join point, 本体, 引数, 値全体も渡すか)
            let mut split: Vec<(u32, JoinId, CExprId, Vec<VarId>, bool)> = Vec::new();
            let mut dispatch = Vec::new();
            for arm in &arms {
                if !arm.fields.is_empty() && !targeted.contains(&arm.tag) {
                    dispatch.push(arm.clone());
                    continue;
                }
                let mut arm_params = Vec::new();
                for &field in &arm.fields {
                    let fresh = self.fresh_like(field);
                    self.substitute(arm.body, field, Atom::Var(fresh));
                    arm_params.push(fresh);
                }
                let whole = if arm.fields.is_empty() {
                    self.substitute(arm.body, param, Atom::Tag(arm.tag));
                    false
                } else if self.uses(arm.body, param) {
                    let fresh = self.fresh_like(param);
                    self.substitute(arm.body, param, Atom::Var(fresh));
                    arm_params.push(fresh);
                    true
                } else {
                    false
                };
                let arm_join = JoinId(self.function.joins.len() as u32);
                // 索引は、下で組み立てた `Join` の位置に直す
                self.function.joins.push(arm.body);
                let mut args: Vec<Atom> = arm.fields.iter().map(|&field| Atom::Var(field)).collect();
                if whole {
                    args.push(Atom::Var(param));
                }
                let jump = self.push(CExpr::Jump {
                    join: arm_join,
                    args,
                });
                dispatch.push(Arm {
                    tag: arm.tag,
                    fields: arm.fields.clone(),
                    body: jump,
                });
                split.push((arm.tag, arm_join, arm.body, arm_params, whole));
            }
            self.set(
                body,
                CExpr::Switch {
                    scrutinee: Atom::Var(param),
                    arms: dispatch,
                },
            );
            for (&site, value) in sites.iter().zip(&values) {
                let Some((tag, fields)) = value else {
                    continue;
                };
                let (_, arm_join, _, _, whole) = split
                    .iter()
                    .find(|(arm_tag, ..)| arm_tag == tag)
                    .expect("checked above");
                let CExpr::Jump { args: passed, .. } = self.expr(site) else {
                    unreachable!("a jump site holds a jump")
                };
                let mut args = fields.clone();
                if *whole {
                    args.push(passed[0]);
                }
                self.set(
                    site,
                    CExpr::Jump {
                        join: *arm_join,
                        args,
                    },
                );
            }
            // 枝の join point を外側に並べ、元の join point をいちばん内側に置く。枝は元の join point の定義全体を
            // 範囲にするので、元の本体からも、範囲の中の jump からも届く。元の位置には最初の枝の join point が入る
            let mut inner = self.push(CExpr::Join {
                join,
                params: vec![param],
                captures: Vec::new(),
                body,
                scope,
            });
            self.function.joins[index] = inner;
            for (position, (_, arm_join, arm, arm_params, _)) in split.iter().enumerate().rev() {
                let expr = CExpr::Join {
                    join: *arm_join,
                    params: arm_params.clone(),
                    captures: Vec::new(),
                    body: *arm,
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

    /// `var` と同じ名前と性質の新しい変数。
    fn fresh_like(&mut self, var: VarId) -> VarId {
        let info = self.function.vars[var.0 as usize].clone();
        self.function.vars.push(info);
        VarId(self.function.vars.len() as u32 - 1)
    }

    /// `root` の部分木が `var` を使うか。
    fn uses(&self, root: CExprId, var: VarId) -> bool {
        let mut work = vec![root];
        while let Some(id) = work.pop() {
            if used_atoms(self.expr(id)).contains(&Atom::Var(var)) {
                return true;
            }
            work.extend(children(self.expr(id)));
        }
        false
    }
```

- [ ] **Step 4: テストを通し、変わるスナップショットを確かめる**

Run: `cargo test -p eml_core_ir`
Expected: 新しいテストが PASS。次の4件のスナップショットが変わる (spec の節6の種類2)。`cargo insta review` で、`con` を渡していた jump がフィールドの値を渡す jump になり、使われなくなった `con` が消えていることを確かめて受け入れる。

- `simplify.rs`: `a_mixed_switch_splits_only_the_arms_without_fields`、`arms_with_fields_keep_the_join_point_argument`、`jumps_that_pass_constructed_values_are_left_alone`
- `perceus.rs`: `a_split_switch_still_unpacks_the_arm_with_fields`

名前と冒頭のコメントが新しい内容と合わなくなったものは直す。たとえば `jumps_that_pass_constructed_values_are_left_alone` は `jumps_that_pass_constructed_values_go_to_their_arms` にし、コメントの「後で入れる」の説明を消す。この4件以外が変わったら作業を止めて相談する。

- [ ] **Step 5: UI テストに例を足す**

`tests/ui/run/data/known_constructors.em` の `main` の前に足し、`main` で呼ぶ。

```haskell
size : Option Int -> Int
size o = match o with
  | None -> 0
  | Some _ -> 1

pick : Bool -> Int
pick c =
  let n = match (if c then Some 10 else Some 20) with
    | Some n -> n
    | None -> 0
  n + 1

whole : Bool -> Int
whole c =
  let n = match (if c then Some 1 else None) with
    | None -> 0
    | x -> size x
  n + 1
```

`main` の末尾に足す行:

```haskell
  println (show_int (pick True))
  println (show_int (pick False))
  println (show_int (whole True))
  println (show_int (whole False))
```

Run: `cargo test -p eml_cli --test ui`、続けて `cargo insta review`
Expected: stdout の追加の行は `11`、`21`、`2`、`1`。この内容なら受け入れる。

- [ ] **Step 6: test-changes.md に記録してコミット**

`docs/implementation/test-changes.md` の末尾に、段階6a の節を作って (既存の節の書き方に合わせる)、上の4件を種類2として記録する。理由は「B2 がフィールドを持つコンストラクタの値を渡す jump を枝へ直接向け、DCE が使われなくなった `con` を消すようになったため」。

```bash
cargo test && cargo clippy --all-targets && cargo fmt
git add crates/eml_core_ir tests/ui/run/data/known_constructors.em crates/eml_cli/tests/snapshots docs/implementation/test-changes.md
git commit -m "Route jumps that pass constructed values to their arms"
```

---

### Task 3: F (join point の外出し) と `simplify` の文書

**Files:**
- Modify: `crates/eml_core_ir/src/simplify.rs`
- Test: `crates/eml_core_ir/tests/simplify.rs`
- Modify: `tests/ui/run/data/known_constructors.em`
- Modify: `docs/spec/core-ir.md`、`docs/implementation/architecture.md`、`docs/implementation/status.md`

- [ ] **Step 1: 失敗するテストを書く**

```rust
#[test]
fn a_wildcard_arm_does_not_block_the_known_tags() {
    // 決定木が `if` の join point の本体の中に置いた残りの枝の join point を F が外へ出すので、B2 が `switch` に届き、
    // `if` の結果で分岐し直さない (docs/implementation/status.md にあった制限)
    let text = "data Color = | Red | Green | Blue\n\npick : Bool -> Int\npick b =\n  let n = match (if b then Red else Green) with\n    | Red -> 1\n    | _ -> 2\n  n + 1\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    let pick = function(&core_text(text, Pass::Simplify), "pick");
    assert_eq!(pick.matches("switch").count(), 1, "{pick}");
}
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_core_ir --test simplify a_wildcard_arm_does_not_block_the_known_tags`
Expected: FAIL (`switch` が2つ。`b0` の `switch` と、join point の引数の `switch`)

- [ ] **Step 3: F を足す**

`simplify` の呼び出しの先頭に `pass.float_joins();` を足し、モジュールの先頭のコメントの順の説明の頭に「F (join point の外出し)」を足す。F を最初に置く理由 (「決定木が置いた残りの枝の join point を外へ出し、最初の B3 と B2 が `Switch` に届くようにするため」と「パイプラインが直前に `captures` を埋めているので、F だけは正しい `captures` を使える」) も書く。

```rust
    /// F: join point の本体の先頭に並ぶ join point の定義を、外側の join point の引数を使わなければ、外側の定義の位置へ
    /// 出す。外側の本体は外へ出した join point の範囲に入るので、本体の中の jump はそのまま届く。外へ出す本体は外側の
    /// 引数を使わず、外側の本体の中で束縛した変数も使えない (先頭に並ぶので、その前に束縛はない) ので、外側の定義の位置
    /// でも範囲にある変数しか使わない。式の ID を入れ替えるだけなので、親の表は変わらない。
    fn float_joins(&mut self) {
        for index in 0..self.function.joins.len() {
            let mut node = self.function.joins[index];
            loop {
                let CExpr::Join {
                    join,
                    params,
                    captures,
                    body,
                    scope,
                } = self.expr(node).clone()
                else {
                    unreachable!("the join index points at join points")
                };
                let CExpr::Join {
                    join: inner,
                    params: inner_params,
                    captures: inner_captures,
                    body: inner_body,
                    scope: inner_scope,
                } = self.expr(body).clone()
                else {
                    break;
                };
                if inner_captures.iter().any(|var| params.contains(var)) {
                    break;
                }
                self.set(
                    node,
                    CExpr::Join {
                        join: inner,
                        params: inner_params,
                        captures: inner_captures,
                        body: inner_body,
                        scope: body,
                    },
                );
                self.set(
                    body,
                    CExpr::Join {
                        join,
                        params,
                        captures,
                        body: inner_scope,
                        scope,
                    },
                );
                self.function.joins[inner.0 as usize] = node;
                self.function.joins[join.0 as usize] = body;
                node = body;
            }
        }
    }
```

- [ ] **Step 4: テストを通す**

Run: `cargo test -p eml_core_ir`
Expected: 新しいテストが PASS。Task 2 で受け入れた4件と、このタスクの新しいテスト以外のスナップショットが変わったら、作業を止めて相談する。

- [ ] **Step 5: UI テストに例を足す**

`tests/ui/run/data/known_constructors.em` に足す。

```haskell
data Color =
  | Red
  | Green
  | Blue

score : Bool -> Int
score b =
  let n = match (if b then Red else Green) with
    | Red -> 1
    | _ -> 2
  n + 1
```

`main` の末尾に `println (show_int (score True))` と `println (show_int (score False))` を足す。

Run: `cargo test -p eml_cli --test ui`、続けて `cargo insta review`
Expected: 追加の行は `2` と `3`。

- [ ] **Step 6: 文書を直す**

- `docs/spec/core-ir.md` の `simplify` の段落 (「`simplify` は、変換の後、Perceus の前に置き」で始まる段落) を、spec の節1のとおりに書き換える。書き換えは、F、K1、B2 (フィールドを持つコンストラクタを含む)、B5、B3、B4、DCE の7つで、順は F、B3、K1、B2、B5、B3、B4、DCE の1巡である。DCE が消す右辺と消さない右辺、エフェクトの順と短絡評価が変わらない理由を書く
- `docs/implementation/architecture.md` の `simplify` の項目 (「`simplify` (`simplify.rs`) は」で始まる項目) を同じ内容で書き換える。分かっているコンストラクタの表を関数全体で1つ作る理由 (変数の束縛が1回で、使用が束縛の範囲にあること) と、切り出す join point の引数を新しい変数にする理由を書く
- `docs/implementation/status.md` の「次の作業の注意点」から、「後で入れる最適化: 引数を持つコンストラクタの case-of-case」の項と「case-of-case (B2) は、`match` にワイルドカードの枝があると働かない」の項を消す。scrutinee そのものを受ける枝の join point は F で外へ出せない、という残りの制限を1項で足す

- [ ] **Step 7: コミット**

```bash
cargo test && cargo clippy --all-targets && cargo fmt
git add crates/eml_core_ir tests/ui/run/data/known_constructors.em crates/eml_cli/tests/snapshots docs
git commit -m "Float join points out of join bodies so known tags reach the switch"
```

---

### Task 4: fixity の表と fixity の宣言

**Files:**
- Modify: `crates/eml_syntax/src/ast.rs` (`FixityItem` のアクセサ、`operator_token`)
- Modify: `crates/eml_hir/src/prelude.em`、`crates/eml_hir/src/lower/prelude.rs`
- Modify: `crates/eml_hir/src/lower/scope.rs`、`crates/eml_hir/src/lower/mod.rs`、`crates/eml_hir/src/lower/ops.rs`、`crates/eml_hir/src/lower/expr.rs`
- Modify: `crates/eml_hir/src/builtin.rs` (`fixity()` を消す)、`crates/eml_hir/src/lib.rs` (コード)
- Test: `crates/eml_hir/tests/operators.rs`
- Create: `tests/ui/check-fail/names/fixity_declarations.em`

**Interfaces:**
- Produces: `ast::FixityItem::{assoc() -> Option<SyntaxToken>, precedence() -> Option<SyntaxToken>, operators() -> impl Iterator<Item = SyntaxToken>}`、`ast.rs` の自由関数 `operator_token(node: &SyntaxNode) -> Option<SyntaxToken>` (Task 9 が使う)、`lower::scope::Fixity { precedence: u8, assoc: Assoc }`、`Fixity::DEFAULT`、`ItemScope::{declare_prelude_fixity, declare_fixity, defines_value, fixity}`、`codes::DUPLICATE_FIXITY`、`codes::FIXITY_WITHOUT_DEFINITION`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_hir/tests/operators.rs` に足す。`common::diagnostics` も使うので、`use common::{diagnostics, lower_text};` にする。

```rust
#[test]
fn a_declared_fixity_regroups_a_constructor_operator() {
    // 宣言がなければ `infixl 9` で `(a :+ b) :+ c` になるところを、`infixr` で右に組む
    let text = "infixr 5 :+\ndata P = | E | Int :+ P\n\np : P\np = 1 :+ 2 :+ E";
    insta::assert_snapshot!(lower_text(text), @r"
    data P
      | E
      | Int :+ P
    p : P
    p = (:+ 1 (:+ 2 E))
    ");
}

#[test]
fn a_fixity_declared_after_its_use_still_applies() {
    let text = "data P = | E | Int :+ P\n\np : P\np = 1 :+ 2 :+ E\n\ninfixr 5 :+";
    insta::assert_snapshot!(lower_text(text), @r"
    data P
      | E
      | Int :+ P
    p : P
    p = (:+ 1 (:+ 2 E))
    ");
}

#[test]
fn fixity_declarations_need_one_definition_in_this_module() {
    let text = "infixr 5 :+\ninfixl 6 :+\ninfixl 6 +\ninfix 4 <=>\ndata P = | E | Int :+ P\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    assert_eq!(
        diagnostics(text),
        vec![
            "E1021 2:10 `:+` has more than one fixity declaration",
            "E1022 3:10 `+` is not defined in this module",
            "E1022 4:9 `<=>` is not defined in this module",
        ]
    );
}
```

`p = (:+ 1 (:+ 2 E))` の形と、`data` の表示の形 (`| Int :+ P`) は今の表示の規則から予想したものである。予想と違ったら、表示の細部ではなく組み方 (`:+` が右に組まれていること) を確かめて期待値を直す。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_hir --test operators`
Expected: 3件とも FAIL (今は fixity の宣言が E0004)

- [ ] **Step 3: `FixityItem` のアクセサを足す**

`crates/eml_syntax/src/ast.rs` に足す。

```rust
impl FixityItem {
    /// `infixl`、`infixr`、`infix` のキーワード。
    pub fn assoc(&self) -> Option<SyntaxToken> {
        self.syntax.first_token().filter(|token| {
            matches!(
                token.kind(),
                SyntaxKind::INFIXL_KW | SyntaxKind::INFIXR_KW | SyntaxKind::INFIX_KW
            )
        })
    }

    pub fn precedence(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .find(|token| token.kind() == SyntaxKind::INT)
    }

    /// 宣言した演算子。`-` と `:` で始まる演算子も含む。
    pub fn operators(&self) -> impl Iterator<Item = SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .filter(|token| is_operator(token.kind()))
    }
}

/// ノードの直接の子の、最初の演算子のトークン。演算子の参照とセクションが使う。
fn operator_token(node: &SyntaxNode) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|token| is_operator(token.kind()))
}

fn is_operator(kind: SyntaxKind) -> bool {
    matches!(kind, SyntaxKind::OP | SyntaxKind::MINUS | SyntaxKind::CONOP)
}
```

`operator_token` は Task 9 まで使われないので、`#[allow(dead_code)]` は付けず、Task 9 と同じコミットに入れてもよい。clippy が未使用を指摘するなら、このタスクでは `operator_token` を書かずに Task 9 で足す。

- [ ] **Step 4: `Fixity` と表を `ItemScope` に足す**

`crates/eml_hir/src/lower/scope.rs`:

```rust
use eml_diagnostics::TextRange;

use crate::builtin::{Assoc, Builtin};

/// 演算子の優先順位と結合 (docs/spec/declarations.md の「fixity」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Fixity {
    pub precedence: u8,
    pub assoc: Assoc,
}

impl Fixity {
    /// fixity の宣言がない演算子 (Haskell と同じ)。
    pub(super) const DEFAULT: Fixity = Fixity {
        precedence: 9,
        assoc: Assoc::Left,
    };
}
```

`ItemScope` にフィールドを足す。

```rust
    /// Prelude の演算子の fixity。
    prelude_fixities: HashMap<String, Fixity>,
    /// ユーザーが宣言した fixity と、宣言の演算子の位置。
    fixities: HashMap<String, (Fixity, TextRange)>,
```

`impl ItemScope` に足す。

```rust
    pub(super) fn declare_prelude_fixity(&mut self, op: &str, fixity: Fixity) {
        self.prelude_fixities.insert(op.to_string(), fixity);
    }

    /// 2回目の宣言なら、1回目の演算子の位置を返し、表を変えない。
    pub(super) fn declare_fixity(
        &mut self,
        op: &str,
        fixity: Fixity,
        range: TextRange,
    ) -> Result<(), TextRange> {
        if let Some((_, first)) = self.fixities.get(op) {
            return Err(*first);
        }
        self.fixities.insert(op.to_string(), (fixity, range));
        Ok(())
    }

    /// このモジュールで定義した値か。Prelude の `data` のコンストラクタも同じ表にあるが、演算子の名前のものはない。
    pub(super) fn defines_value(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }

    /// 演算子の fixity。fixity は名前が解決した先の定義に付く。ユーザーの定義は Prelude の演算子を隠すので、宣言の
    /// ないユーザーの演算子は `infixl 9` である (docs/spec/declarations.md の「fixity」)。
    pub(super) fn fixity(&self, op: &str) -> Fixity {
        if let Some((fixity, _)) = self.fixities.get(op) {
            return *fixity;
        }
        if self.values.contains_key(op) {
            return Fixity::DEFAULT;
        }
        self.prelude_fixities
            .get(op)
            .copied()
            .unwrap_or(Fixity::DEFAULT)
    }
```

`crates/eml_hir/src/builtin.rs` から `fixity()` 関数を消す。`Assoc` は残す。`Assoc` の上の doc コメント「標準の演算子の表 (docs/spec/declarations.md)。fixity の宣言は段階6で読む。」を、「演算子の結合の向き。標準の演算子の fixity は Prelude (`prelude.em`) の宣言で持つ (docs/spec/declarations.md の「fixity」)。」にする。

- [ ] **Step 5: Prelude に fixity の宣言を置く**

`crates/eml_hir/src/prelude.em` の先頭のコメントの後に足す。

```haskell
-- 標準の演算子の fixity (docs/spec/declarations.md の表)。`&&`、`||`、`|>`、`<|` はシグネチャがなく HIR で脱糖し、
-- `::` は S2 のリストの演算子だが、fixity は Prelude の中でだけ宣言できる。
infixr 0 <|
infixl 1 |>
infixr 2 ||
infixr 3 &&
infix 4 ==, !=, <, <=, >, >=
infixr 5 ++, ::
infixl 6 +, -
infixl 7 *, /, %
infixr 9 >>, <<
```

`crates/eml_hir/src/lower/prelude.rs` の `lower_prelude` で、シグネチャの変換の前に読む。

```rust
    for item in tree.items() {
        let ast::Item::FixityItem(item) = item else {
            continue;
        };
        let fixity = super::fixity_of(&item).expect("every Prelude fixity is well formed");
        for op in item.operators() {
            scope.declare_prelude_fixity(op.text(), fixity);
        }
    }
```

同じファイルに、spec の表と一致することを確かめる単体テストを足す。

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::Assoc;
    use crate::lower::scope::{Fixity, ItemScope};

    #[test]
    fn prelude_fixities_follow_the_standard_table() {
        // docs/spec/declarations.md の標準の演算子の表
        let table: &[(&[&str], u8, Assoc)] = &[
            (&["<|"], 0, Assoc::Right),
            (&["|>"], 1, Assoc::Left),
            (&["||"], 2, Assoc::Right),
            (&["&&"], 3, Assoc::Right),
            (&["==", "!=", "<", "<=", ">", ">="], 4, Assoc::None),
            (&["++", "::"], 5, Assoc::Right),
            (&["+", "-"], 6, Assoc::Left),
            (&["*", "/", "%"], 7, Assoc::Left),
            (&[">>", "<<"], 9, Assoc::Right),
        ];
        let mut scope = ItemScope::new();
        let mut types = Arena::new();
        let mut effects = Arena::new();
        super::super::scope::builtin_items(&mut types, &mut effects, &mut scope);
        lower_prelude(&mut scope, &mut types, &mut Arena::new());
        for (ops, precedence, assoc) in table {
            for op in *ops {
                assert_eq!(
                    scope.fixity(op),
                    Fixity {
                        precedence: *precedence,
                        assoc: *assoc
                    },
                    "{op}"
                );
            }
        }
    }
}
```

`builtin_items` の引数の型が上と違えば、`lower/mod.rs` の `lower` の呼び方に合わせる。

- [ ] **Step 6: ユーザーの fixity の宣言を集めて検査する**

`crates/eml_hir/src/lib.rs` の `codes` に足す。

```rust
    pub const DUPLICATE_FIXITY: ErrorCode = ErrorCode(1021);
    pub const FIXITY_WITHOUT_DEFINITION: ErrorCode = ErrorCode(1022);
```

`crates/eml_hir/src/lower/mod.rs`:

- `collect` の戻り値に `Vec<ast::FixityItem>` を足し、`ast::Item::FixityItem(item) => fixity_items.push(item)` にする (E0004 を消す)
- 関数を `scope.define_function` で定義し終えた後、本体を変換する前 (`// 本体は、すべての関数の名前がそろってから変換する` の直前) に、次を呼ぶ

```rust
    // fixity の宣言は位置によらずモジュール全体の組み直しに効くので、本体の変換の前に、すべての値を定義してから読む
    declare_fixities(file, &fixity_items, &mut scope, &mut diagnostics);
```

```rust
/// fixity の宣言の結合と優先順位。優先順位の範囲の誤りはパーサが報告済みなので、読めなければ `None` にする。
pub(super) fn fixity_of(item: &ast::FixityItem) -> Option<Fixity> {
    let assoc = match item.assoc()?.kind() {
        SyntaxKind::INFIXL_KW => Assoc::Left,
        SyntaxKind::INFIXR_KW => Assoc::Right,
        _ => Assoc::None,
    };
    let precedence = item
        .precedence()?
        .text()
        .parse::<u8>()
        .ok()
        .filter(|precedence| *precedence <= 9)?;
    Some(Fixity { precedence, assoc })
}

/// ユーザーの fixity の宣言を表に入れる。同じ演算子への2回目の宣言は E1021、このモジュールで定義していない演算子
/// への宣言は E1022 にし、どちらも表に入れない (docs/spec/declarations.md の「fixity」)。
fn declare_fixities(
    file: FileId,
    items: &[ast::FixityItem],
    scope: &mut ItemScope,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for item in items {
        let Some(fixity) = fixity_of(item) else {
            continue;
        };
        for op in item.operators() {
            let (name, range) = (op.text(), op.text_range());
            if !scope.defines_value(name) {
                diagnostics.push(Diagnostic::error(
                    codes::FIXITY_WITHOUT_DEFINITION,
                    format!("`{name}` is not defined in this module"),
                    Label::new(file, range, "a fixity declaration needs a definition of its operator in the same module"),
                ));
                continue;
            }
            if let Err(first) = scope.declare_fixity(name, fixity, range) {
                diagnostics.push(
                    Diagnostic::error(
                        codes::DUPLICATE_FIXITY,
                        format!("`{name}` has more than one fixity declaration"),
                        Label::new(file, range, "declared again here"),
                    )
                    .with_secondary(Label::new(file, first, "first declared here")),
                );
            }
        }
    }
}
```

`use` に `scope::Fixity` と `crate::builtin::Assoc` を足す。

- [ ] **Step 7: 組み直しが表を引くようにする**

`crates/eml_hir/src/lower/ops.rs` の `climb`:

```rust
            // fixity は名前が解決した先の定義に付く (docs/spec/declarations.md の「fixity」)
            let Fixity { precedence, assoc } = self.items.fixity(&text);
```

`crates/eml_hir/src/lower/expr.rs` の `climb_pat` も同じく `self.items.fixity(operator.text())` を使う。`climb_pat` の doc コメントの「表の中で `:` で始まる演算子は `::` だけなので、同じ優先順位で結合の向きが違う並びは起きず、E1006 は出さない。」は、ユーザーが `:` で始まる演算子に fixity を宣言できるようになったので、「中置のコンストラクタの並びで同じ優先順位の結合の向きが違っても E1006 は出さず、左から順に組む。パターンの組み方は値の束縛に影響しないためである」と直すか、E1006 を出すようにする。E1006 を出すなら `climb` と同じ検査を足し、テストを1件足す。どちらにしたかを spec に書き足す。`fixity` と `Assoc` の `use` を直す。

- [ ] **Step 8: テストを通す**

Run: `cargo test -p eml_hir`
Expected: 新しいテストが PASS。既存の `operators.rs` の組み直しのテストの期待値は変わらない (Prelude の表が今の表と同じであるため)。

- [ ] **Step 9: UI テストを足してコミット**

`tests/ui/check-fail/names/fixity_declarations.em`:

```haskell
-- E1021 and E1022: one fixity per operator, and only for operators defined in this module.
infixr 5 :+
infixl 6 :+
infixl 6 +

data P =
  | E
  | Int :+ P

main : Unit -> <IO> Unit
main () = ()
```

Run: `cargo test -p eml_cli --test ui`、続けて `cargo insta review` で、E1021 (2回目の `:+` が primary、1回目が secondary) と E1022 (`+`) の2件が出ていることを確かめて受け入れる。

```bash
cargo test && cargo clippy --all-targets && cargo fmt
git add crates/eml_syntax crates/eml_hir tests/ui/check-fail/names/fixity_declarations.em crates/eml_cli/tests/snapshots
git commit -m "Read fixity declarations from the Prelude and the module"
```

---

### Task 5: 演算子の定義

**Files:**
- Modify: `crates/eml_hir/src/lower/mod.rs` (`value_name`)
- Modify: `crates/eml_hir/src/lower/ops.rs` (`binary`)
- Test: `crates/eml_hir/tests/lower.rs`、`crates/eml_hir/tests/operators.rs`
- Create: `tests/ui/run/functions/user_operators.em`

**Interfaces:**
- Produces: `BodyLowering::binary` を `pub(super)` にする (Task 9 が使う)

- [ ] **Step 1: 失敗するテストを書き、種類1のテストを直す**

`crates/eml_hir/tests/operators.rs` に足す。

```rust
#[test]
fn user_operators_are_functions_with_their_own_fixity() {
    let text = "infixr 5 <+>\n(<+>) : Int -> Int -> Int\na <+> b = a + b\n\nf : Int\nf = 1 <+> 2 <+> 3";
    insta::assert_snapshot!(lower_text(text), @r"
    <+> : Int -> Int -> Int
    <+> a#0 b#1 = (+ a#0 b#1)
    f : Int
    f = (@<+> 1 (@<+> 2 3))
    ");
}

#[test]
fn a_user_operator_without_a_fixity_is_infixl_9() {
    // ユーザーの `+` は Prelude の `+` を隠し、宣言がないので `infixl 9` になる。`*` (7) より強く結合する
    let text = "(+) : Int -> Int -> Int\na + b = a - b\n\nf : Int\nf = 1 * 2 + 3";
    insta::assert_snapshot!(lower_text(text), @r"
    + : Int -> Int -> Int
    + a#0 b#1 = (- a#0 b#1)
    f : Int
    f = (* 1 (@+ 2 3))
    ");
}

#[test]
fn a_user_definition_hides_a_desugared_operator() {
    // ユーザーが `&&` を定義すると、短絡の `if` ではなく普通の呼び出しになる
    let text = "(&&) : Bool -> Bool -> Bool\na && b = b\n\nf : Bool\nf = True && False";
    insta::assert_snapshot!(lower_text(text), @r"
    && : Bool -> Bool -> Bool
    && a#0 b#1 = b#1
    f : Bool
    f = (@&& True False)
    ");
}
```

`crates/eml_hir/tests/lower.rs` の `operator_definitions_and_qualified_names` は種類1の変更である。テキストはそのままにし、期待値の E0004 の2行 (`defining operators`) が消え、`<+>` の関数が表示されるようにする。新しい期待値は `cargo insta review` で確かめる (`<+> : Int`、`<+> a#0 b#1 = a#0`、E1020 は出ない。シグネチャが `Int` で引数が2つなので、型の誤りは型検査が出す)。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_hir --test operators --test lower`
Expected: 新しい3件と `operator_definitions_and_qualified_names` が FAIL

- [ ] **Step 3: 演算子の名前を受け付ける**

`crates/eml_hir/src/lower/mod.rs` の `value_name` を次にする。doc コメントも直す。

```rust
/// シグネチャと等式の名前。演算子の定義 (`(</>) : …` と `a </> b = …`) は、演算子の文字列を名前にした関数である
/// (docs/spec/declarations.md の「fixity」)。名前がなければパーサが報告済み。
fn value_name(token: Option<SyntaxToken>) -> Option<SyntaxToken> {
    token.filter(|token| {
        matches!(
            token.kind(),
            SyntaxKind::LIDENT | SyntaxKind::OP | SyntaxKind::MINUS
        )
    })
}
```

呼び出し側の `value_name(file, …, diagnostics)` を `value_name(…)` にする。

- [ ] **Step 4: `binary` で名前を先に解決する**

`crates/eml_hir/src/lower/ops.rs` の `binary` を `pub(super) fn binary` にし、本体の頭に次を足す。

```rust
        // ユーザーの定義は Prelude の演算子を隠す。脱糖する演算子 (`&&` など) も同じで、定義すれば普通の呼び出しになる
        // (docs/spec/declarations.md の「fixity」)
        if let Some(callee) = self.user_operator(op, op_range) {
            return self.alloc(
                ExprKind::Call {
                    callee,
                    args: vec![lhs, rhs],
                    evaluate_first: None,
                },
                range,
            );
        }
```

`match op` の `_` の枝から、コンストラクタと `is_unusable` を見る部分を `user_operator` に移す。

```rust
    /// ユーザーが定義した演算子 (関数と中置のコンストラクタ) の参照。
    fn user_operator(&mut self, op: &str, op_range: TextRange) -> Option<ExprId> {
        let res = match self.items.value(op)? {
            ValueItem::Function(id) => Res::Function(id),
            ValueItem::Constructor(ctor) => Res::Constructor(ctor),
            ValueItem::Unusable => return Some(self.alloc(ExprKind::Missing, op_range)),
            ValueItem::Operation(_) | ValueItem::Builtin(_) => return None,
        };
        Some(self.alloc(ExprKind::Path(res), op_range))
    }
```

`_` の枝は `Builtin::binary_operator(op)`、`::` の E0004、E1001 だけを残す。`use super::scope::ValueItem;` を足す。

- [ ] **Step 5: テストを通す**

Run: `cargo test -p eml_hir`
Expected: PASS

- [ ] **Step 6: UI テストを足してコミット**

`tests/ui/run/functions/user_operators.em`:

```haskell
-- Operators defined by the user, with and without a fixity declaration, and one that hides `&&`.
infixr 5 <+>

(<+>) : String -> String -> String
a <+> b = a ++ "/" ++ b

(<->) : Int -> Int -> Int
a <-> b = a - b

data P =
  | E
  | Int :+ P

infixr 5 :+

total : P -> Int
total p = match p with
  | E -> 0
  | n :+ rest -> n + total rest

main : Unit -> <IO> Unit
main () =
  println ("a" <+> "b" <+> "c")
  println (show_int (10 <-> 3 <-> 2))
  println (show_int (total (1 :+ 2 :+ 3 :+ E)))
```

Run: `cargo test -p eml_cli --test ui`、続けて `cargo insta review`
Expected: stdout は `a/b/c`、`5`、`6`。

`test-changes.md` の段階6a の節に、`operator_definitions_and_qualified_names` を種類1として記録する (理由: 演算子の定義を受け付けるようになった)。

```bash
cargo test && cargo clippy --all-targets && cargo fmt
git add crates/eml_hir tests/ui/run/functions/user_operators.em crates/eml_cli/tests/snapshots docs/implementation/test-changes.md
git commit -m "Accept operator definitions and resolve user operators first"
```

---

### Task 6: 複数の等式の脱糖と、等式の並びの検査

**Files:**
- Modify: `crates/eml_hir/src/hir.rs`、`crates/eml_hir/src/lib.rs` (コード)
- Modify: `crates/eml_hir/src/lower/mod.rs`、`crates/eml_hir/src/lower/expr.rs`
- Modify: `ExprKind::Match` を分解する箇所 (`eml_hir/src/pretty.rs`、`eml_types/src/{carry.rs,usage.rs,exhaustive.rs,check/body.rs}`、`eml_core_ir/src/translate/{expr.rs,mod.rs}`)
- Test: `crates/eml_hir/tests/lower.rs`、`crates/eml_core_ir/tests/perceus.rs`
- Create: `tests/ui/run/functions/equations.em`、`tests/ui/check-fail/names/equation_order.em`

**Interfaces:**
- Produces: `eml_hir::MatchSource { Expr, Equations }`、`ExprKind::Match { scrutinee, arms, source: MatchSource }`、`Function::equation_ranges: Vec<TextRange>`、`BodyLowering::lower_equations(self, equations: &[(ast::Equation, TextRange)]) -> Body`、`BodyLowering::lower_param_group(&mut self, pats: impl IntoIterator<Item = ast::Pat>, fallback: TextRange) -> Vec<PatId>` (Task 12 が使う)、`BodyLowering::hidden_param(&mut self, name: &str, range: TextRange) -> (PatId, ExprId)` (Task 9 が使う)、コード E1018〜E1020

- [ ] **Step 1: 失敗するテストを書き、種類1のテストを直す**

`crates/eml_hir/tests/lower.rs` に足す。`use common::{diagnostics, lower_text};` にする。

```rust
#[test]
fn several_equations_become_a_match_on_the_arguments() {
    let text = "f : Int -> Int -> Int\nf 0 y = y\nf x y = x + y";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> Int -> Int
    f $0#0 $1#1 = (match ($0#0, $1#1) with | (0, y#2) -> y#2 | (x#3, y#4) -> (+ x#3 y#4))
    ");
}

#[test]
fn equations_of_one_argument_match_on_it_directly() {
    let text = "g : Int -> Int\ng 0 = 1\ng n = n";
    insta::assert_snapshot!(lower_text(text), @r"
    g : Int -> Int
    g $0#0 = (match $0#0 with | 0 -> 1 | n#1 -> n#1)
    ");
}

#[test]
fn equations_must_be_consecutive_and_follow_their_signature() {
    let text = "f : Int -> Int\n\ng : Int\ng = 1\n\nf 0 = 1\n\nh : Int\nh = 2\n\nf n = n";
    assert_eq!(
        diagnostics(text),
        vec![
            "E1019 6:1 the signature of `f` is not followed by its equations",
            "E1018 11:1 the equations of `f` are not consecutive",
        ]
    );
}

#[test]
fn equations_must_take_the_same_number_of_arguments() {
    let text = "f : Int -> Int -> Int\nf 0 y = y\nf x = x\nf x y = undefined_name";
    assert_eq!(
        diagnostics(text),
        vec![
            "E1020 3:1 the equations of `f` take different numbers of arguments",
            "E1001 4:9 cannot find value `undefined_name`",
        ]
    );
}
```

`signatures_and_equations_are_paired_by_name` は種類1の変更である。テキストの `d = 3\nd = 4` は2つの等式の値になるので、期待値の `E0004 8:1 defining a function with several equations is not supported yet` の行が消え、`d` の表示が `d = (match () with | () -> 3 | () -> 4)` になる。`cargo insta review` で、この2点だけが変わったことを確かめる。

`crates/eml_core_ir/tests/perceus.rs` に足す。

```rust
#[test]
fn equations_allocate_no_tuple_for_their_arguments() {
    // 等式の脱糖が作る引数のタプルは、simplify の K1 と DCE で消える
    let text = "f : Int -> Int -> Int\nf 0 y = y\nf x y = x + y\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    let core = core_text(text, Pass::Perceus);
    let start = core.find("fn f(").expect("the function");
    let f = &core[start..core[start..].find("\n}\n").map_or(core.len(), |end| start + end)];
    assert!(!f.contains("con #"), "{f}");
}
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_hir --test lower`
Expected: 新しい4件と `signatures_and_equations_are_paired_by_name` が FAIL

- [ ] **Step 3: HIR の型を足す**

`crates/eml_hir/src/hir.rs`:

```rust
/// `match` の由来。等式から作った `match` は、網羅性の検査が `match` 式ではなく等式として報告する
/// (docs/spec/diagnostics.md の「網羅性の診断」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchSource {
    Expr,
    Equations,
}
```

`ExprKind::Match` に `source: MatchSource` を足す。doc コメントに「等式が2つ以上ある関数の本体は、引数のタプル (引数が1つならその変数、0個なら `()`) に対する `Equations` の `match` である (docs/spec/declarations.md)」を足す。

`Function` に足す。

```rust
    /// 等式の関数名の位置。ソースの順である。網羅されていない等式の診断 (E4002) の secondary が指す。
    pub equation_ranges: Vec<TextRange>,
```

`ExprKind::Match { scrutinee, arms }` を分解している箇所 (このタスクの Files の一覧) に `..` を足す。`lower_match` (`lower/expr.rs`) は `source: MatchSource::Expr` で作る。`exhaustive.rs` の `positions` は Task 7 で直すので、ここでは `ExprKind::Match { scrutinee, arms, .. }` にするだけでよい。

`crates/eml_hir/src/lib.rs` の `codes` に足す。

```rust
    pub const NON_CONSECUTIVE_EQUATIONS: ErrorCode = ErrorCode(1018);
    pub const SIGNATURE_NOT_ADJACENT: ErrorCode = ErrorCode(1019);
    pub const EQUATION_ARITY_MISMATCH: ErrorCode = ErrorCode(1020);
```

- [ ] **Step 4: 等式の並びを検査する**

`crates/eml_hir/src/lower/mod.rs` の、定義ごとのループの頭 (`let mut equations = equations.into_iter();` から、離れたシグネチャの E0004 まで) を次に置き換える。

```rust
        // 等式は連続していなければならない (docs/spec/declarations.md)。離れていても、網羅性の誤りを連鎖させないよう
        // ソースの順に1つの関数として扱う
        for pair in equations.windows(2) {
            let (previous_index, _, previous_range) = &pair[0];
            let (index, _, range) = &pair[1];
            if *index != previous_index + 1 {
                diagnostics.push(
                    Diagnostic::error(
                        codes::NON_CONSECUTIVE_EQUATIONS,
                        format!("the equations of `{name}` are not consecutive"),
                        Label::new(file, *range, "this equation is separated from the ones above"),
                    )
                    .with_secondary(Label::new(file, *previous_range, "the previous equation"))
                    .with_help(format!(
                        "put every equation of `{name}` together, right after its signature"
                    )),
                );
            }
        }
        match (&signature, equations.first()) {
            (Some((_, _, range)), None) => /* E1005 は今のまま */,
            (None, Some((_, _, range))) => /* E1004 は今のまま */,
            (Some((signature_index, _, signature_range)), Some((equation_index, _, range)))
                if *equation_index != signature_index + 1 =>
            {
                diagnostics.push(
                    Diagnostic::error(
                        codes::SIGNATURE_NOT_ADJACENT,
                        format!("the signature of `{name}` is not followed by its equations"),
                        Label::new(file, *range, format!("this equation is not right after the signature of `{name}`")),
                    )
                    .with_secondary(Label::new(file, *signature_range, "the signature is here"))
                    .with_help(format!("move the equations of `{name}` right after its signature")),
                );
            }
            _ => {}
        }
```

`/* … は今のまま */` の2つの枝は、今の `match (&signature, &first_equation)` の同じ枝の中身をそのまま移す (`first_equation` を `equations.first()` に読み替える)。

`name_range` は `equations.first().map_or(first_range, |(_, _, range)| *range)`、`Function` には `equation_ranges: equations.iter().map(|(_, _, range)| *range).collect()` を入れる。`pending` には `(id, equations.into_iter().map(|(_, equation, range)| (equation, range)).collect::<Vec<_>>())` を積み、本体の変換は `.lower_equations(&equations)` にする (等式が0個の定義は今どおり積まない)。

- [ ] **Step 5: 等式を脱糖する**

`crates/eml_hir/src/lower/expr.rs` の `lower_equation` を次に置き換える。

```rust
    /// 等式が1つなら、引数のパターンをそのまま関数の引数にする。2つ以上なら、引数を隠れた変数で受け、引数のタプルに
    /// 対する `match` に脱糖する (docs/spec/declarations.md)。
    pub(super) fn lower_equations(mut self, equations: &[(ast::Equation, TextRange)]) -> Body {
        let reported = self.diagnostics.len();
        let (params, root) = match equations {
            [(equation, _)] => {
                let range = equation.range();
                let params = self.lower_param_group(equation.params(), range);
                let root = self.lower_expr(equation.body(), range);
                (params, root)
            }
            _ => self.lower_equation_match(equations),
        };
        Body {
            params,
            root,
            exprs: self.exprs,
            pats: self.pats,
            locals: self.locals,
            types: self.types,
            has_errors: self.diagnostics.len() > reported,
        }
    }

    fn lower_equation_match(
        &mut self,
        equations: &[(ast::Equation, TextRange)],
    ) -> (Vec<PatId>, ExprId) {
        let (first, first_name) = &equations[0];
        let name = first.name().map_or(String::new(), |name| name.text().to_string());
        let first_params: Vec<ast::Pat> = first.params().collect();
        let arity = first_params.len();
        // 隠れた変数の範囲は、最初の等式のその位置のパターンである。引数が矢印より多いときの診断がここを指す
        let (params, scrutinees): (Vec<PatId>, Vec<ExprId>) = first_params
            .iter()
            .enumerate()
            .map(|(position, pat)| self.hidden_param(&format!("${position}"), pat.range()))
            .unzip();
        let whole = first
            .range()
            .cover(equations[equations.len() - 1].0.range());
        let scrutinee = match scrutinees.as_slice() {
            [] => self.alloc(ExprKind::Literal(Literal::Unit), *first_name),
            [one] => *one,
            _ => self.alloc(ExprKind::Tuple(scrutinees), whole),
        };
        let mut arms = Vec::new();
        for (equation, name_range) in equations {
            let mark = self.scope.len();
            let pats = self.lower_param_group(equation.params(), equation.range());
            let body = self.lower_expr(equation.body(), equation.range());
            self.scope.truncate(mark);
            if pats.len() != arity {
                self.diagnostics.push(
                    Diagnostic::error(
                        codes::EQUATION_ARITY_MISMATCH,
                        format!("the equations of `{name}` take different numbers of arguments"),
                        Label::new(self.file, *name_range, format!("expected {}", arguments(arity))),
                    )
                    .with_secondary(Label::new(self.file, *first_name, "the first equation")),
                );
                continue;
            }
            let pat = match pats.as_slice() {
                [] => self.pats.alloc(Pat {
                    kind: PatKind::Unit,
                    range: *name_range,
                }),
                [one] => *one,
                _ => {
                    let range = self.pats[pats[0]]
                        .range
                        .cover(self.pats[pats[pats.len() - 1]].range);
                    self.pats.alloc(Pat {
                        kind: PatKind::Tuple(pats),
                        range,
                    })
                }
            };
            arms.push(MatchArm { pat, body });
        }
        let root = self.alloc(
            ExprKind::Match {
                scrutinee,
                arms,
                source: MatchSource::Equations,
            },
            whole,
        );
        (params, root)
    }

    /// 引数の並びを1つの組として変換する。組の中で同じ名前を2回束縛したら E1017 にする。
    pub(super) fn lower_param_group(
        &mut self,
        pats: impl IntoIterator<Item = ast::Pat>,
        fallback: TextRange,
    ) -> Vec<PatId> {
        let outer = std::mem::replace(&mut self.group_start, self.scope.len());
        let params = pats
            .into_iter()
            .map(|pat| self.lower_pat_in_group(Some(pat), fallback))
            .collect();
        self.group_start = outer;
        params
    }

    /// 脱糖で作る引数。名前は `$` で始まり、ソースの名前とぶつからない。スコープに積まないので、ソースからは引けない。
    pub(super) fn hidden_param(&mut self, name: &str, range: TextRange) -> (PatId, ExprId) {
        let local = self.locals.alloc(Local {
            name: name.to_string(),
            range,
        });
        let pat = self.pats.alloc(Pat {
            kind: PatKind::Bind(local),
            range,
        });
        let path = self.alloc(ExprKind::Path(Res::Local(local)), range);
        (pat, path)
    }
```

- [ ] **Step 6: テストを通す**

Run: `cargo test -p eml_hir`、続けて `cargo test -p eml_core_ir`
Expected: PASS。`several_equations_become_a_match_on_the_arguments` の表示の細部 (`$0#0` の番号) が予想と違ったら、隠れた変数が先に番号を取り、各等式の変数がその後に続くことを確かめて期待値を直す。

- [ ] **Step 7: UI テストを足してコミット**

`tests/ui/run/functions/equations.em`:

```haskell
-- Functions defined by several equations, over literals, constructors and several arguments.
data List a =
  | Nil
  | Cons a (List a)

len : List a -> Int
len Nil = 0
len (Cons _ rest) = 1 + len rest

describe : Int -> String
describe 0 = "zero"
describe 1 = "one"
describe _ = "many"

zip_sum : List Int -> List Int -> Int
zip_sum (Cons x xs) (Cons y ys) = x + y + zip_sum xs ys
zip_sum _ _ = 0

main : Unit -> <IO> Unit
main () =
  let xs = Cons 1 (Cons 2 (Cons 3 Nil))
  println (show_int (len xs))
  println (describe 0)
  println (describe 1)
  println (describe 5)
  println (show_int (zip_sum (Cons 1 (Cons 2 Nil)) (Cons 10 (Cons 20 (Cons 30 Nil)))))
```

`tests/ui/check-fail/names/equation_order.em`:

```haskell
-- E1018, E1019 and E1020: equations follow their signature, stay together and take the same number of arguments.
f : Int -> Int

g : Int
g = 1

f 0 = 1

h : Int
h = 2

f n = n

k : Int -> Int -> Int
k 0 y = y
k x = x

main : Unit -> <IO> Unit
main () = ()
```

Run: `cargo test -p eml_cli --test ui`、続けて `cargo insta review`
Expected: `equations` の stdout は `3`、`zero`、`one`、`many`、`33`。`equation_order` は E1019、E1018、E1020 の3件。

`test-changes.md` の段階6a の節に、`signatures_and_equations_are_paired_by_name` を種類1として記録する。

```bash
cargo test && cargo clippy --all-targets && cargo fmt
git add crates tests/ui docs/implementation/test-changes.md
git commit -m "Desugar several equations into a match on the arguments"
```

---

### Task 7: 等式の網羅性 (E4002 と E4005)

**Files:**
- Modify: `crates/eml_types/src/exhaustive.rs`、`crates/eml_types/src/lib.rs` (コード)
- Test: `crates/eml_types/tests/exhaustive.rs`
- Create: `tests/ui/check-fail/exhaustiveness/non_exhaustive_equations.em`、`tests/ui/run/functions/unreachable_equation.em`

**Interfaces:**
- Consumes: Task 6 の `MatchSource::Equations`、`Function::equation_ranges`
- Produces: `codes::UNREACHABLE_EQUATION` (E4005)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/exhaustive.rs` に足す。

```rust
#[test]
fn several_equations_that_miss_an_argument() {
    let text = "data Option a = | None | Some a\n\nf : Option Int -> Option Int -> Int\nf (Some x) _ = x\nf None (Some y) = y";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4002 3:1 the equations of `f` do not cover every argument
      3:1 `f` is not defined for some arguments
      4:1 an equation of `f`
      5:1 an equation of `f`
      note: not covered: `f None None`
    ");
}

#[test]
fn an_equation_after_a_catch_all_is_unreachable() {
    let text = "g : Int -> Int\ng _ = 0\ng 1 = 1";
    let checked = check(text);
    assert!(!has_errors(&checked.diagnostics));
    insta::assert_snapshot!(full(&checked.files, &checked.diagnostics), @r"
    E4005 3:3 unreachable equation
      3:3 the equations above already match these arguments
    ");
    assert_eq!(checked.diagnostics[0].severity, Severity::Warning);
}

#[test]
fn a_value_defined_twice_has_an_unreachable_equation() {
    let text = "pi : Int\npi = 3\npi = 4";
    insta::assert_snapshot!(diagnostics(text), @r"
    E4005 3:1 unreachable equation
      3:1 the equations above already match these arguments
    ");
}

#[test]
fn equations_with_a_type_error_add_no_exhaustiveness_errors() {
    let text = "f : Int -> Int\nf \"a\" = 1\nf 2 = 2";
    let text_out = diagnostics(text);
    assert!(!text_out.contains("E4002"), "{text_out}");
}
```

`full` の表示の形 (secondary の行の順、note の書き方) は既存の `a_match_that_misses_a_constructor` に合わせて予想したものである。違ったら、primary、各等式の secondary、漏れの例 `f None None` がそろっていることを確かめて期待値を直す。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_types --test exhaustive`
Expected: 新しい3件 (型の誤りの件以外) が FAIL

- [ ] **Step 3: 等式の行列を作る**

`crates/eml_types/src/lib.rs` の `codes` に `pub const UNREACHABLE_EQUATION: ErrorCode = ErrorCode(4005);` を足す。

`crates/eml_types/src/exhaustive.rs` の `equation` を次に置き換える。`use` に `MatchSource` を足す。

```rust
    /// 等式の引数の並びを行列の行にして調べる。等式が1つなら引数のパターンを1行に、2つ以上なら脱糖した `match` の
    /// 枝を1行ずつにする。spec の「引数のタプルに対する `match`」と同じ結果になる (docs/spec/exhaustiveness.md)。
    fn equation(&mut self, function: &Function) {
        let Some(signature_name) = function.signature_name_range else {
            return;
        };
        let (rows, patterns) = match &self.body.exprs[self.body.root].kind {
            ExprKind::Match {
                arms,
                source: MatchSource::Equations,
                ..
            } => {
                let mut rows = Vec::new();
                for arm in arms {
                    let Some(row) = self.equation_row(arm.pat) else {
                        return;
                    };
                    rows.push(row);
                }
                (rows, arms.iter().map(|arm| arm.pat).collect())
            }
            _ => match self.row(&self.body.params) {
                Some(row) => (vec![row], Vec::new()),
                None => return,
            },
        };
        for (index, row) in rows.iter().enumerate() {
            match self.useful(&rows[..index], row) {
                Ok(true) => {}
                Ok(false) => self.unreachable_equation(patterns[index]),
                Err(Mixed) => return,
            }
        }
        let width = self.body.params.len();
        let Ok(missing) = self.missing(&rows, width, SHOWN + 1) else {
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
        let file = self.module.file;
        let several = function.equation_ranges.len() > 1;
        let message = if several {
            format!("the equations of `{name}` do not cover every argument")
        } else {
            format!("the equation of `{name}` does not cover every argument")
        };
        let mut diagnostic = Diagnostic::error(
            codes::NON_EXHAUSTIVE_EQUATION,
            message,
            Label::new(file, signature_name, format!("`{name}` is not defined for some arguments")),
        );
        if several {
            for &range in &function.equation_ranges {
                diagnostic = diagnostic
                    .with_secondary(Label::new(file, range, format!("an equation of `{name}`")));
            }
        } else {
            diagnostic = diagnostic.with_secondary(Label::new(
                file,
                function.name_range,
                "this equation does not match every argument",
            ));
        }
        self.diagnostics
            .push(diagnostic.with_note(not_covered(&examples)));
    }

    /// 等式の `match` の枝のパターンを、引数ごとの欄の行にする。脱糖は、引数が2つ以上ならタプル、1つならその
    /// パターン、0個なら `()` を置く (docs/spec/declarations.md)。
    fn equation_row(&self, pat: PatId) -> Option<Row> {
        match (&self.body.pats[pat].kind, self.body.params.len()) {
            (_, 0) => Some(Vec::new()),
            (_, 1) => self.pat(pat).map(|pat| vec![pat]),
            (PatKind::Tuple(elements), _) => self.row(elements),
            _ => None,
        }
    }

    fn unreachable_equation(&mut self, pat: PatId) {
        self.diagnostics.push(Diagnostic::new(
            codes::UNREACHABLE_EQUATION,
            Severity::Warning,
            "unreachable equation",
            Label::new(
                self.module.file,
                self.body.pats[pat].range,
                "the equations above already match these arguments",
            ),
        ));
    }
```

`positions` の `match` の枝を `ExprKind::Match { scrutinee, arms, source: MatchSource::Expr } => self.match_expr(expr.range, *scrutinee, arms)` にし、等式の `match` は `_ => {}` に落とす。

`useful` と `missing` が幅0の行 (引数のない値) を扱えないなら、`missing` の呼び出しを `width == 0` のときに飛ばす (引数のない値に漏れはない)。`useful(&[], &[])` が `Ok(true)`、`useful(&[vec![]], &[])` が `Ok(false)` になることを、`a_value_defined_twice_has_an_unreachable_equation` が確かめる。

- [ ] **Step 4: テストを通す**

Run: `cargo test -p eml_types`
Expected: PASS。既存の `non_exhaustive_equation` の UI テストの出力 (等式が1つ) は変わらない。

- [ ] **Step 5: UI テストを足してコミット**

`tests/ui/check-fail/exhaustiveness/non_exhaustive_equations.em`:

```haskell
-- E4002 for several equations: the signature is primary and every equation is secondary.
data Option a =
  | None
  | Some a

first : Option Int -> Option Int -> Int
first (Some x) _ = x
first None (Some y) = y

main : Unit -> <IO> Unit
main () = println (show_int (first None (Some 1)))
```

`tests/ui/run/functions/unreachable_equation.em`:

```haskell
-- E4005: an equation after a catch-all is a warning, and the program still runs.
classify : Int -> String
classify _ = "any"
classify 0 = "zero"

main : Unit -> <IO> Unit
main () = println (classify 0)
```

Run: `cargo test -p eml_cli --test ui`、続けて `cargo insta review`
Expected: `non_exhaustive_equations` は E4002 の1件で、note に `first None None`。`unreachable_equation` は stdout が `any` で、stderr に E4005 の Warning。

```bash
cargo test && cargo clippy --all-targets && cargo fmt
git add crates/eml_types tests/ui crates/eml_cli/tests/snapshots
git commit -m "Report missing and unreachable equations"
```

---

### Task 8: 等式の検査の連鎖を止める

**Files:**
- Modify: `crates/eml_types/src/check/body.rs` (`check_function`)
- Test: `crates/eml_types/tests/check.rs`
- Create: `tests/ui/check-fail/types/broken_signature_effects.em`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/check.rs` に足す。

```rust
#[test]
fn a_broken_signature_adds_no_effect_errors_to_the_equation() {
    // 壊れたシグネチャや足りない矢印の後の本体は、ラムダと同じく末尾が `Error` の row で検査する
    let text = "f : Int -> Undefined\nf a b = println \"x\"\n\ng : Int -> Unit\ng a b = println \"x\"";
    let checked = eml_test_support::check(text);
    let lines = eml_test_support::short(&checked.files, &checked.diagnostics);
    assert!(lines.iter().any(|line| line.starts_with("E1002")), "{lines:?}");
    assert!(lines.iter().any(|line| line.starts_with("E2001")), "{lines:?}");
    assert!(!lines.iter().any(|line| line.starts_with("E2002")), "{lines:?}");
}
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_types --test check a_broken_signature_adds_no_effect_errors_to_the_equation`
Expected: FAIL (E2002 が2件ある)

- [ ] **Step 3: `check_function` を直す**

`crates/eml_types/src/check/body.rs` の `check_function` の `Arrow::Error` と `Arrow::NotFunction` の枝で、`self.ambient = Row::error();` を足す。

```rust
                Arrow::Error => {
                    let error = self.table.error;
                    self.bind_pat(pat, error);
                    // 期待する型が壊れていれば、どのエフェクトも受け入れて診断を連鎖させない (ラムダの検査と同じ)
                    self.ambient = Row::error();
                }
                Arrow::NotFunction => {
                    let diagnostic = self.signature_arity_error(pat, index);
                    self.diagnostics.push(diagnostic);
                    let error = self.table.error;
                    for &rest in &body.params[index..] {
                        self.bind_pat(rest, error);
                    }
                    expected = error;
                    self.ambient = Row::error();
                    break;
                }
```

- [ ] **Step 4: テストを通す**

Run: `cargo test -p eml_types`
Expected: PASS。既存の期待値は変わらない (今の UI テストと型のテストには、この連鎖を写したものがない)。変わったら作業を止めて相談する。

- [ ] **Step 5: UI テストを足し、status.md を直してコミット**

`tests/ui/check-fail/types/broken_signature_effects.em`:

```haskell
-- A broken signature or a missing arrow does not add E2002 for the effects of the body.
f : Int -> Undefined
f a b = println "x"

g : Int -> Unit
g a b = println "x"

main : Unit -> <IO> Unit
main () = ()
```

Run: `cargo test -p eml_cli --test ui`、続けて `cargo insta review`
Expected: E1002 と E2001 だけで、E2002 はない。

`docs/implementation/status.md` の「次の作業の注意点」から「診断の連鎖の残り: 等式の検査 (`check_function`) は…」の項を消す。

```bash
cargo test && cargo clippy --all-targets && cargo fmt
git add crates/eml_types tests/ui crates/eml_cli/tests/snapshots docs/implementation/status.md
git commit -m "Check equations under a broken signature with an error row"
```

---

### Task 9: 演算子の参照とセクション (E1023)

**Files:**
- Modify: `crates/eml_syntax/src/ast.rs` (`OpRef`、`LeftSection`、`RightSection`)
- Create: `crates/eml_hir/src/lower/section.rs`
- Modify: `crates/eml_hir/src/lower/mod.rs` (`mod section;`)、`crates/eml_hir/src/lower/expr.rs`、`crates/eml_hir/src/lib.rs` (コード)
- Test: `crates/eml_hir/tests/operators.rs`、`crates/eml_hir/tests/lower.rs`、`crates/eml_types/tests/check.rs`、`crates/eml_types/tests/tuples.rs` か `check.rs` (`(==)`)
- Create: `tests/ui/run/functions/sections.em`、`tests/ui/check-fail/names/invalid_section.em`、`tests/ui/check-fail/types/ambiguous_equality_section.em`

**Interfaces:**
- Consumes: Task 4 の `operator_token`、`ItemScope::fixity`、`Fixity`。Task 5 の `binary`。Task 6 の `hidden_param`
- Produces: `codes::INVALID_SECTION`

- [ ] **Step 1: 失敗するテストを書き、種類1のテストを直す**

`crates/eml_hir/tests/operators.rs` に足す。

```rust
#[test]
fn operator_references_and_sections_become_lambdas() {
    let text = "a : Int -> Int -> Int\na = (+)\nb : Int -> Int\nb = (+ 1)\nc : Int -> Int\nc = (10 -)\nd : Bool -> Bool -> Bool\nd = (&&)\ne : Int\ne = (- 1)";
    insta::assert_snapshot!(lower_text(text), @r"
    a : Int -> Int -> Int
    a = (fn $a#0 $b#1 -> (+ $a#0 $b#1))
    b : Int -> Int
    b = (fn $x#0 -> (+ $x#0 1))
    c : Int -> Int
    c = (fn $x#0 -> (- 10 $x#0))
    d : Bool -> Bool -> Bool
    d = (fn $a#0 $b#1 -> (if $a#0 $b#1 False))
    e : Int
    e = (negate 1)
    ");
}

#[test]
fn a_section_operand_may_be_an_operator_sequence_that_binds_tighter() {
    let text = "a : Int -> Int\na = (+ 2 * 3)\nb : Int -> Int\nb = (2 * 3 +)\nc : Int -> Int\nc = (1 + 2 +)\nd : String -> String\nd = (++ \"a\" ++ \"b\")\ne : Int -> Int\ne = (+ -1)";
    insta::assert_snapshot!(lower_text(text), @r#"
    a : Int -> Int
    a = (fn $x#0 -> (+ $x#0 (* 2 3)))
    b : Int -> Int
    b = (fn $x#0 -> (+ (* 2 3) $x#0))
    c : Int -> Int
    c = (fn $x#0 -> (+ (+ 1 2) $x#0))
    d : String -> String
    d = (fn $x#0 -> (++ $x#0 (++ "a" "b")))
    e : Int -> Int
    e = (fn $x#0 -> (+ $x#0 (negate 1)))
    "#);
}

#[test]
fn a_section_operand_that_binds_looser_needs_parentheses() {
    let text = "a : Int -> Int\na = (* 1 + 2)\nb : Int -> Int\nb = (+ 1 + 2)\nc : Bool -> Bool\nc = (== 1 == 2)";
    assert_eq!(
        common::diagnostics(text),
        vec![
            "E1023 2:5 the section of `*` needs parentheses around its operand",
            "E1023 4:5 the section of `+` needs parentheses around its operand",
            "E1023 6:5 the section of `==` needs parentheses around its operand",
        ]
    );
}
```

`e = (+ -1)` の `-1` は前置の `-` なので、E1023 の検査の対象ではない。`(+ -1)` が E1006 (「a prefix `-` cannot appear here」) になる場合は、今の組み直しの規則どおりなので、テストからこの行を外し、別のテストで E1006 を確かめる。

`crates/eml_hir/tests/lower.rs` の `constructs_of_later_stages_are_not_yet_supported` は種類1の変更である。`let plus = (+)` の行が `(fn $a#… $b#… -> (+ …))` になり、`E0004 4:14 operator references are not supported yet` が消える。`let y = x in y` の E0004 は Task 10 で消えるので、このタスクではまだ残る。`crates/eml_types/tests/check.rs` の `later_stage_constructs_add_no_type_errors` も種類1で、`plus#3` の型が `Int -> Int -> Int` の形になり、演算子の参照の E0004 の2行が消える。どちらも `cargo insta review` で、この変化だけであることを確かめる。

`crates/eml_types/tests/check.rs` に、`(==)` の比べ方のテストを足す。

```rust
#[test]
fn an_equality_section_compares_by_the_type_it_is_used_at() {
    let text = "ints : Int -> Bool\nints = (== 1)\n\nstrings : String -> Bool\nstrings = (== \"a\")\n\neq : Bool -> Bool -> Bool\neq = (==)";
    let checked = eml_test_support::check(text);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
}

#[test]
fn an_equality_reference_with_an_unknown_type_is_not_comparable() {
    let text = "f : Int -> Int\nf x =\n  let eq = (==)\n  x";
    let checked = eml_test_support::check(text);
    let lines = eml_test_support::short(&checked.files, &checked.diagnostics);
    assert!(lines.iter().any(|line| line.starts_with("E2006 3:13")), "{lines:?}");
}
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_hir --test operators`、`cargo test -p eml_types --test check`
Expected: 新しいテストが FAIL (今は E0004)

- [ ] **Step 3: AST のアクセサを足す**

`crates/eml_syntax/src/ast.rs` に足す (Task 4 で `operator_token` を書かなかった場合はここで足す)。

```rust
impl OpRef {
    pub fn operator(&self) -> Option<SyntaxToken> {
        operator_token(&self.syntax)
    }
}

impl LeftSection {
    /// `(e op)` の `e`。
    pub fn operand(&self) -> Option<Expr> {
        support::child(&self.syntax)
    }

    pub fn operator(&self) -> Option<SyntaxToken> {
        operator_token(&self.syntax)
    }
}

impl RightSection {
    /// `(op e)` の `e`。
    pub fn operand(&self) -> Option<Expr> {
        support::child(&self.syntax)
    }

    pub fn operator(&self) -> Option<SyntaxToken> {
        operator_token(&self.syntax)
    }
}
```

被演算子が演算子の列のとき、列の中の演算子は `OP_SEQ` の子なので、`operator_token` はセクションの演算子だけを見つける。

- [ ] **Step 4: 脱糖を書く**

`crates/eml_hir/src/lib.rs` の `codes` に `pub const INVALID_SECTION: ErrorCode = ErrorCode(1023);` を足す。

`crates/eml_hir/src/lower/section.rs` (新規):

```rust
//! 演算子の参照とセクションを、ラムダに脱糖する (docs/spec/expressions.md の「セクション」)。本体の二項演算は、演算子の
//! 列の組み直しと同じ `binary` で作るので、`&&` の短絡、中置のコンストラクタ、ユーザーの演算子がそのまま効く。`==` の
//! 比べ方は、ラムダの本体の呼び出しについて型検査が決める。

use eml_diagnostics::{Diagnostic, Label, TextRange};
use eml_syntax::ast::{self, OpSeqElement};
use eml_syntax::SyntaxToken;

use super::expr::BodyLowering;
use crate::builtin::Assoc;
use crate::codes;
use crate::hir::*;

/// 空いている被演算子の側。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Hole {
    Left,
    Right,
}

impl BodyLowering<'_> {
    /// `(op)` を `fn $a $b -> $a op $b` にする。
    pub(super) fn lower_op_ref(&mut self, op_ref: &ast::OpRef, range: TextRange) -> ExprId {
        let Some(op) = op_ref.operator() else {
            return self.alloc(ExprKind::Missing, range);
        };
        let (a_pat, a) = self.hidden_param("$a", range);
        let (b_pat, b) = self.hidden_param("$b", range);
        let body = self.binary(op.text(), op.text_range(), a, b);
        self.alloc(
            ExprKind::Lambda {
                params: vec![a_pat, b_pat],
                body,
            },
            range,
        )
    }

    /// `(e op)` を `fn $x -> e op $x` にする。
    pub(super) fn lower_left_section(
        &mut self,
        section: &ast::LeftSection,
        range: TextRange,
    ) -> ExprId {
        match section.operator() {
            Some(op) => self.section(&op, section.operand(), Hole::Right, range),
            None => self.alloc(ExprKind::Missing, range),
        }
    }

    /// `(op e)` を `fn $x -> $x op e` にする。
    pub(super) fn lower_right_section(
        &mut self,
        section: &ast::RightSection,
        range: TextRange,
    ) -> ExprId {
        match section.operator() {
            Some(op) => self.section(&op, section.operand(), Hole::Left, range),
            None => self.alloc(ExprKind::Missing, range),
        }
    }

    /// 被演算子 `e` は、spec のとおりラムダを呼ぶたびに評価する。
    fn section(
        &mut self,
        op: &SyntaxToken,
        operand: Option<ast::Expr>,
        hole: Hole,
        range: TextRange,
    ) -> ExprId {
        if let Some(ast::Expr::OpSeq(seq)) = &operand
            && let Some(inner) = self.looser_operator(op.text(), seq, hole)
        {
            self.diagnostics.push(
                Diagnostic::error(
                    codes::INVALID_SECTION,
                    format!(
                        "the section of `{}` needs parentheses around its operand",
                        op.text()
                    ),
                    Label::new(
                        self.file,
                        range,
                        format!(
                            "`{}` does not bind more tightly than `{}`",
                            inner.text(),
                            op.text()
                        ),
                    ),
                )
                .with_help("put the operand in parentheses"),
            );
            // 組み方が決まらないので、型の誤りを連鎖させないようにセクション全体を Missing にする
            return self.alloc(ExprKind::Missing, range);
        }
        let operand_range = operand.as_ref().map_or(range, |operand| operand.range());
        let value = self.lower_expr(operand, operand_range);
        let (pat, x) = self.hidden_param("$x", range);
        let body = match hole {
            Hole::Left => self.binary(op.text(), op.text_range(), x, value),
            Hole::Right => self.binary(op.text(), op.text_range(), value, x),
        };
        self.alloc(
            ExprKind::Lambda {
                params: vec![pat],
                body,
            },
            range,
        )
    }

    /// セクションの演算子を根にして組めない、被演算子の中の二項演算子。被演算子の中の演算子は、セクションの演算子より
    /// 強く結合するか、優先順位が同じで両方の結合の向きが空いた側に合っていなければならない (docs/spec/expressions.md の
    /// 「セクション」)。列の先頭か演算子の直後の `-` は前置の `-` で、組み直しの検査 (E1006) が受け持つ。
    fn looser_operator(&self, op: &str, seq: &ast::OpSeq, hole: Hole) -> Option<SyntaxToken> {
        let outer = self.items.fixity(op);
        // 左が空いていれば `$x op (e)` と組むので右結合、右が空いていれば `(e) op $x` と組むので左結合が合う
        let toward_hole = match hole {
            Hole::Left => Assoc::Right,
            Hole::Right => Assoc::Left,
        };
        let mut after_operator = true;
        for element in seq.elements() {
            match element {
                OpSeqElement::Operand(_) => after_operator = false,
                OpSeqElement::Operator(token) => {
                    let prefix = after_operator;
                    after_operator = true;
                    if prefix {
                        continue;
                    }
                    let inner = self.items.fixity(token.text());
                    let tighter = inner.precedence > outer.precedence;
                    let same_side = inner.precedence == outer.precedence
                        && inner.assoc == toward_hole
                        && outer.assoc == toward_hole;
                    if !tighter && !same_side {
                        return Some(token);
                    }
                }
            }
        }
        None
    }
}
```

`crates/eml_hir/src/lower/mod.rs` に `mod section;` を足す。`crates/eml_hir/src/lower/expr.rs` の `lower_expr` の2つの枝を次にする。

```rust
            ast::Expr::OpRef(op_ref) => self.lower_op_ref(&op_ref, range),
            ast::Expr::LeftSection(section) => self.lower_left_section(&section, range),
            ast::Expr::RightSection(section) => self.lower_right_section(&section, range),
            ast::Expr::FieldSection(_) => {
                self.unsupported(range, "field sections are not supported yet")
            }
```

`OpSeqElement::Operator` のトークンの型が `SyntaxToken` でなければ (`ops.rs` の `lower_op_seq` を見る)、`looser_operator` の戻り値をそれに合わせる。

- [ ] **Step 5: テストを通す**

Run: `cargo test -p eml_hir`、`cargo test -p eml_types`
Expected: PASS。`$x#0` などの番号は、関数ごとに局所変数の表が新しいので `#0` から始まる。

- [ ] **Step 6: UI テストを足してコミット**

`tests/ui/run/functions/sections.em`:

```haskell
-- Operator references and sections are lambdas; `(==)` compares by the type it is used at.
apply : (Int -> Int) -> Int -> Int
apply f x = f x

apply2 : (a -> a -> b) -> a -> a -> b
apply2 f x y = f x y

check : (String -> Bool) -> String -> String
check p s = if p s then "yes" else "no"

main : Unit -> <IO> Unit
main () =
  println (show_int (apply (+ 1) 41))
  println (show_int (apply (100 -) 1))
  println (show_int (apply2 (*) 6 7))
  println (show_int (apply (+ 2 * 3) 1))
  println (check (== "a") "a")
  println (check (!= "a") "a")
  println (if apply2 (==) 3 3 then "same" else "different")
  println (if apply2 (&&) True False then "both" else "not both")
```

`tests/ui/check-fail/names/invalid_section.em`:

```haskell
-- E1023: the operand of a section must bind more tightly than the section's operator.
f : Int -> Int
f = (* 1 + 2)

main : Unit -> <IO> Unit
main () = ()
```

`tests/ui/check-fail/types/ambiguous_equality_section.em`:

```haskell
-- E2006: an equality reference whose argument type is never fixed cannot choose how to compare.
f : Int -> Int
f x =
  let eq = (==)
  x

main : Unit -> <IO> Unit
main () = ()
```

Run: `cargo test -p eml_cli --test ui`、続けて `cargo insta review`
Expected: `sections` の stdout は `42`、`99`、`42`、`7`、`yes`、`no`、`same`、`not both`。`invalid_section` は E1023 の1件、`ambiguous_equality_section` は E2006 の1件 (ほかに `eq` が使われないことの診断が出るなら、それも受け入れてよい。`eq` は関数値で `Unr` なので出ないはずである)。

`test-changes.md` の段階6a の節に、`constructs_of_later_stages_are_not_yet_supported` と `later_stage_constructs_add_no_type_errors` を種類1として記録する。`docs/implementation/status.md` から「段階6a: `==` の演算子の参照 `(==)` とセクション `(== 1)` を入れるときは…」の項と、「段階6a: パーサは、セクションの被演算子を…」の項を消す。

```bash
cargo test && cargo clippy --all-targets && cargo fmt
git add crates tests/ui docs/implementation
git commit -m "Desugar operator references and sections into lambdas"
```

---

### Task 10: `use` (E1024) と `let ... in`

**Files:**
- Modify: `crates/eml_syntax/src/ast.rs` (`UseStmt`、`LetExpr`)
- Modify: `crates/eml_hir/src/lower/expr.rs`、`crates/eml_hir/src/lib.rs` (コード)
- Test: `crates/eml_hir/tests/lower.rs`、`crates/eml_syntax/tests/parser.rs` (アクセサ)
- Create: `tests/ui/run/functions/use.em`、`tests/ui/run/basics/let_in.em`、`tests/ui/check-fail/names/use_at_end_of_block.em`

**Interfaces:**
- Produces: `ast::UseStmt::{pat, expr}`、`ast::LetExpr::{pat, ty, init, body}`、`codes::USE_AT_END_OF_BLOCK`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_hir/tests/lower.rs` に足す。

```rust
#[test]
fn use_passes_the_rest_of_the_block_as_the_last_argument() {
    let text = "wrap : Int -> (Unit -> Int) -> Int\nwrap n k = k ()\n\nbind : (Int -> Int) -> Int\nbind k = k 1\n\nf : Unit -> Int\nf () =\n  use wrap 1\n  use x <- bind\n  x + 2";
    insta::assert_snapshot!(lower_text(text), @r"
    wrap : Int -> (Unit -> Int) -> Int
    wrap n#0 k#1 = (k#1 ())
    bind : (Int -> Int) -> Int
    bind k#0 = (k#0 1)
    f : Unit -> Int
    f () = {
      (@wrap 1 (fn () -> {
        (@bind (fn x#0 -> {
          (+ x#0 2)
        }))
      }))
    }
    ");
}

#[test]
fn use_at_the_end_of_a_block_has_nothing_to_wrap() {
    let text = "wrap : (Unit -> Int) -> Int\nwrap k = k ()\n\nf : Unit -> Int\nf () =\n  let a = 1\n  use wrap";
    assert_eq!(
        common::diagnostics(text),
        vec!["E1024 7:3 a `use` must be followed by the rest of its block"]
    );
}

#[test]
fn let_in_is_a_block_with_one_let() {
    let text = "f : Int -> Int\nf x = let y : Int = x + 1 in y * 2";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> Int
    f x#0 = {
      let y#1 : Int = (+ x#0 1)
      (* y#1 2)
    }
    ");
}
```

ブロックの表示の字下げ (`{` の中の行) は既存の `later_lets_shadow_earlier_names` の形から予想したものである。入れ子のブロックの字下げが違ったら、呼び出しの最後の引数がラムダで、その本体が残りの文のブロックであることを確かめて期待値を直す。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_hir --test lower`
Expected: 3件とも FAIL (今は E0004)

- [ ] **Step 3: AST のアクセサを足す**

`crates/eml_syntax/src/ast.rs`:

```rust
impl UseStmt {
    /// `use p <- e` の `p`。`<-` がなければ `None`。
    pub fn pat(&self) -> Option<Pat> {
        support::child(&self.syntax)
    }

    pub fn expr(&self) -> Option<Expr> {
        support::child(&self.syntax)
    }
}

impl LetExpr {
    pub fn pat(&self) -> Option<Pat> {
        support::child(&self.syntax)
    }

    pub fn ty(&self) -> Option<Type> {
        support::child(&self.syntax)
    }

    /// `in` の前の式。
    pub fn init(&self) -> Option<Expr> {
        self.split_at_in().0
    }

    /// `in` の後の式。
    pub fn body(&self) -> Option<Expr> {
        self.split_at_in().1
    }

    /// 片方の式が欠けても取り違えないよう、`in` の位置で分ける。
    fn split_at_in(&self) -> (Option<Expr>, Option<Expr>) {
        let in_start = self
            .syntax
            .children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .find(|token| token.kind() == SyntaxKind::IN_KW)
            .map(|token| token.text_range().start());
        let mut before = None;
        let mut after = None;
        for expr in support::children::<Expr>(&self.syntax) {
            match in_start {
                Some(start) if expr.range().start() >= start => {
                    after.get_or_insert(expr);
                }
                _ => {
                    before.get_or_insert(expr);
                }
            }
        }
        (before, after)
    }
}
```

`support::children` がイテレータを返す形が `AstChildren<Expr>` なら、そのまま `for` で回せる。

- [ ] **Step 4: `let ... in` を脱糖する**

`crates/eml_hir/src/lower/expr.rs` の `ast::Expr::LetExpr` の枝を次にする。

```rust
            ast::Expr::LetExpr(e) => {
                // ブロックの `let` と同じく右辺を先に変換し、束縛は `in` の後の式でだけ見える (docs/spec/expressions.md)
                let mark = self.scope.len();
                let init = self.lower_expr(e.init(), range);
                let ty = e.ty().map(|ty| self.lower_type(Some(ty), range));
                let pat = self.lower_pat(e.pat(), range);
                let tail = self.lower_expr(e.body(), range);
                self.scope.truncate(mark);
                self.alloc(
                    ExprKind::Block {
                        stmts: vec![Stmt::Let { pat, ty, init }],
                        tail: Some(tail),
                        last_line: None,
                    },
                    range,
                )
            }
```

- [ ] **Step 5: `use` を脱糖する**

`crates/eml_hir/src/lib.rs` の `codes` に `pub const USE_AT_END_OF_BLOCK: ErrorCode = ErrorCode(1024);` を足す。

`lower_block` を次の2つの関数に置き換える。

```rust
    fn lower_block(&mut self, block: &ast::Block, range: TextRange) -> ExprId {
        let all: Vec<ast::Stmt> = block.stmts().collect();
        self.lower_stmts(&all, range)
    }

    /// 文の並びをブロックにする。`use` の文に出会ったら、残りの文を包んだラムダを最後の引数に足した呼び出しを、この
    /// ブロックの値にする (docs/spec/expressions.md の「`use`」)。
    fn lower_stmts(&mut self, all: &[ast::Stmt], range: TextRange) -> ExprId {
        let mark = self.scope.len();
        let mut stmts = Vec::new();
        let mut tail = None;
        let mut end = all.len();
        for (index, stmt) in all.iter().enumerate() {
            let stmt_range = stmt.range();
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
                    tail = Some(self.lower_use(stmt, &all[index + 1..], range));
                    end = index + 1;
                    break;
                }
            }
        }
        self.scope.truncate(mark);
        let last_line = all[..end].last().and_then(|stmt| {
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
    }

    /// `use f a b` を `f a b (fn () -> 残り)` に、`use p <- f a b` を `f a b (fn p -> 残り)` にする。ラムダと残りの
    /// ブロックの範囲は、`use` の文の始まりからブロックの終わりまでである。
    fn lower_use(
        &mut self,
        stmt: &ast::UseStmt,
        rest: &[ast::Stmt],
        block_range: TextRange,
    ) -> ExprId {
        let stmt_range = stmt.range();
        let callee = self.lower_expr(stmt.expr(), stmt_range);
        if rest.is_empty() {
            self.diagnostics.push(
                Diagnostic::error(
                    codes::USE_AT_END_OF_BLOCK,
                    "a `use` must be followed by the rest of its block",
                    Label::new(self.file, stmt_range, "nothing follows this `use`"),
                )
                .with_help("write the code that the `use` wraps after it, or call the function directly"),
            );
            // 包む残りがないので、最後の引数を Missing にして型の誤りを連鎖させない
            let missing = self.alloc(ExprKind::Missing, stmt_range);
            return self.call(callee, vec![missing], None, stmt_range);
        }
        let wrapped = TextRange::new(stmt_range.start(), block_range.end());
        let mark = self.scope.len();
        let param = match stmt.pat() {
            Some(pat) => self.lower_pat(Some(pat), stmt_range),
            None => self.pats.alloc(Pat {
                kind: PatKind::Unit,
                range: stmt_range,
            }),
        };
        let body = self.lower_stmts(rest, wrapped);
        self.scope.truncate(mark);
        let lambda = self.alloc(
            ExprKind::Lambda {
                params: vec![param],
                body,
            },
            wrapped,
        );
        self.call(callee, vec![lambda], None, wrapped)
    }
```

- [ ] **Step 6: テストを通す**

Run: `cargo test -p eml_hir`、`cargo test -p eml_types`
Expected: PASS。Task 9 で残した `constructs_of_later_stages_are_not_yet_supported` の `let ... in` の E0004 の行が消え、ブロックの表示になる (同じ種類1の変更の続きとして `cargo insta review` で確かめる)。

- [ ] **Step 7: UI テストを足してコミット**

`tests/ui/run/functions/use.em`:

```haskell
-- `use` passes the rest of the block as the last argument, with and without a bound name.
effect Log where
  log : String -> Unit

with_log : (Unit -> <Log, IO> a) -> <IO> a
with_log body =
  handle body () with
    | log s k ->
        println ("log: " ++ s)
        resume k ()

twice : (Int -> <IO> Unit) -> <IO> Unit
twice k =
  k 1
  k 2

main : Unit -> <IO> Unit
main () =
  use with_log
  log "start"
  use n <- twice
  println (show_int n)
```

`with_log` の型が今の型検査で通らない場合 (row 変数の書き方など) は、通る形に直してよい。直したら、`use` で包んだ残りが handler の中で動くことと、`use n <- twice` の残りが2回動くことが出力から分かる形を保つ。

`tests/ui/run/basics/let_in.em`:

```haskell
-- `let ... in` is a one-line form of a block `let`.
main : Unit -> <IO> Unit
main () =
  let n = 20
  println (show_int (let x = n + 1 in x * 2))
```

`tests/ui/check-fail/names/use_at_end_of_block.em`:

```haskell
-- E1024: a `use` at the end of a block has nothing to wrap.
wrap : (Unit -> <IO> Unit) -> <IO> Unit
wrap k = k ()

main : Unit -> <IO> Unit
main () =
  println "start"
  use wrap
```

Run: `cargo test -p eml_cli --test ui`、続けて `cargo insta review`
Expected: `use` の stdout は `log: start`、`1`、`2`。`let_in` は `42`。`use_at_end_of_block` は E1024 の1件。

```bash
cargo test && cargo clippy --all-targets && cargo fmt
git add crates tests/ui crates/eml_cli/tests/snapshots
git commit -m "Desugar use and let-in"
```

---

### Task 11: パターンの型の明示を文法に広げる

**Files:**
- Modify: `crates/eml_syntax/src/grammar/patterns.rs`、`crates/eml_syntax/src/grammar/expressions.rs`
- Modify: `crates/eml_hir/src/lower/expr.rs`
- Modify: `crates/eml_types/src/check/body.rs`、`crates/eml_types/src/check/report.rs`
- Test: `crates/eml_syntax/tests/parser.rs`、`crates/eml_hir/tests/lower.rs`、`crates/eml_types/tests/check.rs`
- Create: `tests/ui/run/data/annotated_patterns.em`、`tests/ui/check-fail/types/annotated_pattern_mismatch.em`
- Modify: `docs/spec/grammar.md`、`docs/spec/expressions.md`

**Interfaces:**
- Produces: `Origin::AnnotatedPattern`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_syntax/tests/parser.rs` に足す (既存のテストの補助関数の名前に合わせる。`parse_clean` を使う形の例):

```rust
#[test]
fn annotated_patterns_can_appear_in_any_pattern() {
    let text = "f (Some (x : Int)) = x\ng = match y with\n  | ((a : Int), b) -> a\nh = let (z : Int) = 1 in z";
    let parsed = eml_test_support::parse_clean(text);
    let tree = eml_syntax::debug_tree(&parsed.parse.syntax());
    assert_eq!(tree.matches("ANNOT_PAT").count(), 3, "{tree}");
}
```

タプルの要素に型を明示するときは `((a : Int), b)` と書く。`(a : Int, b)` は、括弧の最初のパターンの後の `:` を型の明示として読むので、`,` で `)` を期待する構文エラーになる (文法の `'(' pat ':' type ')'` のとおり)。

`crates/eml_types/tests/check.rs` に足す。

```rust
#[test]
fn an_annotated_pattern_must_have_the_type_of_its_value() {
    let text = "data Option a = | None | Some a\n\nf : Option Int -> Int\nf (Some (x : String)) = 1\nf None = 0";
    let checked = eml_test_support::check(text);
    insta::assert_snapshot!(eml_test_support::full(&checked.files, &checked.diagnostics), @"");
}
```

期待値は `cargo insta review` で、E2001 が `(x : String)` を指し、note が「an annotated pattern must have the type of the value it matches」であることを確かめて受け入れる。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_syntax --test parser annotated_patterns_can_appear_in_any_pattern`
Expected: FAIL (構文エラー)

- [ ] **Step 3: パーサを直す**

`crates/eml_syntax/src/grammar/patterns.rs`:

- `param` と `apat_with` を消し、`apat` を元の `apat_with` の本体にする。`L_PAREN => paren_pat(p),` にする
- `paren_pat` の `annotated` の引数を消し、`let kind = if p.eat(COLON) {` にする
- `apat` の doc コメントに「`(pat : type)` の型の明示を含む (docs/spec/grammar.md の `apat`)」を足す

`crates/eml_syntax/src/grammar/expressions.rs` の `patterns::param(p);` を `patterns::apat(p);` にする。

- [ ] **Step 4: HIR を直す**

`crates/eml_hir/src/lower/expr.rs`:

- `lower_pat_in_group` の `ast::Pat::AnnotPat(_)` の E0004 の枝を次にする

```rust
            ast::Pat::AnnotPat(annot) => {
                // 型を先に変換する。ラムダの引数で使っていた順で、局所変数の番号を変えないため
                let ty = self.lower_type(annot.ty(), range);
                let pat = self.lower_pat_in_group(annot.pat(), range);
                PatKind::Annot { pat, ty }
            }
```

- `lower_lambda_param` を消し、ラムダの引数は `.map(|pat| self.lower_pat(Some(pat), TextRange::default()))` にする (Task 12 で引数の組にする)
- `unsupported_pat` が使われなくなったかを確かめる (修飾された名前のパターンでまだ使う)

- [ ] **Step 5: 型検査を直す**

`crates/eml_types/src/check/report.rs` の `Origin` に足す。

```rust
    /// 型を明示したパターン。明示した型が、パターンが受ける値の型と一致しなければならない。
    AnnotatedPattern,
```

note の `match` に足す。

```rust
            Origin::AnnotatedPattern => diagnostic.with_note(
                "an annotated pattern must have the type of the value it matches",
            ),
```

`crates/eml_types/src/check/body.rs` の `bind_pat` の `PatKind::Annot` の枝を次にする。

```rust
            PatKind::Annot {
                pat: inner,
                ty: annotation,
            } => {
                let annotated = lower_type(self.table, &body.types, self.rigids, *annotation);
                self.expect(body.pats[pat].range, ty, annotated, &Origin::AnnotatedPattern);
                self.bind_pat(*inner, annotated);
            }
```

`bind_param` (ラムダの引数のいちばん外側) は今のまま `Origin::LambdaParameter` で照合し、内側を `bind_pat` に渡す。

- [ ] **Step 6: テストを通す**

Run: `cargo test`
Expected: PASS。既存のラムダの引数の型の明示のテスト (HIR の表示と型の診断) は変わらない。変わったら作業を止めて相談する。

- [ ] **Step 7: UI テストと文書、コミット**

`tests/ui/run/data/annotated_patterns.em`:

```haskell
-- Type annotations may appear in any pattern.
data Option a =
  | None
  | Some a

get : Option Int -> Int
get (Some (n : Int)) = n
get None = 0

main : Unit -> <IO> Unit
main () =
  let (x : Int) = get (Some 41)
  let label = match Some "done" with
    | Some (s : String) -> s
    | None -> "none"
  println (show_int (x + 1))
  println label
```

`tests/ui/check-fail/types/annotated_pattern_mismatch.em`:

```haskell
-- E2001: an annotated pattern must have the type of the value it matches.
data Option a =
  | None
  | Some a

get : Option Int -> Int
get (Some (n : String)) = 1
get None = 0

main : Unit -> <IO> Unit
main () = ()
```

Run: `cargo test -p eml_cli --test ui`、続けて `cargo insta review`
Expected: `annotated_patterns` の stdout は `42` と `done`。`annotated_pattern_mismatch` は E2001 の1件。

文書:

- `docs/spec/grammar.md`: `apat` に `| '(' pat ':' type ')'` を足し、`param ::= …` の規則を消し、ラムダの規則の `param` を `apat` にする
- `docs/spec/expressions.md` の「ラムダ」: 型の明示は、ラムダの引数だけでなく、どのパターンの位置でも `(p : T)` と書けることを足す
- spec (作業用の設計文書) に、`(a : Int, b)` が構文エラーで `((a : Int), b)` と書くことと、`climb_pat` の扱い (Task 4 の Step 7) を書き足す

```bash
cargo test && cargo clippy --all-targets && cargo fmt
git add crates tests/ui docs
git commit -m "Allow type annotations in any pattern"
```

---

### Task 12: ラムダと handler の節の重複束縛 (E1017)

**Files:**
- Modify: `crates/eml_hir/src/lower/expr.rs` (ラムダ)、`crates/eml_hir/src/lower/handler.rs` (節)
- Test: `crates/eml_hir/tests/lower.rs`
- Modify: `tests/ui/check-fail/names/duplicate_binding.em`

**Interfaces:**
- Consumes: Task 6 の `lower_param_group`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_hir/tests/lower.rs` に足す。

```rust
#[test]
fn lambda_and_clause_parameters_are_one_group() {
    let text = "effect Ask where\n  ask : Int -> Int\n\nf : Unit -> Int\nf () =\n  let g = fn x x -> x\n  handle 1 with\n    | ask n n -> 0\n    | return r -> r";
    assert_eq!(
        common::diagnostics(text),
        vec![
            "E1017 6:16 `x` is bound more than once",
            "E1017 8:13 `n` is bound more than once",
        ]
    );
}
```

`| ask n n` の2つ目の `n` は `k` の位置である。操作の引数と `k` も1つの組として調べる。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_hir --test lower lambda_and_clause_parameters_are_one_group`
Expected: FAIL (診断がない)

- [ ] **Step 3: 引数の並びを組にする**

`crates/eml_hir/src/lower/expr.rs` のラムダの枝:

```rust
                // ラムダの引数の並びは、等式の引数と同じく1つの組である (E1017)
                let params = self.lower_param_group(lambda.params(), TextRange::default());
```

`crates/eml_hir/src/lower/handler.rs` の `op_clause` と `return_clause`:

```rust
        // 節の引数の並び (操作の引数と `k`) は、等式の引数と同じく1つの組である (E1017)
        let params: Vec<PatId> = self.lower_param_group(clause.params(), clause.range());
```

- [ ] **Step 4: テストを通す**

Run: `cargo test -p eml_hir`
Expected: PASS。既存の HIR の表示は変わらない (組の始まりが変わるだけで、局所変数の番号の順は同じ)。

- [ ] **Step 5: UI テストと status.md、コミット**

`tests/ui/check-fail/names/duplicate_binding.em` は種類1の変更になるので、既存の内容は変えず、新しいファイル `tests/ui/check-fail/names/duplicate_lambda_binding.em` を足す。

```haskell
-- E1017: a lambda binds `x` twice in its parameters.
main : Unit -> <IO> Unit
main () =
  let f = fn x x -> x
  println (show_int (f 1 2))
```

Run: `cargo test -p eml_cli --test ui`、続けて `cargo insta review`
Expected: E1017 の1件で、2つ目の `x` が primary、1つ目が secondary。

`docs/implementation/status.md` から「ラムダの引数の並びと handler の節の引数の並びで同じ名前を2回束縛しても…」の項を消す。`docs/spec/diagnostics.md` の E1017 の説明に、ラムダの引数の並びと handler の節の引数の並びを足す。

```bash
cargo test && cargo clippy --all-targets && cargo fmt
git add crates/eml_hir tests/ui crates/eml_cli/tests/snapshots docs
git commit -m "Treat lambda and clause parameters as one binding group"
```

---

### Task 13: 文書をそろえ、段階6a を閉じる

**Files:**
- Modify: `docs/spec/diagnostics.md`、`docs/spec/declarations.md`、`docs/spec/exhaustiveness.md`、`docs/spec/expressions.md`
- Modify: `docs/implementation/architecture.md`、`docs/implementation/status.md`、`docs/implementation/test-changes.md`

- [ ] **Step 1: spec を直す**

- `diagnostics.md`:
  - 番号の表に E1018〜E1024 と E4005 を足す (名前と内容は Global Constraints と spec の節2〜4の表)
  - 「番号を割り当てていない診断」の E1xxx の行を「修飾なしの名前の衝突」だけにする
  - 網羅性の診断の表の「なし | 到達しない等式」を「E4005 | 到達しない等式」にし、本文の「到達しない等式の Warning は、複数の等式を実装する段階6a で入れる。」を消す
  - E4002 の行に、等式が複数あるときは各等式の先頭を secondary にすることを書く (今の文言のままでよければ変えない)
- `declarations.md` の「fixity」: fixity が名前の解決した先の定義に付くこと、宣言の位置を問わないこと、宣言がなければ `infixl 9` であること、Prelude の演算子の fixity は変えられないこと (E1022)、ユーザーの定義が脱糖用の演算子 (`&&`、`||`、`|>`、`<|`) を隠せること、標準の演算子の fixity は Prelude が宣言することを書く。「シグネチャと等式」の E1xxx の箇条に番号 (E1018〜E1020) を書く
- `exhaustiveness.md` の「検査パス」: 等式の `match` は等式として報告すること (E4002 と E4005) を書く
- `expressions.md` の「`use`」と「セクション」の E1xxx に番号 (E1024、E1023) を書く。「ブロックと `let`」の `let p = e in e2` は今のままでよい

- [ ] **Step 2: 実装の文書を直す**

- `architecture.md`:
  - `eml_hir` の節に、等式の脱糖 (`MatchSource::Equations`、`Function::equation_ranges`、隠れた変数 `$0`)、fixity の表 (`ItemScope` の `Fixity`、Prelude の宣言)、`binary` の名前解決の順、セクションの脱糖 (`lower/section.rs`)、`use` の脱糖 (`lower_stmts`) を書く
  - `eml_types` の節の網羅性の項目に、等式の行列を書く
  - `simplify` の項目は Task 3 で直してある
- `status.md`:
  - 段階の表の 6a を「完了」にする
  - 各 crate の状況の `eml_syntax`、`eml_hir`、`eml_types`、`eml_core_ir` の行に、6a で足したものを書く
  - 「次の作業の注意点」に残っている「段階6a:」の項をすべて消す
  - 「spec に反映済みで、実装は後の段階で扱うもの」の「網羅されていない等式の診断は…到達しない等式の Warning は段階6a で実装する」の項を消す
  - 「完了した作業」の表に「縦の貫通 段階6a」の行を足す。内容は、複数の等式の脱糖と E1018〜E1020、演算子の定義と fixity の表と E1021・E1022、セクションと E1023、`use` と E1024、`let ... in`、パターンの型の明示の文法、ラムダと節の E1017、等式の E4002・E4005、等式の検査の連鎖の修正、`simplify` の F・K1・B2 の拡張・DCE
- `test-changes.md`: 段階6a の節に、Task 2、5、6、9 で記録したものがそろっていることを確かめる

- [ ] **Step 3: 全体を確かめてコミット**

```bash
cargo test && cargo clippy --all-targets && cargo fmt
python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py docs/implementation/status.md
git add docs
git commit -m "Document stage 6a"
```

リンターの「英単語の前後の半角空白」の指摘は、既存の文書の書き方に合わせるので直さない。

- [ ] **Step 4: 作業用の文書を消す**

段階6a のレビューが済んだら、spec と計画の内容が `docs/spec/` と `docs/implementation/` に移ったことを確かめ、`docs/superpowers/specs/2026-10-05-stage-6a-syntax-sugar-design.md` とこの計画を消してコミットする (`Remove the working design and plan of stage 6a`)。
