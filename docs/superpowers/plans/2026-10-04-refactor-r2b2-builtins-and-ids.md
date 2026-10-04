# リファクタリング R2b-2: 組み込みと ID 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 組み込みのシグネチャを eml の Prelude に置いてスキームの経路で型を作り、型とエフェクトを ID で表し、`TypedModule` がスキームを返すようにする。言語の振る舞いは変えない。

**Architecture:** まず組み込みの表を1つにし (Task 1)、Prelude を HIR に読み込み (Task 2)、型検査が Prelude からスキームを作る (Task 3)。次に、型とエフェクトを item と ID にして HIR、型検査、Core IR を追随させる (Task 4)。最後に `TypedModule` をスキームにする (Task 5)。

**Tech Stack:** Rust (edition 2024)、la-arena、insta

**Spec:** `docs/superpowers/specs/2026-10-04-refactor-r2b2-builtins-and-ids-design.md`

## Global Constraints

- 期待値は、このプランで名前を挙げたテストだけを変える。名前を挙げたテストはなく、期待値を変えない機械的な追随 (種類3) だけを許す。種類3の対象は `eml_types/src/table/tests.rs`、`eml_types/src/ty.rs` の単体テスト、`eml_hir/src/lower/scope.rs` の単体テストである (docs/implementation/testing.md の「テストの変更の運用」)
- スナップショット (HIR、型、Core IR、UI) の期待値が1つでも変わったら、変えずに止まる。差分と理由をユーザーに示し、承認を得てから変え、`testing.md` に記録する
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

- 組み込みの型をスキームの経路で作ったことで、内側の矢印の線形性が `Known(Unr)` から Kind 変数に変わる。これが型の不一致や `kinds:` の行の表示を変えないこと (Task 3 で、すべてのスナップショットが変わらないことで確かめる)
- 組み込みのスキームを作る順序が、Kind 変数の番号と、`kinds:` の行の制約の並び順を変えないこと (Task 3 で、`BUILTINS` の表の順に作る)
- Prelude の範囲 (`FileId::PRELUDE`) が、どの診断にも出ないこと (Task 2 の `debug_assert` と、すべての UI テストが通ること)
- エフェクトの名前と型の名前の表示が、ID にした後も今と同じであること (Task 4 で、すべてのスナップショットが変わらないことで確かめる)
- `negate` を名前で引けないままであること (Task 2 のテスト)

---

### Task 1: 組み込みの表

**Files:**
- Modify: `crates/eml_hir/src/builtin.rs`

**Interfaces:**
- Consumes: なし
- Produces: `eml_hir::builtin::{Access, BuiltinInfo, BUILTINS}`。`Access` は `Named`、`Operator`、`Internal` (`Debug`、`Clone`、`Copy`、`PartialEq`、`Eq`)。`BuiltinInfo { builtin: Builtin, name: &'static str, access: Access, arity: usize }`。`Builtin::info(self) -> &'static BuiltinInfo`、`Builtin::arity(self) -> usize`、`Builtin::from_prelude_name(name: &str) -> Option<Builtin>`。`from_name`、`binary_operator`、`name` は今の名前と戻り値のまま

振る舞いは変えない。

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_hir/src/builtin.rs` の末尾に足す。

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_has_one_row_in_the_table() {
        for info in BUILTINS {
            assert_eq!(info.builtin.info().name, info.name);
            assert_eq!(
                BUILTINS.iter().filter(|other| other.builtin == info.builtin).count(),
                1,
                "{}",
                info.name
            );
        }
    }

    #[test]
    fn names_are_looked_up_by_access() {
        assert_eq!(Builtin::from_name("println"), Some(Builtin::Println));
        assert_eq!(Builtin::from_name("True"), Some(Builtin::True));
        assert_eq!(Builtin::from_name("+"), None);
        assert_eq!(Builtin::from_name("negate"), None);
        assert_eq!(Builtin::binary_operator("+"), Some(Builtin::IntAdd));
        assert_eq!(Builtin::binary_operator("println"), None);
        assert_eq!(Builtin::from_prelude_name("negate"), Some(Builtin::IntNeg));
        assert_eq!(Builtin::IntNeg.name(), "negate");
        assert_eq!(Builtin::ComposeFwd.arity(), 3);
    }
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_hir --lib builtin`
Expected: コンパイルエラー (`cannot find value BUILTINS`、`no method named info`)

- [ ] **Step 3: 実装する**

`crates/eml_hir/src/builtin.rs` の `impl Builtin { ... }` の `from_name`、`binary_operator`、`name` を消し、次の表と関数に置き換える。`Builtin` の enum、`BuiltinType`、`builtin_effect`、`Assoc`、`fixity` は今のまま残す (`BuiltinType` と `builtin_effect` は Task 4 で消す)。

