# リファクタリング R2b-1: HIR の構造 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** HIR の `Function` から型の注釈と型変数の表を `Signature` と `Body` に移し、トップレベルの名前の解決を `ItemScope` にまとめ、HIR の走査関数を `Body` に置き、`x |> f a` が `x` を先に評価するようにする。

**Architecture:** データモデルの変更 (Task 1) と名前の解決 (Task 2) は振る舞いを変えない。走査関数 (Task 3) は `eml_core_ir` と `eml_types` の手書きの走査を置き換える。`|>` の脱糖 (Task 4) だけが振る舞いを変える。

**Tech Stack:** Rust (edition 2024)、la-arena、insta

**Spec:** `docs/superpowers/specs/2026-10-04-refactor-r2b1-hir-structure-design.md`

## Global Constraints

- 期待値は、このプランで名前を挙げたテストだけを変える。名前を挙げたのは `crates/eml_hir/tests/operators.rs` の `pipes_become_applications` (種類2、Task 4) だけである。期待値を変えない機械的な追随は許す (docs/implementation/testing.md の「テストの変更の運用」)
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

- シグネチャのない関数の本体の注釈が型変数を使ったときに、今と同じ E1002 が出ること (Task 1。空の `Generics` で引く)
- ユーザーが組み込みと同じ名前の関数を定義したときに、ユーザーの定義が組み込みを隠すこと (Task 2。既存の `functions_resolve_in_any_order_and_shadow_builtins` と新しい単体テスト)
- `lambda_captures` の順序が今の `captures` と同じで、Core IR のスナップショットが変わらないこと (Task 3)
- `usage.rs` の捕まえた変数の集合と `lambda_captures` が一致すること (Task 3 の `debug_assert` が、すべてのテストで発火しないこと)
- `|>` の型の誤りが、今と同じく左辺の位置に「argument N of `f`」として出ること (Task 4 で既存の型のテストと UI テストが変わらないことで確かめる)

---

### Task 1: `Signature`、`Generics`、本体の注釈のアリーナ

**Files:**
- Modify: `crates/eml_hir/src/hir.rs`
- Modify: `crates/eml_hir/src/lower/mod.rs`、`crates/eml_hir/src/lower/expr.rs`、`crates/eml_hir/src/lower/types.rs`
- Modify: `crates/eml_hir/src/pretty.rs`
- Modify: `crates/eml_types/src/scheme.rs`、`crates/eml_types/src/check/mod.rs`、`crates/eml_types/src/check/body.rs`
- Create: `crates/eml_hir/tests/structure.rs`
- Modify: `crates/eml_hir/Cargo.toml` (dev-dependencies は R0 の `eml_test_support` の `hir` で足りる。足りなければ確かめる)

**Interfaces:**
- Consumes: なし
- Produces: `eml_hir::Signature { ty, range, types: Arena<TypeRef>, generics: Generics }`、`eml_hir::Generics { type_vars: Arena<TypeVarDecl>, row_vars: Arena<RowVarDecl> }` (`Debug`、`Default`)、`Body::types: Arena<TypeRef>`。`Function` から `types`、`type_vars`、`row_vars` が無くなる。`eml_types::scheme::Rigids::new(table, &Generics)`、`lower_signature(table, &Signature, &Rigids) -> Ty`、`lower_type(table, &Arena<TypeRef>, &Rigids, TypeRefId) -> Ty`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_hir/tests/structure.rs` を作る。

```rust
//! HIR のデータ構造と走査関数のテスト。

use eml_hir::{Function, Module, TypeRefKind};

/// 診断のエラーがないことを確かめて HIR を返す。
fn module(text: &str) -> Module {
    let lowered = eml_test_support::lower(text);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    lowered.module
}

fn function<'m>(module: &'m Module, name: &str) -> &'m Function {
    module
        .functions
        .iter()
        .map(|(_, function)| function)
        .find(|function| function.name == name)
        .expect("the function")
}

