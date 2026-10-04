# リファクタリング R2a: 型検査器の内部 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `eml_types` の `table.rs` と `check.rs` を分けて名前を直し、row の末尾に `Error` を置き、表示のために Kind の束を解かないようにし、E2002 の副ラベルを矢印に向ける。

**Architecture:** 先に改名とファイルの分割 (処理を変えない移動) を済ませ、その上で `display` / `export` の分離と `Tail::Error` を入れる。`check.rs` も分割してから、重複の解消と E2002 の変更を入れる。

**Tech Stack:** Rust (edition 2024)、insta

**Spec:** `docs/superpowers/specs/2026-10-04-refactor-r2a-type-checker-design.md`

## Global Constraints

- 期待値は、このプランで名前を挙げたテストだけを変える。期待値を変えない機械的な追随は許す (docs/implementation/testing.md の「テストの変更の運用」)
- 名前を挙げたのは、`crates/eml_types/tests/check.rs` の `main_with_an_erroneous_row_is_not_reported_again` と `an_undefined_effect_row_is_fresh_at_each_call` の2件 (種類2、Task 4) だけである。`table.rs` の単体テストの書き換えは種類3 (期待値を変えない)
- 名前を挙げていないテストの期待値が変わったら、変えずに止まる。差分と理由をユーザーに示し、承認を得てから期待値を変え、`testing.md` に記録する
- テストを変えないことを理由に設計を曲げない
- コードのコメントと `docs/` の文書は日本語で書き、`yomiyasu:yomiyasu` スキルの規則に従う。コメントは「なぜ」を書き、規則を指すときは `docs/` のパスを引く
- `git diff` には、つねに `--no-ext-diff` を付ける
- 各タスクの終わりに `cargo test`、`cargo clippy --all-targets` (警告0件)、`cargo fmt --check` を通す
- コミットメッセージの末尾に次の2行を付ける

```
Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014mCDZTcwb5EYZfQ1MpvtHn
```

## Review Focus

- 今の row の末尾が `Error` のときに、rigid な row 変数を末尾に持つ呼び出し先を含めると、`include_row` が `MissingRowVar` を誤って返すこと (Task 4 の `an_error_row_is_included_and_includes_any_row` の3つ目のアサーション)
- `display` に置き換えた診断の文言が、線形性の違いで変わらないこと。とくに `check_main` の型の比較 (Task 3 で既存の UI テストと型のテストがすべて通ることで確かめる)
- `export` を `solve_kinds` の前に呼んでいる経路が残っていないこと (Task 3 の `debug_assert` と、全テストが debug ビルドで通ること)
- `if` と `ブロック` の check と infer を1つにしたときに、期待する型の由来 (`Origin`) が変わって診断の文言がずれること (Task 6 で既存のテストが変わらないことで確かめる)
- E2002 の副ラベルを矢印に向けたことで、引数が1つの関数の既存の診断が変わること (Task 7 で既存のテストが変わらないことで確かめる)

---

### Task 1: 改名

**Files:**
- Modify: `crates/eml_types/src/table.rs`、`crates/eml_types/src/check.rs`、`crates/eml_types/src/scheme.rs`、`crates/eml_types/src/builtins.rs`

**Interfaces:**
- Consumes: なし
- Produces: `TyShape` (旧 `TyKind`)、`Table::shape` (旧 `Table::kind`)、`ArrowLin` (旧 `Mult`)、`Table::fresh_arrow_lin` (旧 `fresh_mult`)、`Table::fresh_lin_var` (旧 `fresh_lin_kind`)、`Table::fresh_mult_var` (旧 `fresh_mult_kind`)、`Table::unify_arrow_lin` (旧 `unify_mult`)。`Table` のフィールド `kinds` は `shapes` になる

振る舞いは変えない。テストは書き換えない (`table.rs` の単体テストは同じファイルの中で改名に追随する。種類3)。

- [ ] **Step 1: 改名する**

`crates/eml_types/src/` の4つのファイルで、次の順に置き換える。どれも単語の境界で置き換え、`Multiplicity` や `TypeRefKind` などの別の名前に当てないようにする。

| 置き換え前 | 置き換え後 | 対象 |
|---|---|---|
| `fresh_mult_kind` | `fresh_mult_var` | 4つのファイル |
| `fresh_lin_kind` | `fresh_lin_var` | 4つのファイル |
| `fresh_mult(` | `fresh_arrow_lin(` | 4つのファイル |
| `unify_mult` | `unify_arrow_lin` | `table.rs` |
| `TyKind` (単語) | `TyShape` | 4つのファイル |
| `Mult` (単語) | `ArrowLin` | 4つのファイル |
| `.kind(` | `.shape(` | `table.rs`、`check.rs` |
| `pub fn kind(&self, ty: Ty)` | `pub fn shape(&self, ty: Ty)` | `table.rs` |
| `self.kinds` | `self.shapes` | `table.rs` |
| `kinds: Vec<TyShape>` と `kinds: Vec::new()` (`Table` の定義と `Table::new`) | `shapes: Vec<TyShape>` と `shapes: Vec::new()` | `table.rs` |

`table.rs` の `ArrowLin` の doc コメントは「関数型の矢印の線形性 `m`。多重度 (`Multiplicity`) と取り違えないよう、名前に矢印を入れる。内部では最初から Kind 変数を扱う (docs/spec/types.md)。」にする。

- [ ] **Step 2: 確かめてコミットする**

Run: `grep -rnw "TyKind\|Mult\|fresh_mult\|fresh_mult_kind\|fresh_lin_kind\|unify_mult" crates/eml_types/src; grep -rn "\.kind(" crates/eml_types/src`
Expected: 何も出ない

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る。`git diff --no-ext-diff --stat -- crates/eml_types/tests '*.snap'` は何も出さない

```bash
git add crates/eml_types
git commit -m "Rename the type table's shape and arrow-linearity names"
```

---

### Task 2: `table.rs` を `table/` に分ける