```rust
/// 名前空間での見え方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// 名前で引く値。
    Named,
    /// 二項演算子として引く。
    Operator,
    /// 名前で引けない。前置の `-` は `negate` に脱糖するが、ユーザーは `negate` と書けない。
    Internal,
}

#[derive(Debug)]
pub struct BuiltinInfo {
    pub builtin: Builtin,
    /// Prelude (`prelude.em`) と診断での書き方。
    pub name: &'static str,
    pub access: Access,
    /// 実装が受け取る引数の数。部分適用のクロージャの Kind に使う (docs/spec/types.md の「関数型」)。
    pub arity: usize,
}

/// 組み込みの表。名前と見え方と引数の数はここだけに置き、型は Prelude に置く。
pub const BUILTINS: &[BuiltinInfo] = &[
    info(Builtin::Println, "println", Access::Named, 1),
    info(Builtin::ShowInt, "show_int", Access::Named, 1),
    info(Builtin::Not, "not", Access::Named, 1),
    info(Builtin::True, "True", Access::Named, 0),
    info(Builtin::False, "False", Access::Named, 0),
    info(Builtin::IntNeg, "negate", Access::Internal, 1),
    info(Builtin::IntAdd, "+", Access::Operator, 2),
    info(Builtin::IntSub, "-", Access::Operator, 2),
    info(Builtin::IntMul, "*", Access::Operator, 2),
    info(Builtin::IntDiv, "/", Access::Operator, 2),
    info(Builtin::IntMod, "%", Access::Operator, 2),
    info(Builtin::IntEq, "==", Access::Operator, 2),
    info(Builtin::IntNe, "!=", Access::Operator, 2),
    info(Builtin::IntLt, "<", Access::Operator, 2),
    info(Builtin::IntLe, "<=", Access::Operator, 2),
    info(Builtin::IntGt, ">", Access::Operator, 2),
    info(Builtin::IntGe, ">=", Access::Operator, 2),
    info(Builtin::StrConcat, "++", Access::Operator, 2),
    info(Builtin::ComposeFwd, ">>", Access::Operator, 3),
    info(Builtin::ComposeBwd, "<<", Access::Operator, 3),
];

const fn info(builtin: Builtin, name: &'static str, access: Access, arity: usize) -> BuiltinInfo {
    BuiltinInfo {
        builtin,
        name,
        access,
        arity,
    }
}

impl Builtin {
    pub fn info(self) -> &'static BuiltinInfo {
        BUILTINS
            .iter()
            .find(|info| info.builtin == self)
            .expect("every builtin has a row in the table")
    }

    /// 名前で引ける組み込み。演算子は `binary_operator` で引く。
    pub fn from_name(name: &str) -> Option<Builtin> {
        Self::find(name, |access| access == Access::Named)
    }

    /// 二項演算子の組み込み。`&&`、`||`、`|>`、`<|` は HIR で脱糖するので含まない (docs/spec/declarations.md)。
    pub fn binary_operator(op: &str) -> Option<Builtin> {
        Self::find(op, |access| access == Access::Operator)
    }

    /// Prelude のシグネチャの名前。見え方によらずに引く。
    pub fn from_prelude_name(name: &str) -> Option<Builtin> {
        Self::find(name, |_| true)
    }

    /// ソースでの書き方。診断と表示で使う。
    pub fn name(self) -> &'static str {
        self.info().name
    }

    pub fn arity(self) -> usize {
        self.info().arity
    }

    fn find(name: &str, access: impl Fn(Access) -> bool) -> Option<Builtin> {
        BUILTINS
            .iter()
            .find(|info| info.name == name && access(info.access))
            .map(|info| info.builtin)
    }
}
```

`Builtin` の enum のコメント「型は `eml_types` が、実装は `eml_interp` が、この enum の `match` で与える」は、「型は Prelude (`prelude.em`) が、Core IR への変換は `eml_core_ir` が与える」にする (型の部分は Task 3 で実際にそうなる)。

- [ ] **Step 4: テストが通り、期待値が変わっていないことを確かめる**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED"`
Expected: 何も出ない

Run: `git diff --no-ext-diff --stat -- '*.snap' crates/*/tests`
Expected: 何も出ない

- [ ] **Step 5: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates/eml_hir
git commit -m "Keep builtin names, access, and arity in one table"
```

---

### Task 2: Prelude を HIR に読み込む

**Files:**
- Modify: `crates/eml_diagnostics/src/source.rs` (`FileId::PRELUDE`)
- Create: `crates/eml_hir/src/prelude.em`
- Create: `crates/eml_hir/src/lower/prelude.rs`
- Modify: `crates/eml_hir/src/hir.rs` (`Module::builtins`)、`crates/eml_hir/src/lower/mod.rs`
- Modify: `crates/eml_hir/tests/structure.rs`

**Interfaces:**
- Consumes: Task 1 の `Builtin::from_prelude_name`
- Produces: `eml_diagnostics::FileId::PRELUDE`、`Module::builtins: HashMap<Builtin, Signature>`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_hir/tests/structure.rs` の末尾に足す。`use` に `eml_hir::builtin::{BUILTINS, Builtin}` を足す。

```rust
#[test]
fn the_prelude_has_a_signature_for_every_builtin_function() {
    // コンストラクタはシグネチャの構文で書けないので、`True` と `False` は Prelude にない
    let module = module("");
    for info in BUILTINS {
        let expected = !matches!(info.builtin, Builtin::True | Builtin::False);
        assert_eq!(
            module.builtins.contains_key(&info.builtin),
            expected,
            "{}",
            info.name
        );
    }
}

#[test]
fn internal_builtins_cannot_be_named() {
    let lowered = eml_test_support::lower("f : Int -> Int\nf x = negate x");
    let codes: Vec<String> = lowered
        .diagnostics
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    assert_eq!(codes, ["E1001"]);
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_hir --test structure`
Expected: コンパイルエラー (`no field builtins on type Module`)。`internal_builtins_cannot_be_named` は今の実装でも通る (今も `negate` は名前で引けない)