#[test]
fn signature_and_body_annotations_live_in_separate_arenas() {
    // 本体を書き換えても、シグネチャのアリーナは変わらない。本体の注釈の型変数は、シグネチャの表を指す
    let module = module("f : a -> a\nf x = (x : a)");
    let f = function(&module, "f");
    let signature = f.signature.as_ref().expect("a signature");
    assert_eq!(signature.generics.type_vars.len(), 1);
    let body = f.body.as_ref().expect("a body");
    assert_eq!(body.types.len(), 1);
    let (_, annotation) = body.types.iter().next().expect("an annotation");
    let TypeRefKind::Var(var) = annotation.kind else {
        panic!("a type variable");
    };
    assert_eq!(signature.generics.type_vars[var].name, "a");
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_hir --test structure`
Expected: コンパイルエラー (`no field generics`、`no field types on Body`)

- [ ] **Step 3: HIR のデータモデルを変える**

`crates/eml_hir/src/hir.rs`:

- `Function` から `types`、`type_vars`、`row_vars` のフィールドとその doc コメントを消す。
- `Signature` に `pub types: Arena<TypeRef>` (doc: 「シグネチャの型の注釈。」) と `pub generics: Generics` を足す。
- `Generics` を足す。

```rust
/// 型変数と row 変数の表。シグネチャが持つ。段階3と4では、`data` とエフェクトの宣言も持つ。
#[derive(Debug, Default)]
pub struct Generics {
    pub type_vars: Arena<TypeVarDecl>,
    pub row_vars: Arena<RowVarDecl>,
}
```

- `Body` に `pub types: Arena<TypeRef>` を足す。doc は「本体の型の注釈。型変数と row 変数は、シグネチャの `Generics` を指す。シグネチャのアリーナと分けるのは、本体を書き換えてもシグネチャが変わらないようにするため。」にする。

`crates/eml_hir/src/lower/types.rs`:

- `TypeScope` を消す。
- `TypeLowering` のフィールド `type_vars` と `row_vars` を `pub generics: &'a mut Generics` にし、中の `self.type_vars` と `self.row_vars` を `self.generics.type_vars` と `self.generics.row_vars` にする。

`crates/eml_hir/src/lower/mod.rs`:

- シグネチャの変換で、`let mut types = Arena::new(); let mut generics = Generics::default();` を作り、`TypeLowering { file, types: &mut types, generics: &mut generics, define: true, diagnostics: &mut diagnostics }` で変換し、`Signature { ty, range, types, generics }` にする。
- `functions.alloc(Function { ... })` から `types`、`type_vars`、`row_vars` を消す。
- 本体の変換を次にする。

```rust
    // 本体は、すべての関数の名前がそろってから変換する。後ろで定義した関数も呼べるようにするため
    for (id, equation) in pending {
        // シグネチャがなければ、本体の注釈は型変数を引けない (docs/spec/types.md の「推論」)
        let mut no_generics = Generics::default();
        let generics = match &mut functions[id].signature {
            Some(signature) => &mut signature.generics,
            None => &mut no_generics,
        };
        let body =
            BodyLowering::new(file, &names, generics, &mut diagnostics).lower_equation(&equation);
        functions[id].body = Some(body);
    }
```

`crates/eml_hir/src/lower/expr.rs`:

- `BodyLowering` のフィールド `types: TypeScope<'a>` を、`types: Arena<TypeRef>` (本体の注釈) と `generics: &'a mut Generics` の2つにする。`new` は `generics: &'a mut Generics` を受け取り、`types: Arena::new()` で始める。
- `lower_equation` が返す `Body` に `types: self.types` を足す。
- `lower_type` を次にする。

```rust
    fn lower_type(&mut self, ty: Option<ast::Type>, fallback: TextRange) -> TypeRefId {
        TypeLowering {
            file: self.file,
            types: &mut self.types,
            generics: &mut *self.generics,
            define: false,
            diagnostics: &mut *self.diagnostics,
        }
        .lower(ty, fallback)
    }
```

- `use super::types::{TypeLowering, TypeScope};` を `use super::types::TypeLowering;` にする。

`crates/eml_hir/src/pretty.rs`:

- `Printer` に `generics: &'a Generics` のフィールドを足す。`pretty` で関数ごとに `Printer` を作るところで、シグネチャがあればその `generics` を、なければ関数ごとに作った空の `Generics::default()` を渡す。本体の注釈もシグネチャの型変数を指し、シグネチャがなければ型変数は現れないため。
- `fn ty(&self, id)` を `fn ty(&self, types: &Arena<TypeRef>, id: TypeRefId)` にし、型変数と row 変数の名前は `self.generics` から引く。
- シグネチャの表示は `self.ty(&signature.types, signature.ty)`、本体の `let` の注釈、`Annot`、`PatKind::Annot` の表示は `self.ty(&body.types, *ty)` にする。`ty` の中の再帰と row の表示も、受け取った `types` を渡す。

`crates/eml_types/src/scheme.rs`:

- `Rigids::new(table: &mut Table, generics: &Generics) -> Rigids` にし、`function.type_vars` と `function.row_vars` を `generics.type_vars` と `generics.row_vars` にする。
- `lower_signature(table: &mut Table, signature: &Signature, rigids: &Rigids) -> Ty` は `lower(table, &signature.types, rigids, signature.ty, true)` にする。
- `lower_type(table: &mut Table, types: &Arena<TypeRef>, rigids: &Rigids, id: TypeRefId) -> Ty` は `lower(table, types, rigids, id, false)` にする。
- `lower` の `function: &Function` を `types: &Arena<TypeRef>` にし、`function.types[id]` を `types[id]` にする。
- `use` を合わせる (`Function` を消し、`Generics`、`Signature`、`TypeRef`、`la_arena::Arena` を足す)。

`crates/eml_types/src/check/mod.rs`:

- `Rigids::new(&mut table, function)` を、シグネチャがあれば `&signature.generics`、なければ `&Generics::default()` を渡す形にする。

```rust
        let no_generics = Generics::default();
        let generics = function
            .signature
            .as_ref()
            .map_or(&no_generics, |signature| &signature.generics);
        let function_rigids = Rigids::new(&mut table, generics);
```

- `lower_signature(&mut table, function, &function_rigids, signature.ty)` を `lower_signature(&mut table, signature, &function_rigids)` にする。
- `has_error(function, signature.ty)` を `has_error(&signature.types, signature.ty)` にし、`has_error` は `types: &Arena<TypeRef>` を受け取る。

`crates/eml_types/src/check/body.rs`:

- `body_arrow_range` の `self.function.types[...]` を `signature.types[...]` にする。
- 本体の `Annot` と `let` の注釈の `lower_type(self.table, self.function, self.rigids, *ty)` を `lower_type(self.table, &self.body.types, self.rigids, *ty)` に、`self.function.types[*ty].range` を `self.body.types[*ty].range` にする。

- [ ] **Step 4: テストが通り、期待値が変わっていないことを確かめる**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED"`
Expected: 何も出ない

Run: `git diff --no-ext-diff --stat -- '*.snap' crates/*/tests`
Expected: `crates/eml_hir/tests/structure.rs` (新しいファイル) だけが出る

- [ ] **Step 5: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add -A crates/eml_hir crates/eml_types
git commit -m "Keep signature and body annotations in separate arenas and move type variables to Generics"
```

---

### Task 2: `ItemScope`

**Files:**
- Create: `crates/eml_hir/src/lower/scope.rs`
- Modify: `crates/eml_hir/src/lower/mod.rs`、`crates/eml_hir/src/lower/expr.rs`、`crates/eml_hir/src/lower/types.rs`
- Modify: `crates/eml_hir/src/builtin.rs`

**Interfaces:**
- Consumes: Task 1 の `TypeLowering`、`BodyLowering`
- Produces: `lower::scope::ItemScope` (`new()`、`define_function(&mut self, name: &str, id: FunctionId)`、`value(&self, name: &str) -> Option<ValueItem>`、`type_item(&self, name: &str) -> Option<TypeItem>`)、`ValueItem { Function(FunctionId), Builtin(Builtin) }`、`TypeItem { Builtin(BuiltinType), Effect(EffectRef) }`、`eml_hir::builtin::builtin_effect(name: &str) -> Option<EffectRef>`

振る舞いは変えない。既存のテストが変わらないことと、新しい単体テストで確かめる。

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_hir/src/lower/scope.rs` を、テストだけで作る。`crates/eml_hir/src/lower/mod.rs` に `mod scope;` を足す。

```rust
//! トップレベルの名前の解決。名前空間は、値 (関数、組み込みの値) と型 (型名とエフェクト名) の2つである
//! (docs/spec/modules.md の「名前空間」)。

#[cfg(test)]
mod tests {
    use la_arena::Arena;

    use super::*;
    use crate::hir::Function;

    #[test]
    fn user_functions_shadow_builtins() {
        let mut functions: Arena<Function> = Arena::new();
        let id = functions.alloc(Function {
            name: "not".to_string(),
            name_range: Default::default(),
            signature: None,
            body: None,
        });
        let mut scope = ItemScope::new();
        assert_eq!(scope.value("not"), Some(ValueItem::Builtin(Builtin::Not)));
        scope.define_function("not", id);
        assert_eq!(scope.value("not"), Some(ValueItem::Function(id)));
        assert_eq!(scope.value("nope"), None);
    }

    #[test]
    fn types_and_effects_share_the_type_namespace() {
        let scope = ItemScope::new();
        assert_eq!(
            scope.type_item("Int"),
            Some(TypeItem::Builtin(BuiltinType::Int))
        );
        assert_eq!(scope.type_item("IO"), Some(TypeItem::Effect(EffectRef::Io)));
        assert_eq!(scope.type_item("Console"), None);
    }
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_hir --lib scope`
Expected: コンパイルエラー (`cannot find type ItemScope`)

- [ ] **Step 3: 実装する**

`crates/eml_hir/src/builtin.rs` に足す。

```rust
/// 組み込みのエフェクト。S2 で `Prelude` モジュールに移すまで、名前解決の最も外側のスコープとして扱う。
pub fn builtin_effect(name: &str) -> Option<EffectRef> {
    match name {
        "IO" => Some(EffectRef::Io),
        _ => None,
    }
}
```

(`use crate::hir::EffectRef;` を足す。)

`crates/eml_hir/src/lower/scope.rs` の `#[cfg(test)]` の前に置く。

```rust
use std::collections::HashMap;

use crate::builtin::{Builtin, BuiltinType, builtin_effect};
use crate::hir::{EffectRef, FunctionId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ValueItem {
    Function(FunctionId),
    Builtin(Builtin),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TypeItem {
    Builtin(BuiltinType),
    Effect(EffectRef),
}

/// 組み込みは名前解決の最も外側のスコープで、ユーザーの定義で隠せる。そのため、ユーザーの定義を先に引き、
/// なければ組み込みを引く。
#[derive(Debug, Default)]
pub(super) struct ItemScope {
    functions: HashMap<String, FunctionId>,
}

impl ItemScope {
    pub(super) fn new() -> ItemScope {
        ItemScope::default()
    }

    pub(super) fn define_function(&mut self, name: &str, id: FunctionId) {
        self.functions.insert(name.to_string(), id);
    }

    pub(super) fn value(&self, name: &str) -> Option<ValueItem> {
        self.functions
            .get(name)
            .map(|&id| ValueItem::Function(id))
            .or_else(|| Builtin::from_name(name).map(ValueItem::Builtin))
    }

    pub(super) fn type_item(&self, name: &str) -> Option<TypeItem> {
        BuiltinType::from_name(name)
            .map(TypeItem::Builtin)
            .or_else(|| builtin_effect(name).map(TypeItem::Effect))
    }
}
```

`crates/eml_hir/src/lower/mod.rs`:

- `let mut names = HashMap::new();` を `let mut scope = ItemScope::new();` に、`names.insert(name, id);` を `scope.define_function(&name, id);` にする。
- `BodyLowering::new(file, &names, ...)` を `BodyLowering::new(file, &scope, ...)` にする。
- シグネチャの `TypeLowering` に `items: &scope` を渡す (下の `types.rs` の変更)。シグネチャは、その関数を `define_function` する前に変換しているが、型の表はユーザーの定義を持たないので順序は影響しない。

`crates/eml_hir/src/lower/expr.rs`:

- `BodyLowering` の `functions: &'a HashMap<String, FunctionId>` を `items: &'a ItemScope` にする。
- `lower_path` の解決を次にする。

```rust
        let res = self
            .scope
            .iter()
            .rev()
            .find(|(local, _)| local == text)
            .map(|&(_, local)| Res::Local(local))
            .or_else(|| {
                self.items.value(text).map(|item| match item {
                    ValueItem::Function(id) => Res::Function(id),
                    ValueItem::Builtin(builtin) => Res::Builtin(builtin),
                })
            });
```

- `lower_type` の `TypeLowering` に `items: self.items` を足す。
- 使わなくなった `use` (`HashMap`、`crate::builtin::Builtin`) を消す。

`crates/eml_hir/src/lower/types.rs`:

- `TypeLowering` に `pub items: &'a ItemScope` を足す。
- `path` の `match BuiltinType::from_name(name.text())` を次にする。エフェクトの名前を型の位置に書いたときの診断は、今と同じ「cannot find type」にする。

```rust
        match self.items.type_item(name.text()) {
            Some(TypeItem::Builtin(builtin)) => TypeRefKind::Builtin(builtin),
            Some(TypeItem::Effect(_)) | None => {
```

- `row` の `if name.text() == "IO" { effects.push(EffectRef::Io); } else { ... }` を次にする。

```rust
            match self.items.type_item(name.text()) {
                Some(TypeItem::Effect(effect)) => effects.push(effect),
                // ユーザー定義のエフェクトは段階3で入れる。宣言も E0004 になるので、ここでは未定義として扱う
                Some(TypeItem::Builtin(_)) | None => {
                    // 今の else の腕の中身 (E1002 の診断と valid = false)
                }
            }
```

- 使わなくなった `use` (`crate::builtin::BuiltinType`) を消し、`super::scope::{ItemScope, TypeItem}` を足す。

- [ ] **Step 4: テストが通り、期待値が変わっていないことを確かめる**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED"`
Expected: 何も出ない

Run: `grep -n '"IO"\|BuiltinType::from_name\|Builtin::from_name' crates/eml_hir/src/lower/*.rs`
Expected: `scope.rs` の中だけが出る (テストを除く)

Run: `git diff --no-ext-diff --stat -- '*.snap' crates/*/tests`
Expected: 何も出ない

- [ ] **Step 5: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates/eml_hir
git commit -m "Resolve top-level names through one item scope with value and type namespaces"
```

---

### Task 3: `Body` の走査関数

**Files:**
- Modify: `crates/eml_hir/src/hir.rs` (`impl Body`)
- Modify: `crates/eml_hir/tests/structure.rs`
- Modify: `crates/eml_core_ir/src/lower.rs` (`captures`、`bind_locals`、`binder`、`bind_pat`)
- Modify: `crates/eml_types/src/usage.rs` (`remove_bound`、`debug_assert`)

**Interfaces:**
- Consumes: Task 1 の `Body`
- Produces: `Body::walk_child_exprs(&self, id: ExprId, f: impl FnMut(ExprId))`、`Body::pat_bindings(&self, pat: PatId) -> Vec<LocalId>`、`Body::lambda_captures(&self, lambda: ExprId) -> Vec<LocalId>`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_hir/tests/structure.rs` の `use` を `use eml_hir::{Body, ExprId, ExprKind, Function, Module, PatKind, TypeRefKind};` にし、末尾に足す。

```rust
fn body<'m>(module: &'m Module, name: &str) -> &'m Body {
    function(module, name).body.as_ref().expect("a body")
}

fn children(body: &Body, id: ExprId) -> Vec<ExprId> {
    let mut out = Vec::new();
    body.walk_child_exprs(id, |child| out.push(child));
    out
}

/// 引数の名前が `param` のラムダ。
fn lambda(body: &Body, param: &str) -> ExprId {
    body.exprs
        .iter()
        .find(|(_, expr)| match &expr.kind {
            ExprKind::Lambda { params, .. } => params.iter().any(|&pat| {
                body.pat_bindings(pat)
                    .iter()
                    .any(|&local| body.locals[local].name == param)
            }),
            _ => false,
        })
        .map(|(id, _)| id)
        .expect("the lambda")
}

fn names(body: &Body, locals: &[eml_hir::LocalId]) -> Vec<String> {
    locals.iter().map(|&local| body.locals[local].name.clone()).collect()
}

#[test]
fn child_expressions_include_let_initializers_and_lambda_bodies() {
    let module = module("f : Int -> Int\nf x =\n  let y = x + 1\n  (fn z -> z) y");
    let body = body(&module, "f");
    let root = children(body, body.root);
    assert_eq!(root.len(), 2, "the let initializer and the tail");
    assert!(matches!(body.exprs[root[0]].kind, ExprKind::Call { .. }));
    let call = children(body, root[1]);
    let ExprKind::Lambda { body: lambda_body, .. } = body.exprs[call[0]].kind else {
        panic!("the callee is the lambda");
    };
    assert_eq!(children(body, call[0]), [lambda_body]);
}

#[test]
fn bindings_look_inside_annotated_patterns() {
    let module = module("f : Int -> Int\nf x = (fn (y : Int) -> y) x");
    let body = body(&module, "f");
    let ExprKind::Lambda { params, .. } = &body.exprs[lambda(body, "y")].kind else {
        unreachable!();
    };
    assert!(matches!(body.pats[params[0]].kind, PatKind::Annot { .. }));
    assert_eq!(names(body, &body.pat_bindings(params[0])), ["y"]);
}

#[test]
fn a_lambda_captures_what_its_nested_lambdas_capture() {
    // 入れ子のラムダが捕まえる変数は、内側のクロージャを作る外側のラムダも捕まえる (docs/spec/core-ir.md)
    let text = "f : Int -> Int -> Int\nf a b =\n  let g = fn x ->\n    let c = x + b\n    fn y -> a + c + y\n  g 1 2";
    let module = module(text);
    let body = body(&module, "f");
    assert_eq!(names(body, &body.lambda_captures(lambda(body, "x"))), ["a", "b"]);
    assert_eq!(names(body, &body.lambda_captures(lambda(body, "y"))), ["a", "c"]);
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_hir --test structure`
Expected: コンパイルエラー (`no method named walk_child_exprs`、`pat_bindings`、`lambda_captures`)

- [ ] **Step 3: 走査関数を実装する**

`crates/eml_hir/src/hir.rs` に足す (`use std::collections::{BTreeSet, HashSet};` を足す)。

```rust
impl Body {
    /// 式の直接の子を、ソースの順に `f` に渡す。子を辿る規則はここだけに置き、段階3と4で `match` や `handle` を
    /// 足すときはここを直す。
    pub fn walk_child_exprs(&self, id: ExprId, mut f: impl FnMut(ExprId)) {
        match &self.exprs[id].kind {
            ExprKind::Missing | ExprKind::Literal(_) | ExprKind::Path(_) => {}
            ExprKind::Call { callee, args } => {
                f(*callee);
                for &arg in args {
                    f(arg);
                }
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                f(*condition);
                f(*then_branch);
                if let Some(else_branch) = else_branch {
                    f(*else_branch);
                }
            }
            ExprKind::Block { stmts, tail } => {
                for stmt in stmts {
                    match stmt {
                        Stmt::Let { init, .. } => f(*init),
                        Stmt::Expr(expr) => f(*expr),
                    }
                }
                if let Some(tail) = tail {
                    f(*tail);
                }
            }
            ExprKind::Annot { expr, .. } => f(*expr),
            ExprKind::Lambda { body, .. } => f(*body),
        }
    }

    /// パターンが束縛する局所変数。型を明示したパターンは内側を見る。
    pub fn pat_bindings(&self, pat: PatId) -> Vec<LocalId> {
        let mut out = Vec::new();
        self.collect_bindings(pat, &mut out);
        out
    }

    fn collect_bindings(&self, pat: PatId, out: &mut Vec<LocalId>) {
        match &self.pats[pat].kind {
            PatKind::Bind(local) => out.push(*local),
            PatKind::Annot { pat, .. } => self.collect_bindings(*pat, out),
            PatKind::Missing | PatKind::Wildcard | PatKind::Unit => {}
        }
    }

    /// ラムダの本体が参照する局所変数のうち、ラムダの中で束縛していないもの。`LocalId` の順に並べる。ラムダは捕まえた
    /// 変数を先頭の引数に持つ関数に持ち上げるので (docs/spec/core-ir.md)、入れ子のラムダが捕まえる変数は外側のラムダも
    /// 捕まえる。式の木は作業リストでたどる。
    pub fn lambda_captures(&self, lambda: ExprId) -> Vec<LocalId> {
        let mut used = BTreeSet::new();
        let mut bound = HashSet::new();
        let mut work = vec![lambda];
        while let Some(id) = work.pop() {
            match &self.exprs[id].kind {
                ExprKind::Path(Res::Local(local)) => {
                    used.insert(*local);
                }
                ExprKind::Lambda { params, .. } => {
                    for &param in params {
                        bound.extend(self.pat_bindings(param));
                    }
                }
                ExprKind::Block { stmts, .. } => {
                    for stmt in stmts {
                        if let Stmt::Let { pat, .. } = stmt {
                            bound.extend(self.pat_bindings(*pat));
                        }
                    }
                }
                _ => {}
            }
            self.walk_child_exprs(id, |child| work.push(child));
        }
        used.into_iter()
            .filter(|local| !bound.contains(local))
            .collect()
    }
}
```

Run: `cargo test -p eml_hir --test structure`
Expected: PASS (4件)

- [ ] **Step 4: 手書きの走査を置き換える**

`crates/eml_core_ir/src/lower.rs`:

- `captures(self.body, ...)` などの `captures(` の呼び出しを `body.lambda_captures(` にし、`fn captures` と `fn bind_locals` を消す。
- `fn binder` を消し、呼び出し (`let local = binder(body, pat);`) を `let local = body.pat_bindings(pat).first().copied();` にする。
- `FnLowering::bind_pat` を次にする (doc コメントは今のまま)。

```rust
    fn bind_pat(&mut self, pat: PatId, value: Atom) {
        for local in self.body.pat_bindings(pat) {
            self.locals.insert(local, value);
        }
    }
```

- 使わなくなった `use` (`BTreeSet`、`HashSet`、`Stmt` など) を消す。

`crates/eml_types/src/usage.rs`:

- `fn remove_bound` の本体を次にする。

```rust
fn remove_bound(body: &Body, pat: PatId, uses: &mut Uses) {
    for local in body.pat_bindings(pat) {
        uses.remove(&local);
    }
}
```

- `Lambda` の腕で、`captured.sort();` の後に足す。

```rust
                // 捕まえた変数の集合は、Core IR の変換が使う `lambda_captures` と同じでなければならない
                debug_assert_eq!(captured, body.lambda_captures(id));
```

- [ ] **Step 5: テストが通り、期待値が変わっていないことを確かめる**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED|panicked"`
Expected: 何も出ない (`debug_assert_eq!` が発火しない)

Run: `grep -n "fn captures\|fn bind_locals\|fn binder" crates/eml_core_ir/src/lower.rs`
Expected: 何も出ない

Run: `git diff --no-ext-diff --stat -- '*.snap' crates/eml_core_ir/tests crates/eml_types/tests crates/eml_cli/tests`
Expected: 何も出ない

- [ ] **Step 6: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates/eml_hir crates/eml_core_ir crates/eml_types
git commit -m "Walk child expressions, pattern bindings, and lambda captures through Body"
```

---

### Task 4: `|>` の脱糖

**Files:**
- Modify: `crates/eml_hir/src/lower/expr.rs` (`pipe`)、`crates/eml_hir/src/lower/ops.rs`
- Modify: `crates/eml_hir/tests/operators.rs` (`pipes_become_applications`)
- Create: `tests/ui/run/pipe_evaluation_order.em`、`crates/eml_cli/tests/snapshots/ui__run@pipe_evaluation_order.em.snap`
- Modify: `docs/spec/declarations.md:126`、`docs/spec/expressions.md:179`

**Interfaces:**
- Consumes: Task 3 までの HIR
- Produces: `BodyLowering::pipe(&mut self, value: ExprId, function: ExprId, range: TextRange) -> ExprId`

このタスクで期待値を変えるテスト (種類2): `pipes_become_applications`。

- [ ] **Step 1: 失敗するテストを書く**

`tests/ui/run/pipe_evaluation_order.em`:

```
-- `x |> f a` evaluates `x` before the arguments of `f a`.
say : String -> <IO> Int
say s =
  println s
  1

add : Int -> Int -> Int
add a b = a + b

main : Unit -> <IO> Unit
main () =
  let n = say "left" |> add (say "right")
  println (show_int n)
```

`crates/eml_cli/tests/snapshots/ui__run@pipe_evaluation_order.em.snap` (末尾の改行は既存の `ui__run@hello.em.snap` に合わせる):

```
---
source: crates/eml_cli/tests/ui.rs
expression: "format!(\"--- stdout ---\\n{stdout}--- stderr ---\\n{stderr}\")"
input_file: tests/ui/run/pipe_evaluation_order.em
---
--- stdout ---
left
right
2
--- stderr ---
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `INSTA_UPDATE=no cargo test -p eml_cli --test ui run 2>&1 | grep -E "test result|^-left|^\+right|pipe_evaluation_order"`
Expected: FAIL。今の実装の stdout は `right`、`left`、`2` の順である

- [ ] **Step 3: 実装する**

`crates/eml_hir/src/lower/expr.rs` の `impl BodyLowering` に足す。

```rust
    /// `x |> f a` を `{ let $pipe = x; f a $pipe }` にする。`x` を先に評価するため (docs/spec/declarations.md の標準の
    /// 演算子の表)。`$` は識別子に使えないので、作った変数はユーザーの名前と重ならない。作った式の範囲は `x` の範囲に
    /// して、型の誤りを `x` の位置に出す。
    pub(super) fn pipe(&mut self, value: ExprId, function: ExprId, range: TextRange) -> ExprId {
        let value_range = self.exprs[value].range;
        let local = self.locals.alloc(Local {
            name: "$pipe".to_string(),
            range: value_range,
        });
        let pat = self.pats.alloc(Pat {
            kind: PatKind::Bind(local),
            range: value_range,
        });
        let arg = self.alloc(ExprKind::Path(Res::Local(local)), value_range);
        let call = self.call(function, vec![arg], range);
        self.alloc(
            ExprKind::Block {
                stmts: vec![Stmt::Let {
                    pat,
                    ty: None,
                    init: value,
                }],
                tail: Some(call),
            },
            range,
        )
    }
```

`crates/eml_hir/src/lower/ops.rs` の `"|>" => self.call(rhs, vec![lhs], range),` を `"|>" => self.pipe(lhs, rhs, range),` にする。その上のコメントで `|>` を関数適用と説明している箇所があれば、`let` と関数適用への脱糖に直す。

- [ ] **Step 4: UI テストが通り、HIR のスナップショットの差分を確かめる**

Run: `cargo test -p eml_cli --test ui`
Expected: PASS

Run: `cargo test -p eml_hir --test operators pipes_become_applications 2>&1 | grep -E "^\s+[0-9]* *[0-9]* *│[-+]"`
Expected: FAIL。`p = (@g 2 (@f 1))` の行が消え、`p = {` で始まる入れ子のブロックになる。外側のブロックは `let $pipe#N = {` で内側のブロック (`let $pipe#M = 1` と `(@f $pipe#M)`) を束縛し、`(@g 2 $pipe#N)` で終わる。`M` は `N` より小さい。`q = (@g 1 (@f 2))` の行は変わらない

差分が上のとおりであることを確かめてから、`pipes_become_applications` の期待値を新しい出力に置き換える (種類2。spec で合意済み)。差分が上と違う場合は、変えずに止まってユーザーに示す。

- [ ] **Step 5: spec を直す**

`docs/spec/declarations.md` の演算子の表の `| \`\|>\` \`<\|\` | \`x \|> f\` と \`f <\| x\` は、HIR で関数適用 \`f x\` に脱糖する |` の行を、次にする。

```markdown
| `\|>` `<\|` | `x \|> f` は `x` を先に評価してから `f` に適用する。HIR で `{ let p = x; f p }` に脱糖する。`f <\| x` は関数適用 `f x` に脱糖する |
```

`docs/spec/expressions.md` の脱糖の一覧の「`|>` と `<|` の関数適用への脱糖」を、「`|>` の `let` と関数適用への脱糖、`<|` の関数適用への脱糖」にする。

- [ ] **Step 6: テストが通り、名前を挙げたものだけが変わったことを確かめる**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED"`
Expected: 何も出ない

Run: `git diff --no-ext-diff --stat -- '*.snap' crates/*/tests`
Expected: `crates/eml_hir/tests/operators.rs` と、新しい UI のスナップショットのファイルだけが出る

- [ ] **Step 7: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates/eml_hir tests/ui/run crates/eml_cli/tests/snapshots docs/spec
git commit -m "Evaluate the left side of |> first by binding it before the call"
```

---

### Task 5: 文書

**Files:**
- Modify: `docs/implementation/architecture.md`、`docs/implementation/status.md`、`docs/implementation/testing.md`

**Interfaces:**
- Consumes: Task 1〜4 の結果。承認を得て変えたテストがあれば、その内容
- Produces: なし

日本語の文書を書く前に `yomiyasu:yomiyasu` スキルを読み込み、その規則に従う。

- [ ] **Step 1: `architecture.md` の「`eml_hir` の内部」を直す**

- 1つ目の箇条の最後の文「型の注釈は関数ごとの `types` に置く」を、「型の注釈は、シグネチャのものを `Signature::types` に、本体のものを `Body::types` に置く。本体を書き換えてもシグネチャが変わらないようにするため」にする。
- 2つ目の箇条の「型変数と row 変数の表は `Function::type_vars` / `row_vars` に置く」を、「型変数と row 変数の表は `Signature::generics` (`Generics`) に置く」にする。
- 4つ目の箇条の後に足す。

```markdown
- トップレベルの名前は、変換の中の `ItemScope` (`lower/scope.rs`) で解決する。値と型 (型名とエフェクト名) の2つの名前空間を持ち、ユーザーの定義を先に引き、なければ組み込みを引く
- `Body` は走査関数を持つ。`walk_child_exprs` は式の直接の子を辿り、`pat_bindings` はパターンが束縛する変数を、`lambda_captures` はラムダが捕まえる変数を返す。段階3と4で式やパターンの種類を足すときは、これらを直す
```

- 5つ目の箇条の「`|>` / `<|` は関数適用に脱糖する」を、「`x |> f` は `{ let $pipe = x; f $pipe }` に、`f <| x` は関数適用に脱糖する。`|>` の左辺を先に評価するため」にする。

- [ ] **Step 2: `status.md` を直す**

「リファクタリング」の表の R2b の行を、次の2行にする。

```markdown
| R2b-1 | HIR の構造 | `Signature` と `Generics`、本体の注釈のアリーナ、`ItemScope`、`Body` の走査関数、`\|>` の評価順 | 完了 |
| R2b-2 | 組み込みと ID | 組み込みの表と lang item、エフェクトと型構成子の ID、`TypedModule` のスキーム。`eml_core_ir` と `eml_interp` も追随させる | 未着手 |
```

表の上の段落の「5つの回に分け、R0 → R1 → R2a → R2b → R3 の順に」を「6つの回に分け、R0 → R1 → R2a → R2b-1 → R2b-2 → R3 の順に」にする。

`#### R2b データモデル` の節を次にする (箇条の中身は今の一覧から写す)。

```markdown
#### R2b-1 HIR の構造

R2b-1 で済んだ。`Function` の型の注釈と型変数の表を `Signature` (`Generics`) と `Body` に分け、トップレベルの名前の解決を `ItemScope` にまとめ、走査関数を `Body` に置き、`|>` の左辺を先に評価するようにした。

#### R2b-2 組み込みと ID

- (今の一覧の「組み込みの名前、fixity、型、Core IR への変換を1つの表にまとめる…」の箇条)
- (「エフェクトと型構成子を ID で表す…」の箇条)。組み込みの型とエフェクトを item にして、`ItemScope` を通して引く
- (「`TypedModule` がスキームを返すようにし…」の箇条)
```

「完了した作業」の表の末尾に足す。

```markdown
| リファクタリング R2b-1 | HIR の `Function` の型の注釈と型変数の表を `Signature` と `Body` に分け、トップレベルの名前の解決を `ItemScope` にまとめた。子の式、パターンの束縛、ラムダが捕まえる変数を `Body` の走査関数にした。`x \|> f` が `x` を先に評価するようにした |
```

- [ ] **Step 3: `testing.md` に記録する**

「テストの変更の記録」の「リファクタリング R2a」の後に足す。承認を得て変えたテストがあれば、最後の箇条の後に足す。

```markdown
### リファクタリング R2b-1

- `x |> f a` を `{ let $pipe = x; f a $pipe }` に脱糖し、`x` を先に評価するようにした ([宣言](../spec/declarations.md) の標準の演算子の表)。`eml_hir/tests/operators.rs` の `pipes_become_applications` の HIR が、`$pipe` の `let` を持つ入れ子のブロックになった (種類2)。評価順は `tests/ui/run/pipe_evaluation_order.em` で確かめる
```

- [ ] **Step 4: 文書を検査してコミットする**

Run: `python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py docs/implementation/architecture.md`
Expected: 書き足した部分の指摘を見直す。英単語の前後の半角空白と箇条書きの比率は直さない

Run: `cargo test`
Expected: PASS

```bash
git add docs/implementation
git commit -m "Document refactor R2b-1 in the architecture, status, and test-change record"
```

---

### Task 6: 仕上げの確認

**Files:**
- なし (確認だけ。直す必要が出たら、該当するタスクの範囲で直してコミットする)

**Interfaces:**
- Consumes: Task 1〜5 のすべて
- Produces: なし

- [ ] **Step 1: すべての検査を通す**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る。clippy の警告は0件

- [ ] **Step 2: 成功の条件を確かめる**

Run: `grep -n "pub types\|pub type_vars\|pub row_vars" crates/eml_hir/src/hir.rs`
Expected: `Signature::types`、`Generics::type_vars`、`Generics::row_vars`、`Body::types` だけが出る (`Function` の中にはない)

Run: `grep -rn '"IO"' crates/eml_hir/src/lower`
Expected: 何も出ない

Run: `grep -rn "fn captures\|fn bind_locals\|fn binder" crates/eml_core_ir/src`
Expected: 何も出ない

- [ ] **Step 3: 変わったテストが名前を挙げたものだけであることを確かめる**

Run: `git diff --no-ext-diff main --stat | grep -E "tests/|\.snap"`
Expected: `crates/eml_hir/tests/operators.rs`、`crates/eml_hir/tests/structure.rs` (新しいファイル)、`tests/ui/run/pipe_evaluation_order.em` と、そのスナップショットのファイルだけが出る

Run: `git diff --no-ext-diff -U0 main -- crates/eml_hir/tests/operators.rs | grep '^-' | grep -v '^---'`
Expected: `p = (@g 2 (@f 1))` の行だけが出る

## 完了後の後始末

ブランチ全体のレビューが済んだら、作業用の文書を削除する。

```bash
git rm docs/superpowers/specs/2026-10-04-refactor-r2b1-hir-structure-design.md docs/superpowers/plans/2026-10-04-refactor-r2b1-hir-structure.md
git commit -m "Remove the work documents of refactor R2b-1"
```
