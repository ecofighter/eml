# リファクタリング R5: 型検査の SCC ごとの独立 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `eml_types` の型検査を、関数ごとの本体の検査 (段1) と SCC ごとの Kind の解決 (段2) に分け、型検査の時間をプログラムの大きさに比例させる。

**Architecture:** モジュール全体の情報を `Context` に分け、型の表は関数ごとの作業領域にする (Task 1)。書き出す `Type` から使われていない線形性を除き (Task 2)、Kind の制約の由来を表から切り離す (Task 3)。表を使わずに Kind の問題を解く段2 (Task 4) と、シグネチャの閉じた形 `Shape` (Task 5) を、まだつながずに作ってテストする。Task 6 で `check_module` を段0〜2の組み立てに書き換え、古い束と古いスキームを消す。Task 7 で性能の受け入れテストを、Task 8 で文書を足す。

**Tech Stack:** Rust (edition 2024)、la-arena 0.3、insta。新しい外部 crate は足さない。

**Spec:** `docs/superpowers/specs/2026-10-06-refactor-r5-scc-independent-checking-design.md` (R5 の設計)。規範は `docs/spec/types.md` の「推論」と `docs/spec/diagnostics.md`。テストの運用は `docs/implementation/testing.md`。

## Global Constraints

- 作業は `main` から切ったブランチ `refactor-r5` で行う。タスクごとにコミットする。コミットメッセージは英語で、末尾に次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_019tCBpiHGDbtirq3ZX1XfKv
  ```

- 外部 crate は増やさない。`unsafe` を書かない
- コードのコメントは日本語で、何をするかではなく理由を書く。規則を指すときは `docs/` のパスを書く。日本語を書く前に `yomiyasu:yomiyasu` スキルを読み、その規則に従う (CLAUDE.md)
- 言語の観測できる振る舞いは、E3006 の重複報告を直すこと (Task 3 で表示が同じ重複がまとまり、Task 6 で組ごとに1件になる) を除いて変えない。`tests/ui/` のスナップショット、`eml_cli` のテスト、`eml_core_ir` と `eml_interp` のテストの期待値は1文字も変えない
- 既存のテストで期待値を変えてよいのは、次のものだけである。それ以外の期待値が変わったら、変えずに止まり、差分と理由をユーザーに示して承認を得る。設計を曲げてテストを守ることはしない
  - 削除 (種類1。spec で合意済み): `table/tests.rs` の `export_needs_solved_kinds` と `display_does_not_solve_kinds` (Task 2)
  - 削除 (種類1。この計画で足した。実行の前にユーザーの承認を得る): `table/tests.rs` の `copy_type_replaces_rigid_variables` と `copying_keeps_an_error_row` (Task 6)。消す `Table::copy_type` の代わりに `Shape` の具体化を使うので、同じ意図のテストを `shape.rs` に足す (Task 5)
  - 種類2 (起きたときだけ): `eml_types/tests/` の dump の `kinds:` の行が、残す変数どうしが同じ循環に入る関数で変わった場合。変わったテストと差分を test-changes.md に書き、ユーザーに知らせる
- 期待値を変えない機械的な追随 (種類3) は許す。この計画で起きるのは、`Table::new` の呼び方、`Type` の組み立てから `linearity` を除くこと、`display` を `export` に改名すること、`kind.rs` の単体テストを `kind/solve.rs` へ移して新しい API で組み立てることである
- 各タスクの最後に `cargo test`、`cargo clippy --all-targets`、`cargo fmt` を通す。clippy の警告を残さない
- 各段階は純粋な関数のままにする。可変の大域状態を持たない
- 長い連鎖をたどる処理はループで書く。Rust の再帰を新しく増やさない。ただし、型の木をたどる再帰 (`lower`、`Shape` の写しと具体化) は、今の `lower` と同じく E0013 の入れ子の上限で深さが抑えられるので、再帰で書いてよい

## Review Focus

- シグネチャだけで等式のない関数を、ほかの関数が参照する。E1005 だけが出て、段2が呼び出し先のスキームを探して panic してはならない → Task 6 の `check.rs` の `a_reference_to_a_function_without_equations_is_checked`
- シグネチャに未定義のエフェクトがある (row の末尾 `Error`)。閉じた形にしても具体化しても末尾 `Error` のまま残り、どのエフェクトも受け入れて診断を連鎖させない → Task 5 の `shape.rs` の `an_error_row_survives_closing_and_instantiation`
- 3つ以上の関数が環状に呼び合い、1つだけが値を2回使う。輪のすべての関数のスキームに `Unr` の上限が付かなければならない → Task 6 の `check.rs` の `kinds_are_shared_around_a_ring_of_functions`
- 持ち越しが3段の多相な関数を通る。E3006 は1件だけで、一番外側の関数の中で位置が最も前の持ち越しを指さなければならない → Task 6 の `linearity.rs` の `a_carry_over_through_three_functions_is_reported_once`
- 8000個の持ち越しの連鎖。今は終わらないが、R5 の後は 2000個の時間のおよそ4倍で終わらなければならない → Task 7 の `scaling.rs` の `a_chain_of_carry_overs`

---

### Task 1: モジュール全体の情報を `Context` に分ける

`Table::new` が毎回作っている `data_kinds`、名前、多重度の表を `Context` に移し、`Table` はそれを借りるだけにする。テストの期待値は変わらない (種類3)。

**Files:**
- Create: `crates/eml_types/src/context.rs`
- Modify: `crates/eml_types/src/lib.rs` (`mod context;` を足す)
- Modify: `crates/eml_types/src/table/mod.rs` (`Table` の定義、`Table::new`、`effect_multiplicity`、`operation_multiplicity`)
- Modify: `crates/eml_types/src/table/kinds.rs` (`kind_bounds` の `data_kinds`)
- Modify: `crates/eml_types/src/table/export.rs` (`type_names`、`effect_names`)
- Modify: `crates/eml_types/src/table/{unify,row,kinds,copy,export}.rs` (`impl Table` を `impl Table<'_>` に)
- Modify: `crates/eml_types/src/check/{mod,body,handle,equality,report}.rs`、`crates/eml_types/src/usage.rs`、`crates/eml_types/src/carry.rs` (寿命の引数)
- Test: `crates/eml_types/src/table/tests.rs`

**Interfaces:**
- Consumes: なし
- Produces:
  - `pub(crate) struct Context { pub lang: LangItems, pub data_kinds: ArenaMap<TypeDefId, DataKind>, pub type_names: ArenaMap<TypeDefId, String>, pub effect_names: ArenaMap<EffectId, String>, pub effect_multiplicities: ArenaMap<EffectId, Multiplicity>, pub operation_multiplicities: ArenaMap<OperationId, Multiplicity> }`
  - `Context::new(lang: LangItems, types: &Arena<TypeDef>, constructors: &Arena<Constructor>, effects: &Arena<EffectDef>, operations: &Arena<Operation>) -> Context`
  - `#[cfg(test)] pub(crate) fn context::test_context() -> Context` (今の `table/tests.rs` の `new_table` と同じ組み込みの型を持つ)
  - `pub(crate) struct Table<'c>` と `Table::new(context: &'c Context) -> Table<'c>`
  - `BodyCheck<'a, 'c>`、`Usage<'a, 'c>`、`Carrying<'a, 'c>` (`table: &'a mut Table<'c>`)

- [ ] **Step 1: 今のテストが通ることを確かめ、ブランチを切る**

Run: `git switch -c refactor-r5 && cargo test`
Expected: すべて PASS。ここで落ちるテストがあれば、R5 を始める前にユーザーに知らせる。

- [ ] **Step 2: `context.rs` を作る**

`crates/eml_types/src/context.rs` を次の内容にする。`Context::new` の本体は、今の `Table::new` (table/mod.rs の 196-258行) の前半を移したものである。

```rust
//! モジュール全体で1回だけ求める、型検査の前提。関数ごとの型の表はこれを借りるだけにして、表を作る費用を関数の
//! 大きさに比例させる (docs/implementation/architecture.md の「`eml_types` の内部」)。

use eml_hir::{
    Constructor, EffectDef, EffectId, LangItems, OpMultiplicity, Operation, OperationId, TypeDef,
    TypeDefId,
};
use la_arena::{Arena, ArenaMap};

use crate::data::{DataKind, data_kinds};
use crate::ty::Multiplicity;

pub(crate) struct Context {
    pub lang: LangItems,
    /// 型構成子ごとの、Kind の決まり方 (`crate::data`)。
    pub data_kinds: ArenaMap<TypeDefId, DataKind>,
    /// 名前を持つのは、`Module` を渡さずに型を書き出せるようにするため。
    pub type_names: ArenaMap<TypeDefId, String>,
    pub effect_names: ArenaMap<EffectId, String>,
    /// エフェクトがその row に入れる操作の上限。操作の多重度の最大である (docs/spec/types.md の「Kind」)。
    pub effect_multiplicities: ArenaMap<EffectId, Multiplicity>,
    pub operation_multiplicities: ArenaMap<OperationId, Multiplicity>,
}

impl Context {
    pub fn new(
        lang: LangItems,
        types: &Arena<TypeDef>,
        constructors: &Arena<Constructor>,
        effects: &Arena<EffectDef>,
        operations: &Arena<Operation>,
    ) -> Context {
        let operation_multiplicities = operations
            .iter()
            .map(|(id, operation)| (id, multiplicity(operation.multiplicity)))
            .collect();
        let effect_multiplicities = effects
            .iter()
            .map(|(id, effect)| {
                // 組み込みの `IO` は、実行時が必ず1回再開するので `Once` である (docs/spec/effects.md)
                let multiplicity = if id == lang.io {
                    Multiplicity::Once
                } else {
                    effect
                        .operations
                        .iter()
                        .map(|&op| multiplicity(operations[op].multiplicity))
                        .max()
                        .unwrap_or(Multiplicity::Never)
                };
                (id, multiplicity)
            })
            .collect();
        Context {
            lang,
            data_kinds: data_kinds(types, constructors, &lang),
            type_names: types
                .iter()
                .map(|(id, def)| (id, def.name.clone()))
                .collect(),
            effect_names: effects
                .iter()
                .map(|(id, def)| (id, def.name.clone()))
                .collect(),
            effect_multiplicities,
            operation_multiplicities,
        }
    }
}

fn multiplicity(multiplicity: OpMultiplicity) -> Multiplicity {
    match multiplicity {
        OpMultiplicity::Never => Multiplicity::Never,
        OpMultiplicity::Once => Multiplicity::Once,
        OpMultiplicity::Multi => Multiplicity::Multi,
    }
}

/// 組み込みの型だけを持つ前提。表と閉じた形の単体テストが、`Module` を組まずに表を作るのに使う。
#[cfg(test)]
pub(crate) fn test_context() -> Context {
    use eml_diagnostics::TextRange;
    use eml_hir::Generics;

    let mut types = Arena::new();
    let mut effects = Arena::new();
    let mut constructors = Arena::new();
    let int = types.alloc(TypeDef::builtin("Int"));
    let string = types.alloc(TypeDef::builtin("String"));
    let bool = types.alloc(TypeDef::builtin("Bool"));
    let unit = types.alloc(TypeDef::builtin("Unit"));
    let file = types.alloc(TypeDef::builtin("File"));
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
        file,
        io: effects.alloc(EffectDef {
            name: "IO".to_string(),
            generics: Generics::default(),
            operations: Vec::new(),
        }),
        true_ctor,
        false_ctor,
    };
    Context::new(lang, &types, &constructors, &effects, &Arena::new())
}
```

`lib.rs` の `mod carry;` の次に `mod context;` を足す。

- [ ] **Step 3: `Table` が `Context` を借りるようにする**

`table/mod.rs` の `Table` から `data_kinds`、`type_names`、`effect_names`、`effect_multiplicities`、`operation_multiplicities` を除き、`context: &'c Context` を先頭に足す。`lang` は `LangItems` が `Copy` なので、`Table` にも写しを残す (`lower` が `table.lang` を読むため)。

```rust
pub(crate) struct Table<'c> {
    /// モジュール全体の情報。表ごとに作り直さず借りる。
    context: &'c Context,
    shapes: Vec<TyShape>,
    // (ty_vars から lin_solution までは今のまま)
    pub int: Ty,
    pub string: Ty,
    pub bool: Ty,
    pub unit: Ty,
    pub error: Ty,
    pub lang: LangItems,
    /// 持ち越しの制約 (docs/spec/types.md の「推論」)。
    carries: Vec<Carry>,
}
```

`impl Table` を `impl<'c> Table<'c>` にし、`Table::new` を次にする。

```rust
    pub fn new(context: &'c Context) -> Table<'c> {
        let lang = context.lang;
        let mut table = Table {
            context,
            shapes: Vec::new(),
            ty_vars: Vec::new(),
            row_vars: Vec::new(),
            rigids: Vec::new(),
            linearity: Lattice::new(Linearity::Unr),
            kind_origin: None,
            multiplicity: Lattice::new(Multiplicity::Never),
            lin_solution: None,
            int: Ty(0),
            string: Ty(0),
            bool: Ty(0),
            unit: Ty(0),
            error: Ty(0),
            lang,
            carries: Vec::new(),
        };
        table.int = table.alloc(TyShape::Con(lang.int, Vec::new()));
        table.string = table.alloc(TyShape::Con(lang.string, Vec::new()));
        table.bool = table.alloc(TyShape::Con(lang.bool, Vec::new()));
        table.unit = table.alloc(TyShape::Record(Vec::new()));
        table.error = table.alloc(TyShape::Error);
        table
    }
```

`effect_multiplicity` は `self.context.effect_multiplicities[effect]`、`operation_multiplicity` は `self.context.operation_multiplicities[operation]` を返す。`table/kinds.rs` の `kind_bounds` は `self.context.data_kinds[*id]`、`table/export.rs` は `self.context.type_names[id]` と `self.context.effect_names[label.effect]` を読む。`table/` のほかのファイルの `impl Table {` は `impl Table<'_> {` にする。`use` から要らなくなったもの (`Constructor`、`EffectDef`、`Operation`、`OpMultiplicity`、`TypeDef`、`Arena`) を除き、`use crate::context::Context;` を足す。

- [ ] **Step 4: 表を借りる構造体に寿命の引数を足す**

次の4つの構造体に、表の寿命 `'c` を足す。`&'a mut Table<'a>` にすると、表を借りている間ずっと `'c` まで借りることになり、検査の後で表を使えなくなるためである。

| ファイル | 変更 |
|---|---|
| `check/body.rs` | `pub(super) struct BodyCheck<'a, 'c>` にし、`table: &'a mut Table<'c>` にする。`impl BodyCheck<'_>` を `impl BodyCheck<'_, '_>` にする |
| `check/handle.rs`、`check/equality.rs`、`check/report.rs` | `impl BodyCheck<'_>` を `impl BodyCheck<'_, '_>` にする |
| `usage.rs` | `struct Usage<'a, 'c>` にし、`table: &'a mut Table<'c>` にする。`impl<'a> Usage<'a>` を `impl<'a> Usage<'a, '_>` にする |
| `carry.rs` | `struct Carrying<'a, 'c>` にし、`table: &'a mut Table<'c>` にする。`impl Carrying<'_>` を `impl Carrying<'_, '_>` にする |

関数の引数の `&mut Table` と `&Table` は、寿命を省いたままでよい。

`check/mod.rs` の `check_module` の先頭を次にする。

```rust
    let context = Context::new(
        module.lang,
        &module.types,
        &module.constructors,
        &module.effects,
        &module.operations,
    );
    let mut table = Table::new(&context);
```

- [ ] **Step 5: 表の単体テストを追随させる**

`table/tests.rs` の `new_table` を消し、`use` を次にする (`TextRange`、`Constructor`、`EffectDef`、`Generics`、`LangItems`、`Arena` は使わなくなる)。

```rust
use super::*;
use crate::context::test_context;
```

各テストの `let mut table = new_table();` と `let table = new_table();` を、次のコマンドで2行に置き換える。

```bash
perl -0pi -e 's/^(\s*)let (mut )?table = new_table\(\);/$1let context = test_context();\n$1let $2table = Table::new(&context);/mg' crates/eml_types/src/table/tests.rs
```

- [ ] **Step 6: ビルドとテストを通す**

Run: `cargo build -p eml_types && cargo test`
Expected: すべて PASS。スナップショットは1つも変わらない (`cargo insta pending-snapshots` が空)。

- [ ] **Step 7: clippy と fmt**

Run: `cargo clippy --all-targets && cargo fmt`
Expected: 警告なし。

- [ ] **Step 8: コミット**

```bash
git add -A crates/eml_types/src
git commit -m "Move module-wide facts of type checking into Context

<末尾の2行>"
```

---

### Task 2: 書き出す型から矢印の線形性を除く

`Type::Fn` と `Type::Cont` の `linearity` は、後の段階の誰も読んでいない。これを除き、`Table::export` を Kind の解なしで呼べるようにして `display` と1つにまとめる。表示の文字列は変わらない。

**Files:**
- Modify: `crates/eml_types/src/ty.rs` (`Type::Fn`、`Type::Cont`、単体テスト)
- Modify: `crates/eml_types/src/table/export.rs`
- Modify: `crates/eml_types/src/table/mod.rs` (`lin_solution` を除く)
- Modify: `crates/eml_types/src/table/kinds.rs` (`solve_kinds`)
- Modify: `crates/eml_types/src/check/{mod,body,equality,report}.rs` (`display` の呼び出しと `check_main`)
- Test: `crates/eml_types/src/table/tests.rs`

**Interfaces:**
- Consumes: Task 1 の `Table<'c>`
- Produces:
  - `Type::Fn { param: Box<Type>, effects: Vec<EffectLabel>, tail: Option<RowTail>, ret: Box<Type> }`
  - `Type::Cont { arg: Box<Type>, ret: Box<Type>, effects: Vec<EffectLabel>, tail: Option<RowTail> }`
  - `Table::export(&self, ty: Ty) -> Type` (いつでも呼べる。`display` はなくなる)
  - `Table::export_label(&self, label: &Label) -> EffectLabel` (今の `display_label`)
  - `Table::solve_kinds(&self) -> Vec<KindOrigin>` (`&mut self` から変える)

- [ ] **Step 1: 不要になる2つのテストを消す**

`table/tests.rs` の `export_needs_solved_kinds` (`#[should_panic(expected = "export is for after solve_kinds")]` の付いたもの) と `display_does_not_solve_kinds` を消す。解く前に書き出せないという性質と、書き出しと表示の区別がなくなるためである (種類1。spec で合意済み)。

- [ ] **Step 2: `Type` から `linearity` を除く**

`ty.rs` の `Type::Fn` と `Type::Cont` から `linearity: Linearity,` の行を消す。`Display` と `contains_error` は `..` で照合しているので、そのまま通る。`Type::Fn` の doc コメントの上に、次のコメントを足す。