- [ ] **Step 3: 実装する**

`crates/eml_diagnostics/src/source.rs` の `FileId` の後に足す。

```rust
impl FileId {
    /// 組み込みの Prelude の範囲に使う。Prelude の範囲が診断に出ることはない。出たら `SourceFiles` がこのファイルを
    /// 引けずに panic するので、誤りにすぐ気づける。
    pub const PRELUDE: FileId = FileId(u32::MAX);
}
```

`crates/eml_hir/src/prelude.em` を作る。中身は spec の「Prelude」の節の18行のシグネチャを、そのままの順で並べる。先頭に次のコメントを置く。

```
-- 組み込みの関数と演算子のシグネチャ。型は docs/spec/declarations.md の標準の演算子の表と、docs/spec/effects.md の
-- 組み込みの IO に従う。名前と見え方と引数の数は eml_hir::builtin::BUILTINS にある。
```

`crates/eml_hir/src/lower/prelude.rs` を作る。

```rust
//! 組み込みの Prelude (`prelude.em`) のシグネチャを変換する。S2 で `Prelude` モジュールに移す。

use std::collections::HashMap;

use eml_diagnostics::FileId;
use eml_syntax::ast;
use la_arena::Arena;

use super::scope::ItemScope;
use super::types::TypeLowering;
use crate::builtin::Builtin;
use crate::hir::{Generics, Signature};

const PRELUDE: &str = include_str!("../prelude.em");

pub(super) fn lower_prelude(items: &ItemScope) -> HashMap<Builtin, Signature> {
    let (parse, syntax_errors) = eml_syntax::parse(FileId::PRELUDE, PRELUDE);
    debug_assert!(syntax_errors.is_empty(), "{syntax_errors:?}");
    let mut diagnostics = Vec::new();
    let mut signatures = HashMap::new();
    for item in parse.tree().items() {
        let ast::Item::Signature(signature) = item else {
            continue;
        };
        let name = signature.name().expect("every Prelude signature has a name");
        let builtin = Builtin::from_prelude_name(name.text())
            .expect("every Prelude signature names a builtin in the table");
        let range = signature.ty().map_or(signature.range(), |ty| ty.range());
        let mut types = Arena::new();
        let mut generics = Generics::default();
        let ty = TypeLowering {
            file: FileId::PRELUDE,
            types: &mut types,
            generics: &mut generics,
            items,
            define: true,
            diagnostics: &mut diagnostics,
        }
        .lower(signature.ty(), range);
        signatures.insert(
            builtin,
            Signature {
                ty,
                range,
                types,
                generics,
            },
        );
    }
    debug_assert!(diagnostics.is_empty(), "{diagnostics:?}");
    signatures
}
```

`crates/eml_hir/src/hir.rs` の `Module` に `pub builtins: HashMap<Builtin, Signature>` を足す (doc: 「Prelude の組み込みのシグネチャ。」)。`use std::collections::HashMap;` を足す。

`crates/eml_hir/src/lower/mod.rs` に `mod prelude;` を足す。`lower` の中で `ItemScope::new()` を作った直後に `let builtins = prelude::lower_prelude(&scope);` を呼び、最後の `Module { file, functions }` を `Module { file, functions, builtins }` にする。

- [ ] **Step 4: テストが通り、期待値が変わっていないことを確かめる**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED|panicked"`
Expected: 何も出ない (Prelude の `debug_assert` が発火しない)

Run: `git diff --no-ext-diff --stat -- '*.snap' crates/*/tests`
Expected: `crates/eml_hir/tests/structure.rs` だけが出る

