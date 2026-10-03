# 縦の貫通 段階2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ラムダ、クロージャ、部分適用と関数値、シグネチャの型変数と row 変数による多相、Kind の SCC ごとの推論、`>>` / `<<` を、HIR、型検査、Core IR、ランタイム、インタプリタの全体に通す。

**Architecture:** 下の層から積む。型の表 (`eml_types` の `table.rs` と `kind.rs`) の基本の操作と誤りの修正 (Task 1〜3) の上に、HIR の新しい構文 (Task 4〜6)、型検査の多相・関数値・ラムダ・Kind の推論 (Task 7〜10) を載せる。実行側は、ランタイムとインタプリタのクロージャ (Task 11)、Core IR の関数値 (Task 12) とラムダの持ち上げ (Task 13) の順に入れ、UI テスト (Task 14) と文書 (Task 15) で締める。関数値は eval/apply 方式のクロージャ (「関数 + 先頭の引数の並び」) で表す。

**Tech Stack:** Rust (edition 2024)、rowan 0.16、la-arena 0.3、insta 1.49。新しい外部 crate は足さない。

**Spec:** `docs/superpowers/specs/2026-10-04-vertical-slice-2-design.md` (段階2の内部設計)。規範は `docs/spec/` の `types.md` (型変数のスコープ、引数が矢印より少ない等式、関数値の row を開くこと、ラムダの推論、Kind の制約のスキーム)、`linearity.md` (捕獲の数え方、SCC ごとのパス)、`diagnostics.md` (E1002、ラムダの E2002)、`core-ir.md` (クロージャと eval/apply)、`runtime.md` (クロージャのオブジェクト)。

## Global Constraints

- 作業は `main` から切ったブランチ `vertical-slice-2` で行う。タスクごとにコミットする。コミットメッセージは英語で、末尾にこのセッションの `Co-Authored-By` と `Claude-Session` の行を付ける
- 外部 crate は増やさない。集合と表は `std::collections` を使う
- コードのコメントは日本語で、何をするかではなく理由を書く。規則を指すときは `docs/` のパスを書く。日本語を書く前に `yomiyasu:yomiyasu` スキルを読み、その規則に従う (CLAUDE.md)
- 各段階は `fn stage(&In) -> (Out, Vec<Diagnostic>)` の純粋関数のままにする。`eml_core_ir::lower` だけは、エラーのないプログラムを受け取り診断を返さない
- 診断の help / note は eml 自身の規則の説明に限る。他の言語の書き方を前提にしたヒントは入れない
- インタプリタとランタイムの値とフレームに `Rc` と `RefCell` を使わない。`unsafe` を書かない
- 既存のテストは、このプランで名前を挙げたものだけを変える。変える理由は Task 0 で `docs/implementation/testing.md` に記録する
- 各タスクの最後に `cargo test`、`cargo clippy --all-targets`、`cargo fmt` を通す。clippy の警告を残さない
- インラインスナップショットの期待値は、このプランのコードが出す形を書いてある。食い違ったら、まず実装がプランのコードと一致しているかを確かめる。プランの期待値の誤り (位置の数え違いなど) だと判断した場合は、理由をコミットメッセージに書いてから直す
- 診断の位置の表記は、1 始まりの行と、文字数で数えた列 (`2:7`) である
- 深い再帰を避ける。`Let` の連鎖や長いリストをたどる処理はループか作業リストで書く。式の入れ子は E0013 の上限 (256) で抑えられているので、式の木の再帰はよい

## Review Focus

- 2回呼ぶクロージャ (共有されたクロージャの `Apply`)。捕まえた文字列の参照を複製してからクロージャを手放し、リークも解放済みアクセスも起こさない → Task 11 の `a_shared_closure_keeps_its_captured_values`、Task 12 の `run/partial_application.em` (`hi` を2回呼ぶ)、Task 13 の `run/closures.em` (`shout` を2回呼ぶ)
- 関数を返す関数への余った引数 (`adder 3 4`、`hello "carol"`)。`ApplyFrame` を経由して正しい値を返す → Task 11 の `extra_arguments_are_applied_to_the_returned_function`、Task 12 の `run/partial_application.em`
- 呼ばずに捨てる部分適用 (`let unused = greet "never called"`)。クロージャが捕まえた文字列も解放される → Task 12 の `run/partial_application.em`
- 使われない多相なラムダ (`let unused = fn x -> x`)。型変数が解けないまま Core IR に来ても落ちない → Task 13 の `run/closures.em`
- クロージャを通した 10 万段の再帰と、その後の 10 万個のクロージャの連鎖の解放。Rust のスタックを溢れさせない → Task 13 の `run/closures.em` の `count_down 100000`

---

### Task 0: ブランチを切り、テストの変更の合意を記録する

段階2で、段階1が E0004 を期待していた構文 (関数値、ラムダ、型変数、row 変数、`>>`) を通すようになる。そのため、いくつかの既存のテストの期待値が変わる。brainstorming で合意した変更なので、先に `testing.md` に記録しておく。

**Files:**
- Modify: `docs/implementation/testing.md` (「テストの変更に関する合意済みの例外」)

**Interfaces:**
- Produces: 以降のタスクが変えてよい既存のテストの一覧

- [ ] **Step 1: ブランチを作る**

```bash
git switch -c vertical-slice-2
```

- [ ] **Step 2: 合意済みの例外を testing.md に足す**

`docs/implementation/testing.md` の「テストの変更に関する合意済みの例外」の箇条書きの最後に、次の2項目を足す。

```markdown
- 縦の貫通の段階2で、段階1が E0004 を期待していた関数値、ラムダ、型変数、row 変数、`>>` / `<<` を通すようになった。そのため、次のテストの期待値を新しい結果に合わせた。後の段階の構文を E0004 にすることは、段階2でも E0004 のまま残る構文 (`data`、`match`、タプルのパターン、`(+)` など) で確かめ続ける
  - `eml_hir`: `lower.rs` の `constructs_of_later_stages_are_not_yet_supported` (ラムダ)、`rows_and_types_of_later_stages` を `row_variables_type_variables_and_unknown_effects` に改名、`operators.rs` の `mixed_associativity_is_rejected_in_both_orders` (`>>`)
  - `eml_types`: `check.rs` の `function_values_are_not_yet_supported` を `function_values_and_partial_application` に、`function_typed_parameters_cannot_be_called_or_passed` を `function_typed_parameters_can_be_called_and_passed` に改名
  - UI テスト: `check-fail/not_yet_supported.em` のスナップショットから関数値とラムダの E0004 が消えた。`check-fail/function_value_parameter.em` はエラーにならなくなったので削除し、同じ内容を `run/higher_order.em` で実行して確かめる
- `eml_types::Type::Fn` に row の末尾 (`tail`) を足したので、`ty.rs` の単体テスト `function_types_are_displayed_like_the_surface_syntax` の型の組み立てに `tail: None` を足した。構造体へのフィールドの追加に伴う機械的な書き換えで、期待値は変えていない
```

- [ ] **Step 3: コミットする**

```bash
git add docs/implementation/testing.md
git commit -m "Record the test changes agreed for the second vertical slice"
```

---

### Task 1: `Type` に row の末尾と型変数を持たせ、記録済みの誤り3件を直す

`docs/implementation/status.md` の「段階2で直す `eml_types` の誤り」の3件を直す。3件目 (export が row の末尾を落とす) を直すために、後の段階に渡す `Type` に row の末尾と型変数を足す。

**Files:**
- Modify: `crates/eml_types/src/ty.rs`
- Modify: `crates/eml_types/src/table.rs`
- Modify: `crates/eml_types/src/check.rs` (`check_main` の `expected` に `tail: None`)
- Modify: `crates/eml_types/src/lib.rs` (`RowTail` の再公開)

**Interfaces:**
- Produces: `eml_types::Type::Var(String)`、`eml_types::Type::Fn { param, linearity, effects, tail: Option<RowTail>, ret }`、`eml_types::RowTail::{Rigid(String), Flexible}`。`Type` の表示は `<IO | e>`、`<e>`、解けない row 変数は `_`、解けない型変数も `_`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/src/table.rs` の `mod tests` の最後に足す。

```rust
    #[test]
    fn rows_with_the_same_tail_and_different_labels_report_the_missing_effects() {
        let mut table = Table::new();
        let r = table.fresh_row_var();
        let a = Row {
            labels: vec![Effect::Io],
            tail: Some(r),
        };
        let b = Row {
            labels: vec![],
            tail: Some(r),
        };
        assert_eq!(
            table.unify_row(&a, &b),
            Err(UnifyError::MissingEffects(vec![Effect::Io]))
        );
    }

    #[test]
    fn a_variable_unified_with_error_becomes_error() {
        let mut table = Table::new();
        let v = table.fresh_var();
        assert_eq!(table.unify(v, table.error), Ok(()));
        assert_eq!(table.unify(v, table.int), Ok(()));
        assert_eq!(table.unify(v, table.string), Ok(()));
    }

    #[test]
    fn export_keeps_an_open_row() {
        let mut table = Table::new();
        let r = table.fresh_row_var();
        let f = table.function(
            table.int,
            Row {
                labels: vec![Effect::Io],
                tail: Some(r),
            },
            table.int,
        );
        assert_eq!(table.export(f).to_string(), "Int -> <IO | _> Int");
        let v = table.fresh_var();
        assert_eq!(table.export(v).to_string(), "_");
    }
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --lib table`
Expected: 3件が FAIL (1件目は `Err(Occurs)`、2件目は3つ目の `unify` が `Err(Mismatch)`、3件目は `"Int -> <IO> Int"` と `"{error}"`)

- [ ] **Step 3: `Type` に末尾と型変数を足す**

`crates/eml_types/src/ty.rs` の `Type` を次に変える。

```rust
/// 型検査の結果として後の段階に渡す型。推論用の変数は解決済みで、解けずに残った変数は `Var("_")` になる。
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
        /// row の末尾。`None` なら閉じた row である。
        tail: Option<RowTail>,
        ret: Box<Type>,
    },
    /// シグネチャの型変数はその名前、推論で解けなかった変数は `_` を持つ。
    Var(String),
    Error,
}

/// row の末尾。シグネチャの row 変数は名前を持つ。推論で解けなかった row 変数は `_` と表示する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowTail {
    Rigid(String),
    Flexible,
}
```

`contains_error` の `match` に `Type::Var(_)` を足し、`false` にする (`Type::Int | Type::String | Type::Bool | Type::Var(_) => false`)。

`Display` の `Type::Fn` の腕を次に変え、`Type::Var` の腕を足す。

```rust
            Type::Fn {
                param,
                effects,
                tail,
                ret,
                ..
            } => {
                if matches!(**param, Type::Fn { .. }) {
                    write!(f, "({param}) -> ")?;
                } else {
                    write!(f, "{param} -> ")?;
                }
                let names: Vec<&str> = effects.iter().map(|e| e.name()).collect();
                let tail = match tail {
                    Some(RowTail::Rigid(name)) => Some(name.as_str()),
                    Some(RowTail::Flexible) => Some("_"),
                    None => None,
                };
                // 空の閉じた row は書かない。省略した row が `<>` だから (docs/spec/types.md)
                match tail {
                    Some(tail) if names.is_empty() => write!(f, "<{tail}> ")?,
                    Some(tail) => write!(f, "<{} | {tail}> ", names.join(", "))?,
                    None if names.is_empty() => {}
                    None => write!(f, "<{}> ", names.join(", "))?,
                }
                write!(f, "{ret}")
            }
            Type::Var(name) => f.write_str(name),
```

`ty.rs` の単体テスト `function_types_are_displayed_like_the_surface_syntax` の2つの `Type::Fn { ... }` に `tail: None,` を足す (Task 0 で記録した機械的な書き換え)。

`crates/eml_types/src/lib.rs` の `pub use ty::{Effect, Linearity, Multiplicity, Type};` を `pub use ty::{Effect, Linearity, Multiplicity, RowTail, Type};` にする。

`crates/eml_types/src/check.rs` の `check_main` の `expected` に `tail: None,` を足す。

- [ ] **Step 4: 誤り3件を直す**

`crates/eml_types/src/table.rs` で次を変える。

`unify` の `match` の先頭の2つの腕を、変数の腕が先に来るように並べ替える。`Error` と単一化した変数は `Error` に束縛し、以後の制約を吸収させるためである。

```rust
        match (
            self.kinds[a.0 as usize].clone(),
            self.kinds[b.0 as usize].clone(),
        ) {
            // 変数を先に束縛する。`Error` と単一化した変数も `Error` に束縛し、後の制約で診断を出させない
            (TyKind::Var(var), _) => self.bind_var(var, b),
            (_, TyKind::Var(var)) => self.bind_var(var, a),
            // `Error` が関わる制約からは診断を出さない (docs/spec/types.md の「エラーの扱い」)
            (TyKind::Error, _) | (_, TyKind::Error) => Ok(()),
```

`unify_row` の `(Some(x), Some(y)) if x == y` の腕を次にする。閉じた row どうしの場合と同じく、余ったラベルを足りないエフェクトとして返す。

```rust
            (Some(x), Some(y)) if x == y => {
                let mut missing = only_a;
                missing.extend(only_b);
                if missing.is_empty() {
                    Ok(())
                } else {
                    Err(UnifyError::MissingEffects(missing))
                }
            }
```

`export` を次にする。

```rust
    /// 後の段階に渡す形にする。解けていない型変数と row 変数は、`_` として残す。
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
            } => {
                let row = self.resolve_row(&row);
                Type::Fn {
                    param: Box::new(self.export(param)),
                    linearity: match lin {
                        Mult::Known(l) => l,
                        Mult::Var(v) => self.linearity.value(v),
                    },
                    effects: row.labels,
                    tail: row.tail.map(|_| RowTail::Flexible),
                    ret: Box::new(self.export(ret)),
                }
            }
            TyKind::Var(_) => Type::Var("_".to_string()),
            TyKind::Error => Type::Error,
        }
    }
```

`table.rs` の先頭の `use crate::ty::{Effect, Linearity, Multiplicity, Type};` を `use crate::ty::{Effect, Linearity, Multiplicity, RowTail, Type};` にする。`fresh_var` の `#[allow(dead_code)]` を外す。

- [ ] **Step 5: テストが通ることを確かめる**

Run: `cargo test -p eml_types`
Expected: PASS (既存のスナップショットは変わらない。段階1では解けない変数も開いた row も表示に出ないため)

- [ ] **Step 6: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`
Expected: PASS、警告なし

```bash
git add crates/eml_types
git commit -m "Keep open rows and unsolved variables in exported types and fix two unification errors"
```

---

### Task 2: 型の表に rigid 変数、row の包含、Kind の上限、型の複製を足す

シグネチャの型変数 (rigid) と row 変数、呼び出しの row の包含、Kind の制約を足す道具、スキームの具体化に使う型の複製を、`Table` に用意する。使うのは Task 6 以降である。

**Files:**
- Modify: `crates/eml_types/src/table.rs`
- Modify: `crates/eml_types/src/kind.rs` (`KindVar::index`)

**Interfaces:**
- Consumes: Task 1 の `RowTail`
- Produces (すべて `pub(crate)`):
  - `RigidVar` (Copy, Eq, Hash)、`TyKind::Rigid(RigidVar)`
  - `RowVar` に `Hash` を足す
  - `UnifyError::MissingRowVar(String)`
  - `Subst { tys: HashMap<RigidVar, Ty>, rows: HashMap<RowVar, RowVar>, lin: HashMap<KindVar, KindVar>, mult: HashMap<KindVar, KindVar> }` (`Default`)
  - `Table::fresh_rigid(&mut self, name: &str) -> (Ty, RigidVar)`
  - `Table::rigid_linearity(&self, r: RigidVar) -> KindVar`
  - `Table::fresh_rigid_row(&mut self, name: &str) -> RowVar`
  - `Table::is_rigid_row(&self, var: RowVar) -> bool`
  - `Table::row_multiplicity_var(&self, var: RowVar) -> KindVar`
  - `Table::fresh_var_with(&mut self, linearity: KindVar) -> Ty`
  - `Table::fresh_row_var_with(&mut self, multiplicity: KindVar) -> RowVar`
  - `Table::fresh_lin_kind(&mut self) -> KindVar`、`Table::fresh_mult_kind(&mut self) -> KindVar`
  - `Table::fresh_mult(&mut self) -> Mult` (`Mult::Var` を返す)
  - `Table::function_with(&mut self, param: Ty, lin: Mult, row: Row, ret: Ty) -> Ty`
  - `Table::kind_bounds(&self, ty: Ty) -> Vec<Bound<Linearity>>`
  - `Table::kind_at_most(&mut self, ty: Ty, upper: Bound<Linearity>)`
  - `Table::closure_kinds(&mut self, ty: Ty, arity: usize, captured: &[Ty])`
  - `Table::include_row(&mut self, callee: &Row, ambient: &Row) -> Result<(), UnifyError>`
  - `Table::open_spine(&mut self, ty: Ty) -> Ty`
  - `Table::copy_type(&mut self, ty: Ty, subst: &Subst) -> Ty`
  - `Table::kind_vars(&self, ty: Ty) -> (Vec<KindVar>, Vec<KindVar>)` (線形性と多重度の Kind 変数)
  - `KindVar::index(self) -> usize`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/src/table.rs` の `mod tests` に足す。

```rust
    #[test]
    fn rigid_variables_unify_only_with_themselves_and_flexible_variables() {
        let mut table = Table::new();
        let (a, _) = table.fresh_rigid("a");
        let (b, _) = table.fresh_rigid("b");
        assert_eq!(table.unify(a, a), Ok(()));
        assert_eq!(table.unify(a, b), Err(UnifyError::Mismatch));
        assert_eq!(table.unify(a, table.int), Err(UnifyError::Mismatch));
        let v = table.fresh_var();
        assert_eq!(table.unify(v, a), Ok(()));
        assert_eq!(table.export(v).to_string(), "a");
    }

    #[test]
    fn a_rigid_row_variable_cannot_be_bound() {
        let mut table = Table::new();
        let e = table.fresh_rigid_row("e");
        let rigid = Row {
            labels: vec![],
            tail: Some(e),
        };
        assert_eq!(
            table.unify_row(&rigid, &Row::closed(vec![Effect::Io])),
            Err(UnifyError::Mismatch)
        );
        let f = table.function(table.int, rigid.clone(), table.int);
        assert_eq!(table.export(f).to_string(), "Int -> <e> Int");
    }

    #[test]
    fn a_closed_callee_row_is_included_in_a_larger_row() {
        let mut table = Table::new();
        let io = Row::closed(vec![Effect::Io]);
        assert_eq!(table.include_row(&Row::pure(), &io), Ok(()));
        assert_eq!(table.include_row(&io, &io), Ok(()));
        assert_eq!(
            table.include_row(&io, &Row::pure()),
            Err(UnifyError::MissingEffects(vec![Effect::Io]))
        );
    }

    #[test]
    fn a_rigid_callee_row_needs_the_same_variable_in_the_ambient_row() {
        let mut table = Table::new();
        let e = table.fresh_rigid_row("e");
        let callee = Row {
            labels: vec![],
            tail: Some(e),
        };
        let wider = Row {
            labels: vec![Effect::Io],
            tail: Some(e),
        };
        assert_eq!(table.include_row(&callee, &wider), Ok(()));
        assert_eq!(table.include_row(&callee, &callee.clone()), Ok(()));
        assert_eq!(
            table.include_row(&callee, &Row::closed(vec![Effect::Io])),
            Err(UnifyError::MissingRowVar("e".to_string()))
        );
    }

    #[test]
    fn open_spine_opens_only_the_rows_on_the_return_side() {
        let mut table = Table::new();
        let param = table.function(table.int, Row::pure(), table.int);
        let inner = table.function(table.int, Row::pure(), table.int);
        let f = table.function(param, Row::pure(), inner);
        let opened = table.open_spine(f);
        assert_eq!(
            table.export(opened).to_string(),
            "(Int -> Int) -> <_> Int -> <_> Int"
        );
    }

    #[test]
    fn copy_type_replaces_rigid_variables() {
        let mut table = Table::new();
        let (a, ra) = table.fresh_rigid("a");
        let e = table.fresh_rigid_row("e");
        let f = table.function(
            a,
            Row {
                labels: vec![],
                tail: Some(e),
            },
            a,
        );
        let mut subst = Subst::default();
        subst.tys.insert(ra, table.int);
        let copied = table.copy_type(f, &subst);
        assert_eq!(table.export(copied).to_string(), "Int -> <e> Int");
        let mut subst = Subst::default();
        subst.rows.insert(e, table.fresh_row_var());
        let copied = table.copy_type(f, &subst);
        assert_eq!(table.export(copied).to_string(), "a -> <_> a");
    }
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --lib table`
Expected: コンパイルエラー (`fresh_rigid` などがない)

- [ ] **Step 3: 実装する**

`crates/eml_types/src/kind.rs` の `KindVar` に足す。

```rust
impl KindVar {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}
```

`crates/eml_types/src/table.rs` を次のように変える。

先頭の `use` に `use std::collections::HashMap;` を足す。`RowVar` の derive に `Hash` を足す。

```rust
/// シグネチャの型変数。本体の中では固定された型として扱う (docs/spec/types.md の「推論」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RigidVar(u32);
```

`Mult::Var` の `#[allow(dead_code)]` と、`TyKind::Var` の `#[allow(dead_code)]` を外す。`TyKind` に `Rigid(RigidVar)` を足す (`Var(TyVar)` の次)。`UnifyError` に足す。

```rust
    /// 呼び出し先の row の末尾にある、シグネチャの row 変数が、今の row に含まれない。
    MissingRowVar(String),
```

`TyVarInfo` の `linearity` の `#[allow(dead_code)]` を外す。`RowVarInfo` に足す。

```rust
    /// シグネチャの row 変数なら、その名前。本体の中では束縛できない (docs/spec/types.md の「推論」)。
    rigid: Option<String>,
```