```rust
    /// 関数型。矢印の線形性は持たない。後の段階は線形性を読まず、線形性は Kind の解に依存するので、持たせると本体の型を
    /// Kind を解くまで確定できなくなる (docs/implementation/architecture.md の「`eml_types` の内部」)。
```

`ty.rs` の単体テストから `linearity: Linearity::...,` の行を消す。

```bash
perl -0pi -e 's/^\s*linearity: Linearity::\w+,\n//mg' crates/eml_types/src/ty.rs
```

(このコマンドは `Type` の定義の行には当たらない。定義は `linearity: Linearity,` で、`Linearity::` を含まないためである。定義の2行は手で消す。)

- [ ] **Step 3: `export` を1つにまとめる**

`table/export.rs` の `export`、`display`、`to_type`、`export_lin`、`display_label`、`label_type`、`export_row` を、次の3つにする。`kind_names` と `push_label_args` は変えない。

```rust
    /// 後の段階と診断の文言に渡す形。解けていない型変数と row 変数は `_` として残す。矢印の線形性は持たないので、Kind の
    /// 束を解かずにいつでも呼べる。
    pub fn export(&self, ty: Ty) -> Type {
        match self.shape(ty).clone() {
            TyShape::Con(id, args) => Type::Con {
                id,
                name: self.context.type_names[id].clone(),
                args: args.into_iter().map(|arg| self.export(arg)).collect(),
            },
            TyShape::Record(fields) => Type::Record(
                fields
                    .into_iter()
                    .map(|(label, field)| (label, self.export(field)))
                    .collect(),
            ),
            TyShape::Fn {
                param, row, ret, ..
            } => {
                let (effects, tail) = self.export_row(&row);
                Type::Fn {
                    param: Box::new(self.export(param)),
                    effects,
                    tail,
                    ret: Box::new(self.export(ret)),
                }
            }
            TyShape::Cont { arg, row, ret, .. } => {
                let (effects, tail) = self.export_row(&row);
                Type::Cont {
                    arg: Box::new(self.export(arg)),
                    ret: Box::new(self.export(ret)),
                    effects,
                    tail,
                }
            }
            TyShape::Var(_) => Type::Flexible,
            TyShape::Rigid(rigid) => Type::Rigid(self.rigids[rigid.0 as usize].name.clone()),
            TyShape::Error => Type::Error,
        }
    }

    pub fn export_label(&self, label: &Label) -> EffectLabel {
        EffectLabel {
            id: label.effect,
            name: self.context.effect_names[label.effect].clone(),
            args: label.args.iter().map(|&arg| self.export(arg)).collect(),
        }
    }

    fn export_row(&self, row: &Row) -> (Vec<EffectLabel>, Option<RowTail>) {
        let row = self.resolve_row(row);
        let effects = row
            .labels
            .iter()
            .map(|label| self.export_label(label))
            .collect();
        let tail = match row.tail {
            Tail::Closed => None,
            Tail::Var(tail) => Some(match &self.row_vars[tail.0 as usize].rigid {
                Some(name) => RowTail::Rigid(name.clone()),
                None => RowTail::Flexible,
            }),
            Tail::Error => Some(RowTail::Error),
        };
        (effects, tail)
    }
```

`kind_names` の中の `self.display(ty)` は `self.export(ty)` にする。

`table/mod.rs` から `lin_solution` のフィールドと初期化を消す。`table/kinds.rs` の `solve_kinds` は `&self` を受け取り、`self.lin_solution = Some(lin);` の行を消す。doc コメントの「線形性の解を覚える。`export` が式ごとに解き直さずに済むようにするため。」の文を消す。

- [ ] **Step 4: 呼び出し側を直す**

`display(` と `display_label(` を `export(` と `export_label(` に改名する。

```bash
grep -rln 'display(\|display_label(' crates/eml_types/src | xargs perl -pi -e 's/\.display\(/.export(/g; s/\.display_label\(/.export_label(/g'
```

`check/mod.rs` の `check_main` の `expected` から `linearity: Linearity::Unr,` を消す。`table/tests.rs` の `display(` も同じコマンドで `export(` になる (種類3)。

- [ ] **Step 5: ビルドとテストを通す**

Run: `cargo build && cargo test`
Expected: すべて PASS。`eml_core_ir` は `Type::Fn { .. }` で照合しているので変わらない。スナップショットは1つも変わらない。

- [ ] **Step 6: clippy と fmt**

Run: `cargo clippy --all-targets && cargo fmt`
Expected: 警告なし。使わなくなった `use` (`ArrowLin` など) が警告に出たら消す。

- [ ] **Step 7: コミット**

```bash
git add -A crates/eml_types/src
git commit -m "Drop arrow linearity from exported types and merge display into export

<末尾の2行>"
```

---

### Task 3: Kind の制約の由来を表から切り離す

`KindReason::CarriedAcross` が持つ表の `Row` を、持ち越しのパスで決めた `multi` の操作に替える。`CarriedThrough` の `inner` を1段の要約にする。報告の文言は変わらない。

**Files:**
- Modify: `crates/eml_types/src/kind.rs` (`KindReason`、`Across`、`CarriedInner`、`InnerLabel`)
- Modify: `crates/eml_types/src/carry.rs`
- Modify: `crates/eml_types/src/table/kinds.rs` (`copy_carries`)
- Modify: `crates/eml_types/src/check/report.rs` (`linear_misuse`、`carried_across`、`carried_through`、`multi_operation` を消す)
- Modify: `crates/eml_types/src/check/mod.rs` (`carry::constrain` と `linear_misuse` の呼び出し)

**Interfaces:**
- Consumes: Task 1、Task 2
- Produces:
  - `KindReason::CarriedAcross { value: CarriedValue, multi: Option<OperationId>, call: CallKind }`
  - `KindReason::CarriedThrough { name: String, inner: Option<CarriedInner> }`
  - `pub(crate) struct CarriedInner { pub range: TextRange, pub label: InnerLabel }` と `CarriedInner::of(origin: &KindOrigin) -> CarriedInner`
  - `pub(crate) enum InnerLabel { Kept(String), Through(String), Value }`
  - `carry::constrain(module: &Module, body: &Body, typing: &BodyTyping, table: &mut Table<'_>, reliable: bool)`
  - `report::linear_misuse(module: &Module, origin: &KindOrigin) -> Diagnostic`

- [ ] **Step 1: 由来の型を替える**

`kind.rs` の `KindReason` の2つの variant を次にする。

```rust
    /// 呼び出しをまたいで持っている値。由来の範囲は呼び出しの範囲である (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。
    /// `multi` は報告が指す `multi` の操作で、持ち越しのパスが決める。row を持たないのは、由来を型の表から切り離し、
    /// スキームに残せるようにするため。
    CarriedAcross {
        value: CarriedValue,
        multi: Option<OperationId>,
        call: CallKind,
    },
    /// スキームから複写した持ち越しの制約。由来の範囲は参照した位置である。`inner` は、呼んだ関数の中で値をまたがせている
    /// 位置の要約である (docs/spec/diagnostics.md の E3006)。
    CarriedThrough {
        name: String,
        inner: Option<CarriedInner>,
    },
```

`Across` の doc コメントを次にし、`PartialEq, Eq` の derive を外す (`KindReason` に入らなくなる)。

```rust
/// 値がまたぐもの。持ち越しのパスが多重度の成分を作るのに使う。操作の直接の呼び出しでは、その操作の多重度だけを見る。
#[derive(Debug, Clone)]
pub(crate) enum Across {
```

`Carry` の定義の前に次を足す。

```rust
/// `CarriedThrough` が指す、呼んだ関数の中の持ち越しの1段分。報告は1段しかたどらないので入れ子にしない。入れ子にすると、
/// 呼び出しの段数だけ由来が深くなり、複写のたびにその深さの時間がかかる (docs/spec/diagnostics.md の E3006)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CarriedInner {
    pub range: TextRange,
    pub label: InnerLabel,
}

/// 報告の secondary の言い方。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InnerLabel {
    /// 名前のある値を持ったまま呼んだ。
    Kept(String),
    /// さらに別の関数を通った。
    Through(String),
    /// 名前のない値を持ったまま呼んだ。
    Value,
}

impl CarriedInner {
    pub fn of(origin: &KindOrigin) -> CarriedInner {
        let label = match &origin.reason {
            KindReason::CarriedAcross {
                value: CarriedValue::Local { name, .. } | CarriedValue::ReturnCapture { name, .. },
                ..
            } => InnerLabel::Kept(name.clone()),
            KindReason::CarriedThrough { name, .. } => InnerLabel::Through(name.clone()),
            _ => InnerLabel::Value,
        };
        CarriedInner {
            range: origin.range,
            label,
        }
    }
}
```

- [ ] **Step 2: 持ち越しのパスで `multi` の操作を決める**

`carry.rs` の `constrain` に先頭の引数 `module: &Module` を足し、`Carrying` に `module: &'a Module` を持たせる。`constrain` の中の `Carrying { ... }` の組み立てにも `module,` を足す。`use` に `eml_hir::{Module, OpMultiplicity, OperationId}` を足す。`carry_value` の由来を次にする。

```rust
        let origin = self.reliable.then(|| KindOrigin {
            range: self.body.exprs[at].range,
            reason: KindReason::CarriedAcross {
                value,
                multi: self.multi_operation(across),
                call: call.clone(),
            },
        });
```

`impl Carrying` に次を足す。中身は今の `report::multi_operation` と同じ規則である。

```rust
    /// 報告が指す `multi` の操作。row を解き、`multi` の操作を持つ最初のラベルのエフェクトから、宣言の順で最初の `multi`
    /// の操作を選ぶ。row を束縛するのはこの本体の検査だけで、このパスはその後に動くので、報告のときに解いても同じ結果に
    /// なる。
    fn multi_operation(&self, across: &Across) -> Option<OperationId> {
        let module = self.module;
        match across {
            Across::Operation(op) => Some(*op),
            Across::Row(row) => self.table.resolve_row(row).labels.iter().find_map(|label| {
                module.effects[label.effect]
                    .operations
                    .iter()
                    .copied()
                    .find(|&op| matches!(module.operations[op].multiplicity, OpMultiplicity::Multi))
            }),
        }
    }
```

`check/mod.rs` の `carry::constrain(body, &typing, &mut table, reliable)` を `carry::constrain(module, body, &typing, &mut table, reliable)` にする。

- [ ] **Step 3: 複写で要約を作る**

`table/kinds.rs` の `copy_carries` の `inner: carry.origin.clone().map(Box::new),` を `inner: carry.origin.as_ref().map(CarriedInner::of),` にする。

- [ ] **Step 4: 報告から表を外す**

`check/report.rs` を次のように直す。

- `linear_misuse(module: &Module, table: &Table, origin: &KindOrigin)` から `table` を外す。`CarriedAcross` の分岐は `carried_across(module, origin.range, value, *multi, call)` を呼ぶ。`CarriedThrough` の分岐は `carried_through(file, origin.range, name, inner.as_ref())` を呼ぶ
- `carried_across` から `table` を外し、`across: &Across` を `multi: Option<OperationId>` に替える。`let operation = multi_operation(module, table, across);` を `let operation = multi;` にする
- `multi_operation` を消す
- `carried_through` の `inner: Option<&KindOrigin>` を `inner: Option<&CarriedInner>` にし、ラベルを次で決める

```rust
    if let Some(inner) = inner {
        let label = match &inner.label {
            InnerLabel::Kept(name) => format!("`{name}` is kept alive across this call"),
            InnerLabel::Through(name) => format!("through this use of `{name}`"),
            InnerLabel::Value => "a value is kept alive across this call".to_string(),
        };
        diagnostic = diagnostic.with_secondary(Label::new(file, inner.range, label));
    }
```

`check/mod.rs` の `report::linear_misuse(module, &table, &origin)` を `report::linear_misuse(module, &origin)` にする。

- [ ] **Step 5: ビルドとテストを通す**

Run: `cargo build -p eml_types && cargo test`
Expected: すべて PASS。E3006 の文言と指す場所は1つも変わらない。

- [ ] **Step 6: clippy と fmt**

Run: `cargo clippy --all-targets && cargo fmt`
Expected: 警告なし。

- [ ] **Step 7: コミット**

```bash
git add -A crates/eml_types/src
git commit -m "Detach kind constraint origins from the type table

<末尾の2行>"
```

---

### Task 4: 段2 (Kind の問題を SCC ごとに解く) を作る

表を使わずに Kind の問題をまとめて解き、違反と残す制約を求める。まだ `check_module` にはつながない。`kind.rs` の単体テスト9件を、新しい API で `kind/solve.rs` へ移す (種類3)。

**Files:**
- Move: `crates/eml_types/src/kind.rs` → `crates/eml_types/src/kind/mod.rs`
- Create: `crates/eml_types/src/kind/problem.rs`
- Create: `crates/eml_types/src/kind/solve.rs`
- Modify: `crates/eml_types/src/table/kinds.rs` (`violated_carries` の参照先)

**Interfaces:**
- Consumes: Task 3 の `CarriedInner`、`KindOrigin`
- Produces (`crate::kind`):
  - `KindVar::from_index(index: usize) -> KindVar`
  - `Bound<T>` に `Hash` を足す
  - `pub(crate) trait Level: Copy + Ord + Hash + Debug { const BOTTOM: Self; }` (`Linearity` と `Multiplicity` に実装)
- Produces (`crate::kind::problem`):
  - `Bounds<T> { pub vars: usize, pub constraints: Vec<(Bound<T>, Bound<T>)>, pub origins: Vec<Option<KindOrigin>> }`、`Bounds::fresh(&mut self) -> KindVar`、`Bounds::require(&mut self, lower, upper, origin: Option<KindOrigin>)`、`Default`
  - `Decl { Function(FunctionId), Builtin(Builtin), Operation(OperationId), Constructor(ConstructorId) }` (`Copy`、`Hash`)
  - `Instance { pub decl: Decl, pub lin: Vec<KindVar>, pub mult: Vec<KindVar>, pub origin: Option<KindOrigin>, pub at: (usize, usize, usize) }`
  - `OwnVars { pub lin: Vec<KindVar>, pub mult: Vec<KindVar> }`
  - `KindProblem { pub lin: Bounds<Linearity>, pub mult: Bounds<Multiplicity>, pub carries: Vec<Carry>, pub instances: Vec<Instance>, pub own: OwnVars }`
  - `KindScheme { pub lin: Vec<(Bound<Linearity>, Bound<Linearity>)>, pub mult: Vec<(Bound<Multiplicity>, Bound<Multiplicity>)>, pub carries: Vec<Carry> }`
- Produces (`crate::kind::solve`):
  - `solve_scc(members: &[(Decl, &KindProblem)], schemes: &HashMap<Decl, KindScheme>) -> Solution`、`Solution { pub schemes: Vec<KindScheme>, pub violated: Vec<KindOrigin> }`
  - `solve<T: Level>(bounds: &Bounds<T>) -> (Vec<T>, Vec<usize>)`
  - `violated_carries(carries: &[Carry], lin: &[Linearity], mult: &[Multiplicity]) -> Vec<usize>` (`kind.rs` から移す)
  - `#[cfg(test)] residual_of<T: Level>(bounds: &Bounds<T>, keep: &[KindVar]) -> Vec<(Bound<T>, Bound<T>)>`

**spec からの細部の変更:** spec は `KindScheme` の持ち越しの制約の由来を `CarriedInner` の要約にすると書いた。Task 3 で `KindOrigin` が表を指さなくなり、`CarriedThrough` の入れ子もなくなったので、`KindScheme` には今と同じ `Carry` (由来は `KindOrigin`) をそのまま残す。大きさは1段で抑えられ、比べられるデータである点は spec と同じである。要約は段2の展開 (`copy_scheme`) で作る。こうすると、移すテストの `assert` の値 (`Carry { lin, mult, origin: None }`) を1文字も変えずに済む。

- [ ] **Step 1: `kind.rs` を `kind/mod.rs` に移し、共通の部品を足す**

```bash
mkdir -p crates/eml_types/src/kind && git mv crates/eml_types/src/kind.rs crates/eml_types/src/kind/mod.rs
```

`kind/mod.rs` を次のように直す。

- 先頭の `use` の後に、子のモジュールを宣言する。段1と段2をつなぐ Task 6 までは使われないので、警告を一時的に止める

  ```rust
  // 段1と段2をつなぐまで (R5 の Task 6) は check_module から使われない
  #[allow(dead_code)]
  pub(crate) mod problem;
  #[allow(dead_code)]
  pub(crate) mod solve;
  ```

- `KindVar` に次を足す

  ```rust
      pub fn from_index(index: usize) -> KindVar {
          KindVar(index as u32)
      }
  ```

- `Bound<T>` の derive を `#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]` にする
- `Bound` の定義の後に次を足す

  ```rust
  /// Kind の束の値。線形性 (`Unr ≤ Lin`) と多重度 (`Never ≤ Once ≤ Multi`) の2つがある (docs/spec/types.md の「Kind」)。
  pub(crate) trait Level: Copy + Ord + std::hash::Hash + std::fmt::Debug {
      const BOTTOM: Self;
  }

  impl Level for Linearity {
      const BOTTOM: Self = Linearity::Unr;
  }

  impl Level for Multiplicity {
      const BOTTOM: Self = Multiplicity::Never;
  }
  ```

- `violated_carries` を `solve.rs` へ移す (Step 3)。`table/kinds.rs` の `crate::kind::violated_carries` を `crate::kind::solve::violated_carries` にする
- `#[cfg(test)] mod tests` を丸ごと消す。9件は Step 4 で `solve.rs` に新しい API で書き直す

- [ ] **Step 2: `kind/problem.rs` を作る**