- [ ] **Step 5: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates/eml_diagnostics crates/eml_hir
git commit -m "Lower builtin signatures from an eml Prelude"
```

---

### Task 3: 組み込みのスキームを Prelude から作る

**Files:**
- Modify: `crates/eml_types/src/check/mod.rs`、`crates/eml_types/src/check/body.rs`
- Delete: `crates/eml_types/src/builtins.rs`
- Modify: `crates/eml_types/src/lib.rs` (`mod builtins;` を消す)
- Modify: `crates/eml_types/tests/check.rs`

**Interfaces:**
- Consumes: Task 2 の `Module::builtins`、Task 1 の `BUILTINS`、`Builtin::arity`
- Produces: `BodyCheck` のフィールド `builtins: &HashMap<Builtin, Scheme>`

- [ ] **Step 1: 振る舞いを固定するテストを書く**

`crates/eml_types/tests/check.rs` の末尾に足す。今の実装 (手書きの型) で通るテストで、置き換えの後も同じ表示であることを確かめる。

```rust
#[test]
fn composition_has_the_prelude_type() {
    // `>>` の型は Prelude にある (crates/eml_hir/src/prelude.em)。手で組み立てていたときと同じ型になる
    let text = "compose : (Int -> Int) -> (Int -> Int) -> Int -> Int\ncompose f g = f >> g\n\nback : (Int -> Int) -> (Int -> Int) -> Int -> Int\nback f g = g << f";
    insta::assert_snapshot!(check_text(text), @"
    compose : (Int -> Int) -> (Int -> Int) -> Int -> Int
      f#0 : Int -> Int
      g#1 : Int -> Int
    back : (Int -> Int) -> (Int -> Int) -> Int -> Int
      f#0 : Int -> Int
      g#1 : Int -> Int
    ");
}
```

Run: `cargo test -p eml_types --test check composition_has_the_prelude_type`
Expected: PASS。通らなければ、今の実装の出力を確かめ、出力が型として正しければ期待値をその出力に合わせる (新しいテストなので、既存の期待値の変更には当たらない)

- [ ] **Step 2: 組み込みのスキームを作る**

`crates/eml_types/src/check/mod.rs` の `check_module` で、`let mut table = Table::new();` の後に足す。`BUILTINS` の表の順に作るのは、Kind 変数の番号と制約の並び順を決まったものにするためである。

```rust
    // 組み込みの型は Prelude のシグネチャから、ユーザーの関数と同じ経路で作る。本体がないので、作ってすぐ多相化する
    let mut builtins: HashMap<Builtin, Scheme> = HashMap::new();
    for info in BUILTINS {
        let Some(signature) = module.builtins.get(&info.builtin) else {
            continue;
        };
        let rigids = Rigids::new(&mut table, &signature.generics);
        let ty = lower_signature(&mut table, signature, &rigids);
        table.closure_kinds(ty, info.arity, &[]);
        let mut scheme = Scheme::new(ty, &rigids);
        scheme.generalize(&table);
        builtins.insert(info.builtin, scheme);
    }
```

`BodyCheck { ... }` を作るところに `builtins: &builtins,` を足す。`use` に `std::collections::HashMap` と `eml_hir::builtin::{BUILTINS, Builtin}` を足す。

`crates/eml_types/src/check/body.rs`:

- `BodyCheck` に `pub(super) builtins: &'a HashMap<Builtin, Scheme>,` を足す (doc: 「Prelude から作った組み込みのスキーム。」)。
- `value` の `Res::Builtin(builtin)` の腕を次にする。

```rust
            Res::Builtin(builtin) => {
                let ty = match builtin {
                    // コンストラクタは Prelude にない。段階4で `data Bool` に置き換える
                    Builtin::True | Builtin::False => self.table.bool,
                    _ => match self.builtins.get(&builtin) {
                        Some(scheme) => scheme.instantiate(self.table),
                        None => self.table.error,
                    },
                };
                self.table.open_spine(ty)
            }
```

- `use crate::builtins::builtin_type;` を消す。

`git rm crates/eml_types/src/builtins.rs` で消し、`crates/eml_types/src/lib.rs` の `mod builtins;` を消す。

- [ ] **Step 3: テストが通り、期待値が変わっていないことを確かめる**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED"`
Expected: 何も出ない

Run: `git diff --no-ext-diff --stat -- '*.snap' crates/*/tests`
Expected: `crates/eml_types/tests/check.rs` (Step 1 で足したテスト) だけが出る。既存のテストの行は変わっていない

- [ ] **Step 4: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add -A crates/eml_types
git commit -m "Build builtin schemes from the Prelude like user functions"
```

---

### Task 4: 型とエフェクトを item と ID で表す

**Files:**
- Modify: `crates/eml_hir/src/hir.rs`、`crates/eml_hir/src/builtin.rs`、`crates/eml_hir/src/lower/scope.rs`、`crates/eml_hir/src/lower/mod.rs`、`crates/eml_hir/src/lower/types.rs`、`crates/eml_hir/src/lower/prelude.rs`、`crates/eml_hir/src/pretty.rs`
- Modify: `crates/eml_types/src/table/mod.rs`、`row.rs`、`export.rs`、`tests.rs`、`crates/eml_types/src/ty.rs`、`crates/eml_types/src/scheme.rs`、`crates/eml_types/src/check/mod.rs`、`crates/eml_types/src/check/report.rs`、`crates/eml_types/src/lib.rs`
- Modify: `crates/eml_core_ir/src/lower.rs`

**Interfaces:**
- Consumes: Task 2 の `Module::builtins`、Task 3 の組み込みのスキーム
- Produces:
  - `eml_hir`: `TypeDef { name: String }`、`EffectDef { name: String }`、`TypeDefId = Idx<TypeDef>`、`EffectId = Idx<EffectDef>`、`LangItems { int, string, bool, unit: TypeDefId, io: EffectId }` (`Debug`、`Clone`、`Copy`)、`Module::{types, effects, lang}`、`TypeRefKind::Con(TypeDefId)`、`RowRef` の `effects: Vec<EffectId>`。`BuiltinType`、`builtin_effect`、`EffectRef` は無くなる
  - `eml_hir::lower::scope`: `TypeItem::{Type(TypeDefId), Effect(EffectId)}`、`ItemScope::{define_type, define_effect}`、`builtin_items(&mut Arena<TypeDef>, &mut Arena<EffectDef>, &mut ItemScope) -> LangItems`
  - `eml_types`: `TyShape::Con(TypeDefId)`、`Table::new(lang: LangItems, types: &Arena<TypeDef>, effects: &Arena<EffectDef>)`、`Table::lang`、row のラベルは `EffectId`、`UnifyError::MissingEffects(Vec<EffectId>)`、`Type::Con { id: TypeDefId, name: String }`、`EffectLabel { id: EffectId, name: String }`、`Type::Fn::effects: Vec<EffectLabel>`。`TyCon` と `ty::Effect` は無くなる

振る舞いは変えない。種類3の書き換えは `table/tests.rs`、`ty.rs` の単体テスト、`scope.rs` の単体テストだけである。

- [ ] **Step 1: HIR の item と lang item を作る**

`crates/eml_hir/src/hir.rs` に足す。

```rust
pub type TypeDefId = Idx<TypeDef>;
pub type EffectId = Idx<EffectDef>;

/// 型の item。今は組み込みの `Int`、`String`、`Bool`、`Unit` だけ。段階4で `data` を足す。
#[derive(Debug)]
pub struct TypeDef {
    pub name: String,
}

/// エフェクトの item。今は組み込みの `IO` だけ。段階3で `effect` の宣言を足す。
#[derive(Debug)]
pub struct EffectDef {
    pub name: String,
}

/// 処理系が名前ではなく役割で引く item。
#[derive(Debug, Clone, Copy)]
pub struct LangItems {
    pub int: TypeDefId,
    pub string: TypeDefId,
    pub bool: TypeDefId,
    pub unit: TypeDefId,
    pub io: EffectId,
}
```

`Module` に `pub types: Arena<TypeDef>`、`pub effects: Arena<EffectDef>`、`pub lang: LangItems` を足す (doc は spec の「HIR」の節のとおり)。`TypeRefKind::Builtin(BuiltinType)` を `Con(TypeDefId)` にし、`RowRef::Closed` と `RowRef::Open` の `effects: Vec<EffectRef>` を `effects: Vec<EffectId>` にする。`enum EffectRef` と `use crate::builtin::BuiltinType` を消す。

`crates/eml_hir/src/builtin.rs` から `BuiltinType` と `builtin_effect` を消す。

`crates/eml_hir/src/lower/scope.rs`:

- `TypeItem` を `Type(TypeDefId)` と `Effect(EffectId)` にする。
- `ItemScope` に `types: HashMap<String, TypeItem>` を足し、`define_type(&mut self, name: &str, id: TypeDefId)` と `define_effect(&mut self, name: &str, id: EffectId)` を足す。`type_item` は `self.types.get(name).copied()` にする。
- 組み込みの型とエフェクトを登録する関数を足す。

```rust
/// 組み込みの型とエフェクトを item として登録し、lang item を返す (docs/spec/declarations.md と docs/spec/effects.md)。
pub(super) fn builtin_items(
    types: &mut Arena<TypeDef>,
    effects: &mut Arena<EffectDef>,
    scope: &mut ItemScope,
) -> LangItems {
    let mut ty = |name: &str| {
        let id = types.alloc(TypeDef {
            name: name.to_string(),
        });
        scope.define_type(name, id);
        id
    };
    let (int, string, bool, unit) = (ty("Int"), ty("String"), ty("Bool"), ty("Unit"));
    let io = effects.alloc(EffectDef {
        name: "IO".to_string(),
    });
    scope.define_effect("IO", io);
    LangItems {
        int,
        string,
        bool,
        unit,
        io,
    }
}
```

- 単体テスト `types_and_effects_share_the_type_namespace` を次にする (種類3。確かめる中身は変えない)。

```rust
    #[test]
    fn types_and_effects_share_the_type_namespace() {
        let mut types = Arena::new();
        let mut effects = Arena::new();
        let mut scope = ItemScope::new();
        let lang = builtin_items(&mut types, &mut effects, &mut scope);
        assert_eq!(scope.type_item("Int"), Some(TypeItem::Type(lang.int)));
        assert_eq!(scope.type_item("IO"), Some(TypeItem::Effect(lang.io)));
        assert_eq!(scope.type_item("Console"), None);
    }
```

`crates/eml_hir/src/lower/mod.rs` の `lower` で、`ItemScope::new()` の直後、`lower_prelude` の前に、`let mut types = Arena::new(); let mut effects = Arena::new(); let lang = scope::builtin_items(&mut types, &mut effects, &mut scope);` を置く。最後の `Module { ... }` に `types, effects, lang` を足す。

`crates/eml_hir/src/lower/types.rs` の `path` を `Some(TypeItem::Type(id)) => TypeRefKind::Con(id)`、`Some(TypeItem::Effect(_)) | None => { ... }` に、`row` を `Some(TypeItem::Effect(effect)) => effects.push(effect)`、`Some(TypeItem::Type(_)) | None => { ... }` にする (診断は今のまま)。

`crates/eml_hir/src/pretty.rs` の型の表示で、`TypeRefKind::Con(id)` は `self.module.types[*id].name`、エフェクトの名前は `self.module.effects[*effect].name` を使う。

- [ ] **Step 2: 型検査の中の型構成子とエフェクトを ID にする**

`crates/eml_types/src/table/mod.rs`:

- `TyCon` を消し、`TyShape::Con(TyCon)` を `TyShape::Con(TypeDefId)` にする。
- `Row::labels` を `Vec<EffectId>` に、`UnifyError::MissingEffects(Vec<Effect>)` を `MissingEffects(Vec<EffectId>)` にする。
- `Table` に `pub lang: LangItems`、`type_names: ArenaMap<TypeDefId, String>`、`effect_names: ArenaMap<EffectId, String>` を足す。名前を持つのは、`Module` を渡さずに `display` と `export` が名前を出せるようにするためである。
- `Table::new()` を `Table::new(lang: LangItems, types: &Arena<TypeDef>, effects: &Arena<EffectDef>)` にし、`int`、`string`、`bool` を `TyShape::Con(lang.int)` などで作る。
- エフェクトの多重度を返すメソッドを足す。

```rust
    /// 今あるエフェクトは組み込みの `IO` だけで、実行時が必ず1回再開するので `Once` である (docs/spec/effects.md)。
    /// 段階3で、エフェクトの宣言の操作ごとの多重度に置き換える。
    pub fn effect_multiplicity(&self, effect: EffectId) -> Multiplicity {
        debug_assert_eq!(effect, self.lang.io);
        Multiplicity::Once
    }
```

`crates/eml_types/src/table/row.rs` の `bind_row` の `label.multiplicity()` を `self.effect_multiplicity(*label)` にする。

`crates/eml_types/src/ty.rs`:

- `enum Effect` と `impl Effect` を消し、外に出すラベルを足す。

```rust
/// 外に出す型の row のラベル。名前を持つのは、`Module` を渡さずに表示するため。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectLabel {
    pub id: EffectId,
    pub name: String,
}
```

- `Type::Int`、`Type::String`、`Type::Bool` を消し、`Type::Con { id: TypeDefId, name: String }` を足す。`Type::Fn` の `effects: Vec<Effect>` を `effects: Vec<EffectLabel>` にする。
- `Display` で `Type::Con { name, .. }` は `name` を出し、エフェクトの名前は `label.name` を使う。`contains_error` の腕を合わせる。
- 単体テスト `function_types_are_displayed_like_the_surface_syntax` は、`Arena<TypeDef>` と `Arena<EffectDef>` から ID を作り、`Type::Con { id, name: "Int".to_string() }` などで型を組み立てる形にする (種類3。期待する文字列は変えない)。

`crates/eml_types/src/table/export.rs` の `to_type` で、`TyShape::Con(id)` は `Type::Con { id, name: self.type_names[id].clone() }` に、row のラベルは `EffectLabel { id, name: self.effect_names[id].clone() }` にする。

`crates/eml_types/src/scheme.rs` の `lower` で、`TypeRefKind::Builtin(...)` の4つの腕を次の1つにし、エフェクトのラベルは `effects.clone()` にする。

```rust
        // `Unit` は空のレコードである (docs/spec/records.md)
        TypeRefKind::Con(id) if *id == table.lang.unit => table.unit,
        TypeRefKind::Con(id) if *id == table.lang.int => table.int,
        TypeRefKind::Con(id) if *id == table.lang.string => table.string,
        TypeRefKind::Con(id) if *id == table.lang.bool => table.bool,
        TypeRefKind::Con(id) => table.alloc(TyShape::Con(*id)),