**Files:**
- Move: `crates/eml_types/src/table.rs` → `crates/eml_types/src/table/mod.rs`
- Create: `crates/eml_types/src/table/unify.rs`、`row.rs`、`kinds.rs`、`copy.rs`、`export.rs`、`tests.rs`

**Interfaces:**
- Consumes: Task 1 の名前
- Produces: 公開している名前と関数の形は変えない (`crate::table::{Table, Ty, Row, ...}` のまま使える)

処理は変えない移動である。

- [ ] **Step 1: 移す**

`git mv crates/eml_types/src/table.rs crates/eml_types/src/table/mod.rs` で移す。`table/mod.rs` に次を置き、関数を各ファイルに移す。各ファイルは `use super::*;` で `mod.rs` の型と `use` を受け取り、`impl Table { ... }` のブロックにメソッドを入れる。doc コメントとコメントは一緒に移す。

```rust
mod copy;
mod export;
mod kinds;
mod row;
#[cfg(test)]
mod tests;
mod unify;
```

| ファイル | 移すもの |
|---|---|
| `table/mod.rs` (残す) | 型と変数の ID、`TyCon`、`ArrowLin`、`Row` と `impl Row`、`TyShape`、`UnifyError`、`TyVarInfo`、`RowVarInfo`、`RigidInfo`、`Table` の構造体、`Table::new`、`alloc`、`function`、`function_with`、`fresh_*` のすべて、`rigid_linearity`、`fresh_rigid_row`、`is_rigid_row`、`row_multiplicity_var`、`resolve`、`shape`、`row_multiplicity` |
| `table/unify.rs` | `unify`、`bind_var`、`occurs`、`unify_arrow_lin` |
| `table/row.rs` | `resolve_row`、`unify_row`、`bind_row`、`include_row`、`open_spine` |
| `table/kinds.rs` | `kind_bounds`、`kind_at_most`、`closure_kinds`、`kind_vars`、`lin_residual`、`mult_residual`、`copy_lin_constraints`、`copy_mult_constraints`、`solve_kinds`、自由関数 `push_unique` |
| `table/copy.rs` | `Subst` の構造体 (`mod.rs` から移し、`pub(crate) use copy::Subst;` で再公開する)、`copy_type` |
| `table/export.rs` | `kind_names`、`export` |
| `table/tests.rs` | `#[cfg(test)] mod tests { ... }` の中身。先頭は `use super::*;` |

移した先で使わない `use` は置かない。`bind_var` から `kind_bounds` を、`unify_row` から `bind_row` を呼ぶように、ファイルをまたぐ呼び出しは、`impl Table` のメソッドなので今のまま呼べる。`pub` でないメソッド (`resolve`、`bind_var`、`occurs`、`bind_row`) は、同じ `table` モジュールの子どうしから呼べる。

- [ ] **Step 2: 確かめてコミットする**

Run: `wc -l crates/eml_types/src/table/*.rs`
Expected: 7つのファイルがある。`mod.rs` は400行を下回る

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る。テストの数は Task 1 の後と同じ

```bash
git add -A crates/eml_types/src
git commit -m "Split the type table into unification, rows, kinds, copying, and export"
```

---

### Task 3: `display` と `export` を分ける

**Files:**
- Modify: `crates/eml_types/src/table/export.rs`、`crates/eml_types/src/table/tests.rs`、`crates/eml_types/src/check.rs`

**Interfaces:**
- Consumes: Task 2 の `table/export.rs`
- Produces: `Table::display(&self, ty: Ty) -> Type` (いつでも呼べる。矢印の線形性は `Unr`)、`Table::export(&self, ty: Ty) -> Type` (`solve_kinds` の後にだけ呼ぶ)

`table/tests.rs` の `export` を `display` に置き換えるのは種類3 (どのテストも線形性を表示しない文字列か、線形性を含まない型を比べている)。

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/src/table/tests.rs` の末尾に足す。

```rust
#[test]
#[should_panic(expected = "export is for after solve_kinds")]
fn export_needs_solved_kinds() {
    let table = Table::new();
    table.export(table.int);
}