```rust
//! 段1が集める Kind の問題と、段2が残す Kind のスキーム (docs/spec/types.md の「推論」)。どちらも型の表を指さず、変数を
//! 番号だけで表す。比べられる純粋なデータなので、クエリに載せたときに変わっていないかを確かめられる。

use eml_hir::builtin::Builtin;
use eml_hir::{ConstructorId, FunctionId, OperationId};

use super::{Bound, Carry, KindOrigin, KindVar};
use crate::ty::{Linearity, Multiplicity};

/// 1つの束の上の制約 `下限 ≤ 上限` の集まり。変数は 0 から `vars` 未満の番号を持つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Bounds<T> {
    pub vars: usize,
    pub constraints: Vec<(Bound<T>, Bound<T>)>,
    /// 制約ごとの由来。`constraints` と同じ順に並ぶ。
    pub origins: Vec<Option<KindOrigin>>,
}

impl<T> Default for Bounds<T> {
    fn default() -> Self {
        Bounds {
            vars: 0,
            constraints: Vec::new(),
            origins: Vec::new(),
        }
    }
}

impl<T> Bounds<T> {
    pub fn fresh(&mut self) -> KindVar {
        self.vars += 1;
        KindVar::from_index(self.vars - 1)
    }

    pub fn require(&mut self, lower: Bound<T>, upper: Bound<T>, origin: Option<KindOrigin>) {
        self.constraints.push((lower, upper));
        self.origins.push(origin);
    }
}

/// スキームを持つ宣言。具体化の記録が、どの宣言のスキームを使うかを指す。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Decl {
    Function(FunctionId),
    Builtin(Builtin),
    Operation(OperationId),
    Constructor(ConstructorId),
}

/// 宣言の型の形を具体化した記録。呼び出し先の制約は段1で複写せず、段2で展開する。段1が呼び出し先の Kind のスキームを
/// 待たずに済むようにするため (docs/spec/types.md の「推論」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Instance {
    pub decl: Decl,
    /// `Shape` の線形性の Kind 変数の番号の順に並べた、具体化した新しい変数。
    pub lin: Vec<KindVar>,
    /// `Shape` の多重度の Kind 変数の番号の順に並べた、具体化した新しい変数。
    pub mult: Vec<KindVar>,
    /// 具体化したときに設定されていた由来。複写する制約の由来になる。
    pub origin: Option<KindOrigin>,
    /// 具体化したときの、線形性の制約、多重度の制約、持ち越しの制約の数。段2は展開した制約をこの位置に差し込む。
    /// 同じ範囲の違反の報告の順を、具体化のときに制約を複写していたときと同じにするため。
    pub at: (usize, usize, usize),
}

/// 宣言のシグネチャの Kind 変数。`Shape` の番号の順に、問題の中の番号を並べる。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct OwnVars {
    pub lin: Vec<KindVar>,
    pub mult: Vec<KindVar>,
}

/// 1つの宣言の Kind の問題 (段1の出力)。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct KindProblem {
    pub lin: Bounds<Linearity>,
    pub mult: Bounds<Multiplicity>,
    pub carries: Vec<Carry>,
    pub instances: Vec<Instance>,
    pub own: OwnVars,
}

/// 多相化した後に残す制約 (段2の出力)。変数は `Shape` の Kind 変数の番号である。並びは正規形にする
/// (`solve::residual` と `solve::carry_residual`)。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct KindScheme {
    pub lin: Vec<(Bound<Linearity>, Bound<Linearity>)>,
    pub mult: Vec<(Bound<Multiplicity>, Bound<Multiplicity>)>,
    pub carries: Vec<Carry>,
}
```

- [ ] **Step 3: `kind/solve.rs` を作る**

```rust
//! 段2: SCC ごとに Kind の問題をまとめて解き、違反を集め、各宣言のスキームに残す制約を求める
//! (docs/spec/types.md の「推論」)。型の表は使わず、番号の上の束だけを扱う。

use std::collections::{HashMap, HashSet};

use super::problem::{Bounds, Decl, KindProblem, KindScheme, OwnVars};
use super::{Bound, CarriedInner, Carry, KindOrigin, KindReason, KindVar, Level};
use crate::ty::{Linearity, Multiplicity};

/// 1つの SCC を解いた結果。`schemes` は `members` と同じ順に並ぶ。
pub(crate) struct Solution {
    pub schemes: Vec<KindScheme>,
    /// 由来のある違反の由来。並べ替えと重複除去は、呼ぶ側がモジュール全体でまとめて行う。
    pub violated: Vec<KindOrigin>,
}

/// `members` は1つの SCC の宣言とその問題、`schemes` は前の SCC と本体のない宣言のスキームである。
pub(crate) fn solve_scc(
    members: &[(Decl, &KindProblem)],
    schemes: &HashMap<Decl, KindScheme>,
) -> Solution {
    let merged = merge(members, schemes);
    let (lin, lin_violated) = solve(&merged.lin);
    let (mult, mult_violated) = solve(&merged.mult);
    let carry_violated = violated_carries(&merged.carries, &lin, &mult);
    let violated = lin_violated
        .iter()
        .filter_map(|&index| merged.lin.origins[index].clone())
        .chain(
            mult_violated
                .iter()
                .filter_map(|&index| merged.mult.origins[index].clone()),
        )
        .chain(
            carry_violated
                .iter()
                .filter_map(|&index| merged.carries[index].origin.clone()),
        )
        .collect();
    let lin_graph = Graph::new(&merged.lin);
    let mult_graph = Graph::new(&merged.mult);
    let schemes = merged
        .own
        .iter()
        .map(|own| KindScheme {
            lin: residual(&lin_graph, &own.lin),
            mult: residual(&mult_graph, &own.mult),
            carries: carry_residual(
                &merged.carries,
                &lin_graph,
                &own.lin,
                &mult_graph,
                &own.mult,
            ),
        })
        .collect();
    Solution { schemes, violated }
}

/// SCC の問題を1つの番号の空間に並べ、具体化の記録を展開したもの。
struct Merged {
    lin: Bounds<Linearity>,
    mult: Bounds<Multiplicity>,
    carries: Vec<Carry>,
    /// 各宣言の自分の Kind 変数。まとめた後の番号である。
    own: Vec<OwnVars>,
}

fn merge(members: &[(Decl, &KindProblem)], schemes: &HashMap<Decl, KindScheme>) -> Merged {
    let mut offsets = Vec::new();
    let (mut lin_vars, mut mult_vars) = (0, 0);
    for (_, problem) in members {
        offsets.push((lin_vars, mult_vars));
        lin_vars += problem.lin.vars;
        mult_vars += problem.mult.vars;
    }
    let position: HashMap<Decl, usize> = members
        .iter()
        .enumerate()
        .map(|(index, (decl, _))| (*decl, index))
        .collect();
    let own = members
        .iter()
        .zip(&offsets)
        .map(|((_, problem), &(l, m))| OwnVars {
            lin: problem.own.lin.iter().map(|&v| shift_var(v, l)).collect(),
            mult: problem.own.mult.iter().map(|&v| shift_var(v, m)).collect(),
        })
        .collect();
    let mut merged = Merged {
        lin: Bounds {
            vars: lin_vars,
            ..Bounds::default()
        },
        mult: Bounds {
            vars: mult_vars,
            ..Bounds::default()
        },
        carries: Vec::new(),
        own,
    };
    for ((_, problem), &(l, m)) in members.iter().zip(&offsets) {
        let mut next = (0, 0, 0);
        for instance in &problem.instances {
            merged.copy_own(problem, (l, m), next, instance.at);
            next = instance.at;
            let lin: Vec<KindVar> = instance.lin.iter().map(|&v| shift_var(v, l)).collect();
            let mult: Vec<KindVar> = instance.mult.iter().map(|&v| shift_var(v, m)).collect();
            match position.get(&instance.decl) {
                Some(&callee) => merged.equate(&lin, &mult, callee),
                None => {
                    let scheme = schemes
                        .get(&instance.decl)
                        .expect("a callee is solved before its callers");
                    merged.copy_scheme(scheme, &lin, &mult, instance.origin.as_ref());
                }
            }
        }
        let end = (
            problem.lin.constraints.len(),
            problem.mult.constraints.len(),
            problem.carries.len(),
        );
        merged.copy_own(problem, (l, m), next, end);
    }
    merged
}

impl Merged {
    /// 宣言の自分の制約のうち、`from` から `to` までを番号をずらして足す。
    fn copy_own(
        &mut self,
        problem: &KindProblem,
        (l, m): (usize, usize),
        from: (usize, usize, usize),
        to: (usize, usize, usize),
    ) {
        for index in from.0..to.0 {
            let (lower, upper) = problem.lin.constraints[index];
            self.lin.require(
                shift(lower, l),
                shift(upper, l),
                problem.lin.origins[index].clone(),
            );
        }
        for index in from.1..to.1 {
            let (lower, upper) = problem.mult.constraints[index];
            self.mult.require(
                shift(lower, m),
                shift(upper, m),
                problem.mult.origins[index].clone(),
            );
        }
        for carry in &problem.carries[from.2..to.2] {
            self.carries.push(Carry {
                lin: shift(carry.lin, l),
                mult: shift(carry.mult, m),
                origin: carry.origin.clone(),
            });
        }
    }

    /// 同じ SCC の宣言の参照は、多相化する前の Kind 変数を共有するのと同じ解にする (docs/spec/types.md の「推論」)。
    /// 変数どうしの制約は違反にならないので、由来は付けない。
    fn equate(&mut self, lin: &[KindVar], mult: &[KindVar], callee: usize) {
        for (&v, &w) in lin.iter().zip(&self.own[callee].lin) {
            self.lin.require(Bound::Var(v), Bound::Var(w), None);
            self.lin.require(Bound::Var(w), Bound::Var(v), None);
        }
        for (&v, &w) in mult.iter().zip(&self.own[callee].mult) {
            self.mult.require(Bound::Var(v), Bound::Var(w), None);
            self.mult.require(Bound::Var(w), Bound::Var(v), None);
        }
    }

    /// 前の SCC の宣言のスキームを、具体化した変数について足す。参照した位置の由来 (`Passed`) は、呼んだ関数の中の
    /// 持ち越しを指す `CarriedThrough` にする。呼んだ側の違反が、呼んだ関数の中の呼び出しを指せるようにするため
    /// (docs/spec/diagnostics.md の E3006)。
    fn copy_scheme(
        &mut self,
        scheme: &KindScheme,
        lin: &[KindVar],
        mult: &[KindVar],
        origin: Option<&KindOrigin>,
    ) {
        let rename_lin = |bound: Bound<Linearity>| match bound {
            Bound::Var(v) => Bound::Var(lin[v.index()]),
            constant => constant,
        };
        let rename_mult = |bound: Bound<Multiplicity>| match bound {
            Bound::Var(v) => Bound::Var(mult[v.index()]),
            constant => constant,
        };
        for &(lower, upper) in &scheme.lin {
            self.lin
                .require(rename_lin(lower), rename_lin(upper), origin.cloned());
        }
        for &(lower, upper) in &scheme.mult {
            self.mult
                .require(rename_mult(lower), rename_mult(upper), origin.cloned());
        }
        for carry in &scheme.carries {
            let origin = origin.map(|origin| match &origin.reason {
                KindReason::Passed(name) => KindOrigin {
                    range: origin.range,
                    reason: KindReason::CarriedThrough {
                        name: name.clone(),
                        inner: carry.origin.as_ref().map(CarriedInner::of),
                    },
                },
                _ => origin.clone(),
            });
            self.carries.push(Carry {
                lin: rename_lin(carry.lin),
                mult: rename_mult(carry.mult),
                origin,
            });
        }
    }
}

fn shift_var(var: KindVar, offset: usize) -> KindVar {
    KindVar::from_index(var.index() + offset)
}

fn shift<T>(bound: Bound<T>, offset: usize) -> Bound<T> {
    match bound {
        Bound::Var(v) => Bound::Var(shift_var(v, offset)),
        constant => constant,
    }
}

/// 最小解と、満たせなかった制約 (定数の上限を超えたもの) の番号。上がった変数をワークリストに入れ、上向きの辺に沿って
/// 伝える。束の高さが小さいので、各変数が上がる回数は高々その高さで、時間は制約の数に比例する。
pub(crate) fn solve<T: Level>(bounds: &Bounds<T>) -> (Vec<T>, Vec<usize>) {
    let mut values = vec![T::BOTTOM; bounds.vars];
    let mut upward: Vec<Vec<usize>> = vec![Vec::new(); bounds.vars];
    let mut work = Vec::new();
    for &(lower, upper) in &bounds.constraints {
        match (lower, upper) {
            (Bound::Var(a), Bound::Var(b)) => upward[a.index()].push(b.index()),
            (Bound::Const(c), Bound::Var(b)) if c > values[b.index()] => {
                values[b.index()] = c;
                work.push(b.index());
            }
            _ => {}
        }
    }
    while let Some(v) = work.pop() {
        for &w in &upward[v] {
            if values[v] > values[w] {
                values[w] = values[v];
                work.push(w);
            }
        }
    }
    let value = |bound: Bound<T>| match bound {
        Bound::Const(c) => c,
        Bound::Var(v) => values[v.index()],
    };
    let violated = bounds
        .constraints
        .iter()
        .enumerate()
        .filter(|(_, (lower, upper))| matches!(upper, Bound::Const(c) if value(*lower) > *c))
        .map(|(index, _)| index)
        .collect();
    (values, violated)
}
```

続けて、今の `kind.rs` の `violated_carries` (doc コメントを含む) をそのまま写す。その後に次を足す。

```rust
/// 変数どうしの制約のグラフを強連結成分に縮めたもの。残す制約は成分の DAG の上で求める。同じ循環に入った変数は等しいので、
/// 関数ごとに循環を1周せずに済む。
struct Graph<T> {
    /// 変数ごとの成分の番号。
    component: Vec<usize>,
    /// 成分ごとの上向き (下限から上限へ) と下向きの辺。重複は除いてある。
    upward: Vec<Vec<usize>>,
    downward: Vec<Vec<usize>>,
    /// 成分の変数に付いた定数の上限の最小と、定数の下限の最大。
    upper: Vec<Option<T>>,
    lower: Vec<Option<T>>,
}

impl<T: Level> Graph<T> {
    fn new(bounds: &Bounds<T>) -> Graph<T> {
        let mut edges = vec![Vec::new(); bounds.vars];
        for &(lower, upper) in &bounds.constraints {
            if let (Bound::Var(a), Bound::Var(b)) = (lower, upper) {
                edges[a.index()].push(b.index());
            }
        }
        let (component, count) = components(&edges);
        let mut upward = vec![Vec::new(); count];
        let mut downward = vec![Vec::new(); count];
        for (a, targets) in edges.iter().enumerate() {
            for &b in targets {
                let (from, to) = (component[a], component[b]);
                if from != to {
                    upward[from].push(to);
                    downward[to].push(from);
                }
            }
        }
        for list in upward.iter_mut().chain(downward.iter_mut()) {
            list.sort_unstable();
            list.dedup();
        }
        let mut upper: Vec<Option<T>> = vec![None; count];
        let mut lower: Vec<Option<T>> = vec![None; count];
        for &(l, u) in &bounds.constraints {
            match (l, u) {
                (Bound::Var(v), Bound::Const(c)) => {
                    let slot = &mut upper[component[v.index()]];
                    *slot = Some(slot.map_or(c, |old| old.min(c)));
                }
                (Bound::Const(c), Bound::Var(v)) => {
                    let slot = &mut lower[component[v.index()]];
                    *slot = Some(slot.map_or(c, |old| old.max(c)));
                }
                _ => {}
            }
        }
        Graph {
            component,
            upward,
            downward,
            upper,
            lower,
        }
    }

    /// `start` の成分から辺をたどる。`up` なら上向き、そうでなければ下向きである。`kept` の成分には入らずにそこで止まり、
    /// 出会った `kept` の成分と、通った成分 (`start` を含む) の定数のうち、上向きなら上限の最小、下向きなら下限の最大を
    /// 返す。止まった成分の先は、その成分の残す制約が受け持つ。
    fn reach(
        &self,
        start: usize,
        kept: &HashMap<usize, Vec<usize>>,
        up: bool,
    ) -> (Vec<usize>, Option<T>) {
        let (edges, bounds) = if up {
            (&self.upward, &self.upper)
        } else {
            (&self.downward, &self.lower)
        };
        let pick = |a: T, b: T| if up { a.min(b) } else { a.max(b) };
        let mut found = Vec::new();
        let mut constant = bounds[start];
        let mut seen = HashSet::from([start]);
        let mut work = vec![start];
        while let Some(component) = work.pop() {
            for &next in &edges[component] {
                if !seen.insert(next) {
                    continue;
                }
                if kept.contains_key(&next) {
                    found.push(next);
                    continue;
                }
                if let Some(c) = bounds[next] {
                    constant = Some(constant.map_or(c, |old| pick(old, c)));
                }
                work.push(next);
            }
        }
        (found, constant)
    }
}

/// Tarjan の方法で強連結成分の番号を振る。変数が多くても Rust のスタックを使わないよう、明示的なスタックでたどる
/// (`scc.rs` と同じ形)。
fn components(edges: &[Vec<usize>]) -> (Vec<usize>, usize) {
    let unvisited = usize::MAX;
    let n = edges.len();
    let mut index = vec![unvisited; n];
    let mut low = vec![0; n];
    let mut on_stack = vec![false; n];
    let mut component = vec![unvisited; n];
    let mut stack = Vec::new();
    let mut next = 0;
    let mut count = 0;
    for root in 0..n {
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
                loop {
                    let w = stack.pop().unwrap();
                    on_stack[w] = false;
                    component[w] = count;
                    if w == v {
                        break;
                    }
                }
                count += 1;
            }
        }
    }
    (component, count)
}

/// 自分の変数を含む成分と、その中の自分の変数の `Shape` の番号 (小さい順)。
fn kept_components<T>(graph: &Graph<T>, own: &[KindVar]) -> HashMap<usize, Vec<usize>> {
    let mut kept: HashMap<usize, Vec<usize>> = HashMap::new();
    for (index, var) in own.iter().enumerate() {
        kept.entry(graph.component[var.index()])
            .or_default()
            .push(index);
    }
    kept
}

fn var<T>(index: usize) -> Bound<T> {
    Bound::Var(KindVar::from_index(index))
}

/// 宣言の自分の変数 `own` (`Shape` の番号の順) について残す制約 (docs/spec/types.md の「推論」)。`own` の変数を含む成分を
/// 残す成分とし、残さない成分を通り抜けて最初に出会う残す成分までをたどる。同じ成分に入った自分の変数は等しいので、
/// 等しいことを輪で表し、境界をそのすべてに付ける。結果の変数は `Shape` の番号である。
fn residual<T: Level>(graph: &Graph<T>, own: &[KindVar]) -> Vec<(Bound<T>, Bound<T>)> {
    let kept = kept_components(graph, own);
    let mut out = Vec::new();
    for (&component, members) in &kept {
        if members.len() >= 2 {
            for pair in members.windows(2) {
                out.push((var(pair[0]), var(pair[1])));
            }
            out.push((var(members[members.len() - 1]), var(members[0])));
        }
        let (targets, upper) = graph.reach(component, &kept, true);
        for target in targets {
            out.push((var(members[0]), var(kept[&target][0])));
        }
        let (_, lower) = graph.reach(component, &kept, false);
        for &member in members {
            if let Some(c) = upper {
                out.push((var(member), Bound::Const(c)));
            }
            // 下限が束の最小元なら何も言わないので省く
            if let Some(c) = lower.filter(|c| *c != T::BOTTOM) {
                out.push((Bound::Const(c), var(member)));
            }
        }
    }
    // 正規形にする。同じ意味のスキームが同じ値になり、表示の並びも変数の順になる
    out.sort_by_key(|&(lower, upper)| match (lower, upper) {
        (Bound::Var(a), Bound::Var(b)) => (a.index(), 0, b.index()),
        (Bound::Var(a), Bound::Const(_)) => (a.index(), 1, 0),
        (Bound::Const(_), Bound::Var(b)) => (b.index(), 2, 0),
        (Bound::Const(_), Bound::Const(_)) => (usize::MAX, 3, 0),
    });
    out.dedup();
    out
}

/// 表の制約から、`keep` の変数について残す制約を求める。表の単体テストが使う。
#[cfg(test)]
pub(crate) fn residual_of<T: Level>(
    bounds: &Bounds<T>,
    keep: &[KindVar],
) -> Vec<(Bound<T>, Bound<T>)> {
    residual(&Graph::new(bounds), keep)
}

/// スキームに残す持ち越しの制約 (docs/spec/types.md の「推論」)。両側を下向きにたどり、出会った残す成分の自分の変数と
/// 定数に置き換える。同じ組は1つにまとめ、由来は位置が最も前のものを残す (docs/spec/diagnostics.md の E3006)。由来ごとに
/// 残すと、多相な関数を重ねるたびに制約が増え、同じ違反を何度も報告するためである。
fn carry_residual(
    carries: &[Carry],
    lin_graph: &Graph<Linearity>,
    lin_own: &[KindVar],
    mult_graph: &Graph<Multiplicity>,
    mult_own: &[KindVar],
) -> Vec<Carry> {
    let lin_kept = kept_components(lin_graph, lin_own);
    let mult_kept = kept_components(mult_graph, mult_own);
    let mut best: HashMap<(Bound<Linearity>, Bound<Multiplicity>), Option<&KindOrigin>> =
        HashMap::new();
    for carry in carries {
        let lins = lowers(lin_graph, &lin_kept, carry.lin, Linearity::Lin);
        let mults = lowers(mult_graph, &mult_kept, carry.mult, Multiplicity::Multi);
        for &lin in &lins {
            for &mult in &mults {
                // 両側が定数の組はその本体の中の違反で、解いたときに報告済みである
                if matches!((lin, mult), (Bound::Const(_), Bound::Const(_))) {
                    continue;
                }
                let origin = carry.origin.as_ref();
                best.entry((lin, mult))
                    .and_modify(|kept| {
                        if earlier(origin, *kept) {
                            *kept = origin;
                        }
                    })
                    .or_insert(origin);
            }
        }
    }
    let mut out: Vec<Carry> = best
        .into_iter()
        .map(|((lin, mult), origin)| Carry {
            lin,
            mult,
            origin: origin.cloned(),
        })
        .collect();
    out.sort_by_key(|carry| (bound_key(carry.lin), bound_key(carry.mult)));
    out
}

/// `bound` の下にある残す成分の自分の変数と、下にある定数のうち `top` (値の側なら `Lin`、row の側なら `Multi`)。ほかの
/// 定数は持ち越しの制約を破らないので残さない。
fn lowers<T: Level>(
    graph: &Graph<T>,
    kept: &HashMap<usize, Vec<usize>>,
    bound: Bound<T>,
    top: T,
) -> Vec<Bound<T>> {
    let (vars, constant): (Vec<usize>, Option<T>) = match bound {
        Bound::Const(c) => (Vec::new(), Some(c)),
        Bound::Var(v) => {
            let component = graph.component[v.index()];
            match kept.get(&component) {
                // 残す成分の変数の下限は、その変数の残す制約が受け持つ
                Some(members) => (members.clone(), None),
                None => {
                    let (found, constant) = graph.reach(component, kept, false);
                    let vars = found
                        .iter()
                        .flat_map(|c| kept[c].iter().copied())
                        .collect();
                    (vars, constant)
                }
            }
        }
    };
    let mut out: Vec<Bound<T>> = vars.into_iter().map(var).collect();
    if constant == Some(top) {
        out.push(Bound::Const(top));
    }
    out
}

/// 由来の位置の比べ方。範囲の始まり、終わりの順に比べ、由来のないものは由来のあるものより後に置く。
fn earlier(a: Option<&KindOrigin>, b: Option<&KindOrigin>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => {
            (a.range.start(), a.range.end()) < (b.range.start(), b.range.end())
        }
        (Some(_), None) => true,
        (None, _) => false,
    }
}

/// 正規形の並びの鍵。変数を番号の順に、定数より前に置く。
fn bound_key<T: Level>(bound: Bound<T>) -> (u8, usize, Option<T>) {
    match bound {
        Bound::Var(v) => (0, v.index(), None),
        Bound::Const(c) => (1, 0, Some(c)),
    }
}
```