```

`crates/eml_types/src/check/mod.rs`:

- `Table::new()` を `Table::new(module.lang, &module.types, &module.effects)` にする。
- `check_main` の期待する型を、lang item から組み立てる。

```rust
    let expected = Type::Fn {
        param: Box::new(Type::unit()),
        linearity: Linearity::Unr,
        effects: vec![EffectLabel {
            id: module.lang.io,
            name: module.effects[module.lang.io].name.clone(),
        }],
        tail: None,
        ret: Box::new(Type::unit()),
    };
```

- `has_error` の `TypeRefKind::Builtin(_) => false` を `TypeRefKind::Con(_) => false` にする。

`crates/eml_types/src/check/report.rs` の `MissingEffects` の名前を、`self.module.effects[*e].name.clone()` で引く。

`crates/eml_types/src/lib.rs` の再公開を `pub use ty::{EffectLabel, KindConstraint, KindTerm, Linearity, Multiplicity, RowTail, Type};` にする。

`crates/eml_types/src/table/tests.rs` (種類3。期待値は変えない):

- 先頭に、組み込みの型とエフェクトを持つ表を作る補助関数を置く。

```rust
use eml_hir::{EffectDef, LangItems, TypeDef};
use la_arena::Arena;

fn new_table() -> Table {
    let mut types = Arena::new();
    let mut effects = Arena::new();
    let mut ty = |name: &str| {
        types.alloc(TypeDef {
            name: name.to_string(),
        })
    };
    let lang = LangItems {
        int: ty("Int"),
        string: ty("String"),
        bool: ty("Bool"),
        unit: ty("Unit"),
        io: effects.alloc(EffectDef {
            name: "IO".to_string(),
        }),
    };
    Table::new(lang, &types, &effects)
}
```

- `Table::new()` をすべて `new_table()` にする。`Effect::Io` を使うテストでは、表を作った直後に `let io = table.lang.io;` を置き、`Effect::Io` を `io` にする。
- `Type::Int`、`Type::String`、`Type::Bool` と直接比べているアサーション (`assert_eq!(table.display(v), Type::Bool)` など) は、表示の文字列で比べる形 (`assert_eq!(table.display(v).to_string(), "Bool")`) にする。比べる中身は変えない。

`crates/eml_core_ir/src/lower.rs`:

- `new_var` の boxed の判定を、次にする。

```rust
            // 文字列と関数と型変数の値はヒープに置く (docs/spec/core-ir.md)
            boxed: match ty {
                Type::Con { id, .. } => *id == self.module.lang.string,
                Type::Fn { .. } | Type::Var(_) => true,
                _ => false,
            },