`RigidInfo` と、`Table` のフィールド `rigids: Vec<RigidInfo>` を足し、`Table::new` で空にする。

```rust
struct RigidInfo {
    name: String,
    /// 型変数の Kind `Type<μ>` の `μ`。
    linearity: KindVar,
}
```

`Subst` を足す。

```rust
/// スキームを具体化するときの置き換え。rigid 変数を新しい推論用の変数に、多相化した Kind 変数を新しい Kind 変数に変える。
#[derive(Default)]
pub(crate) struct Subst {
    pub tys: HashMap<RigidVar, Ty>,
    pub rows: HashMap<RowVar, RowVar>,
    pub lin: HashMap<KindVar, KindVar>,
    pub mult: HashMap<KindVar, KindVar>,
}
```

`impl Table` に次のメソッドを足す。`fresh_row_var` は `fresh_row_var_with` を呼ぶ形に変え、`RowVarInfo` の初期化に `rigid: None` を足す。`fresh_var` は `fresh_var_with` を呼ぶ形に変える。

```rust
    pub fn function_with(&mut self, param: Ty, lin: Mult, row: Row, ret: Ty) -> Ty {
        self.alloc(TyKind::Fn {
            param,
            lin,
            row,
            ret,
        })
    }

    pub fn fresh_lin_kind(&mut self) -> KindVar {
        self.linearity.fresh()
    }

    pub fn fresh_mult_kind(&mut self) -> KindVar {
        self.multiplicity.fresh()
    }

    pub fn fresh_mult(&mut self) -> Mult {
        Mult::Var(self.linearity.fresh())
    }

    pub fn fresh_var(&mut self) -> Ty {
        let linearity = self.linearity.fresh();
        self.fresh_var_with(linearity)
    }

    pub fn fresh_var_with(&mut self, linearity: KindVar) -> Ty {
        self.ty_vars.push(TyVarInfo {
            binding: None,
            linearity,
        });
        let var = TyVar(self.ty_vars.len() as u32 - 1);
        self.alloc(TyKind::Var(var))
    }

    pub fn fresh_row_var(&mut self) -> RowVar {
        let multiplicity = self.multiplicity.fresh();
        self.fresh_row_var_with(multiplicity)
    }

    pub fn fresh_row_var_with(&mut self, multiplicity: KindVar) -> RowVar {
        self.row_vars.push(RowVarInfo {
            binding: None,
            multiplicity,
            rigid: None,
        });
        RowVar(self.row_vars.len() as u32 - 1)
    }

    pub fn fresh_rigid(&mut self, name: &str) -> (Ty, RigidVar) {
        let linearity = self.linearity.fresh();
        self.rigids.push(RigidInfo {
            name: name.to_string(),
            linearity,
        });
        let rigid = RigidVar(self.rigids.len() as u32 - 1);
        (self.alloc(TyKind::Rigid(rigid)), rigid)
    }

    pub fn rigid_linearity(&self, rigid: RigidVar) -> KindVar {
        self.rigids[rigid.0 as usize].linearity
    }

    pub fn fresh_rigid_row(&mut self, name: &str) -> RowVar {
        let var = self.fresh_row_var();
        self.row_vars[var.0 as usize].rigid = Some(name.to_string());
        var
    }

    pub fn is_rigid_row(&self, var: RowVar) -> bool {
        self.row_vars[var.0 as usize].rigid.is_some()
    }

    pub fn row_multiplicity_var(&self, var: RowVar) -> KindVar {
        self.row_vars[var.0 as usize].multiplicity
    }
```

`occurs` の `match` の `TyKind::Con(_) | TyKind::Error => false` に `TyKind::Rigid(_)` を足す。

`unify` の `match` に、`Con` の腕の前に足す。

```rust
            (TyKind::Rigid(x), TyKind::Rigid(y)) if x == y => Ok(()),
```

`bind_var` を次にする。束縛した型の Kind が変数の Kind に伝わるようにする。スキームの具体化で変数に付いた Kind の制約 (`μ ≤ Unr`) が、束縛した型にも効くためである。

```rust
    fn bind_var(&mut self, var: TyVar, ty: Ty) -> Result<(), UnifyError> {
        if self.occurs(var, ty) {
            return Err(UnifyError::Occurs);
        }
        let mu = self.ty_vars[var.0 as usize].linearity;
        for bound in self.kind_bounds(ty) {
            self.linearity.require(bound, Bound::Var(mu));
        }
        self.ty_vars[var.0 as usize].binding = Some(ty);
        Ok(())
    }
```

`unify_row` の `(Some(x), Some(y))` の腕 (x と y が違う場合) を次にする。

```rust
            (Some(x), Some(y)) => match (self.is_rigid_row(x), self.is_rigid_row(y)) {
                (false, false) => {
                    let rest = self.fresh_row_var();
                    self.bind_row(
                        x,
                        Row {
                            labels: only_b,
                            tail: Some(rest),
                        },
                    )?;
                    self.bind_row(
                        y,
                        Row {
                            labels: only_a,
                            tail: Some(rest),
                        },
                    )
                }
                // rigid な側は束縛できないので、推論用の側に残りを入れる
                (false, true) => {
                    if !only_a.is_empty() {
                        return Err(UnifyError::MissingEffects(only_a));
                    }
                    self.bind_row(
                        x,
                        Row {
                            labels: only_b,
                            tail: Some(y),
                        },
                    )
                }
                (true, false) => {
                    if !only_b.is_empty() {
                        return Err(UnifyError::MissingEffects(only_b));
                    }
                    self.bind_row(
                        y,
                        Row {
                            labels: only_a,
                            tail: Some(x),
                        },
                    )
                }
                (true, true) => Err(UnifyError::Mismatch),
            },
```

`bind_row` の先頭に足す。

```rust
        if self.is_rigid_row(var) {
            return Err(UnifyError::Mismatch);
        }
```

次のメソッドを足す。

```rust
    /// 型の Kind の上界の候補。レコードはフィールドの join なので、フィールドごとの境界を並べる (docs/spec/types.md)。
    pub fn kind_bounds(&self, ty: Ty) -> Vec<Bound<Linearity>> {
        match self.kind(ty) {
            TyKind::Con(_) => vec![Bound::Const(Linearity::Unr)],
            TyKind::Record(fields) => fields
                .iter()
                .flat_map(|(_, field)| self.kind_bounds(*field))
                .collect(),
            TyKind::Fn { lin, .. } => vec![match lin {
                Mult::Known(l) => Bound::Const(*l),
                Mult::Var(v) => Bound::Var(*v),
            }],
            TyKind::Var(var) => vec![Bound::Var(self.ty_vars[var.0 as usize].linearity)],
            TyKind::Rigid(rigid) => vec![Bound::Var(self.rigid_linearity(*rigid))],
            TyKind::Error => Vec::new(),
        }
    }

    /// `ty` の Kind が `upper` 以下であること。
    pub fn kind_at_most(&mut self, ty: Ty, upper: Bound<Linearity>) {
        for bound in self.kind_bounds(ty) {
            self.linearity.require(bound, upper);
        }
    }

    /// 引数を `arity` 個まで受ける関数の、部分適用のクロージャの線形性。矢印 `i` (0 始まり) のクロージャは、先頭の
    /// `i` 個の引数と `captured` を捕まえるので、その Kind 以上になる (docs/spec/types.md の「関数型」)。
    pub fn closure_kinds(&mut self, ty: Ty, arity: usize, captured: &[Ty]) {
        let mut held: Vec<Ty> = captured.to_vec();
        let mut current = ty;
        for _ in 0..arity {
            let TyKind::Fn {
                param, lin, ret, ..
            } = self.kind(current).clone()
            else {
                return;
            };
            let upper = match lin {
                Mult::Known(l) => Bound::Const(l),
                Mult::Var(v) => Bound::Var(v),
            };
            for &value in &held {
                self.kind_at_most(value, upper);
            }
            held.push(param);
            current = ret;
        }
    }

    /// 呼び出し先の row `callee` のエフェクトが、今の row `ambient` にすべて含まれることを確かめる
    /// (docs/spec/types.md の「推論」)。閉じた row と推論用の末尾は、残りを row 変数で受けて単一化する。
    /// rigid な末尾は束縛できないので、ラベルを取り除いた残りの末尾が同じ変数であることを確かめる。
    pub fn include_row(&mut self, callee: &Row, ambient: &Row) -> Result<(), UnifyError> {
        let callee = self.resolve_row(callee);
        match callee.tail {
            Some(tail) if !self.is_rigid_row(tail) => self.unify_row(&callee, ambient),
            tail => {
                let rest = self.fresh_row_var();
                self.unify_row(
                    &Row {
                        labels: callee.labels,
                        tail: Some(rest),
                    },
                    ambient,
                )?;
                let Some(rigid) = tail else {
                    return Ok(());
                };
                let rest = self.resolve_row(&Row {
                    labels: Vec::new(),
                    tail: Some(rest),
                });
                if rest.tail == Some(rigid) {
                    Ok(())
                } else {
                    let name = self.row_vars[rigid.0 as usize].rigid.clone();
                    Err(UnifyError::MissingRowVar(name.unwrap_or_default()))
                }
            }
        }
    }

    /// 戻り値の側に並ぶ矢印の閉じた row を、新しい row 変数で開く。純粋な関数を、エフェクトを持つ関数型の引数に
    /// 渡せるようにするため (docs/spec/types.md の「推論」)。引数の型の中は開かない。
    pub fn open_spine(&mut self, ty: Ty) -> Ty {
        let TyKind::Fn {
            param,
            lin,
            row,
            ret,
        } = self.kind(ty).clone()
        else {
            return ty;
        };
        let ret = self.open_spine(ret);
        let row = self.resolve_row(&row);
        let row = match row.tail {
            Some(_) => row,
            None => Row {
                labels: row.labels,
                tail: Some(self.fresh_row_var()),
            },
        };
        self.function_with(param, lin, row, ret)
    }

    /// `subst` に従って rigid 変数、row 変数、Kind 変数を置き換えた型を作る。スキームの具体化で使う。
    pub fn copy_type(&mut self, ty: Ty, subst: &Subst) -> Ty {
        match self.kind(ty).clone() {
            TyKind::Rigid(rigid) => subst.tys.get(&rigid).copied().unwrap_or(ty),
            TyKind::Fn {
                param,
                lin,
                row,
                ret,
            } => {
                let param = self.copy_type(param, subst);
                let ret = self.copy_type(ret, subst);
                let lin = match lin {
                    Mult::Var(v) => Mult::Var(subst.lin.get(&v).copied().unwrap_or(v)),
                    known => known,
                };
                let row = self.resolve_row(&row);
                let row = Row {
                    labels: row.labels,
                    tail: row
                        .tail
                        .map(|tail| subst.rows.get(&tail).copied().unwrap_or(tail)),
                };
                self.function_with(param, lin, row, ret)
            }
            TyKind::Record(fields) => {
                let fields = fields
                    .into_iter()
                    .map(|(label, field)| (label, self.copy_type(field, subst)))
                    .collect();
                self.alloc(TyKind::Record(fields))
            }
            TyKind::Con(_) | TyKind::Var(_) | TyKind::Error => ty,
        }
    }

    /// 型に現れる Kind 変数。線形性 (rigid 変数の `μ` と矢印の `m`) と多重度 (rigid な row 変数の `σ`) に分けて、
    /// 現れた順に重複なく返す。多相化する変数を決めるのに使う。
    pub fn kind_vars(&self, ty: Ty) -> (Vec<KindVar>, Vec<KindVar>) {
        let mut lin = Vec::new();
        let mut mult = Vec::new();
        let mut work = vec![ty];
        while let Some(ty) = work.pop() {
            match self.kind(ty) {
                TyKind::Rigid(rigid) => push_unique(&mut lin, self.rigid_linearity(*rigid)),
                TyKind::Fn {
                    param,
                    lin: m,
                    row,
                    ret,
                } => {
                    if let Mult::Var(v) = m {
                        push_unique(&mut lin, *v);
                    }
                    if let Some(tail) = self.resolve_row(row).tail
                        && self.is_rigid_row(tail)
                    {
                        push_unique(&mut mult, self.row_multiplicity_var(tail));
                    }
                    work.push(*ret);
                    work.push(*param);
                }
                TyKind::Record(fields) => work.extend(fields.iter().rev().map(|(_, f)| *f)),
                TyKind::Con(_) | TyKind::Var(_) | TyKind::Error => {}
            }
        }
        (lin, mult)
    }
```

`impl Table` の外に足す。

```rust
fn push_unique(vars: &mut Vec<KindVar>, var: KindVar) {
    if !vars.contains(&var) {
        vars.push(var);
    }
}
```

`export` の `match` に足す。

```rust
            TyKind::Rigid(rigid) => Type::Var(self.rigids[rigid.0 as usize].name.clone()),
```

`export` の `Type::Fn` の `tail` を、rigid な row 変数の名前を残す形にする。

```rust
                    tail: row.tail.map(|tail| match &self.row_vars[tail.0 as usize].rigid {
                        Some(name) => RowTail::Rigid(name.clone()),
                        None => RowTail::Flexible,
                    }),
```

`row_multiplicity` の `#[allow(dead_code)]` コメントは、Task 10 まで使わないので残す。

- [ ] **Step 4: テストが通ることを確かめる**

Run: `cargo test -p eml_types`
Expected: PASS

- [ ] **Step 5: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`
Expected: PASS。使われないメソッドの `dead_code` 警告が出る場合は、そのメソッドに `#[allow(dead_code)] // Task N で使う` を付け、使い始めるタスクで外す

```bash
git add crates/eml_types
git commit -m "Add rigid variables, row inclusion, kind bounds, and type copying to the type table"
```

---

### Task 3: Kind の束に、多相化で残す制約の計算と制約の複製を足す

多相化したスキームに残す Kind の制約 (残余の制約) を求める。内部の変数を経由した推移も含めて、残す変数どうしの制約と、残す変数と定数の制約を返す。

**Files:**
- Modify: `crates/eml_types/src/kind.rs`
- Modify: `crates/eml_types/src/table.rs` (`lin_residual` などの窓口、Task 2 で外したテストを戻す)

**Interfaces:**
- Produces:
  - `Lattice::residual(&self, keep: &[KindVar]) -> Vec<(Bound<T>, Bound<T>)>`
  - `Lattice::copy_constraints(&mut self, constraints: &[(Bound<T>, Bound<T>)], map: &HashMap<KindVar, KindVar>)`
  - `Table::lin_residual(&self, keep: &[KindVar]) -> Vec<(Bound<Linearity>, Bound<Linearity>)>`
  - `Table::mult_residual(&self, keep: &[KindVar]) -> Vec<(Bound<Multiplicity>, Bound<Multiplicity>)>`
  - `Table::copy_lin_constraints(&mut self, constraints: &[(Bound<Linearity>, Bound<Linearity>)], map: &HashMap<KindVar, KindVar>)`
  - `Table::copy_mult_constraints(&mut self, constraints: &[(Bound<Multiplicity>, Bound<Multiplicity>)], map: &HashMap<KindVar, KindVar>)`
  - `Table::solve_kinds(&mut self) -> bool` (違反があれば `true`。解を覚えて `export` で使う)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/src/table.rs` の `mod tests` に足す。束縛と部分適用が Kind の制約をどう出すかを、残余の制約で確かめる。

```rust
    #[test]
    fn binding_a_variable_passes_the_kind_of_its_type_to_the_variable() {
        let mut table = Table::new();
        let (a, ra) = table.fresh_rigid("a");
        let v = table.fresh_var();
        // v の Kind は Unr でなければならない。v を a に束縛すると、a の Kind も Unr になる
        table.kind_at_most(v, Bound::Const(Linearity::Unr));
        assert_eq!(table.unify(v, a), Ok(()));
        let mu = table.rigid_linearity(ra);
        assert_eq!(
            table.lin_residual(&[mu]),
            vec![(Bound::Var(mu), Bound::Const(Linearity::Unr))]
        );
    }

    #[test]
    fn closure_kinds_bound_each_partial_application() {
        let mut table = Table::new();
        let (a, ra) = table.fresh_rigid("a");
        let m = table.fresh_mult();
        let inner = table.function_with(a, m, Row::pure(), a);
        let f = table.function(a, Row::pure(), inner);
        table.closure_kinds(f, 2, &[]);
        let Mult::Var(m) = m else { unreachable!() };
        let mu = table.rigid_linearity(ra);
        assert_eq!(
            table.lin_residual(&[mu, m]),
            vec![(Bound::Var(mu), Bound::Var(m))]
        );
    }
```

`crates/eml_types/src/kind.rs` の `mod tests` に足す。

```rust
    #[test]
    fn residual_constraints_pass_through_internal_variables() {
        let mut lattice = Lattice::new(Linearity::Unr);
        let a = lattice.fresh();
        let internal = lattice.fresh();
        let b = lattice.fresh();
        lattice.require(Bound::Var(a), Bound::Var(internal));
        lattice.require(Bound::Var(internal), Bound::Const(Linearity::Unr));
        lattice.require(Bound::Var(a), Bound::Var(b));
        assert_eq!(
            lattice.residual(&[a, b]),
            vec![
                (Bound::Var(a), Bound::Var(b)),
                (Bound::Var(a), Bound::Const(Linearity::Unr)),
            ]
        );
    }

    #[test]
    fn residual_lower_bounds_above_the_bottom_are_kept() {
        let mut lattice = Lattice::new(Linearity::Unr);
        let internal = lattice.fresh();
        let c = lattice.fresh();
        lattice.require(Bound::Const(Linearity::Lin), Bound::Var(internal));
        lattice.require(Bound::Var(internal), Bound::Var(c));
        lattice.require(Bound::Const(Linearity::Unr), Bound::Var(c));
        assert_eq!(
            lattice.residual(&[c]),
            vec![(Bound::Const(Linearity::Lin), Bound::Var(c))]
        );
    }

    #[test]
    fn copied_constraints_use_the_new_variables() {
        let mut lattice = Lattice::new(Linearity::Unr);
        let a = lattice.fresh();
        let copy = lattice.fresh();
        let map = std::collections::HashMap::from([(a, copy)]);
        lattice.copy_constraints(&[(Bound::Const(Linearity::Lin), Bound::Var(a))], &map);
        assert_eq!(lattice.value(copy), Linearity::Lin);
        assert_eq!(lattice.value(a), Linearity::Unr);
    }
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --lib`
Expected: コンパイルエラー (`residual` などがない)

- [ ] **Step 3: 実装する**

`crates/eml_types/src/kind.rs` の先頭に `use std::collections::{HashMap, HashSet};` を足し、`impl<T: Copy + Ord> Lattice<T>` に足す。

```rust
    /// `keep` の変数について、他の変数を経由した推移も含めて成り立つ制約を返す。多相化したスキームに残す制約で、
    /// 具体化のたびに複製する (docs/spec/types.md の「推論」)。下限が束の最小元なら何も言わないので省く。
    pub fn residual(&self, keep: &[KindVar]) -> Vec<(Bound<T>, Bound<T>)> {
        let kept: HashSet<KindVar> = keep.iter().copied().collect();
        let mut upward: HashMap<KindVar, Vec<Bound<T>>> = HashMap::new();
        let mut downward: HashMap<KindVar, Vec<Bound<T>>> = HashMap::new();
        for &(lower, upper) in &self.constraints {
            if let Bound::Var(v) = lower {
                upward.entry(v).or_default().push(upper);
            }
            if let Bound::Var(v) = upper {
                downward.entry(v).or_default().push(lower);
            }
        }
        let mut out = Vec::new();
        for &start in keep {
            let (vars, least_upper) = reach(&upward, &kept, start, |a, b| a.min(b));
            for var in vars {
                push_constraint(&mut out, (Bound::Var(start), Bound::Var(var)));
            }
            if let Some(c) = least_upper {
                push_constraint(&mut out, (Bound::Var(start), Bound::Const(c)));
            }
            let (_, greatest_lower) = reach(&downward, &kept, start, |a, b| a.max(b));
            if let Some(c) = greatest_lower.filter(|c| *c != self.bottom) {
                push_constraint(&mut out, (Bound::Const(c), Bound::Var(start)));
            }
        }
        out
    }

    /// スキームに残した制約を、具体化した新しい変数について足す。`map` にない変数はそのまま使う。
    pub fn copy_constraints(
        &mut self,
        constraints: &[(Bound<T>, Bound<T>)],
        map: &HashMap<KindVar, KindVar>,
    ) {
        let rename = |bound: Bound<T>| match bound {
            Bound::Var(v) => Bound::Var(map.get(&v).copied().unwrap_or(v)),
            constant => constant,
        };
        for &(lower, upper) in constraints {
            self.constraints.push((rename(lower), rename(upper)));
        }
    }
```

`impl` の外に足す。

```rust
/// `start` から辺をたどり、出会った `keep` の変数 (その先へは進まない) と、出会った定数を `pick` でまとめた値を返す。
/// `keep` の変数の先は、その変数の残余の制約が受け持つ。
fn reach<T: Copy>(
    edges: &HashMap<KindVar, Vec<Bound<T>>>,
    keep: &HashSet<KindVar>,
    start: KindVar,
    pick: impl Fn(T, T) -> T,
) -> (Vec<KindVar>, Option<T>) {
    let mut vars = Vec::new();
    let mut constant: Option<T> = None;
    let mut seen = HashSet::from([start]);
    let mut work = vec![start];
    while let Some(var) = work.pop() {
        for &next in edges.get(&var).into_iter().flatten() {
            match next {
                Bound::Const(c) => constant = Some(constant.map_or(c, |old| pick(old, c))),
                Bound::Var(w) if !seen.insert(w) => {}
                Bound::Var(w) if keep.contains(&w) => vars.push(w),
                Bound::Var(w) => work.push(w),
            }
        }
    }
    (vars, constant)
}