#[test]
fn display_does_not_solve_kinds() {
    let mut table = Table::new();
    let lin = table.fresh_arrow_lin();
    let f = table.function_with(table.int, lin, Row::pure(), table.int);
    assert_eq!(table.display(f).to_string(), "Int -> Int");
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --lib`
Expected: コンパイルエラー (`no method named display`)

- [ ] **Step 3: 実装する**

`crates/eml_types/src/table/export.rs` の `export` を、次の3つの関数にする。今の `export` の本体は `to_type` に移し、矢印の線形性の決め方だけを変える。

```rust
    /// 後の段階に渡す形にする。矢印の線形性は解いた結果を使うので、`solve_kinds` の後にだけ呼ぶ。解けていない
    /// 型変数と row 変数は、`_` として残す。
    pub fn export(&self, ty: Ty) -> Type {
        debug_assert!(
            self.lin_solution.is_some(),
            "export is for after solve_kinds; use display while checking"
        );
        self.to_type(ty, true)
    }

    /// 診断の文言のための形。型の表示は線形性を出さないので、Kind の束を解かず、矢印の線形性はすべて `Unr` にする。
    pub fn display(&self, ty: Ty) -> Type {
        self.to_type(ty, false)
    }

    fn to_type(&self, ty: Ty, solved: bool) -> Type {
        // 今の `export` の本体。再帰の呼び出しは `self.to_type(..., solved)` にする
    }
```

`to_type` の中の矢印の線形性は次にする (`None => self.linearity.value(v)` の経路を消す)。

```rust
                    linearity: match lin {
                        ArrowLin::Known(l) => l,
                        ArrowLin::Var(_) if !solved => Linearity::Unr,
                        ArrowLin::Var(v) => {
                            let solution = self
                                .lin_solution
                                .as_ref()
                                .expect("export runs after solve_kinds");
                            // 解いた後に Kind 変数を作ると解が古くなる。`export` は解いた後に変数を作らない前提である
                            debug_assert!(v.index() < solution.len());
                            solution[v.index()]
                        }
                    },
```

`kind_names` の中の `self.export(ty)` を `self.display(ty)` にする (名前は表示にだけ使う)。

`crates/eml_types/src/check.rs` の次の4か所の `export` を `display` にする。

- `check_main` の `let found = table.export(scheme.ty);`
- `check_lambda` の `let expected_ty = self.table.export(expected);`
- `mismatch` の `let expected = self.table.export(expected);` と `let found = self.table.export(found);`

`check_module` の中で `TypedModule` を組み立てる `table.export(...)` の3か所は `export` のままにする (`solve_kinds` の後である)。

`crates/eml_types/src/table/tests.rs` の中の `table.export(` を、すべて `table.display(` にする (Step 1 で足した `export_needs_solved_kinds` は除く)。

- [ ] **Step 4: テストが通ることを確かめる**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED"`
Expected: 何も出ない

Run: `grep -n "export(" crates/eml_types/src/check.rs`
Expected: `check_module` の3か所だけが出る

- [ ] **Step 5: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る。`git diff --no-ext-diff --stat -- crates/eml_types/tests '*.snap'` は何も出さない

```bash
git add crates/eml_types
git commit -m "Display types for diagnostics without solving kinds and export only after solving"
```

---

### Task 4: row の末尾の `Error`

**Files:**
- Modify: `crates/eml_types/src/table/mod.rs` (`Row`、`Tail`)、`row.rs`、`copy.rs`、`kinds.rs`、`export.rs`、`tests.rs`
- Modify: `crates/eml_types/src/ty.rs` (`RowTail`)
- Modify: `crates/eml_types/src/scheme.rs`、`crates/eml_types/src/builtins.rs`、`crates/eml_types/src/check.rs`
- Modify: `crates/eml_types/tests/check.rs`

**Interfaces:**
- Consumes: Task 3 の `display`
- Produces: `table::Tail` (`Closed`、`Var(RowVar)`、`Error`。`Debug`、`Clone`、`Copy`、`PartialEq`、`Eq`)、`Row { labels: Vec<Effect>, tail: Tail }`、`Row::error() -> Row`、`eml_types::RowTail::Error` (`{error}` と表示)。`Rigids::error_rows`、`Scheme::error_rows` は無くなる

このタスクで期待値を変えるテスト (種類2): `tests/check.rs` の `main_with_an_erroneous_row_is_not_reported_again` と `an_undefined_effect_row_is_fresh_at_each_call`。`table/tests.rs` の `Row { tail: Some(..) }` を `Tail::Var(..)` にする書き換えは種類3。

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/check.rs` の末尾に足す。

```rust
#[test]
fn an_undefined_effect_row_does_not_pass_the_body_effects_to_callers() {
    // 未定義のエフェクトの row はどのエフェクトも受け入れるが、本体のエフェクトを取り込んで呼び出し側に伝えない
    // (docs/spec/types.md の「エラーの扱い」)
    let text = "g : Unit -> <Console> Unit\ng () = println \"x\"\n\nh : Unit -> Unit\nh () = g ()";
    insta::assert_snapshot!(check_text(text), @"
    g : Unit -> <{error}> Unit
    h : Unit -> Unit
    ---
    E1002 1:14 cannot find effect `Console`
      1:14 not found in this scope
    ");
}
```

`main_with_an_erroneous_row_is_not_reported_again` の期待値の `main : Unit -> <_> Unit` を `main : Unit -> <{error}> Unit` にする。`an_undefined_effect_row_is_fresh_at_each_call` の期待値の `run : (Unit -> <_> Unit) -> <_> Unit` を `run : (Unit -> <{error}> Unit) -> <{error}> Unit` に、`f#0 : Unit -> <_> Unit` を `f#0 : Unit -> <{error}> Unit` にする。ほかの行は変えない。

`crates/eml_types/src/table/tests.rs` の末尾に足す。

```rust
#[test]
fn an_error_row_unifies_with_any_row_without_binding() {
    let mut table = Table::new();
    let e = table.fresh_rigid_row("e");
    assert_eq!(
        table.unify_row(&Row::error(), &Row::closed(vec![Effect::Io])),
        Ok(())
    );
    let rigid = Row {
        labels: vec![Effect::Io],
        tail: Tail::Var(e),
    };
    assert_eq!(table.unify_row(&rigid, &Row::error()), Ok(()));
    assert_eq!(table.resolve_row(&rigid), rigid);
}

#[test]
fn a_flexible_row_unified_with_an_error_row_becomes_an_error_row() {
    let mut table = Table::new();
    let r = table.fresh_row_var();
    let open = Row {
        labels: vec![],
        tail: Tail::Var(r),
    };
    let error = Row {
        labels: vec![Effect::Io],
        tail: Tail::Error,
    };
    assert_eq!(table.unify_row(&open, &error), Ok(()));
    assert_eq!(table.resolve_row(&open), error);
}

#[test]
fn an_error_row_is_included_and_includes_any_row() {
    let mut table = Table::new();
    assert_eq!(table.include_row(&Row::error(), &Row::pure()), Ok(()));
    assert_eq!(
        table.include_row(&Row::closed(vec![Effect::Io]), &Row::error()),
        Ok(())
    );
    let e = table.fresh_rigid_row("e");
    let rigid = Row {
        labels: vec![],
        tail: Tail::Var(e),
    };
    assert_eq!(table.include_row(&rigid, &Row::error()), Ok(()));
}

#[test]
fn copying_keeps_an_error_row() {
    let mut table = Table::new();
    let f = table.function(table.int, Row::error(), table.int);
    let copied = table.copy_type(f, &Subst::default());
    assert_eq!(table.display(copied).to_string(), "Int -> <{error}> Int");
}
```

`table/tests.rs` の既存のテストの `tail: Some(x)` をすべて `tail: Tail::Var(x)` に、`tail: None` を `tail: Tail::Closed` にする。比べている `a.tail` と `b.tail` は、型が `Tail` になるだけで、比べ方は変えない。

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types`
Expected: コンパイルエラー (`cannot find type Tail`、`no function or associated item named error`)

- [ ] **Step 3: `Tail` を入れる**

`crates/eml_types/src/table/mod.rs` の `Row` を次にする。

```rust
/// row の末尾。`Error` は未定義のエフェクトか解決できない row 変数の跡で、型の `Error` と同じく、どのエフェクトも
/// 受け入れて束縛されない (docs/spec/types.md の「エラーの扱い」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tail {
    Closed,
    Var(RowVar),
    Error,
}

/// エフェクトの row。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub labels: Vec<Effect>,
    pub tail: Tail,
}

impl Row {
    pub fn pure() -> Row {
        Row::closed(Vec::new())
    }

    pub fn closed(labels: Vec<Effect>) -> Row {
        Row {
            labels,
            tail: Tail::Closed,
        }
    }

    pub fn error() -> Row {
        Row {
            labels: Vec::new(),
            tail: Tail::Error,
        }
    }
}
```

`table/row.rs`:

- `resolve_row` のループを `while let Tail::Var(var) = tail { ... }` にする。
- `unify_row` の `match (a.tail, b.tail)` の先頭に次の3つの腕を足し、残りの腕は `None` を `Tail::Closed` に、`Some(x)` を `Tail::Var(x)` に書き換える。新しい row 変数で受ける腕の `tail: Some(rest)` などは `tail: Tail::Var(rest)` にする。

```rust
            // `Error` が関わる row の制約からは診断を出さない (docs/spec/types.md の「エラーの扱い」)。推論用の row 変数は、
            // 型の `unify(Var, Error)` と同じく、相手にしかないラベルと末尾 `Error` に束縛する
            (Tail::Error, Tail::Var(y)) if !self.is_rigid_row(y) => self.bind_row(
                y,
                Row {
                    labels: only_a,
                    tail: Tail::Error,
                },
            ),
            (Tail::Var(x), Tail::Error) if !self.is_rigid_row(x) => self.bind_row(
                x,
                Row {
                    labels: only_b,
                    tail: Tail::Error,
                },
            ),
            (Tail::Error, _) | (_, Tail::Error) => Ok(()),
```

- `bind_row` の occurs の検査を `if row.tail == Tail::Var(var)` に、末尾の多重度の制約を `if let Tail::Var(tail) = row.tail { ... }` にする。
- `include_row` を次にする (doc コメントの最後に「どちらかの末尾が `Error` なら含まれるとみなす。」を足す)。

```rust
    pub fn include_row(&mut self, callee: &Row, ambient: &Row) -> Result<(), UnifyError> {
        let callee = self.resolve_row(callee);
        match callee.tail {
            Tail::Error => Ok(()),
            Tail::Var(tail) if !self.is_rigid_row(tail) => self.unify_row(&callee, ambient),
            tail => {
                let rest = self.fresh_row_var();
                self.unify_row(
                    &Row {
                        labels: callee.labels,
                        tail: Tail::Var(rest),
                    },
                    ambient,
                )?;
                let Tail::Var(rigid) = tail else {
                    return Ok(());
                };
                let rest = self.resolve_row(&Row {
                    labels: Vec::new(),
                    tail: Tail::Var(rest),
                });
                match rest.tail {
                    Tail::Var(open) if open == rigid => Ok(()),
                    // 今の row の末尾が `Error` なら、rigid な変数も受け入れる
                    Tail::Error => Ok(()),
                    Tail::Var(open) if !self.is_rigid_row(open) => self.bind_row(
                        open,
                        Row {
                            labels: Vec::new(),
                            tail: Tail::Var(rigid),
                        },
                    ),
                    _ => {
                        let name = self.row_vars[rigid.0 as usize].rigid.clone();
                        Err(UnifyError::MissingRowVar(name.unwrap_or_default()))
                    }
                }
            }
        }
    }
```

- `open_spine` の `match row.tail` を、`Tail::Closed` なら `Tail::Var(self.fresh_row_var())` にし、それ以外はそのまま使う形にする。

`table/copy.rs` の `copy_type` の row の末尾を次にする。

```rust
                let tail = match row.tail {
                    Tail::Var(tail) => Tail::Var(subst.rows.get(&tail).copied().unwrap_or(tail)),
                    other => other,
                };
```

`table/kinds.rs` の `kind_vars` の条件を `if let Tail::Var(tail) = self.resolve_row(row).tail && self.is_rigid_row(tail)` にする。

`table/export.rs` の `to_type` の `tail` を次にする。

```rust
                    tail: match row.tail {
                        Tail::Closed => None,
                        Tail::Var(tail) => Some(match &self.row_vars[tail.0 as usize].rigid {
                            Some(name) => RowTail::Rigid(name.clone()),
                            None => RowTail::Flexible,
                        }),
                        Tail::Error => Some(RowTail::Error),
                    },
```

`crates/eml_types/src/ty.rs`:

- `RowTail` に `Error` を足す。doc コメントは「未定義のエフェクトか解決できない row 変数の跡。どのエフェクトも受け入れる。」にする。
- `Display for Type` の末尾の名前の対応に `Some(RowTail::Error) => Some("{error}"),` を足す。
- `contains_error` の `Type::Fn` の腕を `param.contains_error() || ret.contains_error() || matches!(tail, Some(RowTail::Error))` にする (腕で `tail` を受け取る)。

`crates/eml_types/src/scheme.rs`:

- `Rigids` と `Scheme` から `error_rows` のフィールドと、その初期化を消す。
- `instantiate` の中の「エラーの跡の row は…」のコメントから始まる `for &row in &self.error_rows { ... }` のループを消す。
- `lower_signature` を `pub(crate) fn lower_signature(table: &mut Table, function: &Function, rigids: &Rigids, id: TypeRefId) -> Ty { lower(table, function, rigids, id, true) }` にし、doc コメントの「エラーの跡の row 変数は、具体化のたびに新しくするために記録する。」を消す。`lower_type` も `lower(table, function, rigids, id, false)` にし、doc コメントの「エラーの跡の row 変数は記録しない。」を消す。`lower` から `error_rows` の引数を消す。
- `lower` の `RowRef::Open` の腕を `tail: Tail::Var(rigids.rows[*tail])` に、`RowRef::Error` の腕を `RowRef::Error => Row::error(),` にする (コメント「未定義のエフェクトか、解決できない row 変数の跡。どのエフェクトも受け入れて、診断を連鎖させない」は残す)。

`crates/eml_types/src/check.rs`:

- `lower_signature(&mut table, function, &mut function_rigids, signature.ty)` を `lower_signature(&mut table, function, &function_rigids, signature.ty)` にする。
- `fresh_function` の row を `Row { labels: Vec::new(), tail: Tail::Var(self.table.fresh_row_var()) }` にする。
- `check_lambda` の `None => Row { labels: Vec::new(), tail: Some(self.table.fresh_row_var()) },` を `None => Row::error(),` にする。
- `use crate::table::{...}` に `Tail` を足す。

`crates/eml_types/src/builtins.rs` の `tail: Some(e)` を `tail: Tail::Var(e)` にし、`use` に `Tail` を足す。

- [ ] **Step 4: テストが通り、期待値が名前を挙げたものだけ変わったことを確かめる**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED"`
Expected: 何も出ない

Run: `grep -rn "error_rows" crates/eml_types/src`
Expected: 何も出ない

Run: `git diff --no-ext-diff -U0 -- crates/eml_types/tests/check.rs | grep '^-' | grep -v '^---'`
Expected: 消えた行は、`main : Unit -> <_> Unit`、`run : (Unit -> <_> Unit) -> <_> Unit`、`f#0 : Unit -> <_> Unit` の3行だけである

Run: `git diff --no-ext-diff --stat -- '*.snap'`
Expected: 何も出ない

- [ ] **Step 5: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates/eml_types
git commit -m "Give erroneous rows an Error tail instead of threading error row variables"
```

---

### Task 5: `check.rs` を `check/` に分ける

**Files:**
- Move: `crates/eml_types/src/check.rs` → `crates/eml_types/src/check/mod.rs`
- Create: `crates/eml_types/src/check/body.rs`、`crates/eml_types/src/check/report.rs`

**Interfaces:**
- Consumes: Task 4 の `check.rs`
- Produces: `crate::check::check_module` は今のまま。`BodyCheck` のフィールドは `pub(super)` になる

処理は変えない移動である。

- [ ] **Step 1: 移す**

`git mv crates/eml_types/src/check.rs crates/eml_types/src/check/mod.rs` で移す。`check/mod.rs` に `mod body;` と `mod report;` を足し、次のとおりに分ける。doc コメントとコメントは一緒に移す。

| ファイル | 中身 |
|---|---|
| `check/mod.rs` (残す) | `check_module`、`check_main`、`has_error`、`kind_constraints`、`#[cfg(test)] mod tests` (`counts_are_pluralized`。`count` を `report::count` として呼ぶ) |
| `check/body.rs` | `BodyTyping`、`BodyCheck` の構造体 (フィールドをすべて `pub(super)` にする) と、`file`、`signature_range`、`check_function`、`check_expr`、`infer_expr`、`stmts`、`reference`、`value`、`fresh_function`、`call`、`check_lambda`、`bind_param`、`bind_pat`、`expect` |
| `check/report.rs` | `Origin`、`AmbientSource` (どちらも `pub(super)`)。`impl BodyCheck<'_>` のブロックに `perform` と `mismatch` を移す。自由関数 `callee_subject` と `count` (どちらも `pub(super)`) |

`check_module` は `body::BodyCheck` と `body::BodyTyping` を使う。`usage.rs` が `crate::check::BodyTyping` を使っていれば、`check/mod.rs` で `pub(crate) use body::BodyTyping;` と再公開する。

- [ ] **Step 2: 確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る。テストの数は変わらない

```bash
git add -A crates/eml_types/src
git commit -m "Split the checker into the module driver, body checking, and diagnostics"
```

---

### Task 6: `check/` の重複をなくす

**Files:**
- Modify: `crates/eml_types/src/check/body.rs`、`crates/eml_types/src/check/report.rs`

**Interfaces:**
- Consumes: Task 5 の `check/`
- Produces: `Expectation` (`Has(Ty, Origin)`、`None`)、`BodyCheck::if_expr`、`BodyCheck::block`、`Arrow` (`Fn { param, row, ret }`、`Error`、`NotFunction`)、`BodyCheck::next_arrow`、`BodyCheck::with_ambient`、`BodyCheck::include_call_row` (旧 `perform`)。`report.rs` に `signature_arity_error`、`lambda_arity_error`、`call_arity_error` (どれも `BodyCheck` のメソッドで `Diagnostic` を返す)

振る舞いは変えない。既存のテストが変わらないことで確かめる。

- [ ] **Step 1: `if` とブロックを1つにする**

`check/body.rs` に足す。

```rust
/// 式に期待する型。check と infer で `if` とブロックの処理を共有するため。
pub(super) enum Expectation {
    Has(Ty, Origin),
    None,
}
```

`impl BodyCheck` に足す。

```rust
    /// 期待する型があれば、両方の枝をその型で検査する。なければ then 節の型を推論し、else 節をそれに合わせる。
    /// `else` がなければ then 節は `Unit` で、式の型も `Unit` になる (docs/spec/expressions.md の「if」)。
    fn if_expr(
        &mut self,
        range: TextRange,
        condition: ExprId,
        then_branch: ExprId,
        else_branch: Option<ExprId>,
        expectation: Expectation,
    ) -> Ty {
        let bool = self.table.bool;
        self.check_expr(condition, bool, Origin::IfCondition);
        match (else_branch, expectation) {
            (Some(else_branch), Expectation::Has(expected, origin)) => {
                self.check_expr(then_branch, expected, origin.clone());
                self.check_expr(else_branch, expected, origin);
                expected
            }
            (Some(else_branch), Expectation::None) => {
                let ty = self.infer_expr(then_branch);
                let then_range = self.body.exprs[then_branch].range;
                self.check_expr(else_branch, ty, Origin::IfBranches(then_range));
                ty
            }
            (None, expectation) => {
                let unit = self.table.unit;
                self.check_expr(then_branch, unit, Origin::IfWithoutElse);
                match expectation {
                    Expectation::Has(expected, origin) => {
                        self.expect(range, expected, unit, &origin);
                        expected
                    }
                    Expectation::None => unit,
                }
            }
        }
    }

    /// 最後の文が `let` なら、ブロックの値は `()` である (docs/spec/expressions.md)。
    fn block(
        &mut self,
        range: TextRange,
        stmts: &[Stmt],
        tail: Option<ExprId>,
        expectation: Expectation,
    ) -> Ty {
        self.stmts(stmts);
        match (tail, expectation) {
            (Some(tail), Expectation::Has(expected, origin)) => {
                self.check_expr(tail, expected, origin);
                expected
            }
            (Some(tail), Expectation::None) => self.infer_expr(tail),
            (None, Expectation::Has(expected, origin)) => {
                let unit = self.table.unit;
                self.expect(range, expected, unit, &origin);
                expected
            }
            (None, Expectation::None) => self.table.unit,
        }
    }
```

`check_expr` の `If` の腕と `Block` の腕を次にする。

```rust
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let expectation = Expectation::Has(expected, origin);
                self.if_expr(expr.range, *condition, *then_branch, *else_branch, expectation);
                self.typing.exprs.insert(id, expected);
            }
            ExprKind::Block { stmts, tail } => {
                self.block(expr.range, stmts, *tail, Expectation::Has(expected, origin));
                self.typing.exprs.insert(id, expected);
            }
```

`infer_expr` の `If` の腕と `Block` の腕を次にする。

```rust
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => self.if_expr(
                expr.range,
                *condition,
                *then_branch,
                *else_branch,
                Expectation::None,
            ),
            ExprKind::Block { stmts, tail } => {
                self.block(expr.range, stmts, *tail, Expectation::None)
            }
```

- [ ] **Step 2: 矢印をたどる処理を1つにする**

`check/body.rs` に足す。

```rust
/// 関数型として見たときの最初の矢印。
pub(super) enum Arrow {
    Fn { param: Ty, row: Row, ret: Ty },
    Error,
    NotFunction,
}
```

`impl BodyCheck` に足す。

```rust
    /// 推論用の変数なら、新しい関数型と単一化してから矢印を返す。呼び出しとラムダで、型の決まっていない値を関数として
    /// 使えるようにするため。シグネチャの型は推論用の変数を含まないので、等式の検査でも同じ関数を使える。
    fn next_arrow(&mut self, ty: Ty) -> Arrow {
        if let TyShape::Var(_) = self.table.shape(ty) {
            let function = self.fresh_function();
            // 新しい変数だけでできた関数型なので、単一化は失敗しない
            let unified = self.table.unify(ty, function);
            debug_assert!(
                unified.is_ok(),
                "a fresh function type always unifies with a variable"
            );
        }
        match self.table.shape(ty).clone() {
            TyShape::Fn {
                param, row, ret, ..
            } => Arrow::Fn { param, row, ret },
            TyShape::Error => Arrow::Error,
            _ => Arrow::NotFunction,
        }
    }
```

`check_function`、`call`、`check_lambda` の矢印のループで、`if let TyShape::Var(_) = ... { ... }` の単一化と `match self.table.shape(...).clone() { TyShape::Fn {..} => ..., TyShape::Error => ..., _ => ... }` を、`match self.next_arrow(...) { Arrow::Fn { param, row, ret } => ..., Arrow::Error => ..., Arrow::NotFunction => ... }` に置き換える。各腕の中身は今のまま (下の Step 3 で `NotFunction` の診断だけを関数に移す)。

- [ ] **Step 3: 矢印の数の食い違いの診断を `report.rs` に移す**

`check/report.rs` の `impl BodyCheck<'_>` に、次の3つのメソッドを作り、`check_function`、`check_lambda`、`call` の `NotFunction` の腕で `self.diagnostics.push(Diagnostic::error(...))` の部分をこの呼び出しに置き換える。中の文言と範囲は今のまま写す。

- `pub(super) fn signature_arity_error(&self, param: PatId, index: usize) -> Diagnostic`: `check_function` の「`{name}` has … but its signature has …」の診断
- `pub(super) fn lambda_arity_error(&self, expected: Ty, params: usize, param: PatId, index: usize) -> Diagnostic`: `check_lambda` の「this lambda has … but its expected type … has …」の診断 (`expected` は中で `self.table.display` する)
- `pub(super) fn call_arity_error(&self, name: &str, index: usize, args: usize, arg: ExprId) -> Diagnostic`: `call` の「… is not a function」と「… takes … but … were given」の診断

- [ ] **Step 4: 今の row の保存と `include_call_row`**

`check/body.rs` の `impl BodyCheck` に足す。

```rust
    /// `check` の間だけ今の row とその由来を替え、終わったら戻す。ラムダの本体は、外側の関数ではなく、ラムダで最後に
    /// たどった矢印の row で検査する (docs/spec/types.md の「推論」)。
    fn with_ambient<T>(
        &mut self,
        row: Row,
        source: AmbientSource,
        check: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let saved_row = std::mem::replace(&mut self.ambient, row);
        let saved_source = std::mem::replace(&mut self.ambient_source, source);
        let result = check(self);
        self.ambient = saved_row;
        self.ambient_source = saved_source;
        result
    }
```

`check_lambda` の `let saved = (self.ambient.clone(), self.ambient_source.clone());` と、最後の `self.ambient = match row { ... }`、`self.ambient_source = ...`、`self.check_expr(lambda_body, current, Origin::LambdaBody);`、`(self.ambient, self.ambient_source) = saved;` を、次にする。

```rust
        // 期待する型が壊れていれば、どのエフェクトも受け入れて診断を連鎖させない
        let row = row.unwrap_or_else(Row::error);
        self.with_ambient(row, AmbientSource::Lambda(origin), |this| {
            this.check_expr(lambda_body, current, Origin::LambdaBody)
        });
```

`report.rs` の `perform` を `include_call_row` に改名し、`body.rs` の呼び出しも直す。

- [ ] **Step 5: テストが通り、期待値が変わっていないことを確かめる**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED"`
Expected: 何も出ない

Run: `git diff --no-ext-diff --stat -- crates/*/tests '*.snap'`
Expected: 何も出ない

Run: `grep -n "TyShape::Var(_) = self.table.shape" crates/eml_types/src/check/body.rs`
Expected: `next_arrow` の1か所だけが出る

- [ ] **Step 6: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates/eml_types
git commit -m "Share if, block, and arrow walking between check and infer, and restore the ambient row in one place"
```

---

### Task 7: E2002 の副ラベルを矢印に向ける

**Files:**
- Modify: `crates/eml_types/src/check/body.rs` (`body_arrow_range`)、`crates/eml_types/src/check/report.rs` (`include_call_row`)
- Modify: `crates/eml_types/tests/check.rs`

**Interfaces:**
- Consumes: Task 6 の `include_call_row`
- Produces: `BodyCheck::body_arrow_range(&self) -> TextRange`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/check.rs` の末尾に足す。

```rust
#[test]
fn a_missing_effect_points_at_the_arrow_of_the_body() {
    // 本体のエフェクトは、引数の数だけ矢印をたどった最後の矢印の row に入るので、その矢印を指す
    // (docs/spec/diagnostics.md の E2002)
    let text = "f : Int -> Int -> Unit\nf a b = println \"x\"";
    insta::assert_snapshot!(check_text(text), @"
    f : Int -> Int -> Unit
      a#0 : Int
      b#1 : Int
    ---
    E2002 2:9 `println` performs `IO`, which the signature of `f` does not allow
      2:9 this call performs `IO`
      1:12 the row of this signature does not include it
      help: add `IO` to the row of the signature of `f`, as in `-> <IO> ...`
    ");
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --test check a_missing_effect_points_at_the_arrow_of_the_body`
Expected: FAIL。副ラベルの位置が `1:5` (シグネチャ全体の先頭) になっている

- [ ] **Step 3: 実装する**

`check/body.rs` の `impl BodyCheck` に足す。

```rust
    /// 本体の row が入る矢印の部分の型の範囲。シグネチャの型を、引数の数より1つ少ない回数だけ戻り値の側にたどる。
    /// たどれないときと引数がないときは、シグネチャの型全体を指す (docs/spec/diagnostics.md の E2002)。
    pub(super) fn body_arrow_range(&self) -> TextRange {
        let Some(signature) = &self.function.signature else {
            return self.function.name_range;
        };
        let mut id = signature.ty;
        for _ in 1..self.body.params.len() {
            match &self.function.types[id].kind {
                TypeRefKind::Fn { ret, .. } => id = *ret,
                _ => return signature.range,
            }
        }
        match &self.function.types[id].kind {
            TypeRefKind::Fn { .. } if self.body.params.len() > 1 => self.function.types[id].range,
            _ => signature.range,
        }
    }
```

`report.rs` の `include_call_row` の `AmbientSource::Signature` の腕の副ラベル `Label::new(self.file(), self.signature_range(), "the row of this signature does not include it")` の `self.signature_range()` を `self.body_arrow_range()` にする。`AmbientSource::Lambda(Origin::Return)` の腕は `self.signature_range()` のまま変えない。

- [ ] **Step 4: テストが通り、既存の期待値が変わっていないことを確かめる**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED"`
Expected: 何も出ない

Run: `git diff --no-ext-diff --stat -- '*.snap'`
Expected: 何も出ない (E2002 が出る既存の UI テストは引数が1つの関数である)

- [ ] **Step 5: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates/eml_types
git commit -m "Point E2002 at the arrow whose row holds the body's effects"
```

---

### Task 8: 文書

**Files:**
- Modify: `docs/implementation/architecture.md`、`docs/implementation/status.md`、`docs/implementation/testing.md`

**Interfaces:**
- Consumes: Task 1〜7 の結果。承認を得て変えたテストがあれば、その内容
- Produces: なし

日本語の文書を書く前に `yomiyasu:yomiyasu` スキルを読み込み、その規則に従う。

- [ ] **Step 1: `architecture.md` を直す**

「`eml_types` の内部」の「ラムダの引数の個数が合わないときや期待する型が壊れているときは、新しい開いた row に退避し、エフェクトの誤りを連鎖させない」を、「ラムダの引数の個数が合わないときや期待する型が壊れているときは、末尾が `Error` の row で本体を検査し、エフェクトの誤りを連鎖させない」にする。

同じ節の箇条の最後に足す。

```markdown
- 型の表は `table/` に分ける。`mod.rs` は型と変数の格納、`unify.rs` は型の単一化、`row.rs` は row の単一化と `include_row`、`kinds.rs` は Kind の制約、`copy.rs` はスキームの具体化の写し、`export.rs` は外に出す型への変換である。型の形は `TyShape`、関数の矢印の線形性は `ArrowLin` と呼び、Kind (線形性と多重度) と取り違えないようにする
- row の末尾は `Tail::{Closed, Var, Error}` である。未定義のエフェクトか解決できない row 変数の跡は末尾 `Error` の row になり、型の `Error` と同じく、どのエフェクトも受け入れて束縛されない。外に出す型では `{error}` と表示する
- `Table::display` は診断の文言のための変換で、Kind の束を解かず、矢印の線形性を `Unr` にする。`Table::export` は `solve_kinds` の後にだけ呼び、`TypedModule` を組み立てる
- 検査器は `check/` に分ける。`mod.rs` は SCC の順の検査と `TypedModule` の組み立て、`body.rs` は本体の検査、`report.rs` は診断を作る処理である。`if` とブロックは期待する型の有無 (`Expectation`) で check と infer の処理を共有し、矢印をたどる処理は `next_arrow` に、今の row の保存と復元は `with_ambient` にまとめてある。呼び出しの row を今の row に含める処理は `include_call_row` と呼ぶ
- E2002 の副ラベルは、本体の row が入る矢印の部分の型を指す (`body_arrow_range`)
```

- [ ] **Step 2: `status.md` を直す**

「次の作業の注意点」から、「診断の食い違い: E2002 の副ラベル…」、「診断の連鎖の残り: シグネチャの row に未定義のエフェクトがある関数は…」、「段階3: 未定義のエフェクトの row の末尾は、具体化の写しどうしで…」の3つの箇条を消す。

「リファクタリング」の表の R2 の行を、次の2行にする。

```markdown
| R2a | 型検査器の内部 | `eml_types` の `table/` と `check/` の分割と改名、row の末尾の `Error`、`display` と `export`、E2002 の矢印 | 完了 |
| R2b | データモデル | HIR の item と名前空間、組み込みの表、エフェクトと型構成子の ID、`TypedModule`。`eml_core_ir` と `eml_interp` も追随させる | 未着手 |
```

表の上の段落の「4つの回に分け、R0 → R1 → R2 → R3 の順に」を「5つの回に分け、R0 → R1 → R2a → R2b → R3 の順に」にする。

「R1〜R3 で直す項目」の見出しを「R1〜R3 で直す項目」のままにし、`#### R2 HIR と型` の節を次にする (箇条の中身は今の一覧から写す)。

```markdown
#### R2a 型検査器の内部

R2a で済んだ。`table.rs` と `check.rs` の分割と改名、`error_rows` から row の末尾の `Error` への置き換え (既知の誤り2件を直した)、`check.rs` の重複の解消と診断の分離、表示のために Kind の束を解かないこと、E2002 の副ラベル。

#### R2b データモデル

- (今の一覧の「HIR の `Module` に、関数のほかに…」の箇条)
- (「組み込みの名前、fixity、型、Core IR への変換を1つの表にまとめる…」の箇条)
- (「エフェクトと型構成子を ID で表す…」の箇条)
- (「HIR の子の式を辿る関数…」の箇条)
- (「`TypedModule` がスキームを返すようにし…」の箇条)
- spec の判断が要るもの: `x |> f a` の評価順 (今は `x` を最後に評価する)
```

括弧の中は、今の一覧の該当する箇条の文をそのまま写す。

「完了した作業」の表の末尾に足す。

```markdown
| リファクタリング R2a | 型検査器の `table.rs` と `check.rs` を分けて名前を直した。未定義のエフェクトの row の末尾を `Error` にして、既知の誤り2件を直した。表示のために Kind の束を解かないようにし、E2002 の副ラベルを本体の row の矢印に向けた |
```

- [ ] **Step 3: `testing.md` に記録する**

「テストの変更の記録」の「リファクタリング R1」の後に足す。Task 4〜7 で承認を得て変えたテストがあれば、最後の箇条の後に足す。

```markdown
### リファクタリング R2a

- 未定義のエフェクトか解決できない row 変数の跡の row を、推論用の row 変数ではなく末尾 `Error` の row にした。外に出す型での表示が `<_>` から `<{error}>` になり、`eml_types/tests/check.rs` の `main_with_an_erroneous_row_is_not_reported_again` と `an_undefined_effect_row_is_fresh_at_each_call` の期待値が変わった (種類2)。どちらも、E1002 を重ねて出さないことと、呼び出しごとに独立していることを確かめる目的は変わらない
```

- [ ] **Step 4: 文書を検査してコミットする**

Run: `python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py docs/implementation/status.md`
Expected: 書き足した部分の指摘を見直す。英単語の前後の半角空白と箇条書きの比率は直さない

Run: `cargo test`
Expected: PASS

```bash
git add docs/implementation
git commit -m "Document refactor R2a in the architecture, status, and test-change record"
```

---

### Task 9: 仕上げの確認

**Files:**
- なし (確認だけ。直す必要が出たら、該当するタスクの範囲で直してコミットする)

**Interfaces:**
- Consumes: Task 1〜8 のすべて
- Produces: なし

- [ ] **Step 1: すべての検査を通す**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る。clippy の警告は0件

- [ ] **Step 2: 成功の条件を確かめる**

Run: `ls crates/eml_types/src/table crates/eml_types/src/check; test ! -e crates/eml_types/src/table.rs && test ! -e crates/eml_types/src/check.rs && echo split`
Expected: `table/` に7つ、`check/` に3つのファイルがあり、`split` が出る

Run: `grep -rn "error_rows\|TyKind\|fn kind(\|fresh_mult(\|fn perform" crates/eml_types/src`
Expected: 何も出ない

Run: `grep -rn "\.export(" crates/eml_types/src --include=*.rs | grep -v "table/tests.rs"`
Expected: `check/mod.rs` の `TypedModule` を組み立てる3か所と、`table/export.rs` の定義の中だけが出る

- [ ] **Step 3: 変わったテストが名前を挙げたものだけであることを確かめる**

Run: `git diff --no-ext-diff main --stat | grep -E "tests/|\.snap"`
Expected: `crates/eml_types/tests/check.rs` だけが出る (`snapshots/` のファイルは出ない)

Run: `git diff --no-ext-diff -U0 main -- crates/eml_types/tests/check.rs | grep '^-' | grep -v '^---'`
Expected: Task 4 で名前を挙げた3行だけが出る (承認を得て変えたものがあれば、それも出る)

## 完了後の後始末

ブランチ全体のレビューが済んだら、作業用の文書を削除する。

```bash
git rm docs/superpowers/specs/2026-10-04-refactor-r2a-type-checker-design.md docs/superpowers/plans/2026-10-04-refactor-r2a-type-checker.md
git commit -m "Remove the work documents of refactor R2a"
```