```

- 文字列の定数の `self.bind(out, "s", &Type::String, ...)` は、`lang.string` の `Type::Con` を作って渡す。

```rust
                let string = Type::Con {
                    id: self.module.lang.string,
                    name: "String".to_string(),
                };
                self.bind(out, "s", &string, Rhs::ConstString(index))
```

- [ ] **Step 3: 確かめる**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED"`
Expected: 何も出ない

Run: `git diff --no-ext-diff --stat -- '*.snap' crates/*/tests`
Expected: 何も出ない (変わるのは `src/` の単体テストだけ)

Run: `grep -rn "BuiltinType\|EffectRef\|builtin_effect\|TyCon\|Effect::Io\|Type::Int\b\|Type::String\b\|Type::Bool\b" crates/*/src`
Expected: 何も出ない

- [ ] **Step 4: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates
git commit -m "Represent types and effects as items with ids and look up lang items by role"
```

---

### Task 5: `TypedModule` のスキームと `Type::Rigid` / `Type::Flexible`

**Files:**
- Modify: `crates/eml_types/src/lib.rs` (`TypedModule`、`Scheme`、`dump`)
- Modify: `crates/eml_types/src/ty.rs`、`crates/eml_types/src/table/export.rs`、`crates/eml_types/src/check/mod.rs`
- Modify: `crates/eml_core_ir/src/lower.rs`

**Interfaces:**
- Consumes: Task 4 の `Type`
- Produces: `eml_types::Scheme { pub ty: Type, pub constraints: Vec<KindConstraint> }`、`TypedModule::signatures: ArenaMap<FunctionId, Scheme>`、`Type::Rigid(String)`、`Type::Flexible`。`TypedModule::kinds` と `Type::Var` は無くなる

振る舞いは変えない。テストの期待値は変わらない。

- [ ] **Step 1: 実装する**

`crates/eml_types/src/lib.rs`:

```rust
/// 型付き HIR。HIR は複製せず、型を別テーブルに持つ (docs/implementation/architecture.md)。
#[derive(Debug, Default)]
pub struct TypedModule {
    /// シグネチャのある関数だけを含む。
    pub signatures: ArenaMap<FunctionId, Scheme>,
    /// シグネチャと等式の両方がある関数だけを含む。
    pub bodies: ArenaMap<FunctionId, BodyTypes>,
    pub main: Option<FunctionId>,
}