fn push_constraint<T: PartialEq>(out: &mut Vec<(Bound<T>, Bound<T>)>, constraint: (Bound<T>, Bound<T>)) {
    if !out.contains(&constraint) {
        out.push(constraint);
    }
}
```

`crates/eml_types/src/table.rs` の `Table` にフィールド `lin_solution: Option<Vec<Linearity>>` を足して `new` で `None` にし、`impl Table` に足す。

```rust
    pub fn lin_residual(&self, keep: &[KindVar]) -> Vec<(Bound<Linearity>, Bound<Linearity>)> {
        self.linearity.residual(keep)
    }

    pub fn mult_residual(&self, keep: &[KindVar]) -> Vec<(Bound<Multiplicity>, Bound<Multiplicity>)> {
        self.multiplicity.residual(keep)
    }

    pub fn copy_lin_constraints(
        &mut self,
        constraints: &[(Bound<Linearity>, Bound<Linearity>)],
        map: &HashMap<KindVar, KindVar>,
    ) {
        self.linearity.copy_constraints(constraints, map);
    }

    pub fn copy_mult_constraints(
        &mut self,
        constraints: &[(Bound<Multiplicity>, Bound<Multiplicity>)],
        map: &HashMap<KindVar, KindVar>,
    ) {
        self.multiplicity.copy_constraints(constraints, map);
    }

    /// すべての Kind の制約を解き、線形性の解を覚える。`export` が式ごとに解き直さずに済むようにするため。
    /// 定数の上限を超えた制約があれば `true` を返す。
    pub fn solve_kinds(&mut self) -> bool {
        let (lin, lin_violated) = self.linearity.solve();
        let (_, mult_violated) = self.multiplicity.solve();
        self.lin_solution = Some(lin);
        !lin_violated.is_empty() || !mult_violated.is_empty()
    }
```

`export` の線形性の計算を、覚えた解があれば使う形にする。

```rust
                    linearity: match lin {
                        Mult::Known(l) => l,
                        Mult::Var(v) => match &self.lin_solution {
                            Some(solution) => solution[v.index()],
                            None => self.linearity.value(v),
                        },
                    },
```

- [ ] **Step 4: テストが通ることを確かめる**

Run: `cargo test -p eml_types`
Expected: PASS

- [ ] **Step 5: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`

```bash
git add crates/eml_types
git commit -m "Compute the residual kind constraints a scheme keeps and copy them on instantiation"
```

---

### Task 4: HIR でラムダと型を明示した引数を受ける

`fn x y -> e` と `fn (x : Int) -> e` を HIR に変換する。型検査と Core IR はまだラムダを扱わないので、型検査が E0004 を出す仮の門を置く (Task 9 で外す)。

**Files:**
- Modify: `crates/eml_syntax/src/ast.rs` (`LambdaExpr`、`AnnotPat`、`VarType` のアクセサ)
- Modify: `crates/eml_hir/src/hir.rs`、`crates/eml_hir/src/lower/expr.rs`、`crates/eml_hir/src/pretty.rs`
- Modify: `crates/eml_types/src/check.rs` (仮の門と `PatKind::Annot`)
- Modify: `crates/eml_core_ir/src/lower.rs` (`PatKind::Annot` と `ExprKind::Lambda` の腕)
- Test: `crates/eml_hir/tests/lower.rs`

**Interfaces:**
- Produces:
  - `ast::LambdaExpr::params(&self) -> AstChildren<Pat>`、`ast::LambdaExpr::body(&self) -> Option<Expr>`
  - `ast::AnnotPat::pat(&self) -> Option<Pat>`、`ast::AnnotPat::ty(&self) -> Option<Type>`
  - `ast::VarType::name(&self) -> Option<SyntaxToken>`
  - `eml_hir::ExprKind::Lambda { params: Vec<PatId>, body: ExprId }`
  - `eml_hir::PatKind::Annot { pat: PatId, ty: TypeRefId }` (ラムダの引数にだけ現れる)
  - HIR の表示: `(fn x#1 (z#2 : Int) _ -> body)`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_hir/tests/lower.rs` に足す。

```rust
#[test]
fn lambdas_bind_their_parameters_only_in_the_body() {
    let text = "f : Int -> Int\nf x =\n  let g = fn y (z : Int) _ -> x + y\n  y";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> Int
    f x#0 = {
      let g#3 = (fn y#1 (z#2 : Int) _ -> (+ x#0 y#1))
      <missing>
    }
    ---
    E1001 4:3 cannot find value `y`
    ");
}

#[test]
fn a_lambda_can_be_the_last_argument() {
    let text = "call : Int -> (Int -> Int) -> Int\ncall n f = f n\n\ng : Int -> Int\ng n = call n fn x -> x + 1";
    insta::assert_snapshot!(lower_text(text), @r"
    call : Int -> (Int -> Int) -> Int
    call n#0 f#1 = (f#1 n#0)
    g : Int -> Int
    g n#0 = (@call n#0 (fn x#1 -> (+ x#1 1)))
    ");
}
```

既存のテスト `constructs_of_later_stages_are_not_yet_supported` の期待値を次に変える (Task 0 で記録した変更)。

```rust
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> Int
    f x#0 = {
      let g#2 = (fn y#1 -> y#1)
      <missing>
    }
    ---
    E0004 1:1 `data` declarations are not supported yet
    E0004 5:3 `match` is not supported yet
    ");
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_hir --test lower`
Expected: FAIL (ラムダが `<missing>` になり E0004 が出る)

- [ ] **Step 3: AST のアクセサを足す**

`crates/eml_syntax/src/ast.rs` に足す。

```rust
impl LambdaExpr {
    pub fn params(&self) -> AstChildren<Pat> {
        support::children(&self.syntax)
    }

    pub fn body(&self) -> Option<Expr> {
        support::child(&self.syntax)
    }
}

impl AnnotPat {
    pub fn pat(&self) -> Option<Pat> {
        support::child(&self.syntax)
    }

    pub fn ty(&self) -> Option<Type> {
        support::child(&self.syntax)
    }
}

impl VarType {
    pub fn name(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::LIDENT)
    }
}
```

- [ ] **Step 4: HIR に足す**

`crates/eml_hir/src/hir.rs` の `ExprKind` の最後に足す。

```rust
    /// 引数のスコープは本体だけである (docs/spec/expressions.md の「ラムダ」)。
    Lambda {
        params: Vec<PatId>,
        body: ExprId,
    },
```

`PatKind` の最後に足す。

```rust
    /// `fn (x : Int) -> e` の引数。等式と `let` の型を明示したパターンは、まだ E0004 にする。
    Annot {
        pat: PatId,
        ty: TypeRefId,
    },
```

`crates/eml_hir/src/lower/expr.rs` の `lower_expr` の `ast::Expr::LambdaExpr(e)` の腕を次にする。

```rust
            ast::Expr::LambdaExpr(lambda) => {
                let mark = self.scope.len();
                let params = lambda
                    .params()
                    .map(|pat| self.lower_lambda_param(pat))
                    .collect();
                let body = self.lower_expr(lambda.body(), range);
                self.scope.truncate(mark);
                self.alloc(ExprKind::Lambda { params, body }, range)
            }
```

`impl BodyLowering` に足す。

```rust
    /// ラムダの引数だけは型の明示を受ける (docs/spec/expressions.md の「ラムダ」)。
    fn lower_lambda_param(&mut self, pat: ast::Pat) -> PatId {
        let ast::Pat::AnnotPat(annot) = pat else {
            return self.lower_pat(Some(pat), TextRange::default());
        };
        let range = annot.syntax().text_range();
        let ty = self.lower_type(annot.ty(), range);
        let inner = self.lower_pat(annot.pat(), range);
        self.pats.alloc(Pat {
            kind: PatKind::Annot { pat: inner, ty },
            range,
        })
    }
```

`crates/eml_hir/src/pretty.rs` の `expr` に腕を足す。

```rust
            ExprKind::Lambda { params, body: lambda_body } => {
                let mut s = "(fn".to_string();
                for &param in params {
                    write!(s, " {}", self.pat(body, param)).unwrap();
                }
                write!(s, " -> {})", self.expr(body, *lambda_body, indent)).unwrap();
                s
            }
```

`pat` に腕を足す。

```rust
            PatKind::Annot { pat, ty } => format!("({} : {})", self.pat(body, *pat), self.ty(*ty)),
```

- [ ] **Step 5: 型検査と Core IR に仮の腕を足す**

`crates/eml_types/src/check.rs` の `infer_expr` の `match` に足す。HIR の範囲はラムダ全体なので、段階1の診断と同じく `fn` のキーワードだけを指す。

```rust
            ExprKind::Lambda { .. } => {
                // ラムダの型検査は Task 9 で入れる。それまでは段階1と同じ E0004 を出す
                self.diagnostics.push(not_yet_supported(
                    self.file(),
                    TextRange::at(expr.range.start(), 2.into()),
                    "lambdas are not supported yet",
                ));
                self.table.error
            }
```

`bind_pat` の `match` に足す。

```rust
            PatKind::Annot { pat, .. } => self.bind_pat(*pat, ty),
```

`crates/eml_core_ir/src/lower.rs` の `atom` の `match` に足す。

```rust
            ExprKind::Lambda { .. } => {
                unreachable!("the type checker rejects lambdas until they are lowered")
            }
```

`bind_pat` の `if let` を、`PatKind::Annot` の内側も見る形にする。

```rust
    fn bind_pat(&mut self, pat: PatId, value: Atom) {
        match self.body.pats[pat].kind {
            PatKind::Bind(local) => {
                self.locals.insert(local, value);
            }
            PatKind::Annot { pat, .. } => self.bind_pat(pat, value),
            _ => {}
        }
    }
```

- [ ] **Step 6: テストが通ることを確かめる**

Run: `cargo test -p eml_hir && cargo test -p eml_cli --test ui`
Expected: PASS。`check-fail/not_yet_supported.em` のスナップショットは変わらない (E0004 の位置が `fn` のまま)

- [ ] **Step 7: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`

```bash
git add crates
git commit -m "Lower lambdas and annotated lambda parameters to HIR"
```

---

### Task 5: HIR でシグネチャの型変数と row 変数を受ける

シグネチャに現れた型変数と row 変数を関数ごとの表に入れ、本体の注釈からも同じ表を引く。型検査はまだ多相を扱わないので、型検査が E0004 を出す仮の門を置く (Task 7 で外す)。

**Files:**
- Modify: `crates/eml_hir/src/hir.rs`、`crates/eml_hir/src/lower/mod.rs`、`crates/eml_hir/src/lower/types.rs`、`crates/eml_hir/src/lower/expr.rs`、`crates/eml_hir/src/pretty.rs`
- Modify: `crates/eml_types/src/check.rs` (仮の門)
- Test: `crates/eml_hir/tests/lower.rs`

**Interfaces:**
- Produces:
  - `eml_hir::TypeVarId = Idx<TypeVarDecl>`、`eml_hir::TypeVarDecl { name: String, range: TextRange }`
  - `eml_hir::RowVarId = Idx<RowVarDecl>`、`eml_hir::RowVarDecl { name: String, range: TextRange }`
  - `eml_hir::Function::type_vars: Arena<TypeVarDecl>`、`eml_hir::Function::row_vars: Arena<RowVarDecl>`
  - `eml_hir::TypeRefKind::Var(TypeVarId)`
  - `eml_hir::RowRef::Open { effects: Vec<EffectRef>, tail: RowVarId, range: TextRange }`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_hir/tests/lower.rs` に足す。

```rust
#[test]
fn signature_variables_scope_over_the_body() {
    let text = "f : a -> <e> a\nf x =\n  let y : a = x\n  let g = fn (h : b -> <e> b) -> h\n  y";
    insta::assert_snapshot!(lower_text(text), @r"
    f : a -> <e> a
    f x#0 = {
      let y#1 : a = x#0
      let g#3 = (fn (h#2 : <error> -> <e> <error>) -> h#2)
      y#1
    }
    ---
    E1002 4:19 cannot find type variable `b`
    E1002 4:28 cannot find type variable `b`
    ");
}

#[test]
fn type_variables_and_row_variables_have_separate_names() {
    let text = "f : a -> <IO | a> a\nf x = x\n\ng : Int -> <IO | e> Int\ng x = (x : Int -> <f> Int)";
    insta::assert_snapshot!(lower_text(text), @r"
    f : a -> <IO | a> a
    f x#0 = x#0
    g : Int -> <IO | e> Int
    g x#0 = (x#0 : Int -> <error> Int)
    ---
    E1002 5:20 cannot find row variable `f`
    ");
}
```

既存のテスト `rows_and_types_of_later_stages` を `row_variables_type_variables_and_unknown_effects` に改名し、期待値を次に変える (Task 0 で記録した変更)。

```rust
#[test]
fn row_variables_type_variables_and_unknown_effects() {
    let text = "f : Int -> <e> Int\nf x = x\ng : a -> <State> Int\ng x = 1";
    insta::assert_snapshot!(lower_text(text), @r"
    f : Int -> <e> Int
    f x#0 = x#0
    g : a -> <error> Int
    g x#0 = 1
    ---
    E1002 3:11 cannot find effect `State`
    ");
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_hir --test lower`
Expected: FAIL (型変数と row 変数が E0004 になる)

- [ ] **Step 3: HIR の型を足す**

`crates/eml_hir/src/hir.rs` に足す。

```rust
pub type TypeVarId = Idx<TypeVarDecl>;
pub type RowVarId = Idx<RowVarDecl>;

/// シグネチャの型変数。本体の注釈からも同じ変数を指す (docs/spec/types.md の「推論」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeVarDecl {
    pub name: String,
    /// シグネチャで最初に現れた位置。
    pub range: TextRange,
}

/// シグネチャの row 変数。型変数とは別の名前空間に置く (docs/spec/types.md の「推論」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowVarDecl {
    pub name: String,
    pub range: TextRange,
}
```

`Function` に足す。

```rust
    /// シグネチャに現れた型変数と row 変数。
    pub type_vars: Arena<TypeVarDecl>,
    pub row_vars: Arena<RowVarDecl>,
```

`TypeRefKind` に `Var(TypeVarId),` を足す。`RowRef` に足す。

```rust
    /// `<e>` と `<IO | e>`。
    Open {
        effects: Vec<EffectRef>,
        tail: RowVarId,
        range: TextRange,
    },
```

- [ ] **Step 4: 型の変換で表を引く**

`crates/eml_hir/src/lower/types.rs` の `TypeLowering` を次にする。

```rust
pub(super) struct TypeLowering<'a> {
    pub file: FileId,
    pub types: &'a mut Arena<TypeRef>,
    pub type_vars: &'a mut Arena<TypeVarDecl>,
    pub row_vars: &'a mut Arena<RowVarDecl>,
    /// シグネチャなら真で、新しい変数の名前を表に入れる。本体の注釈では表にある名前だけを使える。
    pub define: bool,
    pub diagnostics: &'a mut Vec<Diagnostic>,
}
```

`use` に `TypeVarDecl, RowVarDecl` を足す。`lower` の `ast::Type::VarType(_)` の腕を次にする。

```rust
            ast::Type::VarType(var) => match var.name() {
                Some(name) => self.type_var(&name, range),
                None => TypeRefKind::Error,
            },
```

`row` を次にする。

```rust
    fn row(&mut self, row: &ast::EffectRow) -> RowRef {
        let mut valid = true;
        let tail = match row.tail() {
            Some(tail) => {
                let id = self.row_var(&tail);
                valid &= id.is_some();
                id
            }
            None => None,
        };
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
                    Label::new(
                        self.file,
                        effect.syntax().text_range(),
                        "not found in this scope",
                    ),
                ));
                valid = false;
            }
        }
        let range = row.syntax().text_range();
        match (valid, tail) {
            (false, _) => RowRef::Error,
            (true, Some(tail)) => RowRef::Open {
                effects,
                tail,
                range,
            },
            (true, None) => RowRef::Closed { effects, range },
        }
    }

    fn type_var(&mut self, name: &SyntaxToken, range: TextRange) -> TypeRefKind {
        let text = name.text();
        if let Some((id, _)) = self.type_vars.iter().find(|(_, var)| var.name == text) {
            return TypeRefKind::Var(id);
        }
        if self.define {
            let id = self.type_vars.alloc(TypeVarDecl {
                name: text.to_string(),
                range,
            });
            return TypeRefKind::Var(id);
        }
        self.diagnostics.push(Diagnostic::error(
            codes::UNDEFINED_TYPE,
            format!("cannot find type variable `{text}`"),
            Label::new(self.file, range, "not found in the signature"),
        ));
        TypeRefKind::Error
    }

    fn row_var(&mut self, name: &SyntaxToken) -> Option<RowVarId> {
        let text = name.text();
        let range = name.text_range();
        if let Some((id, _)) = self.row_vars.iter().find(|(_, var)| var.name == text) {
            return Some(id);
        }
        if self.define {
            return Some(self.row_vars.alloc(RowVarDecl {
                name: text.to_string(),
                range,
            }));
        }
        self.diagnostics.push(Diagnostic::error(
            codes::UNDEFINED_TYPE,
            format!("cannot find row variable `{text}`"),
            Label::new(self.file, range, "not found in the signature"),
        ));
        None
    }
```

`not_yet_supported` の `use` が使われなくなったら外さない (`AppType` と `TupleType` でまだ使う)。

`crates/eml_hir/src/lower/mod.rs` で、シグネチャを変換する前に `let mut type_vars = Arena::new(); let mut row_vars = Arena::new();` を作り、`TypeLowering` に `type_vars: &mut type_vars, row_vars: &mut row_vars, define: true,` を渡す。`Function` の初期化に `type_vars, row_vars,` を足す。本体の変換を次にする。

```rust
    for (id, equation) in pending {
        let function = &mut functions[id];
        let scope = TypeScope {
            types: &mut function.types,
            type_vars: &mut function.type_vars,
            row_vars: &mut function.row_vars,
        };
        let body = BodyLowering::new(file, &names, scope, &mut diagnostics).lower_equation(&equation);
        functions[id].body = Some(body);
    }
```

`crates/eml_hir/src/lower/types.rs` に足す。

```rust
/// 本体の注釈が引く、関数ごとの型の置き場所。
pub(super) struct TypeScope<'a> {
    pub types: &'a mut Arena<TypeRef>,
    pub type_vars: &'a mut Arena<TypeVarDecl>,
    pub row_vars: &'a mut Arena<RowVarDecl>,
}
```

`crates/eml_hir/src/lower/expr.rs` の `BodyLowering` のフィールド `types: &'a mut Arena<TypeRef>` を `types: TypeScope<'a>` に変え、`new` の引数も `types: TypeScope<'a>` にする。`lower_type` を次にする。

```rust
    fn lower_type(&mut self, ty: Option<ast::Type>, fallback: TextRange) -> TypeRefId {
        TypeLowering {
            file: self.file,
            types: &mut *self.types.types,
            type_vars: &mut *self.types.type_vars,
            row_vars: &mut *self.types.row_vars,
            define: false,
            diagnostics: &mut *self.diagnostics,
        }
        .lower(ty, fallback)
    }
```

`crates/eml_hir/src/lower/mod.rs` の `use types::TypeLowering;` を `use types::{TypeLowering, TypeScope};` にする。

- [ ] **Step 5: 表示を足す**

`crates/eml_hir/src/pretty.rs` の `ty` の `match` に足す。

```rust
            TypeRefKind::Var(id) => self.function.type_vars[*id].name.clone(),
```

row の表示を次にする。

```rust
                let effect_names = |effects: &[EffectRef]| -> Vec<String> {
                    effects.iter().map(|EffectRef::Io| "IO".to_string()).collect()
                };
                let row = match row {
                    RowRef::Omitted => String::new(),
                    RowRef::Closed { effects, .. } => {
                        format!("<{}> ", effect_names(effects).join(", "))
                    }
                    RowRef::Open { effects, tail, .. } => {
                        let tail = &self.function.row_vars[*tail].name;
                        if effects.is_empty() {
                            format!("<{tail}> ")
                        } else {
                            format!("<{} | {tail}> ", effect_names(effects).join(", "))
                        }
                    }
                    RowRef::Error => "<error> ".to_string(),
                };
```

- [ ] **Step 6: 型検査に仮の門を足す**

`crates/eml_types/src/check.rs` の `lower_type` の `match` に足す。

```rust
        // 多相は Task 7 で入れる。それまでは `check_module` の先頭で E0004 を報告し、ここでは `Error` にする
        TypeRefKind::Var(_) => table.error,
```

`RowRef::Error` の腕を `RowRef::Error | RowRef::Open { .. } =>` にする。`has_error` の `match` に `TypeRefKind::Var(_) => true,` を足し、`Fn` の腕の `matches!(row, RowRef::Error)` を `matches!(row, RowRef::Error | RowRef::Open { .. })` にする。

`check_module` の先頭 (`let mut signatures` の前) に足す。

```rust
    // 型変数と row 変数は Task 7 で入れる。それまでは段階1と同じ E0004 を出す
    for (_, function) in module.functions.iter() {
        for (_, ty) in function.types.iter() {
            match &ty.kind {
                TypeRefKind::Var(_) => diagnostics.push(not_yet_supported(
                    module.file,
                    ty.range,
                    "type variables are not supported yet",
                )),
                TypeRefKind::Fn {
                    row: RowRef::Open { range, .. },
                    ..
                } => diagnostics.push(not_yet_supported(
                    module.file,
                    *range,
                    "row variables are not supported yet",
                )),
                _ => {}
            }
        }
    }
```

- [ ] **Step 7: テストが通ることを確かめる**

Run: `cargo test -p eml_hir && cargo test -p eml_types && cargo test -p eml_cli`
Expected: PASS