- [ ] **Step 4: 移すテストと新しいテストを書く**

`kind/solve.rs` の末尾に次を足す。前半の9件は今の `kind.rs` のテストで、`assert` の値は変えずに組み立てだけを新しい API に替えた (種類3)。`residual_constraints_pass_through_internal_variables` と `residual_lower_bounds_above_the_bottom_are_kept` は、残す変数を先に作る順に替えた。残す制約の変数は `Shape` の番号 (残す変数の並びの位置) になるので、表の番号と一致させるためである。

```rust
#[cfg(test)]
mod tests {
    use eml_diagnostics::TextRange;
    use eml_hir::builtin::Builtin;
    use eml_hir::FunctionId;
    use la_arena::RawIdx;

    use super::*;
    use crate::kind::problem::Instance;
    use crate::kind::{CallKind, CarriedValue, InnerLabel};

    fn lattice<T>() -> Bounds<T> {
        Bounds::default()
    }

    fn function(index: u32) -> Decl {
        Decl::Function(FunctionId::from_raw(RawIdx::from(index)))
    }

    fn range(start: u32, end: u32) -> TextRange {
        TextRange::new(start.into(), end.into())
    }

    #[test]
    fn a_residual_carry_replaces_internal_variables_with_kept_ones() {
        let mut linearity = lattice::<Linearity>();
        let a = linearity.fresh();
        let internal = linearity.fresh();
        linearity.require(Bound::Var(a), Bound::Var(internal), None);
        let mut multiplicity = lattice::<Multiplicity>();
        let e = multiplicity.fresh();
        let inner = multiplicity.fresh();
        multiplicity.require(Bound::Var(e), Bound::Var(inner), None);
        let carries = [Carry {
            lin: Bound::Var(internal),
            mult: Bound::Var(inner),
            origin: None,
        }];
        assert_eq!(
            carry_residual(
                &carries,
                &Graph::new(&linearity),
                &[a],
                &Graph::new(&multiplicity),
                &[e]
            ),
            vec![Carry {
                lin: Bound::Var(a),
                mult: Bound::Var(e),
                origin: None,
            }]
        );
    }

    #[test]
    fn a_residual_carry_keeps_one_constant_side() {
        let mut linearity = lattice::<Linearity>();
        let internal = linearity.fresh();
        linearity.require(Bound::Const(Linearity::Lin), Bound::Var(internal), None);
        let mut multiplicity = lattice::<Multiplicity>();
        let e = multiplicity.fresh();
        let carries = [
            Carry {
                lin: Bound::Var(internal),
                mult: Bound::Var(e),
                origin: None,
            },
            Carry {
                lin: Bound::Const(Linearity::Lin),
                mult: Bound::Const(Multiplicity::Multi),
                origin: None,
            },
        ];
        assert_eq!(
            carry_residual(
                &carries,
                &Graph::new(&linearity),
                &[],
                &Graph::new(&multiplicity),
                &[e]
            ),
            vec![Carry {
                lin: Bound::Const(Linearity::Lin),
                mult: Bound::Var(e),
                origin: None,
            }]
        );
    }

    #[test]
    fn unconstrained_variables_take_the_bottom() {
        let mut lattice = lattice::<Linearity>();
        let v = lattice.fresh();
        assert_eq!(solve(&lattice).0[v.index()], Linearity::Unr);
    }

    #[test]
    fn lower_bounds_propagate_through_chains() {
        let mut lattice = lattice::<Linearity>();
        let a = lattice.fresh();
        let b = lattice.fresh();
        lattice.require(Bound::Var(a), Bound::Var(b), None);
        lattice.require(Bound::Const(Linearity::Lin), Bound::Var(a), None);
        assert_eq!(
            solve(&lattice),
            (vec![Linearity::Lin, Linearity::Lin], vec![])
        );
    }

    #[test]
    fn an_upper_bound_below_the_solution_is_reported() {
        let mut lattice = lattice::<Multiplicity>();
        let a = lattice.fresh();
        lattice.require(Bound::Const(Multiplicity::Multi), Bound::Var(a), None);
        lattice.require(Bound::Var(a), Bound::Const(Multiplicity::Once), None);
        assert_eq!(solve(&lattice).1, vec![1]);
    }

    #[test]
    fn residual_constraints_pass_through_internal_variables() {
        let mut lattice = lattice::<Linearity>();
        let a = lattice.fresh();
        let b = lattice.fresh();
        let internal = lattice.fresh();
        lattice.require(Bound::Var(a), Bound::Var(internal), None);
        lattice.require(Bound::Var(internal), Bound::Const(Linearity::Unr), None);
        lattice.require(Bound::Var(a), Bound::Var(b), None);
        assert_eq!(
            residual_of(&lattice, &[a, b]),
            vec![
                (Bound::Var(a), Bound::Var(b)),
                (Bound::Var(a), Bound::Const(Linearity::Unr)),
            ]
        );
    }

    #[test]
    fn residual_lower_bounds_above_the_bottom_are_kept() {
        let mut lattice = lattice::<Linearity>();
        let c = lattice.fresh();
        let internal = lattice.fresh();
        lattice.require(Bound::Const(Linearity::Lin), Bound::Var(internal), None);
        lattice.require(Bound::Var(internal), Bound::Var(c), None);
        lattice.require(Bound::Const(Linearity::Unr), Bound::Var(c), None);
        assert_eq!(
            residual_of(&lattice, &[c]),
            vec![(Bound::Const(Linearity::Lin), Bound::Var(c))]
        );
    }

    #[test]
    fn copied_constraints_use_the_new_variables() {
        let mut problem = KindProblem::default();
        let a = problem.lin.fresh();
        let copy = problem.lin.fresh();
        problem.instances.push(Instance {
            decl: Decl::Builtin(Builtin::IntEq),
            lin: vec![copy],
            mult: vec![],
            origin: None,
            at: (0, 0, 0),
        });
        let scheme = KindScheme {
            lin: vec![(Bound::Const(Linearity::Lin), Bound::Var(KindVar::from_index(0)))],
            ..KindScheme::default()
        };
        let schemes = HashMap::from([(Decl::Builtin(Builtin::IntEq), scheme)]);
        let merged = merge(&[(function(0), &problem)], &schemes);
        let values = solve(&merged.lin).0;
        assert_eq!(values[copy.index()], Linearity::Lin);
        assert_eq!(values[a.index()], Linearity::Unr);
    }

    #[test]
    fn a_carry_breaks_only_when_the_value_is_linear_and_the_row_is_multi() {
        let carry = |lin, mult| Carry {
            lin,
            mult,
            origin: None,
        };
        let carries = [
            carry(Bound::Var(KindVar(0)), Bound::Var(KindVar(0))),
            carry(Bound::Const(Linearity::Lin), Bound::Var(KindVar(1))),
            carry(Bound::Var(KindVar(1)), Bound::Const(Multiplicity::Multi)),
        ];
        let lin = [Linearity::Lin, Linearity::Unr];
        let mult = [Multiplicity::Multi, Multiplicity::Once];
        assert_eq!(violated_carries(&carries, &lin, &mult), vec![0]);
    }

    #[test]
    fn a_worklist_reaches_the_end_of_a_chain_given_in_reverse() {
        let mut lattice = lattice::<Multiplicity>();
        let vars: Vec<KindVar> = (0..4).map(|_| lattice.fresh()).collect();
        for pair in vars.windows(2).rev() {
            lattice.require(Bound::Var(pair[0]), Bound::Var(pair[1]), None);
        }
        lattice.require(Bound::Const(Multiplicity::Multi), Bound::Var(vars[0]), None);
        assert_eq!(solve(&lattice).0, vec![Multiplicity::Multi; 4]);
    }

    #[test]
    fn kept_variables_in_one_cycle_get_the_same_bounds() {
        let mut lattice = lattice::<Linearity>();
        let x = lattice.fresh();
        let y = lattice.fresh();
        let internal = lattice.fresh();
        lattice.require(Bound::Var(x), Bound::Var(y), None);
        lattice.require(Bound::Var(y), Bound::Var(internal), None);
        lattice.require(Bound::Var(internal), Bound::Var(x), None);
        lattice.require(Bound::Var(internal), Bound::Const(Linearity::Unr), None);
        assert_eq!(
            residual_of(&lattice, &[x, y]),
            vec![
                (Bound::Var(x), Bound::Var(y)),
                (Bound::Var(x), Bound::Const(Linearity::Unr)),
                (Bound::Var(y), Bound::Var(x)),
                (Bound::Var(y), Bound::Const(Linearity::Unr)),
            ]
        );
    }

    #[test]
    fn a_reference_within_the_scc_becomes_an_equation() {
        // f は自分の a を g に渡し、g は自分の b を2回使う。f の a にも g の b と同じ上限が付く
        let mut f = KindProblem::default();
        let a = f.lin.fresh();
        let passed = f.lin.fresh();
        f.lin.require(Bound::Var(a), Bound::Var(passed), None);
        f.own.lin = vec![a];
        f.instances.push(Instance {
            decl: function(1),
            lin: vec![passed],
            mult: vec![],
            origin: None,
            at: (1, 0, 0),
        });
        let mut g = KindProblem::default();
        let b = g.lin.fresh();
        g.lin.require(Bound::Var(b), Bound::Const(Linearity::Unr), None);
        g.own.lin = vec![b];
        let solution = solve_scc(&[(function(0), &f), (function(1), &g)], &HashMap::new());
        let unr = vec![(
            Bound::Var(KindVar::from_index(0)),
            Bound::Const(Linearity::Unr),
        )];
        assert_eq!(solution.schemes[0].lin, unr);
        assert_eq!(solution.schemes[1].lin, unr);
        assert!(solution.violated.is_empty());
    }

    #[test]
    fn a_reference_to_an_earlier_scc_copies_its_scheme() {
        let inner = KindOrigin {
            range: range(1, 2),
            reason: KindReason::CarriedAcross {
                value: CarriedValue::Local {
                    name: "x".to_string(),
                    binding: range(0, 1),
                },
                multi: None,
                call: CallKind::Call,
            },
        };
        let keep = KindScheme {
            lin: vec![(
                Bound::Var(KindVar::from_index(0)),
                Bound::Const(Linearity::Unr),
            )],
            mult: vec![],
            carries: vec![Carry {
                lin: Bound::Var(KindVar::from_index(0)),
                mult: Bound::Const(Multiplicity::Multi),
                origin: Some(inner),
            }],
        };
        let mut f = KindProblem::default();
        let a = f.lin.fresh();
        let passed = f.lin.fresh();
        f.lin.require(Bound::Var(a), Bound::Var(passed), None);
        f.own.lin = vec![a];
        f.instances.push(Instance {
            decl: function(9),
            lin: vec![passed],
            mult: vec![],
            origin: Some(KindOrigin {
                range: range(5, 9),
                reason: KindReason::Passed("keep".to_string()),
            }),
            at: (1, 0, 0),
        });
        let schemes = HashMap::from([(function(9), keep)]);
        let solution = solve_scc(&[(function(0), &f)], &schemes);
        assert_eq!(
            solution.schemes[0],
            KindScheme {
                lin: vec![(
                    Bound::Var(KindVar::from_index(0)),
                    Bound::Const(Linearity::Unr),
                )],
                mult: vec![],
                carries: vec![Carry {
                    lin: Bound::Var(KindVar::from_index(0)),
                    mult: Bound::Const(Multiplicity::Multi),
                    origin: Some(KindOrigin {
                        range: range(5, 9),
                        reason: KindReason::CarriedThrough {
                            name: "keep".to_string(),
                            inner: Some(CarriedInner {
                                range: range(1, 2),
                                label: InnerLabel::Kept("x".to_string()),
                            }),
                        },
                    }),
                }],
            }
        );
    }

    #[test]
    fn carries_of_one_pair_keep_the_earliest_origin() {
        let mut linearity = lattice::<Linearity>();
        let a = linearity.fresh();
        let mut multiplicity = lattice::<Multiplicity>();
        let e = multiplicity.fresh();
        let origin = |start, end| {
            Some(KindOrigin {
                range: range(start, end),
                reason: KindReason::Unified,
            })
        };
        let carries = [
            Carry {
                lin: Bound::Var(a),
                mult: Bound::Var(e),
                origin: origin(10, 11),
            },
            Carry {
                lin: Bound::Var(a),
                mult: Bound::Var(e),
                origin: None,
            },
            Carry {
                lin: Bound::Var(a),
                mult: Bound::Var(e),
                origin: origin(3, 4),
            },
        ];
        assert_eq!(
            carry_residual(
                &carries,
                &Graph::new(&linearity),
                &[a],
                &Graph::new(&multiplicity),
                &[e]
            ),
            vec![Carry {
                lin: Bound::Var(a),
                mult: Bound::Var(e),
                origin: origin(3, 4),
            }]
        );
    }
}
```

`a_carry_breaks_only_when_the_value_is_linear_and_the_row_is_multi` は `KindVar(0)` と書いて、`KindVar` の非公開のフィールドを直接使う。`solve.rs` は `kind` の子のモジュールなので、この書き方のまま通る。

- [ ] **Step 5: テストを流す**

Run: `cargo test -p eml_types --lib kind::solve`
Expected: 14件すべて PASS。

Run: `cargo test`
Expected: すべて PASS。`check_module` はまだ古い束を使うので、ほかのテストの結果は変わらない。

- [ ] **Step 6: clippy と fmt**

Run: `cargo clippy --all-targets && cargo fmt`
Expected: 警告なし。

- [ ] **Step 7: コミット**

```bash
git add -A crates/eml_types/src
git commit -m "Add the per-SCC kind solver over table-free kind problems

<末尾の2行>"
```

---

### Task 5: シグネチャの閉じた形 `Shape` を作る

シグネチャを表に下ろしてから閉じた形に写す処理と、閉じた形を表に具体化する処理を作る。まだ `check_module` にはつながない。`Rigids` と `lower` の関数は `scheme.rs` から `shape.rs` に移す。