/// 関数の型と、多相化したときに残った Kind の制約のうち、定数を片側に持つもの。変数どうしの制約は部分適用のたびに
/// 増えて読みにくくなるので出さない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scheme {
    pub ty: Type,
    pub constraints: Vec<KindConstraint>,
}
```

`dump` の `typed.signatures.get(id)` で得たスキームの `ty` を表示し、`kinds:` の行は `scheme.constraints` から作る (`typed.kinds` を使っていた部分を置き換える。表示の形は変えない)。

`crates/eml_types/src/check/mod.rs` で `TypedModule` を組み立てるところを、`typed.signatures.insert(id, crate::Scheme { ty: table.export(scheme.ty), constraints: kind_constraints(&table, scheme) })` にし、`typed.kinds.insert(...)` を消す (内部の `scheme::Scheme` と名前が重なるので、外に出す方は `crate::Scheme` と書く)。

`crates/eml_types/src/ty.rs`:

- `Type::Var(String)` を消し、`Rigid(String)` (doc: 「シグネチャの型変数。」) と `Flexible` (doc: 「推論で解けなかった変数。`_` と表示する。」) を足す。
- `Display` は `Rigid(name)` を `name`、`Flexible` を `_` にする。`contains_error` の腕を合わせる。

`crates/eml_types/src/table/export.rs` の `to_type` で、`TyShape::Var(_)` を `Type::Flexible`、`TyShape::Rigid(rigid)` を `Type::Rigid(name)` にする。

`crates/eml_core_ir/src/lower.rs`:

- `typed.signatures` から型を読むところ (28行目付近) を `.ty` を通す形にする。
- `new_var` の `Type::Var(_)` を `Type::Rigid(_) | Type::Flexible` にする。

- [ ] **Step 2: 確かめる**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED"`
Expected: 何も出ない

Run: `git diff --no-ext-diff --stat -- '*.snap' crates/*/tests`
Expected: 何も出ない

Run: `grep -rn "\.kinds\b\|Type::Var\b" crates/*/src`
Expected: 何も出ない