- [ ] **Step 8: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`

```bash
git add crates
git commit -m "Lower signature type and row variables and resolve them from body annotations"
```

---

### Task 6: `>>` / `<<` を組み込みの多相関数にする

`>>` と `<<` を組み込み `ComposeFwd` / `ComposeBwd` の呼び出しにし、型を与える。関数を値として渡すことはまだ E0004 なので (Task 8 で外す)、このタスクで確かめるのは HIR の変換と型だけである。

**Files:**
- Modify: `crates/eml_hir/src/builtin.rs`、`crates/eml_hir/src/lower/ops.rs`
- Modify: `crates/eml_types/src/builtins.rs`
- Modify: `crates/eml_core_ir/src/lower.rs` (`prim` の腕)
- Test: `crates/eml_hir/tests/operators.rs`

**Interfaces:**
- Consumes: Task 2 の `Table::fresh_mult`、`function_with`、`closure_kinds`
- Produces: `eml_hir::builtin::Builtin::{ComposeFwd, ComposeBwd}` (名前は `>>` と `<<`)。型は `(a -> <e> b) -> (b -> <e> c) -> a -> <e> c` と `(b -> <e> c) -> (a -> <e> b) -> a -> <e> c`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_hir/tests/operators.rs` に足す。

```rust
#[test]
fn composition_operators_are_builtin_calls() {
    insta::assert_snapshot!(lower_text("h : Bool -> Bool\nh = not >> not << not"), @r"
    h : Bool -> Bool
    h = (>> not (<< not not))
    ");
}
```

既存のテスト `mixed_associativity_is_rejected_in_both_orders` の期待値を次に変える (Task 0 で記録した変更)。

```rust
    insta::assert_snapshot!(lower_text(text), @r"
    x : Int
    x = <missing>
    y : Int
    y = (>> 1 <missing>)
    ---
    E1001 2:7 cannot find operator `<+>`
    E1006 2:13 `<+>` and `>>` cannot be combined without parentheses
    E1006 4:12 `>>` and `<+>` cannot be combined without parentheses
    ");
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_hir --test operators`
Expected: FAIL (`function composition is not supported yet`)

- [ ] **Step 3: 実装する**

`crates/eml_hir/src/builtin.rs` の `Builtin` に `ComposeFwd,` と `ComposeBwd,` を足す。`binary_operator` に `">>" => Builtin::ComposeFwd,` と `"<<" => Builtin::ComposeBwd,` を足し、`name` に `Builtin::ComposeFwd => ">>",` と `Builtin::ComposeBwd => "<<",` を足す。

`crates/eml_hir/src/lower/ops.rs` の `binary` から、`None if op == ">>" || op == "<<" => ...` の腕を消す。

`crates/eml_types/src/builtins.rs` の `builtin_type` に足す。`use crate::table::Row` はそのまま使う。

```rust
        Builtin::ComposeFwd | Builtin::ComposeBwd => compose(table, builtin == Builtin::ComposeFwd),
```

ファイルの最後に足す。

```rust
/// `f >> g` と `g << f` は、どちらも `fn x -> g (f x)` と同じ関数になる。被演算子は左から右に評価するので、
/// 引数の並びだけが違う2つの組み込みにする (docs/spec/declarations.md の標準の演算子の表)。
fn compose(table: &mut Table, forward: bool) -> Ty {
    let (a, b, c) = (table.fresh_var(), table.fresh_var(), table.fresh_var());
    let e = table.fresh_row_var();
    let mut effectful = |table: &mut Table, from: Ty, to: Ty| {
        let m = table.fresh_mult();
        table.function_with(
            from,
            m,
            Row {
                labels: Vec::new(),
                tail: Some(e),
            },
            to,
        )
    };
    let f = effectful(table, a, b);
    let g = effectful(table, b, c);
    let result = effectful(table, a, c);
    let (first, second) = if forward { (f, g) } else { (g, f) };
    let m = table.fresh_mult();
    let inner = table.function_with(second, m, Row::pure(), result);
    let outer = table.function(first, Row::pure(), inner);
    table.closure_kinds(outer, 3, &[]);
    outer
}
```

`effectful` が `mut` を要らなければ `let effectful = ...` にする (clippy に従う)。

`crates/eml_core_ir/src/lower.rs` の `prim` の最後の腕を次にする。

```rust
        Builtin::Println | Builtin::True | Builtin::False => {
            unreachable!("`println` is performed and constructors are values")
        }
        Builtin::ComposeFwd | Builtin::ComposeBwd => {
            unreachable!("composition is lowered to a closure, not to a primitive")
        }
```

- [ ] **Step 4: テストが通ることを確かめる**

Run: `cargo test -p eml_hir && cargo test -p eml_types`
Expected: PASS

- [ ] **Step 5: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`

```bash
git add crates
git commit -m "Make function composition a polymorphic builtin"
```

---

### Task 7: シグネチャをスキームにし、型変数を rigid にして参照ごとに具体化する

シグネチャの型変数と row 変数を rigid 変数にし、関数を参照するたびにスキームを具体化する。本体の注釈の型変数は、シグネチャの同じ rigid 変数を指す。Task 5 の仮の門を外す。関数値はまだ E0004 のままにする (Task 8)。

**Files:**
- Create: `crates/eml_types/src/scheme.rs`
- Modify: `crates/eml_types/src/lib.rs` (`mod scheme;`)
- Modify: `crates/eml_types/src/check.rs`
- Test: `crates/eml_types/tests/check.rs`

**Interfaces:**
- Consumes: Task 2 の `fresh_rigid`、`fresh_rigid_row`、`copy_type`、`Subst`、`fresh_var_with`、`fresh_row_var_with`、`kind_vars`、Task 3 の `lin_residual`、`copy_lin_constraints` など、Task 5 の `Function::type_vars` / `row_vars`
- Produces (`pub(crate)`):
  - `scheme::Rigids::new(table: &mut Table, function: &Function) -> Rigids`
  - `scheme::Scheme { pub ty: Ty, .. }`、`Scheme::new(ty: Ty, rigids: &Rigids) -> Scheme`、`Scheme::instantiate(&self, table: &mut Table) -> Ty`、`Scheme::generalize(&mut self, table: &Table)`、`Scheme::lin_constraints(&self) -> &[(Bound<Linearity>, Bound<Linearity>)]`
  - `scheme::lower_type(table: &mut Table, function: &Function, rigids: &Rigids, id: TypeRefId, outermost_unr: bool) -> Ty`
  - `BodyCheck::reference(&mut self, function: FunctionId) -> Ty`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/check.rs` に足す。

```rust
#[test]
fn polymorphic_functions_are_instantiated_at_each_use() {
    let text = "id : a -> a\nid x = x\n\nuse_both : Unit -> String\nuse_both () =\n  let n = id 1\n  let s = id \"s\"\n  s";
    insta::assert_snapshot!(check_text(text), @r"
    id : a -> a
      x#0 : a
    use_both : Unit -> String
      n#0 : Int
      s#1 : String
    ");
}

#[test]
fn rigid_type_variables_do_not_unify_with_other_types() {
    let text = "f : a -> b\nf x = x\n\ng : a -> Int\ng x = x";
    insta::assert_snapshot!(check_text(text), @r"
    f : a -> b
      x#0 : a
    g : a -> Int
      x#0 : a
    ---
    E2001 2:7 mismatched types
      2:7 expected `b`, found `a`
      1:5 expected because of the signature of `f`
    E2001 5:7 mismatched types
      5:7 expected `Int`, found `a`
      4:5 expected because of the signature of `g`
    ");
}

#[test]
fn annotations_in_the_body_refer_to_the_signature() {
    let text = "f : a -> a\nf x =\n  let y : a = x\n  y\n\ng : a -> Int\ng x = (x : Int)";
    insta::assert_snapshot!(check_text(text), @r"
    f : a -> a
      x#0 : a
      y#1 : a
    g : a -> Int
      x#0 : a
    ---
    E2001 6:8 mismatched types
      6:8 expected `Int`, found `a`
      6:12 expected because of this annotation
    ");
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --test check`
Expected: FAIL (`type variables are not supported yet`)

- [ ] **Step 3: `scheme.rs` を作る**

`crates/eml_types/src/scheme.rs` を作る。`crates/eml_types/src/lib.rs` に `mod scheme;` を足す。

```rust
//! シグネチャのスキームと、その具体化 (docs/spec/types.md の「推論」)。

use eml_hir::builtin::BuiltinType;
use eml_hir::{EffectRef, Function, RowRef, RowVarId, TypeRefId, TypeRefKind, TypeVarId};
use la_arena::ArenaMap;

use crate::kind::{Bound, KindVar};
use crate::table::{Mult, RigidVar, Row, RowVar, Subst, Table, Ty};
use crate::ty::{Effect, Linearity, Multiplicity};

/// 関数ごとの、シグネチャの型変数と row 変数。本体の注釈も同じ変数を指す (docs/spec/types.md の「推論」)。
pub(crate) struct Rigids {
    tys: ArenaMap<TypeVarId, Ty>,
    rows: ArenaMap<RowVarId, RowVar>,
    vars: Vec<RigidVar>,
}

impl Rigids {
    pub fn new(table: &mut Table, function: &Function) -> Rigids {
        let mut rigids = Rigids {
            tys: ArenaMap::default(),
            rows: ArenaMap::default(),
            vars: Vec::new(),
        };
        for (id, var) in function.type_vars.iter() {
            let (ty, rigid) = table.fresh_rigid(&var.name);
            rigids.tys.insert(id, ty);
            rigids.vars.push(rigid);
        }
        for (id, var) in function.row_vars.iter() {
            rigids.rows.insert(id, table.fresh_rigid_row(&var.name));
        }
        rigids
    }
}

pub(crate) struct Scheme {
    pub ty: Ty,
    rigids: Vec<RigidVar>,
    rigid_rows: Vec<RowVar>,
    lin_vars: Vec<KindVar>,
    lin_constraints: Vec<(Bound<Linearity>, Bound<Linearity>)>,
    mult_vars: Vec<KindVar>,
    mult_constraints: Vec<(Bound<Multiplicity>, Bound<Multiplicity>)>,
}

impl Scheme {
    pub fn new(ty: Ty, rigids: &Rigids) -> Scheme {
        Scheme {
            ty,
            rigids: rigids.vars.clone(),
            rigid_rows: rigids.rows.values().copied().collect(),
            lin_vars: Vec::new(),
            lin_constraints: Vec::new(),
            mult_vars: Vec::new(),
            mult_constraints: Vec::new(),
        }
    }

    /// rigid 変数を新しい推論用の変数に変え、多相化した Kind 変数も新しい変数にして制約を複製する。
    /// SCC の検査を終えるまでは Kind 変数を多相化していないので、同じ SCC の中の参照は Kind 変数を共有する
    /// (docs/spec/types.md の「推論」)。
    pub fn instantiate(&self, table: &mut Table) -> Ty {
        let mut subst = Subst::default();
        for &var in &self.lin_vars {
            let fresh = table.fresh_lin_kind();
            subst.lin.insert(var, fresh);
        }
        for &var in &self.mult_vars {
            let fresh = table.fresh_mult_kind();
            subst.mult.insert(var, fresh);
        }
        table.copy_lin_constraints(&self.lin_constraints, &subst.lin);
        table.copy_mult_constraints(&self.mult_constraints, &subst.mult);
        for &rigid in &self.rigids {
            let mu = table.rigid_linearity(rigid);
            let mu = subst.lin.get(&mu).copied().unwrap_or(mu);
            let fresh = table.fresh_var_with(mu);
            subst.tys.insert(rigid, fresh);
        }
        for &row in &self.rigid_rows {
            let sigma = table.row_multiplicity_var(row);
            let sigma = subst.mult.get(&sigma).copied().unwrap_or(sigma);
            let fresh = table.fresh_row_var_with(sigma);
            subst.rows.insert(row, fresh);
        }
        table.copy_type(self.ty, &subst)
    }

    /// SCC の検査の後に、スキームに現れる Kind 変数を多相化し、それらに関わる制約を残す (docs/spec/types.md の「推論」)。
    pub fn generalize(&mut self, table: &Table) {
        let (lin, mult) = table.kind_vars(self.ty);
        self.lin_constraints = table.lin_residual(&lin);
        self.mult_constraints = table.mult_residual(&mult);
        self.lin_vars = lin;
        self.mult_vars = mult;
    }

    pub fn lin_constraints(&self) -> &[(Bound<Linearity>, Bound<Linearity>)] {
        &self.lin_constraints
    }
}

/// HIR の型を型の表に変換する。`outermost_unr` が真なら、一番外側の矢印はトップレベルの関数そのもので、何度でも
/// 呼べるので `Unr` である。ほかの矢印の線形性は Kind 変数にして推論する (docs/spec/types.md の「関数型」)。
pub(crate) fn lower_type(
    table: &mut Table,
    function: &Function,
    rigids: &Rigids,
    id: TypeRefId,
    outermost_unr: bool,
) -> Ty {
    match &function.types[id].kind {
        TypeRefKind::Error => table.error,
        TypeRefKind::Builtin(BuiltinType::Int) => table.int,
        TypeRefKind::Builtin(BuiltinType::String) => table.string,
        TypeRefKind::Builtin(BuiltinType::Bool) => table.bool,
        TypeRefKind::Builtin(BuiltinType::Unit) => table.unit,
        TypeRefKind::Var(var) => rigids.tys[*var],
        TypeRefKind::Fn { param, row, ret } => {
            let param = lower_type(table, function, rigids, *param, false);
            let ret = lower_type(table, function, rigids, *ret, false);
            let labels = |effects: &[EffectRef]| -> Vec<Effect> {
                effects.iter().map(|EffectRef::Io| Effect::Io).collect()
            };
            let row = match row {
                // 省略した row は空の row である (docs/spec/types.md の「関数型」)
                RowRef::Omitted => Row::pure(),
                RowRef::Closed { effects, .. } => Row::closed(labels(effects)),
                RowRef::Open { effects, tail, .. } => Row {
                    labels: labels(effects),
                    tail: Some(rigids.rows[*tail]),
                },
                // 未定義のエフェクトの跡。どのエフェクトも受け入れて、診断を連鎖させない
                RowRef::Error => Row {
                    labels: Vec::new(),
                    tail: Some(table.fresh_row_var()),
                },
            };
            let lin = if outermost_unr {
                Mult::Known(Linearity::Unr)
            } else {
                table.fresh_mult()
            };
            table.function_with(param, lin, row, ret)
        }
    }
}
```

- [ ] **Step 4: 型検査をスキームで動かす**

`crates/eml_types/src/check.rs` を次のように変える。

- Task 5 で足した `check_module` の先頭の E0004 のループを消す。
- ファイルの中の `fn lower_type` を消し、`use crate::scheme::{Rigids, Scheme, lower_type};` を足す。
- `has_error` の `TypeRefKind::Var(_) => true` を `TypeRefKind::Var(_) => false` にし、`Fn` の腕を `matches!(row, RowRef::Error) || ...` に戻す (row 変数は誤りではない)。
- `check_module` のシグネチャの変換と本体の検査を次にする。

```rust
pub(crate) fn check_module(module: &Module) -> (TypedModule, Vec<Diagnostic>) {
    let mut table = Table::new();
    let mut diagnostics = Vec::new();
    let mut rigids = ArenaMap::default();
    let mut schemes: ArenaMap<FunctionId, Scheme> = ArenaMap::default();
    for (id, function) in module.functions.iter() {
        let function_rigids = Rigids::new(&mut table, function);
        if let Some(signature) = &function.signature {
            let ty = lower_type(&mut table, function, &function_rigids, signature.ty, true);
            // 部分適用のクロージャは、それまでの引数を捕まえる (docs/spec/types.md の「関数型」)
            if let Some(body) = &function.body {
                table.closure_kinds(ty, body.params.len(), &[]);
            }
            schemes.insert(id, Scheme::new(ty, &function_rigids));
        }
        rigids.insert(id, function_rigids);
    }
    let main = module
        .functions
        .iter()
        .find(|(_, function)| function.name == "main")
        .map(|(id, _)| id);
    if let Some(id) = main {
        check_main(module, &table, &schemes, id, &mut diagnostics);
    }
    let mut bodies = Vec::new();
    for (id, function) in module.functions.iter() {
        let (Some(scheme), Some(body)) = (schemes.get(id), &function.body) else {
            continue;
        };
        let signature = scheme.ty;
        let mut checker = BodyCheck {
            module,
            function,
            body,
            rigids: &rigids[id],
            schemes: &schemes,
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
    for (id, scheme) in schemes.iter() {
        typed.signatures.insert(id, table.export(scheme.ty));
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
```

- `check_main` の引数 `signatures: &ArenaMap<FunctionId, Ty>` を `schemes: &ArenaMap<FunctionId, Scheme>` にし、`let (Some(&ty), Some(signature)) = (signatures.get(id), ...)` を `let (Some(scheme), Some(signature)) = (schemes.get(id), ...)` にして、`table.export(scheme.ty)` を使う。
- `BodyCheck` のフィールド `signatures: &'a ArenaMap<FunctionId, Ty>` を `schemes: &'a ArenaMap<FunctionId, Scheme>` に変え、`rigids: &'a Rigids` を足す。
- `impl BodyCheck` に足す。

```rust
    /// トップレベルの関数を参照するたびに、スキームを具体化する (docs/spec/types.md の「推論」)。
    fn reference(&mut self, function: FunctionId) -> Ty {
        let schemes = self.schemes;
        match schemes.get(function) {
            Some(scheme) => scheme.instantiate(self.table),
            None => self.table.error,
        }
    }
```

- `value` と `call` の中の `self.signatures.get(function).copied().unwrap_or(self.table.error)` (2か所) を `self.reference(function)` (`call` では `self.reference(*function)`) にする。
- `infer_expr` の `ExprKind::Annot` と `stmts` の `Stmt::Let` の `lower_type(self.table, self.function, *ty)` を `lower_type(self.table, self.function, self.rigids, *ty, false)` にする。

- [ ] **Step 5: テストが通ることを確かめる**

Run: `cargo test -p eml_types && cargo test -p eml_cli`
Expected: PASS (既存のスナップショットは変わらない)

- [ ] **Step 6: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`

```bash
git add crates/eml_types
git commit -m "Check bodies against signature schemes and instantiate them at each reference"
```

---

### Task 8: 関数値、部分適用、引数の多い呼び出し、row の包含を型検査に入れる

段階1の関数値の E0004 を外す。関数と組み込みの参照は、具体化した後に戻り値の側の row を開く。呼び出しは任意の式を受け、たどった矢印の row を今の row に含める。引数が矢印より少ない等式を受ける。

**Files:**
- Modify: `crates/eml_types/src/check.rs`
- Test: `crates/eml_types/tests/check.rs`
- Delete: `tests/ui/check-fail/function_value_parameter.em`、`crates/eml_cli/tests/snapshots/ui__check_fail@function_value_parameter.em.snap`
- Modify: `crates/eml_cli/tests/snapshots/ui__check_fail@not_yet_supported.em.snap`

**Interfaces:**
- Consumes: Task 2 の `include_row`、`open_spine`、`fresh_mult`、`function_with`、`UnifyError::MissingRowVar`
- Produces: `BodyCheck::fresh_function(&mut self) -> Ty`。`BodyCheck::value(&mut self, res: Res) -> Ty` (引数の `range` を外す)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/check.rs` に足す。

```rust
#[test]
fn row_variables_pass_effects_through() {
    let text = "apply : (a -> <e> b) -> a -> <e> b\napply f x = f x\n\nshout : String -> <IO> Unit\nshout s = apply println s\n\nquiet : String -> Unit\nquiet s = apply println s";
    insta::assert_snapshot!(check_text(text), @r"
    apply : (a -> <e> b) -> a -> <e> b
      f#0 : a -> <e> b
      x#1 : a
    shout : String -> <IO> Unit
      s#0 : String
    quiet : String -> Unit
      s#0 : String
    ---
    E2002 8:11 `apply` performs `IO`, which the signature of `quiet` does not allow
      8:11 this call performs `IO`
      7:9 the row of this signature does not include it
      help: add `IO` to the row of the signature of `quiet`, as in `-> <IO> ...`
    ");
}

#[test]
fn a_rigid_row_variable_must_be_in_the_ambient_row() {
    let text = "run : (Unit -> <e> Unit) -> Unit\nrun f = f ()";
    insta::assert_snapshot!(check_text(text), @r"
    run : (Unit -> <e> Unit) -> Unit
      f#0 : Unit -> <e> Unit
    ---
    E2002 2:9 `f` performs `e`, which the signature of `run` does not allow
      2:9 this call performs `e`
      1:7 the row of this signature does not include it
      help: add `e` to the row of the signature of `run`, as in `-> <e> ...`
    ");
}

#[test]
fn equations_may_return_functions_and_calls_may_pass_more_arguments() {
    let text = "add : Int -> Int -> Int\nadd a b = a + b\n\nadder : Int -> Int -> Int\nadder x = add x\n\ninc : Int -> Int\ninc = adder 1\n\nthree : Unit -> Int\nthree () = adder 1 2 + inc 1";
    insta::assert_snapshot!(check_text(text), @r"
    add : Int -> Int -> Int
      a#0 : Int
      b#1 : Int
    adder : Int -> Int -> Int
      x#0 : Int
    inc : Int -> Int
    three : Unit -> Int
    ");
}
```

既存のテストを次のように変える (Task 0 で記録した変更)。

`function_values_are_not_yet_supported` を `function_values_and_partial_application` に改名し、入力はそのままで、期待値を次にする。

```rust
    insta::assert_snapshot!(check_text(text), @r"
    add : Int -> Int -> Int
      a#0 : Int
      b#1 : Int
    partial : Unit -> Int
      f#0 : Int -> <_> Int
      g#1 : Int -> <_> Int -> <_> Int
    curried : Int -> Int -> Int
      a#0 : Int
    ---
    E2001 11:13 mismatched types
      11:13 expected `Int -> Int`, found `Int`
      10:11 expected because of the signature of `curried`
    ");
```

`function_typed_parameters_cannot_be_called_or_passed` を `function_typed_parameters_can_be_called_and_passed` に改名し、入力はそのままで、期待値を次にする。