**Files:**
- Create: `crates/eml_types/src/shape.rs`
- Modify: `crates/eml_types/src/scheme.rs` (`Rigids`、`Arrows`、`lower_signature`、`lower_type`、`lower`、`lower_labels`、`lower_constructor`、`lower_operation`、`with_effect` を `shape.rs` へ移す。`Scheme` だけを残す)
- Modify: `crates/eml_types/src/lib.rs` (`mod shape;`)
- Modify: `crates/eml_types/src/check/{mod,body,handle}.rs` (`use` の付け替え)
- Modify: `crates/eml_types/src/table/mod.rs` (`fresh_rigid_with`、`fresh_rigid_row_with`、`rigid_name`、`row_name`)

**Interfaces:**
- Consumes: Task 1 の `Context`、`Table<'c>`、Task 4 の `KindVar::from_index`
- Produces (`crate::shape`):
  - `Shape { pub ty: ShapeTy, pub rigids: Vec<(String, KindVar)>, pub rows: Vec<(String, KindVar)>, pub lin_vars: usize, pub mult_vars: usize }`
  - `ShapeTy { Con(TypeDefId, Vec<ShapeTy>), Record(Vec<(String, ShapeTy)>), Fn { param: Box<ShapeTy>, lin: ShapeLin, row: ShapeRow, ret: Box<ShapeTy> }, Rigid(usize), Error }`
  - `ShapeLin { Known(Linearity), Var(KindVar) }`、`ShapeRow { pub labels: Vec<(EffectId, Vec<ShapeTy>)>, pub tail: ShapeTail }`、`ShapeTail { Closed, Rigid(usize), Error }`
  - `signature_shape(context: &Context, signature: &Signature) -> Shape`、`operation_shape(context: &Context, operation: &Operation) -> Shape`、`constructor_shape(context: &Context, def: &TypeDef, constructor: &Constructor) -> Shape`
  - `Shape::instantiate(&self, table: &mut Table<'_>) -> Instantiated`、`Instantiated { pub ty: Ty, pub lin: Vec<KindVar>, pub mult: Vec<KindVar> }`
  - `Shape::instantiate_rigid(&self, table: &mut Table<'_>, generics: &Generics) -> Own`、`Own { pub ty: Ty, pub rigids: Rigids, pub lin: Vec<KindVar>, pub mult: Vec<KindVar> }`
  - `Shape::export(&self, context: &Context) -> Type`、`Shape::kind_names(&self, context: &Context) -> HashMap<KindVar, Type>`、`Shape::row_names(&self) -> HashMap<KindVar, String>`
  - `Rigids` (今と同じ API に `vars(&self) -> &[RigidVar]` と `row_vars(&self) -> Vec<RowVar>` を足す)
- Produces (`Table`):
  - `fresh_rigid_with(&mut self, name: &str, linearity: KindVar) -> (Ty, RigidVar)`
  - `fresh_rigid_row_with(&mut self, name: &str, multiplicity: KindVar) -> RowVar`
  - `rigid_name(&self, rigid: RigidVar) -> &str`、`row_name(&self, var: RowVar) -> &str` (rigid な row 変数だけに呼ぶ)

- [ ] **Step 1: `Rigids` と `lower` を移す**

`scheme.rs` の 13-64行 (`Rigids` とその `impl`) と 135-325行 (`Arrows` から `with_effect` まで) を、そのまま `shape.rs` の先頭に写して `scheme.rs` から消す。`scheme.rs` に残るのは `Scheme` とその `impl` である。`Scheme::new` は `rigids.vars` と `rigids.rows` を読むので、`shape.rs` の `Rigids` に次のアクセサを足し、`Scheme::new` はそれを使う。

```rust
    /// rigid な型変数。`Generics` の並びの順である (エフェクトの型引数に写した変数を除く)。
    pub fn vars(&self) -> &[RigidVar] {
        &self.vars
    }

    /// rigid な row 変数。`Generics` の並びの順である。
    pub fn row_vars(&self) -> Vec<RowVar> {
        self.rows.values().copied().collect()
    }
```

`Scheme::new` は `rigids: rigids.vars().to_vec(), rigid_rows: rigids.row_vars(),` にする。

`lib.rs` の `mod scheme;` の次に `mod shape;` を足す。Task 6 でつなぐまで、閉じた形の関数は使われないので、次の属性を付ける。

```rust
// 閉じた形は R5 の Task 6 で check_module につなぐまで、テストからだけ使われる
#[allow(dead_code)]
mod shape;
```

`check/mod.rs`、`check/body.rs`、`check/handle.rs` の `use crate::scheme::{...}` のうち、`Rigids` と `lower_*` を `use crate::shape::{...}` に付け替える。

- [ ] **Step 2: 表に rigid 変数の作り方を足す**

`table/mod.rs` の `fresh_rigid` と `fresh_rigid_row` を、Kind 変数を受け取る形に分ける。

```rust
    pub fn fresh_rigid(&mut self, name: &str) -> (Ty, RigidVar) {
        let linearity = self.linearity.fresh();
        self.fresh_rigid_with(name, linearity)
    }

    /// Kind 変数 `μ` を決めて rigid 変数を作る。閉じた形を自分の本体のために具体化するときに使う。
    pub fn fresh_rigid_with(&mut self, name: &str, linearity: KindVar) -> (Ty, RigidVar) {
        self.rigids.push(RigidInfo {
            name: name.to_string(),
            linearity,
        });
        let rigid = RigidVar(self.rigids.len() as u32 - 1);
        (self.alloc(TyShape::Rigid(rigid)), rigid)
    }

    pub fn rigid_name(&self, rigid: RigidVar) -> &str {
        &self.rigids[rigid.0 as usize].name
    }

    pub fn fresh_rigid_row(&mut self, name: &str) -> RowVar {
        let multiplicity = self.multiplicity.fresh();
        self.fresh_rigid_row_with(name, multiplicity)
    }

    /// Kind 変数 `σ` を決めて rigid な row 変数を作る。
    pub fn fresh_rigid_row_with(&mut self, name: &str, multiplicity: KindVar) -> RowVar {
        let var = self.fresh_row_var_with(multiplicity);
        self.row_vars[var.0 as usize].rigid = Some(name.to_string());
        var
    }

    /// rigid な row 変数の名前。
    pub fn row_name(&self, var: RowVar) -> &str {
        self.row_vars[var.0 as usize]
            .rigid
            .as_deref()
            .expect("only rigid row variables have names")
    }
```

- [ ] **Step 3: 失敗するテストを書く**

`shape.rs` の末尾に次を足す。ソースから HIR を作るのは `scc.rs` の単体テストと同じ方法である。

```rust
#[cfg(test)]
mod tests {
    use eml_diagnostics::SourceFiles;
    use eml_hir::Module;

    use super::*;

    fn module(text: &str) -> Module {
        let mut files = SourceFiles::new();
        let file = files.add("test.em", text);
        let (parse, _) = eml_syntax::parse(file, text);
        eml_hir::lower(file, &parse.tree()).0
    }

    fn context(module: &Module) -> Context {
        Context::new(
            module.lang,
            &module.types,
            &module.constructors,
            &module.effects,
            &module.operations,
        )
    }

    /// ソースの最初の関数のシグネチャ。
    fn signature(module: &Module) -> &Signature {
        module
            .functions
            .iter()
            .next()
            .and_then(|(_, function)| function.signature.as_ref())
            .unwrap()
    }

    const TWICE: &str = "twice : (a -> <e> a) -> a -> <e> a\ntwice f x = f (f x)";

    #[test]
    fn a_shape_is_exported_like_its_signature() {
        let module = module(TWICE);
        let context = context(&module);
        let shape = signature_shape(&context, signature(&module));
        assert_eq!(
            shape.export(&context).to_string(),
            "(a -> <e> a) -> a -> <e> a"
        );
    }

    #[test]
    fn shape_numbers_follow_the_order_of_kind_vars() {
        let module = module(TWICE);
        let context = context(&module);
        let signature = signature(&module);
        let shape = signature_shape(&context, signature);
        let mut table = Table::new(&context);
        let own = shape.instantiate_rigid(&mut table, &signature.generics);
        assert_eq!(table.kind_vars(own.ty), (own.lin.clone(), own.mult.clone()));
        assert_eq!(
            table.export(own.ty).to_string(),
            "(a -> <e> a) -> a -> <e> a"
        );
    }

    #[test]
    fn instantiation_replaces_rigid_variables_and_rows() {
        let module = module("f : a -> <e> a\nf x = x");
        let context = context(&module);
        let shape = signature_shape(&context, signature(&module));
        let mut table = Table::new(&context);
        let first = shape.instantiate(&mut table);
        let second = shape.instantiate(&mut table);
        assert_eq!(table.export(first.ty).to_string(), "_ -> <_> _");
        assert_ne!(first.lin, second.lin);
        assert_ne!(first.mult, second.mult);
    }

    #[test]
    fn an_error_row_survives_closing_and_instantiation() {
        let module = module("f : Int -> <Missing> Int\nf x = x");
        let context = context(&module);
        let shape = signature_shape(&context, signature(&module));
        assert_eq!(shape.export(&context).to_string(), "Int -> <{error}> Int");
        let mut table = Table::new(&context);
        let instance = shape.instantiate(&mut table);
        assert_eq!(
            table.export(instance.ty).to_string(),
            "Int -> <{error}> Int"
        );
    }
}
```

`instantiation_replaces_rigid_variables_and_rows` と `an_error_row_survives_closing_and_instantiation` は、Task 6 で消す `table/tests.rs` の `copy_type_replaces_rigid_variables` と `copying_keeps_an_error_row` の意図を引き継ぐ。

Run: `cargo test -p eml_types --lib shape`
Expected: FAIL (`signature_shape` などが未定義でコンパイルできない)。

- [ ] **Step 4: 閉じた形を書く**

`shape.rs` の先頭の doc コメントを次にし、移した `Rigids` と `lower` の後に、閉じた形の定義と処理を足す。

```rust
//! シグネチャの閉じた型の形と、型の注釈を型の表に下ろす処理 (docs/spec/types.md の「推論」)。閉じた形は型の表を指さず、
//! 変数をすべてスキームの中の番号で持つ。シグネチャが必須なので、宣言の型の形はシグネチャだけで決まり、本体の検査は
//! 呼び出し先の形だけを見ればよい。
```

```rust
/// 宣言の閉じた型の形。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Shape {
    pub ty: ShapeTy,
    /// rigid な型変数。`Generics` の並びの順に、名前と Kind 変数 `μ` の番号を持つ。
    pub rigids: Vec<(String, KindVar)>,
    /// rigid な row 変数。`Generics` の並びの順に、名前と Kind 変数 `σ` の番号を持つ。
    pub rows: Vec<(String, KindVar)>,
    /// 線形性の Kind 変数 (`μ` と矢印の `m`) の数。
    pub lin_vars: usize,
    /// 多重度の Kind 変数 (`σ`) の数。
    pub mult_vars: usize,
}

/// 閉じた形の型。推論用の変数と継続の型は、シグネチャから作る型に現れないので持たない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ShapeTy {
    Con(TypeDefId, Vec<ShapeTy>),
    Record(Vec<(String, ShapeTy)>),
    Fn {
        param: Box<ShapeTy>,
        lin: ShapeLin,
        row: ShapeRow,
        ret: Box<ShapeTy>,
    },
    /// `Shape::rigids` の番号。
    Rigid(usize),
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShapeLin {
    Known(Linearity),
    Var(KindVar),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShapeRow {
    pub labels: Vec<(EffectId, Vec<ShapeTy>)>,
    pub tail: ShapeTail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShapeTail {
    Closed,
    /// `Shape::rows` の番号。
    Rigid(usize),
    Error,
}

/// 多相な具体化の結果。Kind 変数は `Shape` の番号の順に並ぶ。
pub(crate) struct Instantiated {
    pub ty: Ty,
    pub lin: Vec<KindVar>,
    pub mult: Vec<KindVar>,
}

/// 自分の本体のための rigid な具体化の結果。
pub(crate) struct Own {
    pub ty: Ty,
    pub rigids: Rigids,
    pub lin: Vec<KindVar>,
    pub mult: Vec<KindVar>,
}

/// 段0: 関数と組み込みのシグネチャの形。使い捨ての表に下ろしてから閉じる。下ろす処理を本体の注釈と共有するため。
pub(crate) fn signature_shape(context: &Context, signature: &Signature) -> Shape {
    let mut table = Table::new(context);
    let rigids = Rigids::new(&mut table, &signature.generics);
    let ty = lower_signature(&mut table, signature, &rigids);
    close(&table, ty, &rigids)
}

pub(crate) fn operation_shape(context: &Context, operation: &Operation) -> Shape {
    let mut table = Table::new(context);
    let rigids = Rigids::new(&mut table, &operation.signature.generics);
    let ty = lower_operation(&mut table, operation, &rigids);
    close(&table, ty, &rigids)
}

pub(crate) fn constructor_shape(
    context: &Context,
    def: &TypeDef,
    constructor: &Constructor,
) -> Shape {
    let mut table = Table::new(context);
    let rigids = Rigids::new(&mut table, &def.generics);
    let ty = lower_constructor(&mut table, def, constructor, &rigids);
    close(&table, ty, &rigids)
}

/// 表に下ろした型を閉じた形にする。Kind 変数の番号は、`Table::kind_vars` と同じ順 (外側から、矢印の `m`、row の `σ`、
/// 引数、ラベルの型引数、戻り値の順) にたどって、最初に現れた順に振る。型に現れない rigid 変数の Kind 変数は、その後に
/// 振る。
fn close(table: &Table<'_>, ty: Ty, rigids: &Rigids) -> Shape {
    let mut closer = Closer {
        table,
        rigid_index: rigids
            .vars()
            .iter()
            .enumerate()
            .map(|(index, &rigid)| (rigid, index))
            .collect(),
        row_index: rigids
            .row_vars()
            .into_iter()
            .enumerate()
            .map(|(index, row)| (row, index))
            .collect(),
        lin: HashMap::new(),
        mult: HashMap::new(),
    };
    let ty = closer.ty(ty);
    let rigid_list = rigids
        .vars()
        .iter()
        .map(|&rigid| {
            let mu = closer.lin_var(table.rigid_linearity(rigid));
            (table.rigid_name(rigid).to_string(), mu)
        })
        .collect();
    let row_list = rigids
        .row_vars()
        .into_iter()
        .map(|row| {
            let sigma = closer.mult_var(table.row_multiplicity_var(row));
            (table.row_name(row).to_string(), sigma)
        })
        .collect();
    Shape {
        ty,
        rigids: rigid_list,
        rows: row_list,
        lin_vars: closer.lin.len(),
        mult_vars: closer.mult.len(),
    }
}

struct Closer<'t, 'c> {
    table: &'t Table<'c>,
    /// 表の rigid 変数と rigid な row 変数から、`Shape` の番号への対応。
    rigid_index: HashMap<RigidVar, usize>,
    row_index: HashMap<RowVar, usize>,
    /// 表の Kind 変数から `Shape` の番号への対応。
    lin: HashMap<KindVar, KindVar>,
    mult: HashMap<KindVar, KindVar>,
}

impl Closer<'_, '_> {
    fn lin_var(&mut self, var: KindVar) -> KindVar {
        let next = KindVar::from_index(self.lin.len());
        *self.lin.entry(var).or_insert(next)
    }

    fn mult_var(&mut self, var: KindVar) -> KindVar {
        let next = KindVar::from_index(self.mult.len());
        *self.mult.entry(var).or_insert(next)
    }

    fn ty(&mut self, ty: Ty) -> ShapeTy {
        match self.table.shape(ty).clone() {
            TyShape::Con(id, args) => {
                ShapeTy::Con(id, args.into_iter().map(|arg| self.ty(arg)).collect())
            }
            TyShape::Record(fields) => ShapeTy::Record(
                fields
                    .into_iter()
                    .map(|(label, field)| (label, self.ty(field)))
                    .collect(),
            ),
            TyShape::Fn {
                param,
                lin,
                row,
                ret,
            } => {
                let lin = match lin {
                    ArrowLin::Known(l) => ShapeLin::Known(l),
                    ArrowLin::Var(v) => ShapeLin::Var(self.lin_var(v)),
                };
                let row = self.table.resolve_row(&row);
                let tail = match row.tail {
                    Tail::Closed => ShapeTail::Closed,
                    Tail::Error => ShapeTail::Error,
                    // シグネチャの row 変数はどれも rigid である
                    Tail::Var(var) => {
                        self.mult_var(self.table.row_multiplicity_var(var));
                        ShapeTail::Rigid(self.row_index[&var])
                    }
                };
                let param = self.ty(param);
                let labels = row
                    .labels
                    .into_iter()
                    .map(|label| {
                        let args = label.args.into_iter().map(|arg| self.ty(arg)).collect();
                        (label.effect, args)
                    })
                    .collect();
                let ret = self.ty(ret);
                ShapeTy::Fn {
                    param: Box::new(param),
                    lin,
                    row: ShapeRow { labels, tail },
                    ret: Box::new(ret),
                }
            }
            TyShape::Rigid(rigid) => {
                self.lin_var(self.table.rigid_linearity(rigid));
                ShapeTy::Rigid(self.rigid_index[&rigid])
            }
            TyShape::Error => ShapeTy::Error,
            TyShape::Var(_) | TyShape::Cont { .. } => {
                unreachable!("a signature has no inference variables or continuations")
            }
        }
    }
}

impl Shape {
    /// 多相な具体化。rigid な型変数を新しい推論用の変数に、rigid な row 変数を新しい row 変数に、Kind 変数を新しい変数に
    /// する (docs/spec/types.md の「推論」)。
    pub fn instantiate(&self, table: &mut Table<'_>) -> Instantiated {
        let lin: Vec<KindVar> = (0..self.lin_vars).map(|_| table.fresh_lin_var()).collect();
        let mult: Vec<KindVar> = (0..self.mult_vars).map(|_| table.fresh_mult_var()).collect();
        let tys: Vec<Ty> = self
            .rigids
            .iter()
            .map(|(_, mu)| table.fresh_var_with(lin[mu.index()]))
            .collect();
        let rows: Vec<Tail> = self
            .rows
            .iter()
            .map(|(_, sigma)| Tail::Var(table.fresh_row_var_with(mult[sigma.index()])))
            .collect();
        let ty = build(table, &self.ty, &tys, &rows, &lin);
        Instantiated { ty, lin, mult }
    }

    /// 自分の本体のための rigid な具体化。rigid な変数を表の rigid 変数にし、Kind 変数を新しい変数にする。本体の注釈が
    /// 同じ変数を指せるよう、`generics` の ID から表への対応 (`Rigids`) も返す。
    pub fn instantiate_rigid(&self, table: &mut Table<'_>, generics: &Generics) -> Own {
        let lin: Vec<KindVar> = (0..self.lin_vars).map(|_| table.fresh_lin_var()).collect();
        let mult: Vec<KindVar> = (0..self.mult_vars).map(|_| table.fresh_mult_var()).collect();
        let mut rigids = Rigids {
            tys: ArenaMap::default(),
            rows: ArenaMap::default(),
            vars: Vec::new(),
        };
        let mut tys = Vec::new();
        for ((id, _), (name, mu)) in generics.type_vars.iter().zip(&self.rigids) {
            let (ty, rigid) = table.fresh_rigid_with(name, lin[mu.index()]);
            rigids.tys.insert(id, ty);
            rigids.vars.push(rigid);
            tys.push(ty);
        }
        let mut rows = Vec::new();
        for ((id, _), (name, sigma)) in generics.row_vars.iter().zip(&self.rows) {
            let var = table.fresh_rigid_row_with(name, mult[sigma.index()]);
            rigids.rows.insert(id, var);
            rows.push(Tail::Var(var));
        }
        let ty = build(table, &self.ty, &tys, &rows, &lin);
        Own {
            ty,
            rigids,
            lin,
            mult,
        }
    }

    /// 後の段階に渡す型。rigid な変数は名前で書く。
    pub fn export(&self, context: &Context) -> Type {
        self.export_ty(&self.ty, context)
    }

    fn export_ty(&self, ty: &ShapeTy, context: &Context) -> Type {
        match ty {
            ShapeTy::Con(id, args) => Type::Con {
                id: *id,
                name: context.type_names[*id].clone(),
                args: args.iter().map(|arg| self.export_ty(arg, context)).collect(),
            },
            ShapeTy::Record(fields) => Type::Record(
                fields
                    .iter()
                    .map(|(label, field)| (label.clone(), self.export_ty(field, context)))
                    .collect(),
            ),
            ShapeTy::Fn {
                param, row, ret, ..
            } => {
                let effects = row
                    .labels
                    .iter()
                    .map(|(effect, args)| EffectLabel {
                        id: *effect,
                        name: context.effect_names[*effect].clone(),
                        args: args.iter().map(|arg| self.export_ty(arg, context)).collect(),
                    })
                    .collect();
                let tail = match row.tail {
                    ShapeTail::Closed => None,
                    ShapeTail::Rigid(index) => Some(RowTail::Rigid(self.rows[index].0.clone())),
                    ShapeTail::Error => Some(RowTail::Error),
                };
                Type::Fn {
                    param: Box::new(self.export_ty(param, context)),
                    effects,
                    tail,
                    ret: Box::new(self.export_ty(ret, context)),
                }
            }
            ShapeTy::Rigid(index) => Type::Rigid(self.rigids[*index].0.clone()),
            ShapeTy::Error => Type::Error,
        }
    }

    /// 線形性の Kind 変数の表示名。rigid な型変数の `μ` はその型変数、矢印の `m` はその矢印の型で呼ぶ。外側から順に見て、
    /// 最初に現れた部分を使う。スキームに残った制約を表示するのに使う。
    pub fn kind_names(&self, context: &Context) -> HashMap<KindVar, Type> {
        let mut names = HashMap::new();
        self.collect_names(&self.ty, context, &mut names);
        names
    }

    fn collect_names(
        &self,
        ty: &ShapeTy,
        context: &Context,
        names: &mut HashMap<KindVar, Type>,
    ) {
        match ty {
            ShapeTy::Rigid(index) => {
                let (name, mu) = &self.rigids[*index];
                names.entry(*mu).or_insert_with(|| Type::Rigid(name.clone()));
            }
            ShapeTy::Fn {
                param,
                lin,
                row,
                ret,
            } => {
                if let ShapeLin::Var(v) = lin {
                    names
                        .entry(*v)
                        .or_insert_with(|| self.export_ty(ty, context));
                }
                self.collect_names(param, context, names);
                for (_, args) in &row.labels {
                    for arg in args {
                        self.collect_names(arg, context, names);
                    }
                }
                self.collect_names(ret, context, names);
            }
            ShapeTy::Record(fields) => {
                for (_, field) in fields {
                    self.collect_names(field, context, names);
                }
            }
            ShapeTy::Con(_, args) => {
                for arg in args {
                    self.collect_names(arg, context, names);
                }
            }
            ShapeTy::Error => {}
        }
    }

    /// 多重度の Kind 変数の表示名。rigid な row 変数の `σ` はその名前で呼ぶ。
    pub fn row_names(&self) -> HashMap<KindVar, String> {
        self.rows
            .iter()
            .map(|(name, sigma)| (*sigma, name.clone()))
            .collect()
    }
}

/// 閉じた形を表に組み立てる。`tys` と `rows` は rigid な変数の置き換え先、`lin` は線形性の Kind 変数の置き換え先である。
fn build(table: &mut Table<'_>, ty: &ShapeTy, tys: &[Ty], rows: &[Tail], lin: &[KindVar]) -> Ty {
    match ty {
        ShapeTy::Con(id, args) => {
            let args = args
                .iter()
                .map(|arg| build(table, arg, tys, rows, lin))
                .collect();
            table.alloc(TyShape::Con(*id, args))
        }
        ShapeTy::Record(fields) if fields.is_empty() => table.unit,
        ShapeTy::Record(fields) => {
            let fields = fields
                .iter()
                .map(|(label, field)| (label.clone(), build(table, field, tys, rows, lin)))
                .collect();
            table.alloc(TyShape::Record(fields))
        }
        ShapeTy::Fn {
            param,
            lin: arrow,
            row,
            ret,
        } => {
            let param = build(table, param, tys, rows, lin);
            let labels = row
                .labels
                .iter()
                .map(|(effect, args)| Label {
                    effect: *effect,
                    args: args
                        .iter()
                        .map(|arg| build(table, arg, tys, rows, lin))
                        .collect(),
                })
                .collect();
            let tail = match row.tail {
                ShapeTail::Closed => Tail::Closed,
                ShapeTail::Rigid(index) => rows[index],
                ShapeTail::Error => Tail::Error,
            };
            let ret = build(table, ret, tys, rows, lin);
            let arrow = match arrow {
                ShapeLin::Known(l) => ArrowLin::Known(*l),
                ShapeLin::Var(v) => ArrowLin::Var(lin[v.index()]),
            };
            table.function_with(param, arrow, Row { labels, tail }, ret)
        }
        ShapeTy::Rigid(index) => tys[*index],
        ShapeTy::Error => table.error,
    }
}
```