- [ ] **Step 3: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates
git commit -m "Return schemes from type checking and tell rigid from unsolved type variables"
```

---

### Task 6: 文書

**Files:**
- Modify: `docs/implementation/architecture.md`、`docs/implementation/status.md`

**Interfaces:**
- Consumes: Task 1〜5 の結果。承認を得て変えたテストがあれば、その内容
- Produces: なし

日本語の文書を書く前に `yomiyasu:yomiyasu` スキルを読み込み、その規則に従う。

- [ ] **Step 1: `architecture.md` を直す**

「`eml_hir` の内部」:

- 「名前は `Res::{Local, Function, Builtin}` に解決する。組み込み (`eml_hir::builtin::Builtin`) は名前解決の最も外側のスコープで、ユーザーの定義で隠せる。S2 で `Prelude` モジュールに移す」を、次にする。

```markdown
- 名前は `Res::{Local, Function, Builtin}` に解決する。組み込みは名前解決の最も外側のスコープで、ユーザーの定義で隠せる。組み込みの名前、見え方 (名前で引く値、演算子、名前で引けない内部用)、引数の数は `eml_hir::builtin::BUILTINS` の表に、シグネチャは eml のソースで書いた Prelude (`crates/eml_hir/src/prelude.em`) に置く。変換のはじめに Prelude を構文解析し、`Module::builtins` に置く。Prelude の範囲には `FileId::PRELUDE` を使い、診断には出さない。S2 で `Prelude` モジュールに移す
- 型とエフェクトは item (`Module::types`、`Module::effects`) で、ID (`TypeDefId`、`EffectId`) で参照する。今は組み込みの `Int`、`String`、`Bool`、`Unit` と `IO` だけを、変換のはじめに登録する。処理系が役割で引く item は `Module::lang` (`LangItems`) にある
```

「`eml_types` の内部」:

- 「`dump` は、スキームに残った Kind の制約のうち定数を片側に持つものを `kinds:` の行に出す」を、「`TypedModule::signatures` は、関数の型と、スキームに残った Kind の制約のうち定数を片側に持つもの (`Scheme::constraints`) を持つ。`dump` はこれを `kinds:` の行に出す」にする。
- 箇条の最後に足す。

```markdown
- 組み込みの型は、Prelude のシグネチャから、ユーザーの関数と同じ経路 (`Rigids`、`lower_signature`、`closure_kinds`、`Scheme`) で作る。本体がないので、作ってすぐ多相化する。`True` と `False` は、段階4で `data Bool` にするまで lang item の `Bool` の型である
- 型構成子は `TyShape::Con(TypeDefId)`、row のラベルは `EffectId` である。外に出す型は `Type::Con { id, name }` と `EffectLabel { id, name }` で、`Module` を渡さずに表示できるよう名前を持つ。型変数は、シグネチャの変数 (`Type::Rigid`) と推論で解けなかった変数 (`Type::Flexible`) を区別する
```

- [ ] **Step 2: `status.md` を直す**

- 「リファクタリング」の表の R2b-2 の行の状態を「完了」にする。
- 「テストを変えないために曲げた箇所」の表の4の行の「今の負担」を「R2b-2 で `kinds` を除き、`TypedModule` がスキームを返す形にした」にする。
- `#### R2b-2 組み込みと ID` の節の箇条を消し、次の1段落にする。

```markdown
R2b-2 で済んだ。組み込みのシグネチャを Prelude に置いてスキームの経路で型を作り、型とエフェクトを item と ID にし、`TypedModule` がスキームを返すようにした。Core IR への変換の対応 (`PrimOp` などとの対応) は、R3 の「組み込みの変換を R2 の表から引く」で扱う。
```

- 「各 crate の実装状況」の `eml_hir` の行の末尾に「組み込みのシグネチャは Prelude (`prelude.em`) に、型とエフェクトは ID で表す item にある」を足す。
- 「完了した作業」の表の末尾に足す。

```markdown
| リファクタリング R2b-2 | 組み込みのシグネチャを eml の Prelude に置き、型検査がユーザーの関数と同じ経路でスキームを作るようにした。組み込みの名前と見え方と引数の数を1つの表にした。型とエフェクトを item と ID (`TypeDefId`、`EffectId`) で表し、lang item を足した。`TypedModule` がスキームを返し、`Type::Var` を `Rigid` と `Flexible` に分けた |
```

- [ ] **Step 3: 文書を検査してコミットする**

Run: `python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py docs/implementation/architecture.md`
Expected: 書き足した部分の指摘を見直す。英単語の前後の半角空白と箇条書きの比率は直さない

Run: `cargo test`
Expected: PASS

```bash
git add docs/implementation
git commit -m "Document refactor R2b-2 in the architecture and status"
```

---

### Task 7: 仕上げの確認

**Files:**
- なし (確認だけ。直す必要が出たら、該当するタスクの範囲で直してコミットする)

**Interfaces:**
- Consumes: Task 1〜6 のすべて
- Produces: なし

- [ ] **Step 1: すべての検査を通す**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る。clippy の警告は0件

- [ ] **Step 2: 成功の条件を確かめる**

Run: `test ! -e crates/eml_types/src/builtins.rs && test -e crates/eml_hir/src/prelude.em && echo ok`
Expected: `ok`

Run: `grep -rn "BuiltinType\|EffectRef\|TyCon\|Effect::Io\|Type::Var\b\|\.kinds\b" crates/*/src`
Expected: 何も出ない

- [ ] **Step 3: 変わったテストが名前を挙げたものだけであることを確かめる**

Run: `git diff --no-ext-diff main --stat | grep -E "tests/|\.snap"`
Expected: `crates/eml_hir/tests/structure.rs` と `crates/eml_types/tests/check.rs` (どちらもテストを足しただけ) だけが出る

Run: `git diff --no-ext-diff -U0 main -- crates/eml_hir/tests crates/eml_types/tests | grep '^-' | grep -v '^---'`
Expected: `use` の行の差し替えのほかは何も出ない

## 完了後の後始末

ブランチ全体のレビューが済んだら、作業用の文書を削除する。

```bash
git rm docs/superpowers/specs/2026-10-04-refactor-r2b2-builtins-and-ids-design.md docs/superpowers/plans/2026-10-04-refactor-r2b2-builtins-and-ids.md
git commit -m "Remove the work documents of refactor R2b-2"
```