```rust
    insta::assert_snapshot!(check_text(text), @"
    apply : (Int -> Int) -> Int -> Int
      f#0 : Int -> Int
      x#1 : Int
    pass : (Int -> Int) -> Int
      f#0 : Int -> Int
    ");
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --test check`
Expected: FAIL (E0004 の関数値の診断が出る)

- [ ] **Step 3: 実装する**

`crates/eml_types/src/check.rs` で次を変える。

`value` を次にする。`infer_expr` の `ExprKind::Path(res) => self.value(*res, expr.range)` は `self.value(*res)` にする。

```rust
    /// 関数と組み込みの参照は、具体化した後に戻り値の側の閉じた row を開く。純粋な関数を、エフェクトを持つ関数型の
    /// 引数に渡せるようにするため (docs/spec/types.md の「推論」)。局所変数の型は開かない。
    fn value(&mut self, res: Res) -> Ty {
        match res {
            Res::Local(local) => self.locals.get(local).copied().unwrap_or(self.table.error),
            Res::Function(function) => {
                let ty = self.reference(function);
                self.table.open_spine(ty)
            }
            Res::Builtin(builtin) => {
                let ty = builtin_type(self.table, builtin);
                self.table.open_spine(ty)
            }
        }
    }

    /// 呼ばれる値や期待する型がまだ推論用の変数のとき、それを関数型に決める。
    fn fresh_function(&mut self) -> Ty {
        let param = self.table.fresh_var();
        let ret = self.table.fresh_var();
        let lin = self.table.fresh_mult();
        let row = Row {
            labels: Vec::new(),
            tail: Some(self.table.fresh_row_var()),
        };
        self.table.function_with(param, lin, row, ret)
    }
```

`call` を次にする。

```rust
    /// 等式と同じく、引数を1つ受けるごとに型の矢印を1つたどる。たどった矢印の row はすべて今の row に含まれなければ
    /// ならない。矢印が余れば部分適用で、残りの関数型が値の型になる (docs/spec/expressions.md の「ラムダ」)。
    fn call(&mut self, id: ExprId, callee: ExprId, args: &[ExprId]) -> Ty {
        let body = self.body;
        let callee_expr = &body.exprs[callee];
        let name = match &callee_expr.kind {
            ExprKind::Path(Res::Function(function)) => {
                self.module.functions[*function].name.clone()
            }
            ExprKind::Path(Res::Builtin(builtin)) => builtin.name().to_string(),
            ExprKind::Path(Res::Local(local)) => body.locals[*local].name.clone(),
            _ => "this expression".to_string(),
        };
        let mut ty = self.infer_expr(callee);
        // 1回の呼び出しの E2002 は、どの引数の矢印で起きても1つだけ報告する
        let mut reported = false;
        for (index, &arg) in args.iter().enumerate() {
            if let TyKind::Var(_) = self.table.kind(ty) {
                let function = self.fresh_function();
                // 新しい変数だけでできた関数型なので、単一化は失敗しない
                let _ = self.table.unify(ty, function);
            }
            match self.table.kind(ty).clone() {
                TyKind::Fn {
                    param, row, ret, ..
                } => {
                    let origin = Origin::Argument {
                        callee: callee_expr.range,
                        name: name.clone(),
                        index,
                    };
                    self.check_expr(arg, param, origin);
                    let ok = self.perform(row, body.exprs[id].range, &name, !reported);
                    reported |= !ok;
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
        ty
    }
```

`perform` の先頭から `let names` の前までを次にし、以降の `names.join(", ")` は `missing.join(", ")` にする。

```rust
    /// 呼び出し先の row が今の row に含まれることを確かめる (docs/spec/types.md の「推論」)。含まれなければ
    /// `false` を返す。`report` が偽なら診断を出さない。
    fn perform(&mut self, row: Row, range: TextRange, name: &str, report: bool) -> bool {
        let ambient = self.ambient.clone();
        let missing: Vec<String> = match self.table.include_row(&row, &ambient) {
            Ok(()) => return true,
            Err(UnifyError::MissingEffects(effects)) => {
                effects.iter().map(|e| e.name().to_string()).collect()
            }
            Err(UnifyError::MissingRowVar(var)) => vec![var],
            // 包含の検査は、今の row に新しい row 変数を合わせるだけなので、ほかの失敗は起きない
            Err(other) => unreachable!("including a row reports only missing effects: {other:?}"),
        };
        if !report {
            return false;
        }
        let quoted: Vec<String> = missing.iter().map(|name| format!("`{name}`")).collect();
        let quoted = quoted.join(", ");
```

`check_function` の最後の `if matches!(self.table.kind(expected), TyKind::Fn { .. }) { ... }` (E0004 を出すブロック) を消す。

使われなくなった `not_yet_supported` の `use` を消す (`Lambda` の仮の門がまだ使うなら残す)。

- [ ] **Step 4: UI テストを合わせる**

`tests/ui/check-fail/function_value_parameter.em` と `crates/eml_cli/tests/snapshots/ui__check_fail@function_value_parameter.em.snap` を削除する (Task 0 で記録した変更。同じ内容を Task 12 の `run/higher_order.em` で実行する)。

```bash
git rm tests/ui/check-fail/function_value_parameter.em crates/eml_cli/tests/snapshots/ui__check_fail@function_value_parameter.em.snap
```

Run: `cargo test -p eml_cli --test ui`
Expected: `check_fail` が FAIL し、`not_yet_supported.em` のスナップショットから `using a function as a value is not supported yet` と `partial application is not supported yet` の2つのブロックが消えた差分が出る。`cargo insta review` でその差分だけであることを確かめて承認する。承認後のスナップショットは `data` と `lambdas` の2つのブロックだけを持つ。

- [ ] **Step 5: テストが通ることを確かめる**

Run: `cargo test -p eml_types && cargo test -p eml_cli`
Expected: PASS

- [ ] **Step 6: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`

```bash
git add -A crates tests
git commit -m "Type function values, partial application, and calls through any expression"
```

---

### Task 9: ラムダを型検査する

ラムダを check モードでは期待する関数型に対して検査し、infer モードでは新しい関数型を作って検査する。ラムダの本体のエフェクトは、ラムダの最後の矢印の row に入る。Task 4 の仮の門を外す。

**Files:**
- Modify: `crates/eml_types/src/check.rs`
- Test: `crates/eml_types/tests/check.rs`
- Modify: `crates/eml_cli/tests/snapshots/ui__check_fail@not_yet_supported.em.snap`

**Interfaces:**
- Consumes: Task 8 の `fresh_function`
- Produces: `Origin::{LambdaParameter, LambdaBody, Inferred}`、`AmbientSource::{Signature, Lambda(Origin)}`、`BodyCheck::check_lambda`、`BodyCheck::bind_param`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/check.rs` に足す。

```rust
#[test]
fn lambdas_are_checked_against_the_expected_type_or_inferred() {
    let text = "apply : (Int -> Int) -> Int\napply f = f 1\n\nmain : Unit -> <IO> Unit\nmain () =\n  let n = apply (fn x -> x + 1)\n  let id = fn y -> y\n  let s = id \"s\"\n  let k = fn (z : Int) _ -> z\n  println (show_int (k n s))";
    insta::assert_snapshot!(check_text(text), @r"
    apply : (Int -> Int) -> Int
      f#0 : Int -> Int
    main : Unit -> <IO> Unit
      x#0 : Int
      n#1 : Int
      y#2 : String
      id#3 : String -> <IO> String
      s#4 : String
      z#5 : Int
      k#6 : Int -> <IO> String -> <IO> Int
    ");
}

#[test]
fn a_lambda_cannot_perform_effects_its_expected_type_does_not_allow() {
    let text = "each : (String -> Unit) -> Unit\neach f = f \"a\"\n\nmain : Unit -> <IO> Unit\nmain () = each (fn s -> println s)";
    insta::assert_snapshot!(check_text(text), @r"
    each : (String -> Unit) -> Unit
      f#0 : String -> Unit
    main : Unit -> <IO> Unit
      s#0 : String
    ---
    E2002 5:25 `println` performs `IO`, which this lambda does not allow
      5:25 this call performs `IO`
      5:11 argument 1 of `each` does not allow it
    ");
}

#[test]
fn annotated_lambda_parameters_must_match_and_arities_must_agree() {
    let text = "apply : (Int -> Int) -> Int\napply f = f 1\n\nbad : Unit -> Int\nbad () = apply (fn (x : String) -> 1) + apply (fn a b -> a)";
    insta::assert_snapshot!(check_text(text), @r"
    apply : (Int -> Int) -> Int
      f#0 : Int -> Int
    bad : Unit -> Int
      x#0 : String
      a#1 : Int
      b#2 : {error}
    ---
    E2001 5:20 mismatched types
      5:20 expected `Int`, found `String`
      note: an annotated lambda parameter must have the parameter type the lambda is expected to have
    E2001 5:53 this lambda has 2 parameters but its expected type `Int -> Int` has 1 arrow
      5:53 this parameter has no arrow in the expected type
    ");
}
```

位置の確かめ方: 5行目は `bad () = apply (fn (x : String) -> 1) + apply (fn a b -> a)` である。`(x : String)` の `(` は 20 列目、2つ目のラムダの `b` は 53 列目にある。数え違いがあれば、プランの期待値の誤りとして直す (Global Constraints)。

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --test check`
Expected: FAIL (`lambdas are not supported yet`)

- [ ] **Step 3: 実装する**

`crates/eml_types/src/check.rs` で次を変える。

`Origin` に足す。

```rust
    LambdaParameter,
    LambdaBody,
    /// 推論で決まる型。根拠の場所はない。
    Inferred,
```

`AmbientSource` を足し、`BodyCheck` にフィールド `ambient_source: AmbientSource` を足す。`check_module` の `BodyCheck` の初期化に `ambient_source: AmbientSource::Signature,` を足す。

```rust
/// 今の row がどこから来たか。E2002 の言い方を決める (docs/spec/diagnostics.md)。
#[derive(Debug, Clone)]
enum AmbientSource {
    Signature,
    /// ラムダの最後の矢印の row。ラムダの期待する型の由来を持つ。
    Lambda(Origin),
}
```

`check_expr` の `match` に、`_` の腕の前に足す。

```rust
            ExprKind::Lambda {
                params,
                body: lambda_body,
            } => {
                if matches!(self.table.kind(expected), TyKind::Fn { .. } | TyKind::Error) {
                    self.check_lambda(id, params, *lambda_body, expected, origin);
                } else {
                    let found = self.infer_expr(id);
                    self.expect(expr.range, expected, found, &origin);
                }
            }
```

`infer_expr` の、Task 4 で足した `ExprKind::Lambda { .. }` の仮の腕を次にする。

```rust
            ExprKind::Lambda {
                params,
                body: lambda_body,
            } => {
                let ty = self.table.fresh_var();
                self.check_lambda(id, params, *lambda_body, ty, Origin::Inferred);
                ty
            }
```

`impl BodyCheck` に足す。

```rust
    /// ラムダを、期待する関数型の矢印を引数ごとにたどって検査する。本体のエフェクトは、外側の関数ではなく、最後に
    /// たどった矢印の row に入る (docs/spec/types.md の「推論」)。
    fn check_lambda(
        &mut self,
        id: ExprId,
        params: &[PatId],
        lambda_body: ExprId,
        expected: Ty,
        origin: Origin,
    ) {
        let body = self.body;
        let saved = (self.ambient.clone(), self.ambient_source.clone());
        let mut current = expected;
        let mut row = None;
        for (index, &pat) in params.iter().enumerate() {
            if let TyKind::Var(_) = self.table.kind(current) {
                let function = self.fresh_function();
                // 新しい変数だけでできた関数型なので、単一化は失敗しない
                let _ = self.table.unify(current, function);
            }
            match self.table.kind(current).clone() {
                TyKind::Fn {
                    param,
                    row: arrow,
                    ret,
                    ..
                } => {
                    self.bind_param(pat, param);
                    row = Some(arrow);
                    current = ret;
                }
                TyKind::Error => {
                    let error = self.table.error;
                    self.bind_pat(pat, error);
                }
                _ => {
                    let found = self.table.export(expected);
                    self.diagnostics.push(Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        format!(
                            "this lambda has {} but its expected type `{found}` has {}",
                            count(params.len(), "parameter"),
                            count(index, "arrow"),
                        ),
                        Label::new(
                            self.file(),
                            body.pats[pat].range,
                            "this parameter has no arrow in the expected type",
                        ),
                    ));
                    let error = self.table.error;
                    for &rest in &params[index..] {
                        self.bind_pat(rest, error);
                    }
                    current = error;
                    break;
                }
            }
        }
        self.ambient = match row {
            Some(row) => row,
            // 期待する型が壊れていれば、どのエフェクトも受け入れて診断を連鎖させない
            None => Row {
                labels: Vec::new(),
                tail: Some(self.table.fresh_row_var()),
            },
        };
        self.ambient_source = AmbientSource::Lambda(origin);
        self.check_expr(lambda_body, current, Origin::LambdaBody);
        (self.ambient, self.ambient_source) = saved;
        self.exprs.insert(id, expected);
    }

    /// ラムダの引数を束縛する。型を明示した引数は、明示した型が、ラムダが期待される引数の型と一致しなければならない。
    fn bind_param(&mut self, pat: PatId, ty: Ty) {
        let body = self.body;
        if let PatKind::Annot {
            pat: inner,
            ty: annotation,
        } = &body.pats[pat].kind
        {
            let annotated = lower_type(self.table, self.function, self.rigids, *annotation, false);
            self.expect(body.pats[pat].range, ty, annotated, &Origin::LambdaParameter);
            self.bind_pat(*inner, annotated);
            return;
        }
        self.bind_pat(pat, ty);
    }
```

`mismatch` の `match origin` に足す。

```rust
            Origin::LambdaParameter => diagnostic.with_note(
                "an annotated lambda parameter must have the parameter type the lambda is expected to have",
            ),
            Origin::LambdaBody => diagnostic.with_note(
                "the body of a lambda must have the return type the lambda is expected to have",
            ),
            Origin::Inferred => diagnostic,
```

`perform` の診断を作る部分 (`self.diagnostics.push(...)` の全体) を、`ambient_source` で分ける。`Signature` の場合は今のまま、`Lambda` の場合は次にする。

```rust
        let file = self.file();
        let diagnostic = match &self.ambient_source {
            AmbientSource::Signature => {
                // (今の help と診断の組み立てをここに移す)
            }
            AmbientSource::Lambda(origin) => {
                let diagnostic = Diagnostic::error(
                    codes::EFFECT_NOT_IN_ROW,
                    format!("`{name}` performs {quoted}, which this lambda does not allow"),
                    Label::new(file, range, format!("this call performs {quoted}")),
                );
                // ラムダの row を決めた場所を secondary にする (docs/spec/diagnostics.md の E2002)
                match origin {
                    Origin::Argument {
                        callee,
                        name: callee_name,
                        index,
                    } => diagnostic.with_secondary(Label::new(
                        file,
                        *callee,
                        format!("argument {} of `{callee_name}` does not allow it", index + 1),
                    )),
                    Origin::Annotation(annotation) => diagnostic.with_secondary(Label::new(
                        file,
                        *annotation,
                        "this annotation does not allow it",
                    )),
                    Origin::Return => diagnostic.with_secondary(Label::new(
                        file,
                        self.signature_range(),
                        format!("the signature of `{}` does not allow it", self.function.name),
                    )),
                    _ => diagnostic,
                }
            }
        };
        self.diagnostics.push(diagnostic);
        false
```

`Signature` の腕には、今の `help` の計算と `Diagnostic::error(...).with_secondary(...).with_help(help)` をそのまま入れ、その式を腕の値にする。

使われなくなった `not_yet_supported` と `TextRange::at` の `use` を消す。

- [ ] **Step 4: UI テストを合わせる**

Run: `cargo test -p eml_cli --test ui`
Expected: `check_fail` が FAIL し、`not_yet_supported.em` のスナップショットから `lambdas are not supported yet` のブロックが消えた差分が出る。`cargo insta review` でその差分だけであることを確かめて承認する。承認後のスナップショットは `data` のブロックだけを持つ。

- [ ] **Step 5: テストが通ることを確かめる**

Run: `cargo test -p eml_types && cargo test -p eml_cli`
Expected: PASS

- [ ] **Step 6: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`

```bash
git add -A crates
git commit -m "Check lambdas against expected function types or infer them"
```

---

### Task 10: SCC ごとに使用回数から Kind の制約を出し、スキームを多相化する

トップレベルの関数の呼び出しグラフを SCC に分け、呼ばれる側から順に検査する。SCC ごとに、本体の型を検査し、使用回数を数えて Kind の制約を出し、スキームを多相化する。最後にすべての Kind の制約を解く。`dump` にスキームの Kind の制約を出す。

**Files:**
- Create: `crates/eml_types/src/scc.rs`、`crates/eml_types/src/usage.rs`
- Modify: `crates/eml_types/src/lib.rs`、`crates/eml_types/src/check.rs`、`crates/eml_types/src/ty.rs`、`crates/eml_types/src/table.rs` (`kind_names`)
- Test: `crates/eml_types/tests/check.rs`、`crates/eml_types/src/scc.rs` (単体テスト)