`use` は、`std::collections::HashMap`、`eml_hir::{Constructor, EffectId, Generics, Operation, Signature, TypeDef, TypeDefId}` (と移した `lower` が使うもの)、`crate::context::Context`、`crate::kind::KindVar`、`crate::table::{ArrowLin, Label, RigidVar, Row, RowVar, Table, Tail, Ty, TyShape}`、`crate::ty::{EffectLabel, Linearity, RowTail, Type}` を、使うものだけ並べる。

- [ ] **Step 5: テストを流す**

Run: `cargo test -p eml_types --lib shape`
Expected: 4件すべて PASS。

Run: `cargo test`
Expected: すべて PASS。

- [ ] **Step 6: clippy と fmt**

Run: `cargo clippy --all-targets && cargo fmt`
Expected: 警告なし。

- [ ] **Step 7: コミット**

```bash
git add -A crates/eml_types/src
git commit -m "Add closed signature shapes and move type lowering into shape.rs

<末尾の2行>"
```

---

### Task 6: 本体の検査と Kind の解決を2段に分ける

`check_module` を、段0 (`signatures`)、段1 (`check_body`)、段2 (`solve_scc`) の組み立てに書き換える。型の表は Kind の制約を集めるだけになり、古い束 (`Lattice`)、古いスキーム (`scheme.rs`)、表の複写 (`copy.rs`) を消す。

**Files:**
- Modify: `crates/eml_types/src/check/mod.rs` (書き直す)
- Modify: `crates/eml_types/src/check/body.rs` (`BodyCheck` のフィールド、`value`、`reference`、`constructor_pattern`)
- Modify: `crates/eml_types/src/table/mod.rs` (`Bounds` への置き換え、`Subst` を消す)
- Modify: `crates/eml_types/src/table/{unify,row,kinds,export}.rs`
- Delete: `crates/eml_types/src/table/copy.rs`、`crates/eml_types/src/scheme.rs`
- Modify: `crates/eml_types/src/kind/mod.rs` (`Lattice`、`carry_residual`、`lowers`、`reach`、`push_constraint` を消す。`#[allow(dead_code)]` を外す)
- Modify: `crates/eml_types/src/lib.rs` (`mod scheme;` を消し、`mod shape;` の `#[allow(dead_code)]` を外す)
- Test: `crates/eml_types/src/table/tests.rs`、`crates/eml_types/tests/linearity.rs`、`crates/eml_types/tests/check.rs`

**Interfaces:**
- Consumes: Task 1〜5 のすべて
- Produces:
  - `check::Signatures { pub functions: ArenaMap<FunctionId, Shape>, pub builtins: HashMap<Builtin, Shape>, pub operations: ArenaMap<OperationId, Shape>, pub constructors: ArenaMap<ConstructorId, Shape> }` と `Signatures::get(&self, decl: Decl) -> Option<&Shape>`
  - `check::signatures(module: &Module, context: &Context) -> Signatures`
  - `check::check_body(module: &Module, context: &Context, signatures: &Signatures, id: FunctionId) -> Option<(Checked, Vec<Diagnostic>)>`、`Checked { pub types: BodyTypes, pub problem: KindProblem }`
  - `Table::kind_origin(&self) -> Option<KindOrigin>`、`Table::kind_counts(&self) -> (usize, usize, usize)`、`Table::into_problem(self, instances: Vec<Instance>, own: OwnVars) -> KindProblem`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/linearity.rs` の `a_carry_over_passes_through_two_functions` の後に次を足す。`keep3` は、自分の中の `action ()` と、`keep2` を通る2つの持ち越しで、同じ `(a, e)` の組の持ち越しを3つ持つ。今は組ごとにまとめないので、E3006 が複数出る。

```rust
#[test]
fn a_carry_over_through_three_functions_is_reported_once() {
    let rest = "keep2 : a -> (Unit -> <e> Unit) -> <e> a\nkeep2 x action =\n  action ()\n  keep x action\n\nkeep3 : a -> (Unit -> <e> Unit) -> <e> a\nkeep3 x action =\n  action ()\n  keep2 x action\n\nkept3 : Unit -> <Choice, IO> Unit\nkept3 () =\n  let f = open \"a.txt\"\n  let g = keep3 f chooser\n  close g";
    insta::assert_snapshot!(polymorphic(rest), @r"
    E3006 43:11 `keep3` keeps a linear value alive across a call that may resume more than once
      43:11 `keep3` is used here
      37:3 `x` is kept alive across this call
      note: a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again
    ");
}
```

`crates/eml_types/tests/check.rs` の `kinds_are_shared_within_a_strongly_connected_component` の後に次を足す。

```rust
#[test]
fn kinds_are_shared_around_a_ring_of_functions() {
    let text = "f0 : a -> Int -> a\nf0 x n = if n == 0 then x else f1 x (n - 1)\n\nf1 : b -> Int -> b\nf1 y n = if n == 0 then y else f2 y (n - 1)\n\nf2 : c -> Int -> c\nf2 z n = if n == 0 then first z z else f0 z (n - 1)\n\nfirst : d -> d -> d\nfirst u v = u";
    insta::assert_snapshot!(check_text(text), @r"
    f0 : a -> Int -> a
      kinds: a <= Unr
      x#0 : a
      n#1 : Int
    f1 : b -> Int -> b
      kinds: b <= Unr
      y#0 : b
      n#1 : Int
    f2 : c -> Int -> c
      kinds: c <= Unr
      z#0 : c
      n#1 : Int
    first : d -> d -> d
      kinds: d <= Unr
      u#0 : d
      v#1 : d
    ");
}

#[test]
fn a_reference_to_a_function_without_equations_is_checked() {
    let text = "f : Int -> Int\n\ng : Int -> Int\ng x = f x";
    insta::assert_snapshot!(check_text(text), @r"
    f : Int -> Int
    g : Int -> Int
      x#0 : Int
    ---
    E1005 1:1 `f` has a signature but no equation
      1:1 add an equation for `f` after this signature
    ");
}
```

Run: `cargo test -p eml_types --test linearity a_carry_over_through_three_functions_is_reported_once`
Expected: FAIL。E3006 が2件出る。1件は `37:3` の `x` を指し、もう1件は `38:3 through this use of \`keep2\`` を指す。`keep2` を通る2つの持ち越しは、Task 3 で `inner` を1段の要約にしたため由来が同じになり、報告の重複除去 (`dedup`) で1件にまとまる。R5 の前は3件だった。

Run: `cargo test -p eml_types --test check`
Expected: 新しい2件は今の実装でも PASS する (振る舞いを固定するテスト)。PASS しなければ、出力をユーザーに示して止まる。

- [ ] **Step 2: 表を、制約を集めるだけにする**

`table/mod.rs` を次のように直す。

- `use` に `crate::kind::problem::{Bounds, Instance, KindProblem, OwnVars};` を足し、`Lattice` を除く
- フィールド `linearity: Lattice<Linearity>` と `multiplicity: Lattice<Multiplicity>` を次にする

  ```rust
      /// 線形性の束の制約。段1は集めるだけで、解くのは段2である (docs/implementation/architecture.md の「`eml_types` の内部」)。
      linearity: Bounds<Linearity>,
      multiplicity: Bounds<Multiplicity>,
  ```

  `Table::new` の初期化は `Bounds::default()` にする
- `Subst` と `mod copy;` を消し、`git rm crates/eml_types/src/table/copy.rs` で消す
- `set_kind_origin` を次にする

  ```rust
      /// これから作る Kind の制約の由来を設定し、前の由来を返す。呼び出し側は、制約を作る処理の後で前の由来に戻す。
      pub fn set_kind_origin(&mut self, origin: Option<KindOrigin>) -> Option<KindOrigin> {
          std::mem::replace(&mut self.kind_origin, origin)
      }
  ```

- `row_multiplicity` を消す
- 次を足す

  ```rust
      pub fn kind_origin(&self) -> Option<KindOrigin> {
          self.kind_origin.clone()
      }

      /// 線形性の制約、多重度の制約、持ち越しの制約の数。具体化の記録に、展開する位置として残す。
      pub fn kind_counts(&self) -> (usize, usize, usize) {
          (
              self.linearity.constraints.len(),
              self.multiplicity.constraints.len(),
              self.carries.len(),
          )
      }

      /// 段1の終わりに、集めた Kind の制約を取り出して表を捨てる。
      pub fn into_problem(self, instances: Vec<Instance>, own: OwnVars) -> KindProblem {
          KindProblem {
              lin: self.linearity,
              mult: self.multiplicity,
              carries: self.carries,
              instances,
              own,
          }
      }

      fn require_lin(&mut self, lower: Bound<Linearity>, upper: Bound<Linearity>) {
          self.linearity.require(lower, upper, self.kind_origin.clone());
      }

      fn require_mult(&mut self, lower: Bound<Multiplicity>, upper: Bound<Multiplicity>) {
          self.multiplicity
              .require(lower, upper, self.kind_origin.clone());
      }
  ```

`self.linearity.require(X, Y)` を `self.require_lin(X, Y)` に、`self.multiplicity.require(X, Y)` を `self.require_mult(X, Y)` にする。当たる箇所は、`unify.rs` の `bind_var` と `unify_arrow_lin`、`row.rs` の `bind_row`、`kinds.rs` の `kind_at_most` と `unrestricted` である。

`table/kinds.rs` から `carry_residual`、`copy_carries`、`row_names`、`lin_residual`、`mult_residual`、`copy_lin_constraints`、`copy_mult_constraints`、`solve_kinds` を消す。`kind_bounds`、`kind_at_most`、`carry`、`row_multiplicities`、`closure_kinds`、`unrestricted`、`kind_vars`、`push_unique` は残す。`table/export.rs` から `kind_names` と `push_label_args` を消す (`Shape::kind_names` が受け持つ)。

`kind/mod.rs` から `Lattice` とその `impl`、`carry_residual`、`lowers`、`reach`、`push_constraint` を消し、`mod problem;` と `mod solve;` の `#[allow(dead_code)]` と、その上のコメントを消す。

- [ ] **Step 3: 本体の検査が具体化を記録するようにする**

`check/body.rs` の `BodyCheck` から `schemes`、`builtins`、`operations`、`constructors` を消し、次を足す。

```rust
    /// 全宣言の閉じた型の形。本体の検査が呼び出し先について見るのは、これだけである (docs/spec/types.md の「推論」)。
    pub(super) signatures: &'a Signatures,
    /// 参照の具体化の記録。段2が展開する。
    pub(super) instances: Vec<Instance>,
```

`reference` を消し、代わりに次を足す。

```rust
    /// トップレベルの値を参照するたびに、宣言の型の形を具体化する。呼び出し先の Kind の制約は複写せず、具体化の記録を
    /// 残して段2で展開する (docs/spec/types.md の「推論」)。
    fn instantiate(&mut self, decl: Decl) -> Ty {
        let signatures = self.signatures;
        let Some(shape) = signatures.get(decl) else {
            return self.table.error;
        };
        let at = self.table.kind_counts();
        let instance = shape.instantiate(self.table);
        self.instances.push(Instance {
            decl,
            lin: instance.lin,
            mult: instance.mult,
            origin: self.table.kind_origin(),
            at,
        });
        instance.ty
    }
```

`value` の4つの分岐の中を次にする (由来の設定は今のまま)。

```rust
            Res::Function(function) => {
                let name = module.functions[function].name.clone();
                self.with_kind_origin(range, KindReason::Passed(name), |this| {
                    this.instantiate(Decl::Function(function))
                })
            }
            Res::Constructor(constructor) => {
                let name = module.constructors[constructor].name.clone();
                self.with_kind_origin(range, KindReason::Passed(name), |this| {
                    this.instantiate(Decl::Constructor(constructor))
                })
            }
            Res::Builtin(builtin) => {
                let reason = KindReason::Passed(builtin.name().to_string());
                self.with_kind_origin(range, reason, |this| {
                    this.instantiate(Decl::Builtin(builtin))
                })
            }
            Res::Operation(operation) => {
                let name = module.operations[operation].name.clone();
                self.with_kind_origin(range, KindReason::Passed(name), |this| {
                    this.instantiate(Decl::Operation(operation))
                })
            }
```

`constructor_pattern` の先頭の `let mut ty = match constructors.get(ctor) { ... };` を次にする。

```rust
        let mut ty = self.with_kind_origin(range, KindReason::Unified, |this| {
            this.instantiate(Decl::Constructor(ctor))
        });
```

`use` から `HashMap`、`ConstructorId`、`FunctionId`、`OperationId`、`Scheme` のうち使わなくなったものを除き、`use super::Signatures;` と `use crate::kind::problem::{Decl, Instance};` を足す。

- [ ] **Step 4: `check_module` を書き直す**

`check/mod.rs` の `check_module`、`check_main`、`kind_constraints` を次に置き換え、`signatures`、`check_body`、`declaration_schemes`、`declaration_problem`、`report_violations`、`typed_module` を足す。`has_error` と単体テスト (`counts_are_pluralized`) は残す。

```rust
/// 段0の結果。宣言ごとの閉じた型の形である。
pub(crate) struct Signatures {
    pub functions: ArenaMap<FunctionId, Shape>,
    pub builtins: HashMap<Builtin, Shape>,
    pub operations: ArenaMap<OperationId, Shape>,
    pub constructors: ArenaMap<ConstructorId, Shape>,
}

impl Signatures {
    pub fn get(&self, decl: Decl) -> Option<&Shape> {
        match decl {
            Decl::Function(id) => self.functions.get(id),
            Decl::Builtin(builtin) => self.builtins.get(&builtin),
            Decl::Operation(id) => self.operations.get(id),
            Decl::Constructor(id) => self.constructors.get(id),
        }
    }
}

/// 段1の結果。
pub(crate) struct Checked {
    pub types: BodyTypes,
    pub problem: KindProblem,
}

pub(crate) fn check_module(module: &Module) -> (TypedModule, Vec<Diagnostic>) {
    let context = Context::new(
        module.lang,
        &module.types,
        &module.constructors,
        &module.effects,
        &module.operations,
    );
    let signatures = signatures(module, &context);
    let mut schemes = declaration_schemes(module, &context, &signatures);
    let mut diagnostics = Vec::new();
    let main = module
        .functions
        .iter()
        .find(|(_, function)| function.name == "main")
        .map(|(id, _)| id);
    if let Some(id) = main {
        check_main(module, &context, &signatures, id, &mut diagnostics);
    }
    // 本体の検査は関数ごとに独立している。SCC の順に呼ぶのは、型の誤りの診断の並びを保つためだけである
    let components = scc::components(module);
    let mut bodies = ArenaMap::default();
    let mut problems: ArenaMap<FunctionId, KindProblem> = ArenaMap::default();
    for &id in components.iter().flatten() {
        if let Some((checked, found)) = check_body(module, &context, &signatures, id) {
            diagnostics.extend(found);
            bodies.insert(id, checked.types);
            problems.insert(id, checked.problem);
        }
    }
    // 等式のない関数も参照されうるので、制約のないスキームを持たせる
    for (id, _) in signatures.functions.iter() {
        if problems.get(id).is_none() {
            schemes.insert(Decl::Function(id), KindScheme::default());
        }
    }
    // 呼ばれる側の SCC から順に解き、SCC ごとに Kind を多相化する (docs/spec/types.md の「推論」)
    let mut violated = Vec::new();
    for component in &components {
        let members: Vec<(Decl, &KindProblem)> = component
            .iter()
            .filter_map(|&id| Some((Decl::Function(id), problems.get(id)?)))
            .collect();
        let solution = solve_scc(&members, &schemes);
        for (&(decl, _), scheme) in members.iter().zip(solution.schemes) {
            schemes.insert(decl, scheme);
        }
        violated.extend(solution.violated);
    }
    diagnostics.extend(report_violations(module, violated));
    let typed = typed_module(&context, &signatures, &schemes, bodies, main);
    // 網羅性は型推論と使用回数のパスの後に、書き出した型の上で調べる (docs/spec/exhaustiveness.md の「検査パス」)
    diagnostics.extend(exhaustive::check(module, &typed));
    (typed, diagnostics)
}

/// 段0: すべての宣言のシグネチャを閉じた形にする。宣言ごとに独立している。
pub(crate) fn signatures(module: &Module, context: &Context) -> Signatures {
    let functions = module
        .functions
        .iter()
        .filter_map(|(id, function)| {
            let signature = function.signature.as_ref()?;
            Some((id, signature_shape(context, signature)))
        })
        .collect();
    let builtins = BUILTINS
        .iter()
        .filter_map(|info| {
            let signature = module.builtins.get(&info.builtin)?;
            Some((info.builtin, signature_shape(context, signature)))
        })
        .collect();
    let operations = module
        .operations
        .iter()
        .map(|(id, operation)| (id, operation_shape(context, operation)))
        .collect();
    let constructors = module
        .constructors
        .iter()
        .map(|(id, constructor)| {
            let def = &module.types[constructor.ty];
            (id, constructor_shape(context, def, constructor))
        })
        .collect();
    Signatures {
        functions,
        builtins,
        operations,
        constructors,
    }
}

/// 段1: 1つの関数の本体を、全宣言の型の形だけを見て検査する。呼び出し先の本体の検査の結果は要らない
/// (docs/spec/types.md の「推論」)。シグネチャと本体の両方がある関数だけを検査する。
pub(crate) fn check_body(
    module: &Module,
    context: &Context,
    signatures: &Signatures,
    id: FunctionId,
) -> Option<(Checked, Vec<Diagnostic>)> {
    let function = &module.functions[id];
    let (Some(signature), Some(body), Some(shape)) = (
        &function.signature,
        &function.body,
        signatures.functions.get(id),
    ) else {
        return None;
    };
    let mut table = Table::new(context);
    let own = shape.instantiate_rigid(&mut table, &signature.generics);
    // 部分適用のクロージャは、それまでの引数を捕まえる (docs/spec/types.md の「関数型」)
    table.closure_kinds(own.ty, body.params.len(), &[]);
    let mut diagnostics = Vec::new();
    let mut checker = BodyCheck {
        module,
        function,
        body,
        rigids: &own.rigids,
        signatures,
        table: &mut table,
        diagnostics: &mut diagnostics,
        ambient: Row::pure(),
        ambient_source: AmbientSource::Signature,
        comparisons: Vec::new(),
        declared: ArenaMap::default(),
        typing: BodyTyping::default(),
        instances: Vec::new(),
    };
    checker.check_function(own.ty);
    checker.resolve_equalities(0);
    let typing = checker.typing;
    let instances = checker.instances;
    let reliable = usage::reliable(body, diagnostics.is_empty());
    usage::constrain(body, &typing, &mut table, reliable);
    carry::constrain(module, body, &typing, &mut table, reliable);
    let mut types = BodyTypes::default();
    for (expr, &ty) in typing.exprs.iter() {
        types.exprs.insert(expr, table.export(ty));
    }
    for (local, &ty) in typing.locals.iter() {
        types.locals.insert(local, table.export(ty));
    }
    for (pat, &ty) in typing.pats.iter() {
        types.pats.insert(pat, table.export(ty));
    }
    types.equalities = typing.equalities;
    let own_vars = OwnVars {
        lin: own.lin,
        mult: own.mult,
    };
    let problem = table.into_problem(instances, own_vars);
    Some((Checked { types, problem }, diagnostics))
}

/// 本体のない宣言の Kind のスキーム。宣言から出る制約だけを持つ問題を、1つの宣言だけの SCC として解く。
fn declaration_schemes(
    module: &Module,
    context: &Context,
    signatures: &Signatures,
) -> HashMap<Decl, KindScheme> {
    let mut problems: Vec<(Decl, KindProblem)> = Vec::new();
    for info in BUILTINS {
        let (Some(shape), Some(signature)) = (
            signatures.builtins.get(&info.builtin),
            module.builtins.get(&info.builtin),
        ) else {
            continue;
        };
        let problem = declaration_problem(context, shape, &signature.generics, |table, own| {
            table.closure_kinds(own.ty, info.arity, &[]);
        });
        problems.push((Decl::Builtin(info.builtin), problem));
    }
    for (id, operation) in module.operations.iter() {
        let shape = &signatures.operations[id];
        let generics = &operation.signature.generics;
        let problem = declaration_problem(context, shape, generics, |table, own| {
            table.closure_kinds(own.ty, operation.arity, &[]);
            // エフェクトの型引数は handle ごとに具体的な型で節を検査するので、`Unr` に固定しない
            let effect_kinds: Vec<KindVar> = own
                .rigids
                .effect_args(operation)
                .into_iter()
                .flat_map(|ty| table.kind_bounds(ty))
                .filter_map(|bound| match bound {
                    Bound::Var(var) => Some(var),
                    Bound::Const(_) => None,
                })
                .collect();
            // 結果の型は縛らない。`never fail : String -> a` をどの型としても使えるようにするため
            let mut spine = own.ty;
            for _ in 0..operation.arity {
                let TyShape::Fn { param, ret, .. } = table.shape(spine).clone() else {
                    break;
                };
                table.unrestricted(param, &effect_kinds);
                spine = ret;
            }
        });
        problems.push((Decl::Operation(id), problem));
    }
    for (id, constructor) in module.constructors.iter() {
        let shape = &signatures.constructors[id];
        let generics = &module.types[constructor.ty].generics;
        let problem = declaration_problem(context, shape, generics, |table, own| {
            table.closure_kinds(own.ty, constructor.fields.len(), &[]);
        });
        problems.push((Decl::Constructor(id), problem));
    }
    let mut schemes = HashMap::new();
    for (decl, problem) in &problems {
        let solution = solve_scc(&[(*decl, problem)], &schemes);
        let scheme = solution.schemes.into_iter().next().unwrap_or_default();
        schemes.insert(*decl, scheme);
    }
    schemes
}

/// 宣言の形を使い捨ての表に置き、宣言から出る制約だけを足した Kind の問題。
fn declaration_problem(
    context: &Context,
    shape: &Shape,
    generics: &Generics,
    constrain: impl FnOnce(&mut Table<'_>, &Own),
) -> KindProblem {
    let mut table = Table::new(context);
    let own = shape.instantiate_rigid(&mut table, generics);
    constrain(&mut table, &own);
    let own_vars = OwnVars {
        lin: own.lin,
        mult: own.mult,
    };
    table.into_problem(Vec::new(), own_vars)
}

/// Kind の制約の違反は、線形な値の誤った使い方である (docs/spec/linearity.md)。位置の順に並べ、同じ値の持ち越しの違反は、
/// 呼び出しの位置が最も前のものだけを報告する (docs/spec/diagnostics.md の E3006)。由来の範囲はその SCC の本体の中に
/// しかないので、SCC ごとに解いた由来を全体で並べ直せば、モジュール全体を1回で解いたときと同じ順になる。
fn report_violations(module: &Module, mut origins: Vec<KindOrigin>) -> Vec<Diagnostic> {
    origins.sort_by_key(|origin| (origin.range.start(), origin.range.end()));
    origins.dedup();
    let mut carried = HashSet::new();
    let mut out = Vec::new();
    for origin in origins {
        if let KindReason::CarriedAcross { value, .. } = &origin.reason
            && !carried.insert(value.key())
        {
            continue;
        }
        out.push(report::linear_misuse(module, &origin));
    }
    out
}

fn typed_module(
    context: &Context,
    signatures: &Signatures,
    schemes: &HashMap<Decl, KindScheme>,
    bodies: ArenaMap<FunctionId, BodyTypes>,
    main: Option<FunctionId>,
) -> TypedModule {
    let empty = KindScheme::default();
    let export = |decl: Decl, shape: &Shape| crate::Scheme {
        ty: shape.export(context),
        constraints: kind_constraints(context, shape, schemes.get(&decl).unwrap_or(&empty)),
    };
    TypedModule {
        signatures: signatures
            .functions
            .iter()
            .map(|(id, shape)| (id, export(Decl::Function(id), shape)))
            .collect(),
        bodies,
        main,
        builtins: signatures
            .builtins
            .iter()
            .map(|(&builtin, shape)| (builtin, export(Decl::Builtin(builtin), shape)))
            .collect(),
        operations: signatures
            .operations
            .iter()
            .map(|(id, shape)| (id, export(Decl::Operation(id), shape)))
            .collect(),
        constructors: signatures
            .constructors
            .iter()
            .map(|(id, shape)| (id, export(Decl::Constructor(id), shape)))
            .collect(),
    }
}

fn check_main(
    module: &Module,
    context: &Context,
    signatures: &Signatures,
    id: FunctionId,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let function = &module.functions[id];
    let (Some(shape), Some(signature)) = (signatures.functions.get(id), &function.signature) else {
        return;
    };
    // 未定義のエフェクトや解決できなかった型変数・row 変数の跡から E2004 を連鎖させないため
    // (docs/spec/types.md の「エラーの扱い」)
    if has_error(&signature.types, signature.ty) {
        return;
    }
    let found = shape.export(context);
    let expected = Type::Fn {
        param: Box::new(Type::unit()),
        effects: vec![EffectLabel {
            id: module.lang.io,
            name: module.effects[module.lang.io].name.clone(),
            args: Vec::new(),
        }],
        tail: None,
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

/// スキームに残った制約のうち、定数を片側に持つものを表示用にする。変数どうしの制約は出さない。テストで確かめたいのは
/// `Unr` の上限が付いたかどうかで、変数どうしの制約は部分適用のたびに増えて読みにくくなるため。
fn kind_constraints(context: &Context, shape: &Shape, scheme: &KindScheme) -> Vec<KindConstraint> {
    let names = shape.kind_names(context);
    let rows = shape.row_names();
    let term = |bound: Bound<Linearity>| match bound {
        Bound::Const(Linearity::Unr) => Some(KindTerm::Unr),
        Bound::Const(Linearity::Lin) => Some(KindTerm::Lin),
        Bound::Var(var) => names.get(&var).cloned().map(KindTerm::Of),
    };
    let row_term = |bound: Bound<Multiplicity>| match bound {
        Bound::Const(Multiplicity::Multi) => Some(RowTerm::Multi),
        Bound::Const(_) => None,
        Bound::Var(var) => rows.get(&var).cloned().map(RowTerm::Of),
    };
    let mut constraints: Vec<KindConstraint> = scheme
        .lin
        .iter()
        .filter(|(lower, upper)| {
            matches!(lower, Bound::Const(_)) != matches!(upper, Bound::Const(_))
        })
        .filter_map(|&(lower, upper)| {
            Some(KindConstraint::Linearity {
                lower: term(lower)?,
                upper: term(upper)?,
            })
        })
        .collect();
    for carry in &scheme.carries {
        let (Some(value), Some(row)) = (term(carry.lin), row_term(carry.mult)) else {
            continue;
        };
        let constraint = KindConstraint::Carry { value, row };
        if !constraints.contains(&constraint) {
            constraints.push(constraint);
        }
    }
    constraints
}
```

`check/mod.rs` の `use` を次にする (使わないものは clippy の警告に合わせて除く)。

```rust
use std::collections::{HashMap, HashSet};

use eml_diagnostics::{Diagnostic, Label};
use eml_hir::builtin::{BUILTINS, Builtin};
use eml_hir::{
    ConstructorId, FunctionId, Generics, Module, OperationId, RowRef, TypeRef, TypeRefId,
    TypeRefKind,
};
use la_arena::{Arena, ArenaMap};

use crate::context::Context;
use crate::kind::problem::{Decl, KindProblem, KindScheme, OwnVars};
use crate::kind::solve::solve_scc;
use crate::kind::{Bound, KindOrigin, KindReason, KindVar};
use crate::shape::{Own, Shape, constructor_shape, operation_shape, signature_shape};
use crate::table::{Row, Table, TyShape};
use crate::ty::{EffectLabel, KindConstraint, KindTerm, Linearity, Multiplicity, RowTerm, Type};
use crate::{BodyTypes, TypedModule, carry, codes, exhaustive, scc, usage};
```

`git rm crates/eml_types/src/scheme.rs` で古いスキームを消し、`lib.rs` から `mod scheme;` を消す。`mod shape;` の `#[allow(dead_code)]` とその上のコメントを消す。

- [ ] **Step 5: 表の単体テストを追随させる**

`table/tests.rs` を次のように直す。

- `copy_type_replaces_rigid_variables` と `copying_keeps_an_error_row` を消す。消す `Table::copy_type` を確かめるテストで、同じ意図のテストは Task 5 で `shape.rs` に足した (種類1。実行の前に承認を得たもの)
- `use` に `use crate::kind::solve::{residual_of, solve};` を足す
- `an_open_row_absorbs_the_missing_labels` の最後の `assert` を次にする (種類3)

  ```rust
      let sigma = table.row_multiplicity_var(r);
      assert_eq!(solve(&table.multiplicity).0[sigma.index()], Multiplicity::Once);
  ```

- `binding_a_variable_passes_the_kind_of_its_type_to_the_variable` の `table.lin_residual(&[mu])` を `residual_of(&table.linearity, &[mu])` にする (種類3)。`mu` は表で最初の線形性の変数 (番号 0) なので、残す変数の並びの位置と番号が一致し、期待値はそのまま通る
- `closure_kinds_bound_each_partial_application` の `table.lin_residual(&[mu, m])` を `residual_of(&table.linearity, &[mu, m])` にする (種類3)。`mu` と `m` は番号 0 と 1 で、並びの位置と一致する

- [ ] **Step 6: テストを流す**

Run: `cargo test -p eml_types`
Expected: すべて PASS。Step 1 の E3006 のテストも PASS する。

dump の `kinds:` の行のスナップショットが変わったら、Global Constraints の種類2の手順に従う。それ以外の期待値が変わったら止まる。

Run: `cargo test`
Expected: すべて PASS。`tests/ui/` のスナップショットは1つも変わらない (`cargo insta pending-snapshots` が空)。

- [ ] **Step 7: 古い名前が残っていないことを確かめる**

Run: `grep -rn "Lattice\|lin_residual\|carry_residual\|copy_carries\|solve_kinds\|copy_type\|scheme::" crates/eml_types/src`
Expected: `kind/solve.rs` の `carry_residual` だけが出る。

- [ ] **Step 8: clippy と fmt**

Run: `cargo clippy --all-targets && cargo fmt`
Expected: 警告なし。

- [ ] **Step 9: コミット**

```bash
git add -A crates/eml_types
git commit -m "Check bodies per function and solve kinds per SCC

<末尾の2行>"
```

---

### Task 7: 性能の受け入れテストを足す

5つの形の合成プログラムで、関数の数を4倍にしたときの型検査の時間の比を確かめる。時間を測るので `#[ignore]` を付け、release ビルドで流す。

**Files:**
- Create: `crates/eml_types/tests/scaling.rs`

**Interfaces:**
- Consumes: `eml_test_support::lower_clean`、`eml_types::check`
- Produces: なし

- [ ] **Step 1: テストを書く**

```rust
//! 型検査の時間が、関数の数にほぼ比例して伸びることを確かめる (docs/implementation/testing.md の「性能のテスト」)。時間を
//! 測るので release ビルドで流す: `cargo test --release -p eml_types --test scaling -- --ignored`

use std::time::{Duration, Instant};

use eml_test_support::lower_clean;

/// 小さいほうの関数の数。大きいほうはこの4倍にする。
const SMALL: usize = 2000;
/// 4倍の大きさに対して許す時間の比。ばらつきとハッシュ表の伸びの分の余裕を見込む。
const MAX_RATIO: f64 = 6.0;

/// 前の関数を呼ぶ多相な関数の連鎖。
fn chain(n: usize) -> String {
    let mut text = String::from("f0 : a -> a\nf0 x = x\n");
    for i in 1..n {
        text.push_str(&format!("\nf{i} : a -> a\nf{i} x = f{} x\n", i - 1));
    }
    text
}

/// 互いを呼ばない多相な関数。
fn independent(n: usize) -> String {
    let mut text = String::new();
    for i in 0..n {
        text.push_str(&format!(
            "f{i} : a -> (Unit -> <e> b) -> <e> b\nf{i} x g = g ()\n\n"
        ));
    }
    text
}

/// `data` と `match` を使う関数。
fn data_and_match(n: usize) -> String {
    let mut text = String::from("data Opt a =\n  | None\n  | Some a\n");
    for i in 0..n {
        text.push_str(&format!(
            "\nf{i} : Opt a -> a -> a\nf{i} o d = match o with\n  | Some v -> v\n  | None -> d\n"
        ));
    }
    text
}

/// 値を持ったまま呼び出しをまたぎ、前の関数に渡す連鎖。持ち越しの制約がスキームを通って伝わる。
fn carry_chain(n: usize) -> String {
    let mut text =
        String::from("k0 : a -> (Unit -> <e> Unit) -> <e> a\nk0 x action =\n  action ()\n  x\n");
    for i in 1..n {
        text.push_str(&format!(
            "\nk{i} : a -> (Unit -> <e> Unit) -> <e> a\nk{i} x action =\n  action ()\n  k{} x action\n",
            i - 1
        ));
    }
    text
}

/// 環状に呼び合う関数。全体が1つの SCC になる。
fn ring(n: usize) -> String {
    let mut text = String::new();
    for i in 0..n {
        let j = (i + 1) % n;
        text.push_str(&format!(
            "f{i} : a -> Int -> a\nf{i} x n = if n == 0 then x else f{j} x (n - 1)\n\n"
        ));
    }
    text
}

/// 型検査だけの時間。3回測って最小を使い、ほかの処理の割り込みによるばらつきを除く。
fn check_time(text: &str) -> Duration {
    let lowered = lower_clean(text);
    (0..3)
        .map(|_| {
            let start = Instant::now();
            let (_, diagnostics) = eml_types::check(&lowered.module);
            let elapsed = start.elapsed();
            assert!(diagnostics.is_empty(), "the generated program has errors");
            elapsed
        })
        .min()
        .unwrap()
}

fn assert_linear(generate: fn(usize) -> String) {
    let small = check_time(&generate(SMALL));
    let large = check_time(&generate(SMALL * 4));
    let ratio = large.as_secs_f64() / small.as_secs_f64();
    assert!(
        ratio <= MAX_RATIO,
        "{SMALL} functions took {small:?} and {} took {large:?} (ratio {ratio:.1})",
        SMALL * 4
    );
}

#[test]
#[ignore = "release ビルドで時間を測る"]
fn a_chain_of_polymorphic_functions() {
    assert_linear(chain);
}

#[test]
#[ignore = "release ビルドで時間を測る"]
fn independent_polymorphic_functions() {
    assert_linear(independent);
}

#[test]
#[ignore = "release ビルドで時間を測る"]
fn functions_with_data_and_match() {
    assert_linear(data_and_match);
}

#[test]
#[ignore = "release ビルドで時間を測る"]
fn a_chain_of_carry_overs() {
    assert_linear(carry_chain);
}

#[test]
#[ignore = "release ビルドで時間を測る"]
fn a_ring_of_functions() {
    assert_linear(ring);
}
```

R5 の前の実装では、`a_chain_of_carry_overs` が 2000個で終わらず、`a_ring_of_functions` の比はおよそ16になる (spec の「計測」)。古い実装で失敗を確かめるには時間がかかりすぎるので、このステップでは確かめない。

- [ ] **Step 2: テストを流す**

Run: `cargo test --release -p eml_types --test scaling -- --ignored`
Expected: 5件すべて PASS。

比が6を超えた形があれば、`cargo build --release -p eml_cli` で作った `eml check` と、プロファイラ (macOS なら `sample`) で時間のかかる関数を調べる。原因を直してから進む。閾値を上げて通すことはしない。

Run: `cargo test -p eml_types --test scaling`
Expected: 5件すべて ignored で、通常の `cargo test` の時間は増えない。

- [ ] **Step 3: clippy と fmt**

Run: `cargo clippy --all-targets && cargo fmt`
Expected: 警告なし。

- [ ] **Step 4: コミット**

```bash
git add crates/eml_types/tests/scaling.rs
git commit -m "Add ignored scaling tests for type checking time

<末尾の2行>"
```

---

### Task 8: 文書を更新する

spec の6章の表に従って文書を直す。日本語を書く前に `yomiyasu:yomiyasu` スキルを読む。

**Files:**
- Modify: `docs/spec/types.md`、`docs/spec/diagnostics.md`
- Modify: `docs/implementation/architecture.md`、`docs/implementation/status.md`、`docs/implementation/testing.md`、`docs/implementation/test-changes.md`

**Interfaces:**
- Consumes: Task 1〜7 の結果
- Produces: なし

- [ ] **Step 1: `docs/spec/types.md` の「推論」を直す**

「Kind はシグネチャから決まらないので推論する。Kind の制約は呼び出し関係を通じて伝わるため、…」で始まる項目の最初の段落を次にする (下の例の2つの子の項目はそのまま残す)。

```markdown
- Kind はシグネチャから決まらないので推論する。シグネチャが必須なので、宣言の型の形はシグネチャだけで決まる。SCC の間で流れる情報は、多相化した後に残す Kind の制約 (Kind のスキーム) だけである。そこで推論を2段に分ける。1段目は関数ごとに、呼び出し先の型の形だけを見て本体を検査し、Kind の制約を集める。参照した宣言の制約は複写せず、どの宣言をどの Kind 変数で具体化したかを記録する。2段目は、トップレベルの関数を呼び出しグラフの強連結成分 (SCC) ごとにまとめ、記録を展開して束の上で解く。同じ SCC の参照は、多相化する前の Kind 変数を共有するのと同じく、変数どうしの等式にする。前の SCC の宣言の参照は、そのスキームの制約を、具体化した変数について複写する。推論後に残った Kind 変数を多相化し、多相化した Kind 変数に関わる制約をスキームに残す。
  - 残す制約は、多相化する変数から、多相化しない変数を通り抜けて、最初に出会う多相化する変数か定数までをたどって求める。多相化する変数どうしが制約の循環に入って等しくなるときは、それらが等しいことを残し、たどって出会った定数の境界をそのすべてに付ける。
```

「多相化するときは、持ち越しの制約の両側を、…」の項目の「組ごとに元の由来を持ったままスキームに残す。」を、次の2文に替える。

```markdown
同じ組は1つにまとめ、由来は位置が最も前のものを残す。由来ごとに残すと、多相な関数を重ねるたびに制約が増え、同じ違反を何度も報告するためである。
```

同じ項目の「具体化のたびに複製し、由来は呼んだ関数の名前と元の由来を持つ形にする。」を、次にする。

```markdown
具体化のたびに複製し、由来は呼んだ関数の名前と、元の由来の1段分の要約 (位置と、持っていた値の名前か通った関数の名前) を持つ形にする。報告は1段しかたどらないので入れ子にしない。
```

- [ ] **Step 2: `docs/spec/diagnostics.md` の E3006 を直す**

E3006 の行の説明の末尾 (「同じ値は、呼び出しの位置が最も前の1件だけを報告する」の後) に、次の文を足す。

```markdown
。呼んだ関数のスキームを通る持ち越しの違反は、そのスキームに残した組ごとに1件で、呼んだ関数の中で位置が最も前の持ち越しを指す
```

(表のセルの中なので、前の文の句点の後にそのままつなげる。)

- [ ] **Step 3: `docs/implementation/architecture.md` の「`eml_types` の内部」を直す**

次の項目を置き換える。

「呼び出しグラフの SCC (`scc.rs`) を呼ばれる側から検査し、…」の項目を次にする。

```markdown
- 型検査は4つの純粋な関数に分ける。`Context::new` (`context.rs`) はモジュール全体の情報 (データ型の Kind、名前、多重度) を1回だけ作り、関数ごとの型の表はこれを借りる。段0の `check::signatures` は、宣言ごとにシグネチャを閉じた形 `Shape` (`shape.rs`) にする。段1の `check::check_body` は、関数ごとに新しい表を作り、全宣言の `Shape` だけを見て本体を検査し、`BodyTypes` と Kind の問題 `KindProblem` (`kind/problem.rs`) を返す。段2の `kind::solve::solve_scc` は、呼び出しグラフの SCC (`scc.rs`) ごとに Kind の問題をまとめて解き、違反の由来と各関数の `KindScheme` を返す。`check_module` はこれらを順に呼んで結果を集めるだけである
- 段1は、トップレベルの値の参照ごとに `Shape` を具体化し、呼び出し先の制約を複写せずに具体化の記録 (`Instance`) を残す。記録は、具体化したときの制約の数 (`at`) を持ち、段2は展開した制約をその位置に差し込む。同じ範囲の違反の報告の順を保つためである。段2は、同じ SCC の参照を変数どうしの等式にし、前の SCC の参照にはそのスキームを複写する。解き方はワークリストで、残す制約は制約のグラフを強連結成分に縮めた DAG の上で求める (`Graph`)
- 使用回数のパス (`usage.rs`) は `Unr` の制約を出し、使った位置と使わなかった経路を Kind の制約の由来に入れる。報告が由来から E3001〜E3005 を選ぶ。由来 (`KindOrigin`) は型の表を指さない。持ち越しの由来は報告が指す `multi` の操作を持ち、スキームを通った持ち越しの由来は、呼んだ関数の中の1段分の要約 (`CarriedInner`) を持つ
```

「持ち越しの制約は `Table::carries` に入れ、`solve_kinds` が…」の項目を次にする。

```markdown
- 持ち越しの制約は `Table::carries` に入れ、段2が線形性と多重度の両方の束を解いた後に検査する。スキームには、`solve::carry_residual` が内部の変数を経由した推移を含めて残し、同じ組は位置が最も前の由来の1つにまとめる。具体化の展開では `CarriedThrough` の由来を付けて複写する。違反は `report::linear_misuse` が E3006 にする。同じ値の違反は、`check/mod.rs` の報告で値ごとに最初の1件に絞る
```

「型の表は `table/` に分ける。…」の項目の「`copy.rs` はスキームの具体化の写し、」を消し、「`kinds.rs` は Kind の制約」を「`kinds.rs` は Kind の制約を集める処理 (解くのは段2)」にする。

「`Table::display` は診断の文言のための変換で、…」の項目を次にする。

```markdown
- `Table::export` は、後の段階と診断の文言の両方に渡す形を作る。書き出す `Type` は矢印の線形性を持たない。後の段階は線形性を読まず、持たせると本体の型を Kind を解くまで確定できなくなるためである
```

「検査器は `check/` に分ける。`mod.rs` は SCC の順の検査と…」の「`mod.rs` は SCC の順の検査と `TypedModule` の組み立て」を「`mod.rs` は段0〜2の組み立てと `TypedModule` の組み立て」にする。

「シグネチャはスキーム (`scheme.rs`) で持つ。…」の項目を次にする。

```markdown
- シグネチャは閉じた形 `Shape` (`shape.rs`) で持つ。型の表を指さず、rigid な型変数、row 変数、Kind 変数をスキームの中の番号で持つ。参照するたびに多相な具体化をし、戻り値の側の閉じた row を開く。自分の本体の検査では rigid な具体化をして、本体の注釈が同じ変数を指せるようにする
```

「組み込みの型は、Prelude のシグネチャから、ユーザーの関数と同じ経路 (`Rigids`、`lower_signature`、`closure_kinds`、`Scheme`) で作る。…」の項目を次にする。

```markdown
- 組み込み、操作、コンストラクタの型は、ユーザーの関数と同じ経路 (`lower_signature` などで下ろしてから `Shape` に閉じる) で作る。本体がないので、宣言から出る制約 (`closure_kinds` と、操作の引数の `unrestricted`) だけを持つ Kind の問題を、1つの宣言だけの SCC として段2で解く
```

- [ ] **Step 4: `docs/implementation/status.md` を直す**

- 「段階6b の前に、型検査を SCC ごとに独立させる R5 を足した。…」と「R5 は段階6a の後、…」の2段落は残す
- リファクタリングの表の R5 の行を次にする

  ```markdown
  | R5 | 型検査の SCC ごとの独立 | `Context`、閉じた形 `Shape`、関数ごとの本体の検査 (段1) と SCC ごとの Kind の解決 (段2)、ワークリストと強連結成分による残す制約、持ち越しの制約の組ごとの重複除去、`Type` の線形性を除くこと。`eml_types` の中で済ませる | 完了 |
  ```

- 「### R5 で直す項目」の節の中身を、次の段落に置き換える

  ```markdown
  R5 で済んだ。型検査を、モジュール全体の情報 (`Context`)、宣言ごとの閉じた形 (段0)、関数ごとの本体の検査 (段1)、SCC ごとの Kind の解決 (段2) に分けた。本体の検査は呼び出し先の形だけを見るので、関数ごとに独立している。段2は表を使わずに番号の上の束を解き、ワークリストで解き、制約のグラフを強連結成分に縮めて残す制約を求める。持ち越しの制約は組ごとに1つにまとめ、E3006 の重複報告を直した。`Type` から矢印の線形性を除いた。`crates/eml_types/tests/scaling.rs` が、5つの形の合成プログラムで関数の数を4倍にしたときの時間の比を確かめる。
  ```

- 「各 crate の実装状況」の `eml_types` の行の末尾に、次の文を足す

  ```markdown
  R5 で、本体の検査を関数ごとに独立させ (段1)、Kind の解決を SCC ごとの純粋な関数にした (段2)。閉じた形 `Shape`、`Context`、表を指さない Kind の由来を足し、書き出す `Type` から矢印の線形性を除いた
  ```

- 「次の作業の注意点」から「性能: `Lattice::residual` は、…」の項目を消し、次の項目を足す

  ```markdown
  - 既知の制限 (性能): 段2は、1つの SCC の中で、関数ごとに残す成分から残さない領域をたどる。DAG の形によっては、残さない成分を関数ごとに何度もたどる。また、持ち越しの制約を関数ごとに SCC 全体から見るので、関数の数と持ち越しの制約の数の積がかかる。どちらも、大きな SCC が多くの持ち越しの制約を持つ場合だけに起きる
  ```

- [ ] **Step 5: `docs/implementation/testing.md` を直す**

「よく使うコマンド」のコードブロックに次の行を足す。

```sh
cargo test --release -p eml_types --test scaling -- --ignored   # 型検査の時間の伸び (性能のテスト)
```

「CLI のテスト」の節の後に、次の節を足す。

```markdown
## 性能のテスト

`crates/eml_types/tests/scaling.rs` は、合成プログラムの関数の数を4倍にしたときの型検査の時間の比を確かめる。形は、多相な関数の連鎖、独立した多相な関数、`data` と `match`、持ち越しの連鎖、環状の相互再帰の5つである。HIR まで作ってから `eml_types::check` の時間だけを測り、各大きさで3回測った最小を使う。関数の数は 2000 と 8000 で、比が6以下なら通る。時間を測るので `#[ignore]` を付け、release ビルドで流す。型検査の構造を変えたときに流す。
```

「今あるテストの地図」の `eml_types` の行を次にする。

```markdown
| `eml_types` | `check.rs` (推論と型の診断)、`rows.rs` (エフェクトの row と E2002)、`effects.rs` (エフェクト、handler、継続、線形な継続 (E3001))、`data.rs` (型構成子の引数、データ型の Kind、パターンと `match` の型検査)、`exhaustive.rs` (網羅性の検査と漏れの例。タプルとリテラルを含む)、`tuples.rs` (タプルの型検査、リテラルのパターン、`==` の比べ方の決定と E2006)、`linearity.rs` (線形性の診断)、`scaling.rs` (型検査の時間の伸び。`#[ignore]`) | `table/tests.rs` (単一化と型の書き出し)、`kind/solve.rs` (Kind の問題の解き方と残す制約)、`shape.rs` (閉じた形と具体化)、`ty.rs` (型の表示)、`scc.rs` (関数の呼び出しの強連結成分)、`check/mod.rs` (診断の文言) |
```

(今の行に `linearity.rs` がなければ、上のとおり足す。)

- [ ] **Step 6: `docs/implementation/test-changes.md` に R5 の節を足す**

ファイルの末尾に次を足す。Task 6 で `kinds:` の行が変わった場合は、変わったテストと差分を3つ目の項目の代わりに書く。

```markdown
### リファクタリング R5

- `export` が Kind の解なしで書き出せるようになり、`display` と1つにまとめたので、`eml_types/src/table/tests.rs` の `export_needs_solved_kinds` (解く前の書き出しの panic) と `display_does_not_solve_kinds` (表示が解かないこと) を消した (種類1)。確かめる性質そのものがなくなったためである
- 型の表の複写 (`Table::copy_type`) をなくし、シグネチャの閉じた形 `Shape` の具体化に替えたので、`table/tests.rs` の `copy_type_replaces_rigid_variables` と `copying_keeps_an_error_row` を消した (種類1)。同じ意図のテストは `shape.rs` の `instantiation_replaces_rigid_variables_and_rows` と `an_error_row_survives_closing_and_instantiation` に移した
- dump の `kinds:` の行は変わらなかった (種類2はなし)
- `kind.rs` の単体テスト9件を `kind/solve.rs` に移し、新しい API で組み立て直した。`table/tests.rs` の `Table::new` の呼び方、Kind を読む3件の組み立て、`ty.rs` の単体テストの `Type` の組み立て (`linearity` を除いた) も追随させた。期待値は変えていない (種類3)
```

- [ ] **Step 7: リンターをかける**

Run: `python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py docs/implementation/status.md`
Expected: 新しく書いた文に、文末のコロンや太字にならない書き方の指摘がない。英単語の前後の空白と箇条書きの比率の指摘は、リポジトリの文書の書き方に合わせたものなので直さない。

- [ ] **Step 8: テストを流す**

Run: `cargo test`
Expected: すべて PASS (文書だけの変更なので変わらない)。

- [ ] **Step 9: コミット**

```bash
git add docs
git commit -m "Document refactor R5

<末尾の2行>"
```

R5 の作業用の spec とこの計画は、ブランチ全体のレビューを終えた後に、別のコミットで消す (これまでの回と同じ)。