**Interfaces:**
- Consumes: Task 3 の `solve_kinds`、`lin_residual`、Task 7 の `Scheme::generalize`
- Produces:
  - `scc::components(module: &Module) -> Vec<Vec<FunctionId>>` (呼ばれる側の SCC が先)
  - `usage::constrain(body: &Body, typing: &BodyTyping, table: &mut Table)`
  - `check::BodyTyping { pub exprs: ArenaMap<ExprId, Ty>, pub locals: ArenaMap<LocalId, Ty>, pub pats: ArenaMap<PatId, Ty> }` (`pub(crate)`、`Default`)
  - `eml_types::KindTerm::{Unr, Lin, Of(Type)}`、`eml_types::KindConstraint { lower: KindTerm, upper: KindTerm }` (表示は `a <= Unr`、関数型は括弧で囲む)
  - `TypedModule::kinds: ArenaMap<FunctionId, Vec<KindConstraint>>`
  - `Table::kind_names(&self, ty: Ty) -> HashMap<KindVar, Type>`
  - `dump` は、スキームに Kind の制約があればシグネチャの次の行に `  kinds: a <= Unr, (a -> a) <= Unr` を出す

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/check.rs` に足す。

```rust
#[test]
fn kinds_follow_from_how_values_are_used() {
    let text = "twice : (a -> a) -> a -> a\ntwice f x = f (f x)\n\nid : a -> a\nid x = x\n\nboth : a -> (a -> a -> b) -> b\nboth x g = g x x\n\npick : c -> c -> c\npick p q = p\n\ncall : d -> d\ncall y = both y pick";
    insta::assert_snapshot!(check_text(text), @r"
    twice : (a -> a) -> a -> a
      kinds: (a -> a) <= Unr
      f#0 : a -> a
      x#1 : a
    id : a -> a
      x#0 : a
    both : a -> (a -> a -> b) -> b
      kinds: a <= Unr
      x#0 : a
      g#1 : a -> a -> b
    pick : c -> c -> c
      kinds: c <= Unr
      p#0 : c
      q#1 : c
    call : d -> d
      kinds: d <= Unr
      y#0 : d
    ");
}

#[test]
fn kinds_are_shared_within_a_strongly_connected_component() {
    let text = "ping : a -> Int -> a\nping x n = if n == 0 then x else pong (first x x) (n - 1)\n\npong : b -> Int -> b\npong y n = ping y n\n\nfirst : c -> c -> c\nfirst u v = u";
    insta::assert_snapshot!(check_text(text), @r"
    ping : a -> Int -> a
      kinds: a <= Unr
      x#0 : a
      n#1 : Int
    pong : b -> Int -> b
      kinds: b <= Unr
      y#0 : b
      n#1 : Int
    first : c -> c -> c
      kinds: c <= Unr
      u#0 : c
      v#1 : c
    ");
}

#[test]
fn captured_values_count_inside_the_lambda_body() {
    let text = "dupper : a -> Unit -> (a -> a -> a) -> a\ndupper x = fn () g -> g x x\n\nkeeper : a -> Unit -> a\nkeeper x = fn () -> x";
    insta::assert_snapshot!(check_text(text), @r"
    dupper : a -> Unit -> (a -> a -> a) -> a
      kinds: a <= Unr
      x#0 : a
      g#1 : a -> a -> a
    keeper : a -> Unit -> a
      x#0 : a
    ");
}
```

`crates/eml_types/src/scc.rs` の単体テストは Step 3 のコードに含める。

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --test check`
Expected: FAIL (`kinds:` の行が出ない)

- [ ] **Step 3: SCC を求める**

`crates/eml_types/src/scc.rs` を作る。`lib.rs` に `mod scc;` と `mod usage;` を足す。

```rust
//! トップレベルの関数の呼び出しグラフの強連結成分 (docs/spec/types.md の「推論」)。Kind の制約は呼び出しを通じて
//! 伝わるので、呼ばれる側の SCC から順に検査する。

use std::collections::HashMap;

use eml_hir::{ExprKind, Function, FunctionId, Module, Res};

/// SCC を、呼ばれる側が先になる順に返す。Tarjan の方法で、関数の数が多くても Rust のスタックを使わないように、
/// 明示的なスタックでたどる。
pub(crate) fn components(module: &Module) -> Vec<Vec<FunctionId>> {
    let ids: Vec<FunctionId> = module.functions.iter().map(|(id, _)| id).collect();
    let position: HashMap<FunctionId, usize> =
        ids.iter().enumerate().map(|(i, &id)| (id, i)).collect();
    let edges: Vec<Vec<usize>> = ids
        .iter()
        .map(|&id| {
            callees(&module.functions[id])
                .into_iter()
                .map(|callee| position[&callee])
                .collect()
        })
        .collect();
    let unvisited = usize::MAX;
    let mut index = vec![unvisited; ids.len()];
    let mut low = vec![0; ids.len()];
    let mut on_stack = vec![false; ids.len()];
    let mut stack = Vec::new();
    let mut next = 0;
    let mut out = Vec::new();
    for root in 0..ids.len() {
        if index[root] != unvisited {
            continue;
        }
        let mut work = vec![(root, 0)];
        index[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(&(v, edge)) = work.last() {
            if let Some(&w) = edges[v].get(edge) {
                work.last_mut().unwrap().1 += 1;
                if index[w] == unvisited {
                    index[w] = next;
                    low[w] = next;
                    next += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    work.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
                continue;
            }
            work.pop();
            if let Some(&(parent, _)) = work.last() {
                low[parent] = low[parent].min(low[v]);
            }
            if low[v] == index[v] {
                let mut component = Vec::new();
                loop {
                    let w = stack.pop().unwrap();
                    on_stack[w] = false;
                    component.push(ids[w]);
                    if w == v {
                        break;
                    }
                }
                component.reverse();
                out.push(component);
            }
        }
    }
    out
}

/// 本体で参照するトップレベルの関数。呼び出しと値としての参照の両方を数える。
fn callees(function: &Function) -> Vec<FunctionId> {
    let Some(body) = &function.body else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (_, expr) in body.exprs.iter() {
        if let ExprKind::Path(Res::Function(callee)) = expr.kind
            && !out.contains(&callee)
        {
            out.push(callee);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use eml_diagnostics::SourceFiles;

    use super::*;

    fn names(text: &str) -> Vec<Vec<String>> {
        let mut files = SourceFiles::new();
        let file = files.add("test.em", text);
        let (parse, _) = eml_syntax::parse(file, text);
        let (module, _) = eml_hir::lower(file, &parse.tree());
        components(&module)
            .into_iter()
            .map(|component| {
                component
                    .into_iter()
                    .map(|id| module.functions[id].name.clone())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn callees_come_before_callers_and_cycles_stay_together() {
        let text = "a : Int -> Int\na n = b n + c n\n\nb : Int -> Int\nb n = c n\n\nc : Int -> Int\nc n = if n == 0 then 0 else d n\n\nd : Int -> Int\nd n = c (n - 1)";
        assert_eq!(names(text), vec![vec!["c", "d"], vec!["b"], vec!["a"]]);
    }
}
```

単体テストが使う `eml_syntax` は、`crates/eml_types/Cargo.toml` の `[dev-dependencies]` にすでにある。

- [ ] **Step 4: 使用回数を数える**

`crates/eml_types/src/usage.rs` を作る。

```rust
//! 使用回数の数え上げ (docs/spec/linearity.md の「基本の規則」)。線形性の検査パスの土台で、段階2では Kind の
//! 制約だけを出す。段階5で、枝ごとの消費の一致、持ち越し規則、E3xxx をこのパスに足す。

use std::collections::HashMap;

use eml_hir::{Body, ExprId, ExprKind, LocalId, PatId, PatKind, Res, Stmt};

use crate::check::BodyTyping;
use crate::kind::Bound;
use crate::table::Table;
use crate::ty::Linearity;

/// 制御フローの経路ごとの使用回数の最小と最大。2 回以上は区別しないので 2 で頭打ちにする。
type Uses = HashMap<LocalId, (u8, u8)>;

pub(crate) fn constrain(body: &Body, typing: &BodyTyping, table: &mut Table) {
    let mut usage = Usage {
        body,
        typing,
        table,
    };
    let uses = usage.expr(body.root);
    for &param in &body.params {
        usage.check_pat(param, &uses);
    }
}

struct Usage<'a> {
    body: &'a Body,
    typing: &'a BodyTyping,
    table: &'a mut Table,
}

impl Usage<'_> {
    fn expr(&mut self, id: ExprId) -> Uses {
        let body = self.body;
        match &body.exprs[id].kind {
            ExprKind::Missing | ExprKind::Literal(_) => Uses::new(),
            ExprKind::Path(Res::Local(local)) => Uses::from([(*local, (1, 1))]),
            ExprKind::Path(_) => Uses::new(),
            // 関数型の値を呼ぶことも、その値の1回の使用である (docs/spec/types.md の「推論」)
            ExprKind::Call { callee, args } => {
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
                let else_uses = else_branch.map(|e| self.expr(e)).unwrap_or_default();
                sequence(&mut uses, join(then_uses, else_uses));
                uses
            }
            ExprKind::Block { stmts, tail } => {
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
                    self.check_pat(pat, &uses);
                    remove_bound(body, pat, &mut uses);
                }
                uses
            }
            ExprKind::Annot { expr, .. } => self.expr(*expr),
            ExprKind::Lambda {
                params,
                body: lambda_body,
            } => {
                let mut inner = self.expr(*lambda_body);
                for &param in params {
                    self.check_pat(param, &inner);
                    remove_bound(body, param, &mut inner);
                }
                // 残りは捕まえた変数である。捕まえることは外から見て1回の使用で、本体の中で1回でなければ `Unr`
                // にする。ラムダとその部分適用の線形性は、捕まえた値の Kind 以上になる (docs/spec/linearity.md)
                let mut captured: Vec<LocalId> = inner.keys().copied().collect();
                captured.sort();
                let mut captured_types = Vec::new();
                for &local in &captured {
                    if inner[&local] != (1, 1) {
                        self.unr_local(local);
                    }
                    if let Some(&ty) = self.typing.locals.get(local) {
                        captured_types.push(ty);
                    }
                }
                if let Some(&ty) = self.typing.exprs.get(id) {
                    self.table.closure_kinds(ty, params.len(), &captured_types);
                }
                captured.into_iter().map(|local| (local, (1, 1))).collect()
            }
        }
    }

    /// パターンが束縛した変数を数え終える。どこかの経路で 0 回か2回以上なら、その型の Kind に `Unr` の制約を出す。
    /// `_` で受けた値も使わない値なので同じ扱いにする (docs/spec/linearity.md の「基本の規則」)。
    fn check_pat(&mut self, pat: PatId, uses: &Uses) {
        match &self.body.pats[pat].kind {
            PatKind::Bind(local) => {
                if uses.get(local).copied().unwrap_or((0, 0)) != (1, 1) {
                    self.unr_local(*local);
                }
            }
            PatKind::Wildcard => {
                if let Some(&ty) = self.typing.pats.get(pat) {
                    self.table.kind_at_most(ty, Bound::Const(Linearity::Unr));
                }
            }
            PatKind::Annot { pat, .. } => self.check_pat(*pat, uses),
            PatKind::Unit | PatKind::Missing => {}
        }
    }

    fn unr_local(&mut self, local: LocalId) {
        if let Some(&ty) = self.typing.locals.get(local) {
            self.table.kind_at_most(ty, Bound::Const(Linearity::Unr));
        }
    }
}

fn remove_bound(body: &Body, pat: PatId, uses: &mut Uses) {
    match &body.pats[pat].kind {
        PatKind::Bind(local) => {
            uses.remove(local);
        }
        PatKind::Annot { pat, .. } => remove_bound(body, *pat, uses),
        _ => {}
    }
}

/// 続けて実行する2つの部分の使用回数を足す。
fn sequence(uses: &mut Uses, next: Uses) {
    for (local, (min, max)) in next {
        let entry = uses.entry(local).or_insert((0, 0));
        *entry = ((entry.0 + min).min(2), (entry.1 + max).min(2));
    }
}

/// 分岐の2つの枝の使用回数を合わせる。片方の枝にない変数は、その枝では 0 回である。
fn join(a: Uses, b: Uses) -> Uses {
    let mut out = Uses::new();
    for local in a.keys().chain(b.keys()) {
        let x = a.get(local).copied().unwrap_or((0, 0));
        let y = b.get(local).copied().unwrap_or((0, 0));
        out.insert(*local, (x.0.min(y.0), x.1.max(y.1)));
    }
    out
}
```

- [ ] **Step 5: Kind の制約の表示を足す**

`crates/eml_types/src/ty.rs` に足す。

```rust
/// スキームに残った Kind の制約。テストの表示で使う。`Of` は、その型の Kind を表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KindTerm {
    Unr,
    Lin,
    Of(Type),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindConstraint {
    pub lower: KindTerm,
    pub upper: KindTerm,
}

impl fmt::Display for KindTerm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KindTerm::Unr => f.write_str("Unr"),
            KindTerm::Lin => f.write_str("Lin"),
            KindTerm::Of(ty @ Type::Fn { .. }) => write!(f, "({ty})"),
            KindTerm::Of(ty) => write!(f, "{ty}"),
        }
    }
}

impl fmt::Display for KindConstraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} <= {}", self.lower, self.upper)
    }
}
```

`lib.rs` の `pub use ty::{...}` に `KindConstraint, KindTerm` を足す。`TypedModule` に足す。

```rust
    /// スキームに残った Kind の制約のうち、定数を片側に持つもの。テストの表示で使う。
    pub kinds: ArenaMap<FunctionId, Vec<KindConstraint>>,
```

`dump` の、シグネチャの行を書いた直後に足す。

```rust
        if let Some(kinds) = typed.kinds.get(id).filter(|kinds| !kinds.is_empty()) {
            let kinds: Vec<String> = kinds.iter().map(ToString::to_string).collect();
            writeln!(out, "  kinds: {}", kinds.join(", ")).unwrap();
        }
```

`crates/eml_types/src/table.rs` の `impl Table` に足す。

```rust
    /// スキームの Kind 変数を、その変数を持つ型の部分 (型変数か関数型) で呼ぶ。外側から順に見て、最初に現れた部分を使う。
    pub fn kind_names(&self, ty: Ty) -> HashMap<KindVar, Type> {
        let mut names = HashMap::new();
        let mut work = vec![ty];
        while let Some(ty) = work.pop() {
            match self.kind(ty) {
                TyKind::Rigid(rigid) => {
                    names
                        .entry(self.rigid_linearity(*rigid))
                        .or_insert_with(|| self.export(ty));
                }
                TyKind::Fn {
                    param, lin, ret, ..
                } => {
                    if let Mult::Var(v) = lin {
                        names.entry(*v).or_insert_with(|| self.export(ty));
                    }
                    work.push(*ret);
                    work.push(*param);
                }
                TyKind::Record(fields) => work.extend(fields.iter().rev().map(|(_, f)| *f)),
                TyKind::Con(_) | TyKind::Var(_) | TyKind::Error => {}
            }
        }
        names
    }
```

- [ ] **Step 6: 検査を SCC ごとにする**

`crates/eml_types/src/check.rs` で次を変える。

`BodyTyping` を足し、`BodyCheck` のフィールド `exprs` と `locals` を `typing: BodyTyping` にまとめる。`self.exprs` は `self.typing.exprs`、`self.locals` は `self.typing.locals` に書き換える。

```rust
/// 1つの本体の推論結果。使用回数のパスも読む。
#[derive(Default)]
pub(crate) struct BodyTyping {
    pub exprs: ArenaMap<ExprId, Ty>,
    pub locals: ArenaMap<LocalId, Ty>,
    /// パターンが受けた値の型。`_` で受けた値の Kind に制約を出すのに使う。
    pub pats: ArenaMap<PatId, Ty>,
}
```

`bind_pat` の先頭に `self.typing.pats.insert(pat, ty);` を足す。

`check_module` の本体の検査のループを次にし、`use crate::{scc, usage};`、`use crate::kind::Bound;`、`use crate::ty::{KindConstraint, KindTerm};` を足す。

```rust
    let mut bodies = Vec::new();
    // 呼ばれる側の SCC から順に検査し、SCC ごとに Kind を多相化する (docs/spec/types.md の「推論」)
    for component in scc::components(module) {
        for &id in &component {
            let function = &module.functions[id];
            let (Some(scheme), Some(body)) = (schemes.get(id), &function.body) else {
                continue;
            };
            let signature = scheme.ty;
            let mut checker = BodyCheck {
                module,
                function,
                body,
                rigids: &rigids[id],
                schemes: &schemes,
                table: &mut table,
                diagnostics: &mut diagnostics,
                ambient: Row::pure(),
                ambient_source: AmbientSource::Signature,
                typing: BodyTyping::default(),
            };
            checker.check_function(signature);
            let typing = checker.typing;
            usage::constrain(body, &typing, &mut table);
            bodies.push((id, typing));
        }
        for &id in &component {
            if let Some(scheme) = schemes.get_mut(id) {
                scheme.generalize(&table);
            }
        }
    }
    let violated = table.solve_kinds();
    // 段階2には `Lin` の型がないので、Kind の制約は破れない。違反の診断の番号は段階5で決める
    debug_assert!(!violated, "a kind constraint was violated without linear types");
    let mut typed = TypedModule {
        main,
        ..TypedModule::default()
    };
    for (id, scheme) in schemes.iter() {
        typed.signatures.insert(id, table.export(scheme.ty));
        typed.kinds.insert(id, kind_constraints(&table, scheme));
    }
    for (id, typing) in bodies {
        let mut types = BodyTypes::default();
        for (expr, &ty) in typing.exprs.iter() {
            types.exprs.insert(expr, table.export(ty));
        }
        for (local, &ty) in typing.locals.iter() {
            types.locals.insert(local, table.export(ty));
        }
        typed.bodies.insert(id, types);
    }
    (typed, diagnostics)
```

ファイルに足す。

```rust
/// スキームに残った制約のうち、定数を片側に持つものを表示用にする。変数どうしの制約は出さない。テストで確かめたいのは
/// `Unr` の上限が付いたかどうかで、変数どうしの制約は部分適用のたびに増えて読みにくくなるため。
fn kind_constraints(table: &Table, scheme: &Scheme) -> Vec<KindConstraint> {
    let names = table.kind_names(scheme.ty);
    let term = |bound: Bound<Linearity>| match bound {
        Bound::Const(Linearity::Unr) => Some(KindTerm::Unr),
        Bound::Const(Linearity::Lin) => Some(KindTerm::Lin),
        Bound::Var(var) => names.get(&var).cloned().map(KindTerm::Of),
    };
    scheme
        .lin_constraints()
        .iter()
        .filter(|(lower, upper)| {
            matches!(lower, Bound::Const(_)) != matches!(upper, Bound::Const(_))
        })
        .filter_map(|&(lower, upper)| {
            Some(KindConstraint {
                lower: term(lower)?,
                upper: term(upper)?,
            })
        })
        .collect()
}
```

`table.rs` の `row_multiplicity` に付けた `#[allow(dead_code)]` は、使われないままなら残す。

- [ ] **Step 7: テストが通ることを確かめる**

Run: `cargo test -p eml_types`
Expected: PASS。Task 7〜9 で足したテストのスナップショットに `kinds:` の行が増えないことも確かめる (どのテストも、型変数の値を1回ずつ使っている)

- [ ] **Step 8: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`

```bash
git add crates/eml_types
git commit -m "Infer kinds per call-graph component from how values are used and generalize schemes"
```

---

### Task 11: ランタイムとインタプリタでクロージャを実行する

Core IR に `MakeClosure` と `Apply` を足し、ランタイムにクロージャと余った引数のフレームを足して、インタプリタで eval/apply を実行する。Core IR への変換はまだこれらを作らないので、手で組んだ Core IR でテストする。

**Files:**
- Modify: `crates/eml_core_ir/src/lib.rs`、`crates/eml_core_ir/src/pretty.rs`、`crates/eml_core_ir/src/perceus.rs`
- Modify: `crates/eml_runtime/src/heap.rs`、`crates/eml_runtime/src/lib.rs`
- Modify: `crates/eml_interp/src/lib.rs`
- Create: `crates/eml_interp/tests/closures.rs`

**Interfaces:**
- Produces:
  - `eml_core_ir::Rhs::MakeClosure(FnIdx, Vec<Atom>)`、`eml_core_ir::Rhs::Apply(Atom, Vec<Atom>)`。表示は `closure name(args)` と `apply f(args)`
  - `eml_runtime::Closure { function: u32, args: Vec<Value> }`、`eml_runtime::ApplyFrame { args: Vec<Value>, next: Option<ObjRef> }`、`Payload::Closure`、`Payload::ApplyFrame`、`DescId::CLOSURE`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_runtime/src/heap.rs` の `mod tests` に足す。

```rust
    #[test]
    fn a_closure_releases_its_arguments() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        let closure = heap.alloc(
            DescId::CLOSURE,
            Payload::Closure(Closure {
                function: 0,
                args: vec![Value::Obj(s), Value::Int(1)],
            }),
        );
        heap.decref(closure).unwrap();
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn an_apply_frame_releases_its_arguments_and_the_rest_of_the_continuation() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        let next = frame(&mut heap, vec![], None);
        let apply = heap.alloc(
            DescId::FRAME,
            Payload::ApplyFrame(ApplyFrame {
                args: vec![Value::Obj(s)],
                next: Some(next),
            }),
        );
        heap.decref(apply).unwrap();
        assert!(heap.live_objects().is_empty());
    }
```

`crates/eml_interp/tests/closures.rs` を作る。

```rust
//! 手で組んだ Core IR で、クロージャの eval/apply を確かめる (docs/spec/core-ir.md)。

use std::sync::Arc;

use eml_core_ir::{
    Atom, CExpr, CExprId, CoreFn, FnIdx, IoOp, Linearity, PrimOp, Program, Rhs, VarId, VarInfo,
};
use eml_interp::{RunConfig, run};
use eml_runtime::OutputSink;

enum Step {
    Let(u32, Rhs),
    Dup(u32),
}

fn function(name: &str, params: u32, vars: &[(&str, bool)], steps: Vec<Step>, ret: Atom) -> CoreFn {
    let mut exprs = vec![CExpr::Return(ret)];
    let mut body = CExprId(0);
    for step in steps.into_iter().rev() {
        exprs.push(match step {
            Step::Let(var, rhs) => CExpr::Let {
                var: VarId(var),
                rhs,
                body,
            },
            Step::Dup(var) => CExpr::Dup {
                var: VarId(var),
                body,
            },
        });
        body = CExprId(exprs.len() as u32 - 1);
    }
    CoreFn {
        name: name.to_string(),
        params: (0..params).map(VarId).collect(),
        vars: vars
            .iter()
            .map(|&(name, boxed)| VarInfo {
                name: name.to_string(),
                linearity: Linearity::Unr,
                boxed,
            })
            .collect(),
        body,
        exprs,
    }
}

fn run_program(functions: Vec<CoreFn>, main: u32, strings: &[&str]) -> String {
    let program = Program {
        functions,
        main: FnIdx(main),
        strings: strings.iter().map(|s| s.to_string()).collect(),
    };
    let mut config = RunConfig::default();
    config.debug_heap = true;
    let (sink, buffer) = OutputSink::capture();
    run(Arc::new(program), &config, &sink).unwrap();
    String::from_utf8(buffer.lock().unwrap().clone()).unwrap()
}

fn var(n: u32) -> Atom {
    Atom::Var(VarId(n))
}

/// `first a b = a`。
fn first(boxed: bool) -> CoreFn {
    function("first", 2, &[("a", boxed), ("b", false)], vec![], var(0))
}

/// 値 `n` の変数を文字列にして出力する。
fn print_int(n: u32, show: u32, out: u32) -> Vec<Step> {
    vec![
        Step::Let(show, Rhs::Prim(PrimOp::ShowInt, vec![var(n)])),
        Step::Let(out, Rhs::Perform(IoOp::Println, vec![var(show)])),
    ]
}

#[test]
fn a_partial_application_waits_for_the_rest_of_the_arguments() {
    let mut steps = vec![
        Step::Let(1, Rhs::MakeClosure(FnIdx(0), vec![Atom::Int(10)])),
        Step::Let(2, Rhs::Apply(var(1), vec![Atom::Int(20)])),
    ];
    steps.extend(print_int(2, 3, 4));
    let vars = [("p", false), ("c", true), ("r", false), ("s", true), ("t", false)];
    let main = function("main", 1, &vars, steps, var(4));
    assert_eq!(run_program(vec![first(false), main], 1, &[]), "10\n");
}

#[test]
fn extra_arguments_are_applied_to_the_returned_function() {
    // make x = closure first(x)
    let make = function(
        "make",
        1,
        &[("x", false), ("c", true)],
        vec![Step::Let(1, Rhs::MakeClosure(FnIdx(0), vec![var(0)]))],
        var(1),
    );
    let mut steps = vec![
        Step::Let(1, Rhs::MakeClosure(FnIdx(1), vec![])),
        Step::Let(2, Rhs::Apply(var(1), vec![Atom::Int(5), Atom::Int(6)])),
    ];
    steps.extend(print_int(2, 3, 4));
    let vars = [("p", false), ("m", true), ("r", false), ("s", true), ("t", false)];
    let main = function("main", 1, &vars, steps, var(4));
    assert_eq!(run_program(vec![first(false), make, main], 2, &[]), "5\n");
}

#[test]
fn a_shared_closure_keeps_its_captured_values() {
    let steps = vec![
        Step::Let(1, Rhs::ConstString(0)),
        Step::Let(2, Rhs::MakeClosure(FnIdx(0), vec![var(1)])),
        Step::Dup(2),
        Step::Let(3, Rhs::Apply(var(2), vec![Atom::Int(1)])),
        Step::Let(4, Rhs::Perform(IoOp::Println, vec![var(3)])),
        Step::Let(5, Rhs::Apply(var(2), vec![Atom::Int(2)])),
        Step::Let(6, Rhs::Perform(IoOp::Println, vec![var(5)])),
    ];
    let vars = [
        ("p", false),
        ("s", true),
        ("c", true),
        ("r", true),
        ("t", false),
        ("r", true),
        ("t", false),
    ];
    let main = function("main", 1, &vars, steps, var(6));
    assert_eq!(run_program(vec![first(true), main], 1, &["a"]), "a\na\n");
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_runtime && cargo test -p eml_interp`
Expected: コンパイルエラー (`Closure`、`Rhs::MakeClosure` などがない)

- [ ] **Step 3: Core IR に命令を足す**

`crates/eml_core_ir/src/lib.rs` の `Rhs` に足す。

```rust
    /// 関数と先頭の引数の並びからクロージャを作る。並びの値の所有権はクロージャに移る。ラムダの捕獲と部分適用は、
    /// どちらもこの形になる (docs/spec/core-ir.md)。
    MakeClosure(FnIdx, Vec<Atom>),
    /// クロージャの値を呼ぶ。実行時に引数の個数を比べる (eval/apply)。クロージャと引数の所有権は呼び出しに移る。
    Apply(Atom, Vec<Atom>),
```

`crates/eml_core_ir/src/pretty.rs` の `rhs_text` に足す。

```rust
        Rhs::MakeClosure(target, a) => {
            format!("closure {}({})", program.function(*target).name, args(a))
        }
        Rhs::Apply(callee, a) => format!("apply {}({})", atom(function, callee), args(a)),
```

`function` は `rhs_text` の引数 (表示中の `CoreFn`) である。

`crates/eml_core_ir/src/perceus.rs` の `uses` を次にする。

```rust
    /// 右辺が使う変数を、使う回数の分だけ並べる。
    fn uses(&self, rhs: &Rhs) -> Vec<VarId> {
        let atoms: Vec<&Atom> = match rhs {
            Rhs::Atom(atom) => vec![atom],
            Rhs::CallDirect(_, args)
            | Rhs::Prim(_, args)
            | Rhs::Perform(_, args)
            | Rhs::MakeClosure(_, args) => args.iter().collect(),
            Rhs::Apply(callee, args) => std::iter::once(callee).chain(args).collect(),
            Rhs::ConstString(_) | Rhs::Nested(_) => Vec::new(),
        };
        atoms
            .into_iter()
            .filter_map(|atom| self.atom_var(atom))
            .collect()
    }
```

- [ ] **Step 4: ランタイムにクロージャを足す**

`crates/eml_runtime/src/heap.rs` で次を変える。

`DescId` に `pub const CLOSURE: DescId = DescId(2);` を足し、`Heap::new` の `descriptors` の最後に `Descriptor { name: "Closure".to_string() }` を足す。

`Payload` を次にする。

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum Payload {
    Str(String),
    Frame(Frame),
    Closure(Closure),
    ApplyFrame(ApplyFrame),
}

/// クロージャ。関数と、すでに渡された先頭の引数の並び (docs/spec/core-ir.md)。各値は参照を1つずつ所有する。
/// `Clone` は `ObjRef` を `dup` せずに複製するので、複製した側が参照を数え直す。
#[derive(Debug, Clone, PartialEq)]
pub struct Closure {
    pub function: u32,
    pub args: Vec<Value>,
}

/// 呼んだ関数から戻った値に、余った引数を適用するフレーム (docs/spec/core-ir.md の eval/apply)。継続の連結リストの
/// 要素なので、記述子はフレームと同じにする。
#[derive(Debug, Clone, PartialEq)]
pub struct ApplyFrame {
    pub args: Vec<Value>,
    pub next: Option<ObjRef>,
}
```

`children` を次にする。

```rust
fn children(payload: &Payload, work: &mut Vec<ObjRef>) {
    let objects = |values: &[Value]| -> Vec<ObjRef> {
        values
            .iter()
            .filter_map(|value| match value {
                Value::Obj(obj) => Some(*obj),
                _ => None,
            })
            .collect()
    };
    match payload {
        Payload::Frame(frame) => {
            for owned in frame.slots.iter().flatten().flatten() {
                // Perceus の `dup` で1つの変数が複数の参照を持つので、1つだけ手放すと残りがリークする
                if let Value::Obj(obj) = owned.value {
                    work.extend(std::iter::repeat_n(obj, owned.refs as usize));
                }
            }
            work.extend(frame.next);
        }
        Payload::Closure(closure) => work.extend(objects(&closure.args)),
        Payload::ApplyFrame(frame) => {
            work.extend(objects(&frame.args));
            work.extend(frame.next);
        }
        Payload::Str(_) => {}
    }
}
```

`crates/eml_runtime/src/lib.rs` の `pub use heap::{...}` に `ApplyFrame, Closure` を足す。

- [ ] **Step 5: インタプリタで eval/apply を実行する**

`crates/eml_interp/src/lib.rs` で次を変える。

`use` を次にする。

```rust
use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

use eml_core_ir::{Atom, CExpr, CExprId, FALSE, FnIdx, IoOp, PrimOp, Program, Rhs, TRUE, VarId};
use eml_runtime::{
    ApplyFrame, Closure, DescId, Frame, Heap, HeapError, ObjRef, OutputSink, Owned, Payload, Value,
};
```

`Applied` を足す。

```rust
/// 関数値の適用の結果。関数に入ったか、値ができたか (足りない引数のクロージャ)。
enum Applied {
    Entered,
    Value(Value),
}
```

`step` の `CExpr::Let { var, rhs, body } => self.bind(*var, rhs, *body)?,` を `CExpr::Let { var, rhs, body } => return self.bind(*var, rhs, *body),` にする。

`bind` の戻り値を `Result<bool, String>` にし、`return Ok(());` をすべて `return Ok(false);` に、最後の `Ok(())` を `Ok(false)` にする。`Rhs::CallDirect` の腕を次にし、腕を2つ足す。

```rust
            Rhs::CallDirect(callee, args) => {
                let args = self.atoms(args)?;
                self.push_frame(var, body, true);
                self.enter(*callee, args);
                return Ok(false);
            }
            Rhs::MakeClosure(function, args) => {
                let args = self.atoms(args)?;
                let closure = Closure {
                    function: function.0,
                    args,
                };
                Value::Obj(self.heap.alloc(DescId::CLOSURE, Payload::Closure(closure)))
            }
            Rhs::Apply(callee, args) => {
                let callee = self.atom(callee)?;
                let args = self.atoms(args)?;
                self.push_frame(var, body, true);
                return match self.apply(callee, args)? {
                    Applied::Entered => Ok(false),
                    Applied::Value(value) => self.ret(value),
                };
            }
```

`impl Machine` に足す。

```rust
    fn enter(&mut self, callee: FnIdx, args: Vec<Value>) {
        let target = self.program.function(callee);
        let mut slots = vec![None; target.vars.len()];
        for (param, value) in target.params.iter().zip(args) {
            slots[param.0 as usize] = Some(Owned::new(value));
        }
        self.slots = slots;
        self.function = callee;
        self.control = target.body;
    }

    /// 関数値を引数に適用する (docs/spec/core-ir.md の eval/apply)。引数の個数が揃えば関数に入り、足りなければ
    /// 引数を足したクロージャを値にし、余れば余りを持つフレームを積んでから関数に入る。
    fn apply(&mut self, callee: Value, mut args: Vec<Value>) -> Result<Applied, String> {
        let Value::Obj(obj) = callee else {
            return Err(internal("applying a value that is not a closure"));
        };
        let closure = self.take_closure(obj)?;
        let function = FnIdx(closure.function);
        let mut all = closure.args;
        all.append(&mut args);
        let arity = self.program.function(function).params.len();
        match all.len().cmp(&arity) {
            Ordering::Equal => {
                self.enter(function, all);
                Ok(Applied::Entered)
            }
            Ordering::Less => {
                let closure = Closure {
                    function: closure.function,
                    args: all,
                };
                let value = self.heap.alloc(DescId::CLOSURE, Payload::Closure(closure));
                Ok(Applied::Value(Value::Obj(value)))
            }
            Ordering::Greater => {
                let rest = all.split_off(arity);
                let frame = ApplyFrame {
                    args: rest,
                    next: Some(self.cont),
                };
                self.cont = self.heap.alloc(DescId::FRAME, Payload::ApplyFrame(frame));
                self.enter(function, all);
                Ok(Applied::Entered)
            }
        }
    }

    /// 呼び出しはクロージャの所有権を受け取る。一意なら中身を取り出し、共有されていれば中身の参照を複製してから
    /// 手放す。
    fn take_closure(&mut self, obj: ObjRef) -> Result<Closure, String> {
        if self.heap.is_unique(obj).map_err(heap_error)? {
            return match self.heap.take(obj).map_err(heap_error)? {
                Payload::Closure(closure) => Ok(closure),
                _ => Err(internal("applying an object that is not a closure")),
            };
        }
        let closure = match self.heap.get(obj).map_err(heap_error)? {
            Payload::Closure(closure) => closure.clone(),
            _ => return Err(internal("applying an object that is not a closure")),
        };
        for value in &closure.args {
            if let Value::Obj(captured) = value {
                self.heap.dup(*captured).map_err(heap_error)?;
            }
        }
        self.heap.decref(obj).map_err(heap_error)?;
        Ok(closure)
    }
```

`ret` を次にする。余った引数のフレームが続く間はループで適用し、Rust の再帰を使わない。

```rust
    /// 継続の先頭のフレームに値を返す。最下部の `IO` の handler に届いたら、プログラムが終わる。
    fn ret(&mut self, mut value: Value) -> Result<bool, String> {
        loop {
            // 段階2までは継続を複製しないので、フレームは常に一意である。共有されたフレームは段階3の `multi` で扱う
            let frame = match self.heap.take(self.cont).map_err(heap_error)? {
                Payload::ApplyFrame(frame) => {
                    self.cont = frame
                        .next
                        .ok_or_else(|| internal("an apply frame without a next frame"))?;
                    match self.apply(value, frame.args)? {
                        Applied::Entered => return Ok(false),
                        Applied::Value(result) => {
                            value = result;
                            continue;
                        }
                    }
                }
                Payload::Frame(frame) => frame,
                _ => return Err(internal("the continuation is not a frame")),
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
            self.slots[frame.bind as usize] = Some(Owned::new(value));
            self.cont = frame
                .next
                .ok_or_else(|| internal("a frame without a next frame"))?;
            return Ok(false);
        }
    }
```

`take_string` の `match` を次にする。

```rust
        let text = match self.heap.get(obj).map_err(heap_error)? {
            Payload::Str(text) => text.clone(),
            _ => {
                return Err(internal(
                    "a string operation on a value that is not a string",
                ));
            }
        };
```

- [ ] **Step 6: テストが通ることを確かめる**

Run: `cargo test -p eml_runtime && cargo test -p eml_interp && cargo test -p eml_core_ir`
Expected: PASS

- [ ] **Step 7: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`

```bash
git add crates
git commit -m "Run closures with eval/apply in the runtime and the interpreter"
```

---

### Task 12: Core IR で関数値、部分適用、引数の多い呼び出しを変換する

トップレベルの関数と組み込みを値として使う、局所変数や式を呼ぶ、部分適用、引数の多い呼び出しを、`MakeClosure` と `Apply` に変換する。組み込みを値として使うときは、組み込みを呼ぶだけの関数を作って包む。関数の表を、変換の途中で関数を足せる形にする (Task 13 のラムダも使う)。

**Files:**
- Modify: `crates/eml_core_ir/src/lower.rs`
- Test: `crates/eml_core_ir/tests/lower.rs`
- Create: `tests/ui/run/higher_order.em`、`tests/ui/run/partial_application.em`

**Interfaces:**
- Consumes: Task 11 の `Rhs::MakeClosure`、`Rhs::Apply`、Task 6 の `Builtin::ComposeFwd` / `ComposeBwd`
- Produces (`lower.rs` の中):
  - `ProgramBuilder { functions: Vec<Option<CoreFn>>, arities: Vec<usize>, strings: Strings, wrappers: HashMap<Builtin, FnIdx> }`、`reserve(arity) -> FnIdx`、`arity(FnIdx) -> usize`、`finish(FnIdx, CoreFn)`、`wrapper(Builtin) -> FnIdx`
  - `FnLowering::lower(self, name: &str, captured: &[(LocalId, Type)], params: &[PatId], param_types: &[Type], root: ExprId) -> CoreFn`
  - `param_types(ty: &Type, count: usize) -> Vec<Type>`、`binder(body: &Body, pat: PatId) -> Option<LocalId>`
  - 包んだ組み込みの関数の名前は `builtin$` + 組み込みの名前 (`builtin$not`、`builtin$>>`)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/lower.rs` に足す。

```rust
#[test]
fn partial_and_extra_arguments_use_closures() {
    let text = "add : Int -> Int -> Int\nadd a b = a + b\n\nadder : Int -> Int -> Int\nadder x = add x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let f = add 1\n  let n = f 2 + adder 3 4\n  println (show_int n)";
    insta::assert_snapshot!(core_text(text), @r"
    fn add(a0, b1) {
      let t2 = prim +(a0, b1)
      return t2
    }
    fn adder(x0) {
      let c1 = closure add(x0)
      return c1
    }
    fn main(p0) {
      let c1 = closure add(1)
      let t2 = apply c1(2)
      let t3 = call adder(3)
      let t4 = apply t3(4)
      let t5 = prim +(t2, t4)
      let t6 = prim show_int(t5)
      let t7 = perform println(t6)
      return t7
    }
    ");
}

#[test]
fn builtins_used_as_values_are_wrapped() {
    let text = "apply : (a -> <e> b) -> a -> <e> b\napply f x = f x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let g = not >> not\n  apply println (show_int 1)";
    insta::assert_snapshot!(core_text(text), @r"
    fn apply(f0, x1) {
      let t2 = apply f0(x1)
      return t2
    }
    fn main(p0) {
      let c1 = closure builtin$not()
      let c2 = closure builtin$not()
      let c3 = closure builtin$>>(c1, c2)
      decref c3
      let c4 = closure builtin$println()
      let t5 = prim show_int(1)
      let t6 = call apply(c4, t5)
      return t6
    }
    fn builtin$not(p0) {
      let t1 = prim not(p0)
      return t1
    }
    fn builtin$>>(p0, p1, p2) {
      let t3 = apply p0(p2)
      let t4 = apply p1(t3)
      return t4
    }
    fn builtin$println(p0) {
      let t1 = perform println(p0)
      return t1
    }
    ");
}
```

`tests/ui/run/higher_order.em` を作る。

```haskell
-- Function values: calling function-typed parameters, passing top-level functions and builtins as values,
-- passing effects through a row variable, and composition.
apply : (a -> <e> b) -> a -> <e> b
apply f x = f x

twice : (a -> a) -> a -> a
twice f x = f (f x)

inc : Int -> Int
inc n = n + 1

main : Unit -> <IO> Unit
main () =
  println (show_int (apply inc 1))
  println (show_int (twice inc 5))
  apply println "through a row variable"
  println (show_int ((inc >> inc << inc) 0))
```

`tests/ui/run/partial_application.em` を作る。

```haskell
-- Partial application, extra arguments, equations with fewer parameters than arrows,
-- a closure called twice, and a closure dropped without being called.
greet : String -> String -> String
greet greeting name = greeting ++ ", " ++ name

hello : String -> String
hello = greet "hello"

add : Int -> Int -> Int
add a b = a + b

adder : Int -> Int -> Int
adder x = add x

main : Unit -> <IO> Unit
main () =
  let hi = greet "hi"
  println (hi "alice")
  println (hi "bob")
  println (hello "carol")
  let unused = greet "never called"
  println (show_int (adder 3 4))
  let add_ten = add 10
  println (show_int (add_ten 5))
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_core_ir && cargo test -p eml_cli --test ui`
Expected: FAIL (`unreachable!` で落ちる)

- [ ] **Step 3: 関数の表を、途中で関数を足せる形にする**

`crates/eml_core_ir/src/lower.rs` の `lower` と `Strings` の周りを次にする。

```rust
pub fn lower(module: &Module, typed: &TypedModule) -> Program {
    let mut program = ProgramBuilder::default();
    let mut indices = ArenaMap::default();
    for (id, function) in module.functions.iter() {
        let body = function
            .body
            .as_ref()
            .expect("a program without errors has an equation for every function");
        indices.insert(id, program.reserve(body.params.len()));
    }
    for (id, function) in module.functions.iter() {
        let body = function.body.as_ref().expect("checked above");
        let signature = typed
            .signatures
            .get(id)
            .expect("every function has a signature");
        let params = param_types(signature, body.params.len());
        let mut lambdas = 0;
        let core = FnLowering {
            module,
            body,
            types: typed.bodies.get(id).expect("every body is type-checked"),
            indices: &indices,
            program: &mut program,
            root_name: &function.name,
            lambdas: &mut lambdas,
            exprs: Vec::new(),
            vars: Vec::new(),
            locals: ArenaMap::default(),
        }
        .lower(&function.name, &[], &body.params, &params, body.root);
        program.finish(indices[id], core);
    }
    let main = typed
        .main
        .expect("`eml_cli::compile` reports a missing `main`");
    Program {
        functions: program
            .functions
            .into_iter()
            .map(|function| function.expect("every reserved function is lowered"))
            .collect(),
        main: indices[main],
        strings: program.strings.values,
    }
}

/// 変換の途中で、ラムダと包んだ組み込みの関数を足していく関数の表。番号を先に取り、中身は変換が終わってから入れる。
#[derive(Default)]
struct ProgramBuilder {
    functions: Vec<Option<CoreFn>>,
    arities: Vec<usize>,
    strings: Strings,
    wrappers: HashMap<Builtin, FnIdx>,
}

impl ProgramBuilder {
    fn reserve(&mut self, arity: usize) -> FnIdx {
        self.functions.push(None);
        self.arities.push(arity);
        FnIdx(self.functions.len() as u32 - 1)
    }

    fn arity(&self, function: FnIdx) -> usize {
        self.arities[function.0 as usize]
    }

    fn finish(&mut self, function: FnIdx, mut core: CoreFn) {
        perceus::insert_rc(&mut core);
        self.functions[function.0 as usize] = Some(core);
    }

    /// 組み込みを値や部分適用で使うときに、それを呼ぶだけの関数を作る。組み込みごとに1つだけ作る。
    fn wrapper(&mut self, builtin: Builtin) -> FnIdx {
        if let Some(&function) = self.wrappers.get(&builtin) {
            return function;
        }
        let boxed = builtin_params(builtin);
        let function = self.reserve(boxed.len());
        self.wrappers.insert(builtin, function);
        let mut vars: Vec<VarInfo> = boxed
            .iter()
            .map(|&boxed| VarInfo {
                name: "p".to_string(),
                linearity: Linearity::Unr,
                boxed,
            })
            .collect();
        let params: Vec<VarId> = (0..boxed.len() as u32).map(VarId).collect();
        let atoms: Vec<Atom> = params.iter().map(|&param| Atom::Var(param)).collect();
        let mut fresh = |boxed: bool| {
            vars.push(VarInfo {
                name: "t".to_string(),
                linearity: Linearity::Unr,
                boxed,
            });
            VarId(vars.len() as u32 - 1)
        };
        let steps: Vec<(VarId, Rhs)> = match builtin {
            Builtin::Println => vec![(fresh(false), Rhs::Perform(IoOp::Println, atoms))],
            Builtin::ComposeFwd | Builtin::ComposeBwd => {
                // `>>` は `g (f x)`、`<<` は `f (g x)` である (Task 6)
                let (inner, outer) = if builtin == Builtin::ComposeFwd {
                    (atoms[0], atoms[1])
                } else {
                    (atoms[1], atoms[0])
                };
                let mid = fresh(true);
                let result = fresh(true);
                vec![
                    (mid, Rhs::Apply(inner, vec![atoms[2]])),
                    (result, Rhs::Apply(outer, vec![Atom::Var(mid)])),
                ]
            }
            other => vec![(
                fresh(builtin_result_boxed(other)),
                Rhs::Prim(prim(other), atoms),
            )],
        };
        let mut exprs = Vec::new();
        let last = steps.last().expect("every wrapper binds a result").0;
        exprs.push(CExpr::Return(Atom::Var(last)));
        let mut body = CExprId(0);
        for (var, rhs) in steps.into_iter().rev() {
            exprs.push(CExpr::Let { var, rhs, body });
            body = CExprId(exprs.len() as u32 - 1);
        }
        let core = CoreFn {
            name: format!("builtin${}", builtin.name()),
            params,
            vars,
            body,
            exprs,
        };
        self.finish(function, core);
        function
    }
}

/// 関数型の先頭の `count` 個の引数の型。
fn param_types(ty: &Type, count: usize) -> Vec<Type> {
    let mut out = Vec::new();
    let mut ty = ty;
    for _ in 0..count {
        let Type::Fn { param, ret, .. } = ty else {
            unreachable!("the type checker matched parameters with arrows");
        };
        out.push((**param).clone());
        ty = ret;
    }
    out
}

/// 引数のパターンが束縛する局所変数。型を明示した引数は内側のパターンを見る。
fn binder(body: &Body, pat: PatId) -> Option<LocalId> {
    match &body.pats[pat].kind {
        PatKind::Bind(local) => Some(*local),
        PatKind::Annot { pat, .. } => binder(body, *pat),
        _ => None,
    }
}

/// 組み込みの引数ごとの、ヒープの値かどうか。`True` と `False` は値なので、ここには来ない。
fn builtin_params(builtin: Builtin) -> Vec<bool> {
    match builtin {
        Builtin::Println => vec![true],
        Builtin::ShowInt | Builtin::Not | Builtin::IntNeg => vec![false],
        Builtin::IntAdd
        | Builtin::IntSub
        | Builtin::IntMul
        | Builtin::IntDiv
        | Builtin::IntMod
        | Builtin::IntEq
        | Builtin::IntNe
        | Builtin::IntLt
        | Builtin::IntLe
        | Builtin::IntGt
        | Builtin::IntGe => vec![false, false],
        Builtin::StrConcat => vec![true, true],
        Builtin::ComposeFwd | Builtin::ComposeBwd => vec![true, true, true],
        Builtin::True | Builtin::False => unreachable!("constructors are values, not functions"),
    }
}

fn builtin_result_boxed(builtin: Builtin) -> bool {
    matches!(builtin, Builtin::ShowInt | Builtin::StrConcat)
}
```

`FnLowering` のフィールドを次にする (`strings` を消し、`self.strings.intern` は `self.program.strings.intern` にする)。

```rust
struct FnLowering<'a> {
    module: &'a Module,
    body: &'a Body,
    types: &'a BodyTypes,
    indices: &'a ArenaMap<FunctionId, FnIdx>,
    program: &'a mut ProgramBuilder,
    /// ラムダの関数の名前に使う、トップレベルの関数の名前と、その中のラムダの数 (Task 13)。
    root_name: &'a str,
    lambdas: &'a mut u32,
    exprs: Vec<CExpr>,
    vars: Vec<VarInfo>,
    locals: ArenaMap<LocalId, Atom>,
}
```

`root_name` と `lambdas` は Task 13 で使う。それまでは `#[allow(dead_code)]` を付けておき、Task 13 で外す。

`FnLowering::lower` を次にする。

```rust
    /// ラムダは捕まえた変数を先頭の引数に持つ (docs/spec/core-ir.md)。トップレベルの関数では `captured` は空である。
    fn lower(
        mut self,
        name: &str,
        captured: &[(LocalId, Type)],
        params: &[PatId],
        param_types: &[Type],
        root: ExprId,
    ) -> CoreFn {
        let body = self.body;
        let mut vars = Vec::new();
        for (local, ty) in captured {
            let var = self.new_var(&body.locals[*local].name, ty);
            self.locals.insert(*local, Atom::Var(var));
            vars.push(var);
        }
        for (&pat, ty) in params.iter().zip(param_types) {
            let local = binder(body, pat);
            let name = local.map_or("p", |local| body.locals[local].name.as_str());
            let var = self.new_var(name, ty);
            if let Some(local) = local {
                self.locals.insert(local, Atom::Var(var));
            }
            vars.push(var);
        }
        let root = self.tail(root);
        CoreFn {
            name: name.to_string(),
            params: vars,
            vars: self.vars,
            body: root,
            exprs: self.exprs,
        }
    }
```

`new_var` の `boxed` を次にする。

```rust
            // 関数値と型変数の値は、ヒープのクロージャや文字列かもしれない。インタプリタの `dup` / `decref` は
            // ヒープにない値を無視するので、多めに対象にしても正しく動く (docs/spec/core-ir.md)
            boxed: matches!(ty, Type::String | Type::Fn { .. } | Type::Var(_)),
```

`bind_boxed` を足す。

```rust
    /// 型を見ずに、ヒープの値として変数を作る。クロージャと、関数値を返す途中の結果に使う。
    fn bind_boxed(&mut self, out: &mut Bindings, name: &str, rhs: Rhs) -> Atom {
        self.vars.push(VarInfo {
            name: name.to_string(),
            linearity: Linearity::Unr,
            boxed: true,
        });
        let var = VarId(self.vars.len() as u32 - 1);
        out.push((var, rhs));
        Atom::Var(var)
    }
```

- [ ] **Step 4: 関数値の変換を足す**

`atom` の `ExprKind::Path(Res::Builtin(_))` と `ExprKind::Path(Res::Function(function))` と `ExprKind::Call` の腕を次にする。

```rust
            ExprKind::Path(Res::Builtin(builtin)) => {
                let wrapper = self.program.wrapper(*builtin);
                self.bind_boxed(out, "c", Rhs::MakeClosure(wrapper, Vec::new()))
            }
            // 引数のないトップレベルの値は、参照するたびに呼び出す (docs/spec/core-ir.md)
            ExprKind::Path(Res::Function(function)) => {
                let target = self.indices[*function];
                if self.program.arity(target) == 0 {
                    let ty = self.ty(id);
                    let name = self.module.functions[*function].name.clone();
                    self.bind(out, &name, &ty, Rhs::CallDirect(target, Vec::new()))
                } else {
                    self.bind_boxed(out, "c", Rhs::MakeClosure(target, Vec::new()))
                }
            }
            ExprKind::Call { callee, args } => {
                let ty = self.ty(id);
                match &body.exprs[*callee].kind {
                    ExprKind::Path(Res::Function(function)) => {
                        let args = self.atoms(args, out);
                        let target = self.indices[*function];
                        self.call_known(target, args, &ty, out)
                    }
                    ExprKind::Path(Res::Builtin(builtin)) => {
                        let args = self.atoms(args, out);
                        self.call_builtin(*builtin, args, &ty, out)
                    }
                    _ => {
                        // 呼ばれる式は引数より左にあるので、先に評価する
                        let function = self.atom(*callee, out);
                        let args = self.atoms(args, out);
                        self.bind(out, "t", &ty, Rhs::Apply(function, args))
                    }
                }
            }
```

`impl FnLowering` に足す。

```rust
    fn atoms(&mut self, exprs: &[ExprId], out: &mut Bindings) -> Vec<Atom> {
        exprs.iter().map(|&expr| self.atom(expr, out)).collect()
    }

    /// 呼ぶ相手の引数の個数と比べ、揃えば直接呼び、足りなければクロージャにし、余れば戻った関数値に残りを適用する
    /// (docs/spec/core-ir.md の eval/apply)。
    fn call_known(&mut self, target: FnIdx, mut args: Vec<Atom>, ty: &Type, out: &mut Bindings) -> Atom {
        let arity = self.program.arity(target);
        if args.len() < arity {
            return self.bind_boxed(out, "c", Rhs::MakeClosure(target, args));
        }
        let rest = args.split_off(arity);
        if rest.is_empty() {
            return self.bind(out, "t", ty, Rhs::CallDirect(target, args));
        }
        let function = self.bind_boxed(out, "t", Rhs::CallDirect(target, args));
        self.bind(out, "t", ty, Rhs::Apply(function, rest))
    }

    fn call_builtin(&mut self, builtin: Builtin, mut args: Vec<Atom>, ty: &Type, out: &mut Bindings) -> Atom {
        let arity = builtin_params(builtin).len();
        if args.len() < arity {
            let wrapper = self.program.wrapper(builtin);
            return self.bind_boxed(out, "c", Rhs::MakeClosure(wrapper, args));
        }
        let rest = args.split_off(arity);
        let rhs = match builtin {
            Builtin::Println => Rhs::Perform(IoOp::Println, args),
            Builtin::ComposeFwd | Builtin::ComposeBwd => {
                Rhs::CallDirect(self.program.wrapper(builtin), args)
            }
            other => Rhs::Prim(prim(other), args),
        };
        if rest.is_empty() {
            return self.bind(out, "t", ty, rhs);
        }
        let function = self.bind_boxed(out, "t", rhs);
        self.bind(out, "t", ty, Rhs::Apply(function, rest))
    }
```

`bind_pat` の `PatKind::Annot` の処理は Task 4 のままでよい。`use` に `std::collections::HashMap` (すでにある)、`eml_hir::LocalId`、`crate::{CExprId, Linearity, VarId}` などの不足分を足す。`prim` の `ComposeFwd | ComposeBwd` の腕は残す (包んだ関数の本体が `Apply` なので、`prim` には来ない)。

- [ ] **Step 5: テストが通ることを確かめる**

Run: `cargo test -p eml_core_ir`
Expected: PASS

Run: `cargo test -p eml_cli --test ui`
Expected: `run` が新しい2つのスナップショットを求めて FAIL する。`cargo insta review` で、それぞれ次の出力であること、`--- stderr ---` が空であることを確かめて承認する。

`run/higher_order.em` の出力は次のとおり。

```
2
7
through a row variable
3
```

`run/partial_application.em` の出力は次のとおり。

```
hi, alice
hi, bob
hello, carol
7
15
```

- [ ] **Step 6: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`

```bash
git add crates tests
git commit -m "Lower function values, partial application, and extra arguments to closures"
```

---

### Task 13: ラムダを関数に持ち上げる

ラムダを、捕まえた変数を先頭の引数に持つ新しい `CoreFn` に持ち上げ、ラムダの式は `MakeClosure` にする。

**Files:**
- Modify: `crates/eml_core_ir/src/lower.rs`
- Test: `crates/eml_core_ir/tests/lower.rs`
- Create: `tests/ui/run/closures.em`

**Interfaces:**
- Consumes: Task 12 の `ProgramBuilder`、`FnLowering::lower`、`param_types`、`binder`
- Produces: ラムダの関数の名前は `外側のトップレベルの関数の名前$lambda` + 番号 (トップレベルの関数ごとに 0 から、外側のラムダが先)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/lower.rs` に足す。

```rust
#[test]
fn lambdas_are_lifted_with_their_captures_first() {
    let text = "apply : (a -> <e> b) -> a -> <e> b\napply f x = f x\n\nmain : Unit -> <IO> Unit\nmain () =\n  let s = \"!\"\n  let shout = fn t -> t ++ s\n  println (apply shout \"hi\")\n  println s";
    insta::assert_snapshot!(core_text(text), @r#"
    fn apply(f0, x1) {
      let t2 = apply f0(x1)
      return t2
    }
    fn main(p0) {
      let s1 = const "!"
      dup s1
      let c2 = closure main$lambda0(s1)
      let s3 = const "hi"
      let t4 = call apply(c2, s3)
      let t5 = perform println(t4)
      let t6 = perform println(s1)
      return t6
    }
    fn main$lambda0(s0, t1) {
      let t2 = prim ++(t1, s0)
      return t2
    }
    "#);
}
```

`tests/ui/run/closures.em` を作る。

```haskell
-- Lambdas, closures that capture strings, a closure called twice, a last-argument lambda,
-- an unused polymorphic lambda, and deep recursion through closures.
each : Int -> (Int -> <e> Unit) -> <e> Unit
each n f =
  if n > 0 then
    f n
    each (n - 1) f

count_down : Int -> (Int -> Int) -> Int
count_down n k = if n == 0 then k 0 else count_down (n - 1) (fn m -> k (m + 1))

main : Unit -> <IO> Unit
main () =
  let suffix = "!"
  let shout = fn s -> s ++ suffix
  println (shout "hey")
  println (shout "you")
  each 3 fn n ->
    println ("n = " ++ show_int n)
  let unused = fn x -> x
  println (show_int (count_down 100000 (fn m -> m)))
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_core_ir`
Expected: FAIL (`unreachable!` の `the type checker rejects lambdas until they are lowered`)

- [ ] **Step 3: 実装する**

`crates/eml_core_ir/src/lower.rs` の `atom` の `ExprKind::Lambda { .. }` の腕を次にする。`root_name` と `lambdas` の `#[allow(dead_code)]` を外す。

```rust
            ExprKind::Lambda {
                params,
                body: lambda_body,
            } => {
                let captured: Vec<(LocalId, Type)> = captures(body, id)
                    .into_iter()
                    .map(|local| {
                        let ty = self
                            .types
                            .locals
                            .get(local)
                            .cloned()
                            .expect("every local is typed");
                        (local, ty)
                    })
                    .collect();
                let lambda_ty = self.ty(id);
                let param_types = param_types(&lambda_ty, params.len());
                let function = self.program.reserve(captured.len() + params.len());
                let name = format!("{}$lambda{}", self.root_name, *self.lambdas);
                *self.lambdas += 1;
                let core = FnLowering {
                    module: self.module,
                    body,
                    types: self.types,
                    indices: self.indices,
                    program: &mut *self.program,
                    root_name: self.root_name,
                    lambdas: &mut *self.lambdas,
                    exprs: Vec::new(),
                    vars: Vec::new(),
                    locals: ArenaMap::default(),
                }
                .lower(&name, &captured, params, &param_types, *lambda_body);
                self.program.finish(function, core);
                let atoms = captured.iter().map(|(local, _)| self.locals[*local]).collect();
                self.bind_boxed(out, "c", Rhs::MakeClosure(function, atoms))
            }
```

ファイルに足す。`use std::collections::{BTreeSet, HashMap, HashSet};` にする。

```rust
/// ラムダの本体が参照する、ラムダの外で束縛した局所変数。`LocalId` の順に並べる。入れ子のラムダが捕まえる変数も、
/// 外側のラムダが捕まえる (docs/spec/core-ir.md)。式の木は作業リストでたどる。
fn captures(body: &Body, lambda: ExprId) -> Vec<LocalId> {
    let mut used = BTreeSet::new();
    let mut bound = HashSet::new();
    let mut work = vec![lambda];
    while let Some(id) = work.pop() {
        match &body.exprs[id].kind {
            ExprKind::Path(Res::Local(local)) => {
                used.insert(*local);
            }
            ExprKind::Lambda {
                params,
                body: inner,
            } => {
                for &param in params {
                    bind_locals(body, param, &mut bound);
                }
                work.push(*inner);
            }
            ExprKind::Call { callee, args } => {
                work.push(*callee);
                work.extend(args.iter().copied());
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                work.push(*condition);
                work.push(*then_branch);
                work.extend(*else_branch);
            }
            ExprKind::Block { stmts, tail } => {
                for stmt in stmts {
                    match stmt {
                        Stmt::Let { pat, init, .. } => {
                            bind_locals(body, *pat, &mut bound);
                            work.push(*init);
                        }
                        Stmt::Expr(expr) => work.push(*expr),
                    }
                }
                work.extend(*tail);
            }
            ExprKind::Annot { expr, .. } => work.push(*expr),
            ExprKind::Missing | ExprKind::Literal(_) | ExprKind::Path(_) => {}
        }
    }
    used.into_iter()
        .filter(|local| !bound.contains(local))
        .collect()
}

fn bind_locals(body: &Body, pat: PatId, bound: &mut HashSet<LocalId>) {
    match &body.pats[pat].kind {
        PatKind::Bind(local) => {
            bound.insert(*local);
        }
        PatKind::Annot { pat, .. } => bind_locals(body, *pat, bound),
        PatKind::Wildcard | PatKind::Unit | PatKind::Missing => {}
    }
}
```

- [ ] **Step 4: テストが通ることを確かめる**

Run: `cargo test -p eml_core_ir`
Expected: PASS

Run: `cargo test -p eml_cli --test ui`
Expected: `run/closures.em` の新しいスナップショットを求めて FAIL する。`cargo insta review` で次の出力であること、`--- stderr ---` が空であることを確かめて承認する。

```
hey!
you!
n = 3
n = 2
n = 1
100000
```

- [ ] **Step 5: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`

```bash
git add crates tests
git commit -m "Lift lambdas to functions that take their captured variables first"
```

---

### Task 14: 段階2の診断の UI テストを足す

段階2で新しく出る診断と、段階2でも E0004 のまま残る構文を、`check-fail/` の UI テストで確かめる。

**Files:**
- Create: `tests/ui/check-fail/rigid_type_variable.em`、`tests/ui/check-fail/effect_in_lambda.em`、`tests/ui/check-fail/unknown_type_variable.em`、`tests/ui/check-fail/later_stage_lambda_syntax.em`

**Interfaces:**
- Consumes: Task 7〜9 の診断

- [ ] **Step 1: テストのソースを書く**

`tests/ui/check-fail/rigid_type_variable.em` を作る。

```haskell
-- E2001: a signature type variable is rigid in the body.
first : a -> b -> a
first x y = y

main : Unit -> <IO> Unit
main () = ()
```

`tests/ui/check-fail/effect_in_lambda.em` を作る。

```haskell
-- E2002: a lambda performs an effect that its expected type does not allow.
each : (String -> Unit) -> Unit
each f = f "a"

main : Unit -> <IO> Unit
main () = each (fn s -> println s)
```

`tests/ui/check-fail/unknown_type_variable.em` を作る。

```haskell
-- E1002: an annotation in the body names a type variable that is not in the signature.
id : a -> a
id x = (x : b)

main : Unit -> <IO> Unit
main () = ()
```

`tests/ui/check-fail/later_stage_lambda_syntax.em` を作る。

```haskell
-- E0004: lambda parameters and expressions that later stages implement.
main : Unit -> <IO> Unit
main () =
  let swap = fn (a, b) -> (b, a)
  let plus = (+)
  ()
```

- [ ] **Step 2: スナップショットを確かめて承認する**

Run: `cargo test -p eml_cli --test ui`
Expected: 4つの新しいスナップショットを求めて FAIL する。`cargo insta review` で次を確かめて承認する。

- `rigid_type_variable.em`: E2001 が 3行13列の `y` を指し、`expected `a`, found `b`` と、シグネチャを指す副ラベルがある
- `effect_in_lambda.em`: E2002 が 6行25列の `println s` を指し、`which this lambda does not allow` と、6行11列の `each` を指す `argument 1 of `each` does not allow it` がある
- `unknown_type_variable.em`: E1002 が 3行13列の `b` を指し、`cannot find type variable `b`` である。型の不一致が連鎖していない
- `later_stage_lambda_syntax.em`: E0004 が、タプルのパターン (4行17列)、タプル (4行27列)、演算子の参照 (5行14列) の3つある。それ以外の診断がない

位置が上と違えば、まず実装を疑い、ソースの列を数え直してから判断する。

- [ ] **Step 3: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`

```bash
git add tests crates/eml_cli/tests/snapshots
git commit -m "Add UI tests for the diagnostics of the second vertical slice"
```

---

### Task 15: 文書を更新し、作業用の文書を消す

段階2を完了として記録し、残す価値のある設計を `architecture.md` に移す。前の段階と同じく、作業用の設計文書と計画は削除し、内容は git の履歴で参照する。

**Files:**
- Modify: `docs/implementation/status.md`、`docs/implementation/architecture.md`
- Delete: `docs/superpowers/specs/2026-10-04-vertical-slice-2-design.md`、`docs/superpowers/plans/2026-10-04-vertical-slice-2.md`

- [ ] **Step 1: yomiyasu を読む**

文書を書く前に `yomiyasu:yomiyasu` スキルを読み、その規則に従って書く (CLAUDE.md)。

- [ ] **Step 2: status.md を更新する**

`docs/implementation/status.md` を次のように変える。

- 冒頭の日付を作業した日にする。
- 「名前解決以降の実装段階」の表の段階2を「完了」にする。
- 「各 crate の実装状況」の `eml_hir`、`eml_types`、`eml_core_ir`、`eml_runtime`、`eml_interp` の行を「段階2まで実装済み」にし、足したもの (ラムダ、型変数と row 変数の表、スキームと SCC ごとの Kind の推論、使用回数のパス、クロージャ変換、eval/apply、クロージャのオブジェクト) を書き足す。
- 「次の作業の注意点」から「段階2で直す `eml_types` の誤り」の項目を消す。「段階3の注意」の項目の `Payload` / `Frame` の説明に、`Closure` の `args` と `ApplyFrame` の `args` も `ObjRef` を `dup` せずに複製されることを足す。
- 「次の作業の注意点」に次の3項目を足す。
  - 段階3: row 変数の多重度 `σ` は、スキームの多相化と具体化で制約を複製しているが、段階2では上限の制約が出ないので確かめていない。`multi` と持ち越し規則で上限が出たときにテストを足す
  - 段階5: Kind の制約の違反は、段階2では `debug_assert` だけで、診断にしていない (`eml_types::check` の `solve_kinds` の後)。`Lin` の型が現れたら E3xxx の番号を決めて診断にする
  - 段階5: 使用回数のパス (`eml_types::usage`) は、どの経路でも1回でない変数に `Unr` の制約を出すだけである。枝ごとの消費の一致、持ち越し規則、E3xxx はこのパスに足す
- 「完了した作業」の表に、段階2の行を足す (ラムダ、クロージャ、部分適用と関数値、型変数と row 変数の多相、SCC ごとの Kind の推論、`>>` / `<<`、eval/apply のクロージャ)。

- [ ] **Step 3: architecture.md を更新する**

`docs/implementation/architecture.md` を次のように変える。

- 「`eml_hir` の内部」に、型変数と row 変数の表 (`Function::type_vars` / `row_vars`。シグネチャで定義し、本体の注釈は引くだけ) と、ラムダの引数のスコープを足す。
- 「`eml_types` の内部」の、関数値を E0004 にする項目を消し、次を足す。
  - シグネチャはスキーム (`scheme.rs`) で持つ。型変数と row 変数は rigid で、参照するたびに具体化し、戻り値の側の閉じた row を開く
  - 呼び出しは、たどった矢印の row を今の row に含める (`Table::include_row`)。rigid な末尾は、残りの末尾が同じ変数であることを確かめる
  - 呼び出しグラフの SCC (`scc.rs`) を呼ばれる側から検査し、SCC ごとに使用回数のパス (`usage.rs`) で `Unr` の制約を出してから、スキームの Kind 変数を多相化する。残す制約は、内部の変数を経由した推移も含めて求める (`Lattice::residual`)
  - 部分適用のクロージャの線形性は、それまでの引数と捕まえた値の Kind 以上になる (`Table::closure_kinds`)
  - `dump` は、スキームに残った Kind の制約のうち定数を片側に持つものを `kinds:` の行に出す
- 「`eml_core_ir`、`eml_runtime`、`eml_interp` の内部」に次を足す。
  - ラムダは、捕まえた変数を先頭の引数に持つ関数に持ち上げる (`外側の名前$lambdaN`)。組み込みを値として使うときは、呼ぶだけの関数 (`builtin$名前`) で包む。関数の表は番号を先に取り、変換の途中で関数を足す
  - クロージャは `Payload::Closure { function, args }` で、関数値の呼び出し `Apply` は eval/apply で行う。余った引数は `Payload::ApplyFrame` として継続に積む。設計文書ではフレームの種類の enum にする案だったが、既存の `Frame` を変えずに済むので、ペイロードの別の種類にした
  - 共有されたクロージャを呼ぶときは、捕まえた値の参照を複製してからクロージャを手放す

- [ ] **Step 4: 作業用の文書を消す**

```bash
git rm docs/superpowers/specs/2026-10-04-vertical-slice-2-design.md docs/superpowers/plans/2026-10-04-vertical-slice-2.md
```

- [ ] **Step 5: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`

```bash
git add docs
git commit -m "Record the second vertical slice as done and move its design into architecture.md"
```
