# 縦の貫通 段階3a Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ユーザー定義のエフェクトの `never` と `once` の操作、deep handler、`resume`、`drop` を、HIR、型検査、Core IR、ランタイム、インタプリタの全体に通す。

**Architecture:** 上流から順に積む。構文の AST アクセサ (Task 1) の上に、HIR のエフェクトと操作の item (Task 2) と handler の式 (Task 3) を載せる。型検査は、継続の型と操作のスキーム (Task 4)、handle の検査 (Task 5)、Kind の制約の由来と E3001 (Task 6) の順に入れる。実行側は、Core IR の命令 (Task 7) と変換 (Task 8)、ランタイムのオブジェクトとインタプリタ (Task 9) の順に入れ、`check-fail/` の UI テスト (Task 10) と文書 (Task 11) で締める。handle の本体と節はラムダと同じ方法でクロージャに持ち上げ、`perform` で handler フレームの `next` を切り離し、`resume` でつなぎ直す。

**Tech Stack:** Rust (edition 2024)、rowan 0.16、la-arena 0.3、insta 1.49。新しい外部 crate は足さない。

**Spec:** `docs/superpowers/specs/2026-10-04-stage-3a-effects-design.md` (段階3a の設計)。規範は `docs/spec/` の `effects.md`、`expressions.md` (handler)、`declarations.md` (`effect`)、`modules.md` (名前空間と節の名前の解決)、`types.md`、`linearity.md`、`core-ir.md`、`runtime.md`、`diagnostics.md`。

## Global Constraints

- 作業は `main` から切ったブランチ `stage-3a` で行う。タスクごとにコミットする。コミットメッセージは英語で、末尾に次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01WjPPZfGKCuYNKKYkSe1YwN
  ```

- 外部 crate は増やさない。集合と表は `std::collections` を使う
- コードのコメントは日本語で、何をするかではなく理由を書く。規則を指すときは `docs/` のパスを書く。日本語を書く前に `yomiyasu:yomiyasu` スキルを読み、その規則に従う (CLAUDE.md)
- 各段階は `fn stage(&In) -> (Out, Vec<Diagnostic>)` の純粋関数のままにする。`eml_core_ir::lower` だけは、エラーのないプログラムを受け取り診断を返さない
- 診断の help / note は eml 自身の規則の説明に限る。他の言語の書き方を前提にしたヒントは入れない
- インタプリタとランタイムの値とフレームに `Rc` と `RefCell` を使わない。`unsafe` を書かない
- 既存のテストで変えてよいのは、spec の「変わるテスト」の表にある種類3 (期待値を変えない機械的な追随) だけである。表にないテストの期待値が変わったら、変えずに止まり、差分と理由をユーザーに示して承認を得る。設計を曲げてテストを守ることはしない
- 各タスクの最後に `cargo test`、`cargo clippy --all-targets`、`cargo fmt` を通す。clippy の警告を残さない
- インラインスナップショットと UI テストの期待値は、このプランのコードが出す形を書いてある。食い違ったら、まず実装がプランのコードと一致しているかを確かめる。プランの期待値の誤り (位置の数え違いなど) だと判断した場合は、理由をコミットメッセージに書いてから直す
- 診断の位置の表記は、1 始まりの行と、文字数で数えた列 (`2:7`) である
- 深い再帰を避ける。`Let` の連鎖や長いリストをたどる処理はループか作業リストで書く。式の入れ子は E0013 の上限 (256) で抑えられているので、式の木の再帰はよい
- 途中のタスクでは、後のタスクで実装する分岐を仮の分岐で埋め、ワークスペース全体をビルドできる状態に保つ。仮の分岐は、型検査では `self.table.error` か空の使用回数、Core IR では `unreachable!`、インタプリタでは `Fault::Internal` にする。どのタスクで置き換えるかは各ステップに書いてある。仮の分岐には「後で実装する」という趣旨のコメントを書かない (置き換えるのはこのプランの中で済む)

## Review Focus

- `once` の節が `k` をフレームに退避したまま `never` の操作を起こす。中断で解放する区間が `k` を含み、`k` の区間と内側の handler フレームまでまとめて解放される → Task 9 の `run/effect_abort.em` (`bounded 20`)
- 別のエフェクトの handler フレームを越えて `perform` が handler を探す → Task 9 の `run/effect_deep.em` (`walked`)
- クロージャの中から操作を起こし、継続の区間に `Apply` のフレームやラムダの関数のフレームが入る → Task 9 の `run/operation_values.em` と `run/continuation_values.em`
- 本体が操作を起こさずに値を返し、handler フレームを外して節のクロージャを解放する → Task 9 の `run/effect_abort.em` (`safe 3`) と `run/effect_resume.em`
- 呼ばずに捨てた操作の部分適用のクロージャも解放される → Task 9 の `run/operation_values.em` (`unused`)

---

### Task 1: AST にエフェクトと handler のアクセサを足す

HIR が `effect` の宣言、handler、`resume`、`drop` を読めるように、型付き AST にアクセサを足す。構文と CST は S1 で実装済みで、変えない。

**Files:**
- Modify: `crates/eml_syntax/src/ast.rs`
- Test: `crates/eml_syntax/tests/ast.rs`

**Interfaces:**
- Produces:
  - `ast::Clause` (`OpClause` と `ReturnClause` の enum)
  - `EffectItem::name() -> Option<SyntaxToken>`、`EffectItem::params() -> impl Iterator<Item = SyntaxToken>`、`EffectItem::operations() -> AstChildren<OpDecl>`
  - `OpDecl::multiplicity() -> Option<SyntaxToken>`、`OpDecl::name() -> Option<SyntaxToken>`、`OpDecl::ty() -> Option<Type>`
  - `HandleExpr::body() -> Option<Expr>`、`HandleExpr::from_keyword() -> Option<SyntaxToken>`、`HandleExpr::clauses() -> AstChildren<Clause>`
  - `OpClause::name() -> Option<SyntaxToken>`、`OpClause::params() -> AstChildren<Pat>`、`OpClause::body() -> Option<Expr>`
  - `ReturnClause::params() -> AstChildren<Pat>`、`ReturnClause::body() -> Option<Expr>`
  - `ResumeExpr::args() -> AstChildren<Expr>`、`DropExpr::args() -> AstChildren<Expr>`
  - `Effect::args() -> AstChildren<Type>`

- [ ] **Step 1: ブランチを作る**

```bash
git switch -c stage-3a
```

- [ ] **Step 2: 失敗するテストを書く**

`crates/eml_syntax/tests/ast.rs` の先頭の `use` の並びの後に `use eml_syntax::ast::Clause;` を足し、ファイルの最後に次のテストを足す。

```rust
#[test]
fn effect_declaration_parts() {
    let file = source("effect State s where\n  get : Unit -> s\n  never fail : String -> a");
    let Some(Item::EffectItem(effect)) = file.items().next() else {
        panic!("expected an effect");
    };
    assert_eq!(effect.name().unwrap().text(), "State");
    let params: Vec<String> = effect.params().map(|t| t.text().to_string()).collect();
    assert_eq!(params, ["s"]);
    let operations: Vec<(Option<SyntaxKind>, String)> = effect
        .operations()
        .map(|op| {
            (
                op.multiplicity().map(|t| t.kind()),
                op.name().unwrap().text().to_string(),
            )
        })
        .collect();
    assert_eq!(
        operations,
        [(None, "get".to_string()), (Some(NEVER_KW), "fail".to_string())]
    );
    assert!(
        effect
            .operations()
            .all(|op| matches!(op.ty(), Some(Type::FnType(_))))
    );
}

#[test]
fn handler_clauses_resume_and_drop() {
    let file = source("h = handle f () with\n  | ask key k -> resume k key\n  | return x -> drop x");
    let equation = first_equation(&file);
    let Some(Expr::HandleExpr(handle)) = equation.body() else {
        panic!("expected a handler");
    };
    assert!(matches!(handle.body(), Some(Expr::AppExpr(_))));
    assert!(handle.from_keyword().is_none());
    let clauses: Vec<Clause> = handle.clauses().collect();
    let [Clause::OpClause(op), Clause::ReturnClause(ret)] = clauses.as_slice() else {
        panic!("{clauses:?}");
    };
    assert_eq!(op.name().unwrap().text(), "ask");
    assert_eq!(op.params().count(), 2);
    let Some(Expr::ResumeExpr(resume)) = op.body() else {
        panic!("expected `resume`");
    };
    assert_eq!(resume.args().count(), 2);
    assert_eq!(ret.params().count(), 1);
    let Some(Expr::DropExpr(drop)) = ret.body() else {
        panic!("expected `drop`");
    };
    assert_eq!(drop.args().count(), 1);
}

#[test]
fn handler_with_an_initial_state() {
    let file = source("h = handle f () from s with | return x st -> x");
    let equation = first_equation(&file);
    let Some(Expr::HandleExpr(handle)) = equation.body() else {
        panic!("expected a handler");
    };
    assert!(matches!(handle.body(), Some(Expr::AppExpr(_))));
    assert_eq!(handle.from_keyword().unwrap().kind(), FROM_KW);
}

#[test]
fn effect_arguments_in_a_row() {
    let file = source("f : Unit -> <State Int> Unit");
    let Some(Item::Signature(signature)) = file.items().next() else {
        panic!("expected a signature");
    };
    let Some(Type::FnType(function)) = signature.ty() else {
        panic!("expected a function type");
    };
    let effect = function.row().unwrap().effects().next().unwrap();
    assert_eq!(effect.args().count(), 1);
}
```

- [ ] **Step 3: テストが失敗することを確かめる**

Run: `cargo test -p eml_syntax --test ast`
Expected: コンパイルエラー (`no method named name found for struct EffectItem` など)

- [ ] **Step 4: アクセサを実装する**

`crates/eml_syntax/src/ast.rs` の `ast_enum! { Type { ... } }` の後に、節の enum を足す。

```rust
ast_enum! {
    /// handler の節。
    Clause { OpClause, ReturnClause }
}
```

`impl Effect` の `name` の後に `args` を足し、その後に次の impl を足す。

```rust
impl Effect {
    pub fn name(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::UIDENT)
    }

    /// row に書いたエフェクトの型引数。
    pub fn args(&self) -> AstChildren<Type> {
        support::children(&self.syntax)
    }
}

impl EffectItem {
    pub fn name(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::UIDENT)
    }

    /// エフェクトの型引数。操作の宣言は子のノードなので、直下のトークンだけを見る。
    pub fn params(&self) -> impl Iterator<Item = SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .filter(|token| token.kind() == SyntaxKind::LIDENT)
    }

    pub fn operations(&self) -> AstChildren<OpDecl> {
        support::children(&self.syntax)
    }
}

impl OpDecl {
    /// `never` / `once` / `multi`。省略したら `None` で、`once` として扱う (docs/spec/declarations.md)。
    pub fn multiplicity(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .find(|token| {
                matches!(
                    token.kind(),
                    SyntaxKind::NEVER_KW | SyntaxKind::ONCE_KW | SyntaxKind::MULTI_KW
                )
            })
    }

    pub fn name(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::LIDENT)
    }

    pub fn ty(&self) -> Option<Type> {
        support::child(&self.syntax)
    }
}

impl HandleExpr {
    /// handle する式。`from` があれば初期値より前、なければ `with` より前にある。
    pub fn body(&self) -> Option<Expr> {
        let before = if self.from_keyword().is_some() {
            SyntaxKind::FROM_KW
        } else {
            SyntaxKind::WITH_KW
        };
        child_between(&self.syntax, Some(SyntaxKind::HANDLE_KW), Some(before))
    }

    pub fn from_keyword(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::FROM_KW)
    }

    pub fn clauses(&self) -> AstChildren<Clause> {
        support::children(&self.syntax)
    }
}

impl OpClause {
    /// 節の先頭の操作の名前。引数はパターンのノードなので、直下の最初の小文字の名前が操作の名前である。
    pub fn name(&self) -> Option<SyntaxToken> {
        support::token(&self.syntax, SyntaxKind::LIDENT)
    }

    /// 操作の引数、`k`、(パラメータ付き handler なら) 状態のパターン。
    pub fn params(&self) -> AstChildren<Pat> {
        support::children(&self.syntax)
    }

    pub fn body(&self) -> Option<Expr> {
        child_between(&self.syntax, Some(SyntaxKind::THIN_ARROW), None)
    }
}

impl ReturnClause {
    pub fn params(&self) -> AstChildren<Pat> {
        support::children(&self.syntax)
    }

    pub fn body(&self) -> Option<Expr> {
        child_between(&self.syntax, Some(SyntaxKind::THIN_ARROW), None)
    }
}

impl ResumeExpr {
    /// 個数は文法で制限せず、HIR で検査する (docs/spec/grammar.md の「文法上の補足」)。
    pub fn args(&self) -> AstChildren<Expr> {
        support::children(&self.syntax)
    }
}

impl DropExpr {
    pub fn args(&self) -> AstChildren<Expr> {
        support::children(&self.syntax)
    }
}
```

もとの `impl Effect { pub fn name ... }` は上の新しい `impl Effect` に置き換える (同じ impl を2つ作らない)。

- [ ] **Step 5: テストが通ることを確かめる**

Run: `cargo test -p eml_syntax --test ast`
Expected: PASS

- [ ] **Step 6: 全体を確かめてコミットする**

```bash
cargo test && cargo clippy --all-targets && cargo fmt
git add crates/eml_syntax
git commit -m "Add AST accessors for effects, handlers, resume, and drop"
```

---

### Task 2: エフェクトと操作を HIR の item にする

`effect` の宣言を E0004 にせず、`Module::effects` と新しい `Module::operations` に変換する。操作を値の名前空間に置き、`Res::Operation` に解決する。操作のシグネチャを検査し (E1007、E1008)、重複を E1003 にする。`multi`、エフェクトの型引数、row に書いたエフェクトの型引数は E0004 にする。

**Files:**
- Modify: `crates/eml_hir/src/hir.rs`
- Modify: `crates/eml_hir/src/lib.rs` (`codes`)
- Modify: `crates/eml_hir/src/lower/mod.rs`
- Create: `crates/eml_hir/src/lower/effect.rs`
- Modify: `crates/eml_hir/src/lower/scope.rs`
- Modify: `crates/eml_hir/src/lower/types.rs`
- Modify: `crates/eml_hir/src/lower/expr.rs` (`lower_path`)
- Modify: `crates/eml_hir/src/pretty.rs`
- Modify: `crates/eml_types/src/check/body.rs` (仮の分岐。Task 4 で置き換える)
- Modify: `crates/eml_core_ir/src/lower.rs` (仮の分岐。Task 8 で置き換える)
- Modify: `crates/eml_types/src/ty.rs` と `crates/eml_types/src/table/tests.rs` (`EffectDef` の組み立て。種類3)
- Test: `crates/eml_hir/tests/effects.rs` (新規)

**Interfaces:**
- Consumes: Task 1 のアクセサ
- Produces:
  - `eml_hir::{Operation, OperationId, OpMultiplicity}`。`Operation { name: String, name_range: TextRange, effect: EffectId, multiplicity: OpMultiplicity, signature: Signature, arity: usize }`、`OpMultiplicity::{Never, Once}`
  - `EffectDef { name: String, operations: Vec<OperationId> }`、`Module::operations: Arena<Operation>`
  - `Res::Operation(OperationId)`
  - `eml_hir::codes::{INVALID_OPERATION_SIGNATURE (E1007), NEVER_RESULT_NOT_FREE (E1008)}`
  - `ItemScope::{define_function, define_operation} -> Option<ValueItem>`、`ItemScope::operation(&str) -> Option<OperationId>`、`ValueItem::Operation(OperationId)`
  - `lower::duplicate(file, name, a, b) -> Diagnostic` (E1003。後に書いた方を primary にする)
  - `pretty` はユーザーのエフェクトを `effect 名前` と操作の行で先に表示し、操作の参照を `@エフェクト.操作` と表示する

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_hir/tests/effects.rs` を作る。

```rust
mod common;

use common::lower_text;
use eml_test_support::{lower, short};

/// 構文と HIR の診断を、位置の順に並べたもの。
fn errors(text: &str) -> Vec<String> {
    let mut lowered = lower(text);
    lowered.diagnostics.sort_by_key(|d| d.primary.range.start());
    short(&lowered.files, &lowered.diagnostics)
}

#[test]
fn effects_and_operations_are_items() {
    let text = "effect Ask where\n  ask : String -> String\n  never stop : Unit -> a\n\nf : Unit -> <Ask> String\nf () = ask \"x\"";
    insta::assert_snapshot!(lower_text(text), @r#"
    effect Ask
      ask : String -> String
      never stop : Unit -> a
    f : Unit -> <Ask> String
    f () = (@Ask.ask "x")
    "#);
}

#[test]
fn operations_share_the_value_namespace() {
    let text = "effect A where\n  op : Int -> Int\n\neffect B where\n  op : Int -> Int\n\nhelper : Int -> Int\nhelper x = x\n\neffect C where\n  helper : Int -> Int\n\neffect A where\n  other : Int -> Int";
    insta::assert_snapshot!(lower_text(text), @r"
    effect A
      op : Int -> Int
    effect B
      op : Int -> Int
    effect C
      helper : Int -> Int
    effect A
      other : Int -> Int
    helper : Int -> Int
    helper x#0 = x#0
    ---
    E1003 5:3 `op` is defined more than once
    E1003 11:3 `helper` is defined more than once
    E1003 13:8 `A` is defined more than once
    ");
}

#[test]
fn operation_signatures_are_checked() {
    let text = "effect E where\n  with_row : Int -> <IO> Int\n  constant : Int\n  never bad : a -> a\n  never good : Int -> b\n  multi many : Unit -> Int";
    insta::assert_snapshot!(lower_text(text), @r"
    effect E
      with_row : Int -> <IO> Int
      constant : Int
      never bad : a -> a
      never good : Int -> b
      many : Unit -> Int
    ---
    E1007 2:21 an operation cannot have a row on its outermost arrows
    E1007 3:14 the signature of an operation must be a function type
    E1008 4:20 the result type of a `never` operation must be a type variable that does not appear in its parameters
    E0004 6:3 `multi` operations are not supported yet
    ");
}

#[test]
fn effect_type_parameters_and_arguments_come_in_stage_3b() {
    let text = "effect State s where\n  get : Unit -> s\n\nf : Unit -> <State Int> Int\nf () = 1";
    assert_eq!(
        errors(text),
        [
            "E0004 1:14 effect type parameters are not supported yet",
            "E0004 4:14 effects with type arguments are not supported yet",
        ]
    );
}

#[test]
fn a_function_after_an_operation_of_the_same_name_is_a_duplicate() {
    let text = "effect E where\n  run : Int -> Int\n\nrun : Int -> Int\nrun x = x";
    assert_eq!(errors(text), ["E1003 4:1 `run` is defined more than once"]);
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_hir --test effects`
Expected: FAIL。`effect` の宣言が E0004 になり、表示にエフェクトが出ない

- [ ] **Step 3: HIR のデータ構造を足す**

`crates/eml_hir/src/hir.rs` の `Module` と `EffectDef` を次にし、`Operation` と `OpMultiplicity` を足す。

```rust
#[derive(Debug)]
pub struct Module {
    pub file: FileId,
    pub functions: Arena<Function>,
    /// 型の item。今は組み込みの `Int`、`String`、`Bool`、`Unit` だけ。段階4で `data` を足す。
    pub types: Arena<TypeDef>,
    /// エフェクトの item。組み込みの `IO` と、`effect` の宣言。
    pub effects: Arena<EffectDef>,
    /// エフェクトの操作。値の名前空間に置くトップレベルの値である (docs/spec/modules.md の「名前空間」)。
    pub operations: Arena<Operation>,
    /// Prelude の組み込みのシグネチャ。
    pub builtins: HashMap<Builtin, Signature>,
    pub lang: LangItems,
}

pub type TypeDefId = Idx<TypeDef>;
pub type EffectId = Idx<EffectDef>;
pub type OperationId = Idx<Operation>;

#[derive(Debug)]
pub struct TypeDef {
    pub name: String,
}

#[derive(Debug)]
pub struct EffectDef {
    pub name: String,
    /// 宣言した順の操作。組み込みの `IO` の操作は組み込みの関数なので、ここには入らない。
    pub operations: Vec<OperationId>,
}

/// 操作の多重度 (docs/spec/effects.md)。`multi` は段階3b で足す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpMultiplicity {
    Never,
    Once,
}

#[derive(Debug)]
pub struct Operation {
    pub name: String,
    pub name_range: TextRange,
    pub effect: EffectId,
    pub multiplicity: OpMultiplicity,
    /// 型変数は、操作ごとに暗黙に量化する。
    pub signature: Signature,
    /// シグネチャの一番外側の `->` の数 (docs/spec/declarations.md の「`effect`」)。
    pub arity: usize,
}
```

`Res` に `Operation` を足す。

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Res {
    Local(LocalId),
    Function(FunctionId),
    Operation(OperationId),
    Builtin(Builtin),
}
```

`crates/eml_hir/src/lib.rs` の `codes` に足す。

```rust
    pub const INVALID_OPERATION_SIGNATURE: ErrorCode = ErrorCode(1007);
    pub const NEVER_RESULT_NOT_FREE: ErrorCode = ErrorCode(1008);
```

- [ ] **Step 4: 値の名前空間に操作を置く**

`crates/eml_hir/src/lower/scope.rs` の `ValueItem`、`ItemScope` とその impl を次にする。`builtin_items` の `EffectDef` には `operations: Vec::new()` を足す。

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ValueItem {
    Function(FunctionId),
    Operation(OperationId),
    Builtin(Builtin),
}

/// 組み込みは名前解決の最も外側のスコープで、ユーザーの定義で隠せる。そのため、ユーザーの定義を先に引き、
/// なければ組み込みを引く。
#[derive(Debug, Default)]
pub(super) struct ItemScope {
    /// ユーザーが定義した値 (関数と操作)。
    values: HashMap<String, ValueItem>,
    types: HashMap<String, TypeItem>,
}

impl ItemScope {
    pub(super) fn new() -> ItemScope {
        ItemScope::default()
    }

    /// 同じ名前のユーザーの値があれば、それを返す。重複の診断は呼び出し側が出す。
    pub(super) fn define_function(&mut self, name: &str, id: FunctionId) -> Option<ValueItem> {
        self.values
            .insert(name.to_string(), ValueItem::Function(id))
    }

    pub(super) fn define_operation(&mut self, name: &str, id: OperationId) -> Option<ValueItem> {
        self.values
            .insert(name.to_string(), ValueItem::Operation(id))
    }

    pub(super) fn define_type(&mut self, name: &str, id: TypeDefId) {
        self.types.insert(name.to_string(), TypeItem::Type(id));
    }

    pub(super) fn define_effect(&mut self, name: &str, id: EffectId) {
        self.types.insert(name.to_string(), TypeItem::Effect(id));
    }

    pub(super) fn value(&self, name: &str) -> Option<ValueItem> {
        self.values
            .get(name)
            .copied()
            .or_else(|| Builtin::from_name(name).map(ValueItem::Builtin))
    }

    /// handler の節の先頭の名前は、エフェクトの操作だけから引く (docs/spec/modules.md の「名前の解決」)。
    pub(super) fn operation(&self, name: &str) -> Option<OperationId> {
        match self.values.get(name) {
            Some(ValueItem::Operation(id)) => Some(*id),
            _ => None,
        }
    }

    pub(super) fn type_item(&self, name: &str) -> Option<TypeItem> {
        self.types.get(name).copied()
    }
}
```

`use crate::hir::{...}` に `OperationId` を足す。

- [ ] **Step 5: `effect` の宣言を変換する**

`crates/eml_hir/src/lower/effect.rs` を作る。

```rust
//! `effect` の宣言の変換 (docs/spec/declarations.md の「`effect`」)。エフェクトは型の名前空間に、操作は値の名前空間に
//! 置く (docs/spec/modules.md の「名前空間」)。

use std::collections::HashMap;

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_syntax::{SyntaxKind, ast};
use la_arena::Arena;

use super::duplicate;
use super::scope::ItemScope;
use super::types::TypeLowering;
use crate::codes;
use crate::hir::{
    EffectDef, EffectId, Generics, OpMultiplicity, Operation, RowRef, Signature, TypeRef,
    TypeRefId, TypeRefKind, TypeVarId,
};

/// エフェクトの名前をすべて登録してから、操作のシグネチャを変換する。操作の引数の型の row で、後ろで宣言した
/// エフェクトも引けるようにするため。
pub(super) fn lower_effects(
    file: FileId,
    items: &[ast::EffectItem],
    scope: &mut ItemScope,
    effects: &mut Arena<EffectDef>,
    operations: &mut Arena<Operation>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut declared: HashMap<String, TextRange> = HashMap::new();
    let mut lowered = Vec::new();
    for item in items {
        // 名前がなければパーサが報告済み
        let Some(name) = item.name() else {
            continue;
        };
        for param in item.params() {
            diagnostics.push(Diagnostic::not_yet_supported(
                file,
                param.text_range(),
                "effect type parameters are not supported yet",
            ));
        }
        let range = name.text_range();
        let id = effects.alloc(EffectDef {
            name: name.text().to_string(),
            operations: Vec::new(),
        });
        match declared.get(name.text()) {
            Some(&first) => diagnostics.push(duplicate(file, name.text(), first, range)),
            None => {
                declared.insert(name.text().to_string(), range);
                scope.define_effect(name.text(), id);
            }
        }
        lowered.push((id, item));
    }
    let mut values: HashMap<String, TextRange> = HashMap::new();
    for (effect, item) in lowered {
        for decl in item.operations() {
            let Some(operation) = lower_operation(file, &decl, effect, scope, diagnostics) else {
                continue;
            };
            let name = operation.name.clone();
            let range = operation.name_range;
            let id = operations.alloc(operation);
            effects[effect].operations.push(id);
            match values.get(&name) {
                Some(&first) => diagnostics.push(duplicate(file, &name, first, range)),
                None => {
                    values.insert(name.clone(), range);
                    scope.define_operation(&name, id);
                }
            }
        }
    }
}

fn lower_operation(
    file: FileId,
    decl: &ast::OpDecl,
    effect: EffectId,
    scope: &ItemScope,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Operation> {
    // 名前がなければパーサが報告済み
    let name = decl.name()?;
    let multiplicity = match decl.multiplicity() {
        Some(token) if token.kind() == SyntaxKind::NEVER_KW => OpMultiplicity::Never,
        Some(token) if token.kind() == SyntaxKind::MULTI_KW => {
            diagnostics.push(Diagnostic::not_yet_supported(
                file,
                token.text_range(),
                "`multi` operations are not supported yet",
            ));
            OpMultiplicity::Once
        }
        _ => OpMultiplicity::Once,
    };
    let range = decl.ty().map_or(decl.range(), |ty| ty.range());
    let mut types = Arena::new();
    let mut generics = Generics::default();
    let ty = TypeLowering {
        file,
        types: &mut types,
        generics: &mut generics,
        items: scope,
        define: true,
        diagnostics: &mut *diagnostics,
    }
    .lower(decl.ty(), range);
    let signature = Signature {
        ty,
        range,
        types,
        generics,
    };
    let arity = check_signature(file, name.text(), &signature, multiplicity, diagnostics);
    Some(Operation {
        name: name.text().to_string(),
        name_range: name.text_range(),
        effect,
        multiplicity,
        signature,
        arity,
    })
}

/// 一番外側の `->` をたどり、引数の個数を返す。外側の矢印の row と、関数型でないシグネチャを E1007 に、`never` の
/// 操作の結果の型の誤りを E1008 にする (docs/spec/declarations.md の「`effect`」)。操作の型の row は、型検査が最後の
/// 外側の矢印に付けるので、外側の矢印に row を書く場所はない。引数のない操作には、row を付ける矢印もない。
fn check_signature(
    file: FileId,
    name: &str,
    signature: &Signature,
    multiplicity: OpMultiplicity,
    diagnostics: &mut Vec<Diagnostic>,
) -> usize {
    let types = &signature.types;
    let mut params = Vec::new();
    let mut id = signature.ty;
    while let TypeRefKind::Fn { param, row, ret } = &types[id].kind {
        if let RowRef::Closed { range, .. } | RowRef::Open { range, .. } = row {
            diagnostics.push(
                Diagnostic::error(
                    codes::INVALID_OPERATION_SIGNATURE,
                    "an operation cannot have a row on its outermost arrows",
                    Label::new(file, *range, "remove this row"),
                )
                .with_note("an operation performs only the effect it belongs to"),
            );
        }
        params.push(*param);
        id = *ret;
    }
    if params.is_empty() {
        // 型が壊れていれば、変換が報告済み
        if !matches!(types[id].kind, TypeRefKind::Error) {
            diagnostics.push(
                Diagnostic::error(
                    codes::INVALID_OPERATION_SIGNATURE,
                    "the signature of an operation must be a function type",
                    Label::new(file, signature.range, "this type is not a function type"),
                )
                .with_help(format!(
                    "an operation without arguments takes `Unit`, as in `{name} : Unit -> ...`"
                )),
            );
        }
        return 0;
    }
    if multiplicity == OpMultiplicity::Never {
        let free = match types[id].kind {
            TypeRefKind::Var(var) => !params.iter().any(|&param| mentions(types, param, var)),
            TypeRefKind::Error => true,
            TypeRefKind::Con(_) | TypeRefKind::Fn { .. } => false,
        };
        if !free {
            diagnostics.push(
                Diagnostic::error(
                    codes::NEVER_RESULT_NOT_FREE,
                    "the result type of a `never` operation must be a type variable that does not appear in its parameters",
                    Label::new(file, types[id].range, "this result type"),
                )
                .with_note(
                    "a `never` operation does not return, so its caller may use the result as any type",
                ),
            );
        }
    }
    params.len()
}

fn mentions(types: &Arena<TypeRef>, id: TypeRefId, var: TypeVarId) -> bool {
    match &types[id].kind {
        TypeRefKind::Var(other) => *other == var,
        TypeRefKind::Fn { param, ret, .. } => {
            mentions(types, *param, var) || mentions(types, *ret, var)
        }
        TypeRefKind::Error | TypeRefKind::Con(_) => false,
    }
}
```

- [ ] **Step 6: 変換の入口でエフェクトを先に変換する**

`crates/eml_hir/src/lower/mod.rs` を次のように直す。

1. 先頭の `mod` の並びに `mod effect;` を足し、`use scope::ItemScope;` を `use scope::{ItemScope, ValueItem};` にする。
2. `collect` が `effect` の宣言を集めて返すようにする。戻り値を `(Vec<Definition>, Vec<ast::EffectItem>)` にし、`let mut effects = Vec::new();` を作って、`ast::Item::EffectItem(item)` の腕を `ast::Item::EffectItem(item) => effects.push(item),` にする。最後を `(definitions, effects)` にする。
3. `lower` の先頭を次にする。

```rust
pub fn lower(file: FileId, source: &ast::SourceFile) -> (Module, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let (definitions, effect_items) = collect(file, source, &mut diagnostics);
    let mut functions = Arena::new();
    let mut scope = ItemScope::new();
    let mut types = Arena::new();
    let mut effects = Arena::new();
    let mut operations = Arena::new();
    let lang = scope::builtin_items(&mut types, &mut effects, &mut scope);
    let builtins = prelude::lower_prelude(&scope);
    // 関数のシグネチャの row がユーザーのエフェクトを引けるように、エフェクトを先に変換する
    effect::lower_effects(
        file,
        &effect_items,
        &mut scope,
        &mut effects,
        &mut operations,
        &mut diagnostics,
    );
    let mut pending = Vec::new();
```

4. 関数の定義のループの `scope.define_function(&name, id);` を次にする。

```rust
        if let Some(ValueItem::Operation(operation)) = scope.define_function(&name, id) {
            diagnostics.push(duplicate(
                file,
                &name,
                operations[operation].name_range,
                first_range,
            ));
        }
```

5. 最後の `Module { ... }` に `operations,` を足す。
6. ファイルの最後に `duplicate` を足す。

```rust
/// 同じ名前空間のトップレベルの定義の重複 (docs/spec/modules.md の「名前空間」)。ソースで後に書いた方を primary にする。
pub(super) fn duplicate(file: FileId, name: &str, a: TextRange, b: TextRange) -> Diagnostic {
    let (first, again) = if a.start() <= b.start() { (a, b) } else { (b, a) };
    Diagnostic::error(
        codes::DUPLICATE_DEFINITION,
        format!("`{name}` is defined more than once"),
        Label::new(file, again, "defined again here"),
    )
    .with_secondary(Label::new(file, first, "first defined here"))
}
```

- [ ] **Step 7: row のエフェクトの型引数と、操作の参照を変換する**

`crates/eml_hir/src/lower/types.rs` の `row` で、`for effect in row.effects()` のループの先頭 (`let Some(name) = effect.name()` の前) に次を足す。

```rust
            if effect.args().next().is_some() {
                self.diagnostics.push(Diagnostic::not_yet_supported(
                    self.file,
                    effect.range(),
                    "effects with type arguments are not supported yet",
                ));
                valid = false;
                continue;
            }
```

同じ関数のコメント「ユーザー定義のエフェクトは段階3で入れる。宣言も E0004 になるので、ここでは未定義として扱う」は消す。

`crates/eml_hir/src/lower/expr.rs` の `lower_path` の `match item` に腕を足す。

```rust
                    ValueItem::Operation(id) => Res::Operation(id),
```

- [ ] **Step 8: エフェクトと操作を表示する**

`crates/eml_hir/src/pretty.rs` の `pretty` と `Printer` を次にする。`Printer` は関数を持たず、`function` が引数で受け取る。

```rust
pub fn pretty(module: &Module) -> String {
    let mut out = String::new();
    for (id, effect) in module.effects.iter() {
        // 組み込みの `IO` の操作は組み込みの関数なので、表示しない
        if id == module.lang.io {
            continue;
        }
        writeln!(out, "effect {}", effect.name).unwrap();
        for &operation in &effect.operations {
            let operation = &module.operations[operation];
            let printer = Printer {
                module,
                generics: &operation.signature.generics,
            };
            let never = match operation.multiplicity {
                OpMultiplicity::Never => "never ",
                OpMultiplicity::Once => "",
            };
            let signature = &operation.signature;
            writeln!(
                out,
                "  {never}{} : {}",
                operation.name,
                printer.ty(&signature.types, signature.ty)
            )
            .unwrap();
        }
    }
    for (_, function) in module.functions.iter() {
        // 本体の注釈もシグネチャの型変数を指す。シグネチャがなければ型変数は現れない
        let no_generics = Generics::default();
        let generics = function
            .signature
            .as_ref()
            .map_or(&no_generics, |signature| &signature.generics);
        Printer { module, generics }.function(function, &mut out);
    }
    out
}

struct Printer<'a> {
    module: &'a Module,
    generics: &'a Generics,
}

impl Printer<'_> {
    fn function(&self, function: &Function, out: &mut String) {
```

`function` の本体の `let function = self.function;` の行を消す (引数の `function` を使う)。`res` に腕を足す。

```rust
            Res::Operation(operation) => {
                let operation = &self.module.operations[operation];
                let effect = &self.module.effects[operation.effect].name;
                format!("@{effect}.{}", operation.name)
            }
```

- [ ] **Step 9: 下流の crate をビルドできる状態に保つ**

`crates/eml_types/src/check/body.rs` の `value` の `match res` に仮の腕を足す。Task 4 で操作のスキームの具体化に置き換える。

```rust
            Res::Operation(_) => self.table.error,
```

`crates/eml_core_ir/src/lower.rs` の `atom` の `match &body.exprs[id].kind` で、`ExprKind::Path(Res::Function(function)) => { ... }` の腕の後に仮の腕を足す。Task 8 で操作を包む関数のクロージャに置き換える。

```rust
            ExprKind::Path(Res::Operation(_)) => {
                unreachable!("operations are not lowered to Core IR yet")
            }
```

`EffectDef` を組み立てるテスト (種類3) に `operations: Vec::new()` を足す。対象は `crates/eml_types/src/ty.rs` の `function_types_are_displayed_like_the_surface_syntax` と、`crates/eml_types/src/table/tests.rs` の `new_table` である。期待値は変えない。

- [ ] **Step 10: テストが通ることを確かめる**

Run: `cargo test -p eml_hir --test effects`
Expected: PASS

- [ ] **Step 11: 全体を確かめてコミットする**

```bash
cargo test && cargo clippy --all-targets && cargo fmt
git add crates
git commit -m "Lower effect declarations and operations to HIR items"
```

---
### Task 3: handler、`resume`、`drop` を HIR の式にする

`handle ... with`、`resume`、`drop` を E0004 にせず、HIR の式に変換する。節の先頭の名前を操作だけから解決し、handler の節の誤り (E1001、E1009、E1010、E1012、E1013、E1014) と、`resume` と `drop` の引数の個数の誤り (E1011) を報告する。`from` と `resume` の3引数の形は E0004 のままにする。`Body` の走査関数を handler に広げる。

**Files:**
- Modify: `crates/eml_hir/src/hir.rs`
- Modify: `crates/eml_hir/src/lib.rs` (`codes`)
- Modify: `crates/eml_hir/src/builtin.rs` (`is_io_operation`)
- Modify: `crates/eml_hir/src/lower/mod.rs` (`BodyLowering::new` の引数)
- Modify: `crates/eml_hir/src/lower/expr.rs`
- Create: `crates/eml_hir/src/lower/handler.rs`
- Modify: `crates/eml_hir/src/pretty.rs`
- Modify: `crates/eml_types/src/check/body.rs`、`crates/eml_types/src/usage.rs` (仮の分岐。Task 5 と Task 6 で置き換える)
- Modify: `crates/eml_core_ir/src/lower.rs` (仮の分岐。Task 8 で置き換える)
- Test: `crates/eml_hir/tests/effects.rs`

**Interfaces:**
- Consumes: Task 2 の `Operation`、`OpMultiplicity`、`ItemScope::operation`
- Produces:
  - `ExprKind::Handle { body: ExprId, effect: Option<EffectId>, clauses: Vec<OpClause>, ret: Option<ReturnClause> }`、`ExprKind::Resume { k: ExprId, arg: ExprId }`、`ExprKind::Drop(ExprId)`
  - `OpClause { op: OperationId, params: Vec<PatId>, k: Option<PatId>, body: ExprId, range: TextRange }` と `OpClause::patterns() -> impl Iterator<Item = PatId>` (引数、`k` の順)
  - `ReturnClause { param: PatId, body: ExprId, range: TextRange }`
  - `Body::captures(root: ExprId, bound: &[PatId]) -> Vec<LocalId>` (`lambda_captures(lambda)` は `captures(lambda, &[])`)
  - `Builtin::is_io_operation(self) -> bool`
  - `eml_hir::codes::{UNHANDLEABLE_EFFECT (E1009), CLAUSE_ARITY (E1010), KEYWORD_ARITY (E1011), MIXED_EFFECTS_IN_HANDLER (E1012), MISSING_CLAUSE (E1013), DUPLICATE_CLAUSE (E1014)}`
  - 表示: `(handle 本体 with | 操作 引数... k -> 本体 | return x -> 本体)`、`(resume k v)`、`(drop x)`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_hir/tests/effects.rs` に足す。

```rust
#[test]
fn handlers_resume_and_drop_are_expressions() {
    let text = "effect Ask where\n  ask : String -> String\n  never fail : String -> a\n\nf : Unit -> String\nf () =\n  handle ask \"x\" with\n    | ask key k -> resume k key\n    | fail message -> message\n    | return x -> x\n\ng : String -> Unit\ng s = drop s";
    insta::assert_snapshot!(lower_text(text), @r#"
    effect Ask
      ask : String -> String
      never fail : String -> a
    f : Unit -> String
    f () = {
      (handle (@Ask.ask "x") with | ask key#0 k#1 -> (resume k#1 key#0) | fail message#2 -> message#2 | return x#3 -> x#3)
    }
    g : String -> Unit
    g s#0 = (drop s#0)
    "#);
}

#[test]
fn clause_names_are_resolved_among_operations_only() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nf : Unit -> Int\nf () =\n  let ask = 1\n  handle ask with\n    | ask () k -> resume k ask";
    insta::assert_snapshot!(lower_text(text), @r"
    effect Ask
      ask : Unit -> Int
    f : Unit -> Int
    f () = {
      let ask#0 = 1
      (handle ask#0 with | ask () k#1 -> (resume k#1 ask#0))
    }
    ");
}

#[test]
fn clause_errors_are_reported_together() {
    let text = "effect Ask where\n  ask : Unit -> Int\n  other : Unit -> Int\n\neffect Log where\n  log : String -> Unit\n\neffect Fail where\n  never fail : String -> a\n\nf : Unit -> Int\nf () =\n  handle 1 with\n    | ask () -> 1\n    | ask () k -> resume k 1\n    | log m k -> resume k ()\n    | nope x k -> 1\n    | println s k -> resume k ()\n    | return x -> x\n    | return y -> y\n\ng : Unit -> Int\ng () = handle 1 with | fail m k -> 1\n\nh : Unit -> Int\nh () = handle 1 with | return x -> x";
    assert_eq!(
        errors(text),
        [
            "E1013 13:3 this handler has no clause for `other` of `Ask`",
            "E1010 14:7 the clause for `ask` takes 2 parameters, but this one has 1",
            "E1014 15:7 `ask` has more than one clause in this handler",
            "E1012 16:7 a handler can handle only one effect",
            "E1001 17:7 cannot find effect operation `nope`",
            "E1009 18:7 `IO` cannot be handled",
            "E1014 20:5 this handler has more than one `return` clause",
            "E1010 23:24 the clause for `fail` takes 1 parameter, but this one has 2",
            "E1013 26:8 this handler has no operation clauses",
        ]
    );
}

#[test]
fn resume_and_drop_take_a_fixed_number_of_arguments() {
    let text = "f : Int -> Unit\nf k =\n  resume k\n  drop k 2\n  resume k k k";
    assert_eq!(
        errors(text),
        [
            "E1011 3:3 `resume` takes a continuation and a value, but 1 argument was given",
            "E1011 4:3 `drop` takes one value, but 2 arguments were given",
            "E0004 5:3 `resume` with a handler state is not supported yet",
        ]
    );
}

#[test]
fn handlers_with_an_initial_state_come_in_stage_6() {
    assert_eq!(
        errors("f : Unit -> Int\nf () = handle g () from 1 with | return x st -> x"),
        ["E0004 2:20 handlers with `from` are not supported yet"]
    );
}

#[test]
fn handler_parts_capture_what_they_use() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nf : Int -> Int -> Int\nf a b =\n  handle ask () + a with\n    | ask () k -> resume k b\n    | return x -> x + a";
    let lowered = lower(text);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    let function = lowered
        .module
        .functions
        .iter()
        .find(|(_, function)| function.name == "f")
        .map(|(_, function)| function)
        .unwrap();
    let body = function.body.as_ref().unwrap();
    let names = |locals: Vec<eml_hir::LocalId>| -> Vec<String> {
        locals
            .into_iter()
            .map(|local| body.locals[local].name.clone())
            .collect()
    };
    // 等式の本体はブロックで、handle はその最後の式である
    let (handle, kind) = body
        .exprs
        .iter()
        .find(|(_, expr)| matches!(expr.kind, eml_hir::ExprKind::Handle { .. }))
        .map(|(id, expr)| (id, &expr.kind))
        .unwrap();
    let eml_hir::ExprKind::Handle {
        body: handled,
        clauses,
        ret,
        ..
    } = kind
    else {
        unreachable!();
    };
    assert_eq!(names(body.captures(handle, &[])), ["a", "b"]);
    assert_eq!(names(body.captures(*handled, &[])), ["a"]);
    let clause = &clauses[0];
    let bound: Vec<_> = clause.patterns().collect();
    assert_eq!(names(body.captures(clause.body, &bound)), ["b"]);
    let ret = ret.as_ref().unwrap();
    assert_eq!(names(body.captures(ret.body, &[ret.param])), ["a"]);
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_hir --test effects`
Expected: コンパイルエラー (`no variant named Handle`)

- [ ] **Step 3: HIR の式と走査関数を足す**

`crates/eml_hir/src/hir.rs` の `ExprKind` の最後に足す。

```rust
    /// `effect` は節から決めたエフェクトで、決められなかったら `None` である。HIR が診断を報告済みなので、型検査は
    /// 連鎖する診断を出さない。誤った節 (引数の個数の誤り、重複、別のエフェクトの節) は `clauses` に入れない。
    Handle {
        body: ExprId,
        effect: Option<EffectId>,
        clauses: Vec<OpClause>,
        ret: Option<ReturnClause>,
    },
    Resume {
        k: ExprId,
        arg: ExprId,
    },
    Drop(ExprId),
```

`ExprKind` の後に節の型を足す。

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpClause {
    pub op: OperationId,
    pub params: Vec<PatId>,
    /// `never` の操作の節は `k` を持たない (docs/spec/expressions.md の「handler」)。
    pub k: Option<PatId>,
    pub body: ExprId,
    pub range: TextRange,
}

impl OpClause {
    /// 節が束縛するパターン。操作の引数、`k` の順である。
    pub fn patterns(&self) -> impl Iterator<Item = PatId> + '_ {
        self.params.iter().copied().chain(self.k)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnClause {
    pub param: PatId,
    pub body: ExprId,
    pub range: TextRange,
}
```

`walk_child_exprs` の `match` に腕を足す。

```rust
            ExprKind::Handle {
                body, clauses, ret, ..
            } => {
                f(*body);
                for clause in clauses {
                    f(clause.body);
                }
                if let Some(ret) = ret {
                    f(ret.body);
                }
            }
            ExprKind::Resume { k, arg } => {
                f(*k);
                f(*arg);
            }
            ExprKind::Drop(value) => f(*value),
```

`lambda_captures` を次の2つの関数に置き換える。

```rust
    /// ラムダが捕まえる変数。
    pub fn lambda_captures(&self, lambda: ExprId) -> Vec<LocalId> {
        self.captures(lambda, &[])
    }

    /// `root` の中で参照する局所変数のうち、`root` の中でも `bound` でも束縛していないもの。`LocalId` の順に並べる。
    /// ラムダと handle の本体と節は、捕まえた変数を先頭の引数に持つ関数に持ち上げるので (docs/spec/core-ir.md)、
    /// 入れ子のラムダや節が捕まえる変数は外側も捕まえる。式の木は作業リストでたどる。
    pub fn captures(&self, root: ExprId, bound: &[PatId]) -> Vec<LocalId> {
        let mut used = BTreeSet::new();
        let mut bound: HashSet<LocalId> = bound
            .iter()
            .flat_map(|&pat| self.pat_bindings(pat))
            .collect();
        let mut work = vec![root];
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
                ExprKind::Handle { clauses, ret, .. } => {
                    for clause in clauses {
                        for pat in clause.patterns() {
                            bound.extend(self.pat_bindings(pat));
                        }
                    }
                    if let Some(ret) = ret {
                        bound.extend(self.pat_bindings(ret.param));
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
```

`walk_child_exprs` の doc コメントの「段階3と4で `match` や `handle` を足すときはここを直す」を「段階4で `match` を足すときはここを直す」にする。

`crates/eml_hir/src/lib.rs` の `codes` に足す。

```rust
    pub const UNHANDLEABLE_EFFECT: ErrorCode = ErrorCode(1009);
    pub const CLAUSE_ARITY: ErrorCode = ErrorCode(1010);
    pub const KEYWORD_ARITY: ErrorCode = ErrorCode(1011);
    pub const MIXED_EFFECTS_IN_HANDLER: ErrorCode = ErrorCode(1012);
    pub const MISSING_CLAUSE: ErrorCode = ErrorCode(1013);
    pub const DUPLICATE_CLAUSE: ErrorCode = ErrorCode(1014);
```

`crates/eml_hir/src/builtin.rs` の `impl Builtin` に足す。

```rust
    /// 組み込みの `IO` の操作。handler の節に書けないことを報告するのに使う (docs/spec/effects.md)。段階5で
    /// `open` などを足す。
    pub fn is_io_operation(self) -> bool {
        matches!(self, Builtin::Println)
    }
```

- [ ] **Step 4: `BodyLowering` にエフェクトと操作の表を渡す**

`crates/eml_hir/src/lower/expr.rs` の `BodyLowering` にフィールドを足し、`new` の引数にする。

```rust
pub(super) struct BodyLowering<'a> {
    pub(super) file: FileId,
    pub(super) items: &'a ItemScope,
    /// handler の節の検査で、操作の引数の個数とエフェクトの操作の並びを引く。
    pub(super) effects: &'a Arena<EffectDef>,
    pub(super) operations: &'a Arena<Operation>,
    // ...残りは今のまま。`exprs` に加えて `pats`、`scope` も `pub(super)` にする
}

impl<'a> BodyLowering<'a> {
    pub(super) fn new(
        file: FileId,
        items: &'a ItemScope,
        effects: &'a Arena<EffectDef>,
        operations: &'a Arena<Operation>,
        generics: &'a mut Generics,
        diagnostics: &'a mut Vec<Diagnostic>,
    ) -> Self {
```

`lower_pat` を `pub(super) fn lower_pat` にする。`crates/eml_hir/src/lower/mod.rs` の呼び出しを `BodyLowering::new(file, &scope, &effects, &operations, generics, &mut diagnostics)` にする。

`lower_expr` の handler、`resume`、`drop` の腕を次に置き換える。

```rust
            ast::Expr::HandleExpr(e) => self.lower_handle(&e, range),
            ast::Expr::ResumeExpr(e) => self.lower_resume(&e, range),
            ast::Expr::DropExpr(e) => self.lower_drop(&e, range),
```

`crates/eml_hir/src/lower/mod.rs` の `mod` の並びに `mod handler;` を足す。

- [ ] **Step 5: handler の変換と検査を書く**

`crates/eml_hir/src/lower/handler.rs` を作る。

```rust
//! handler、`resume`、`drop` の変換と検査 (docs/spec/expressions.md の「handler」)。1つの handler は1つのエフェクトを
//! 扱い、そのすべての操作に節を書く (docs/spec/effects.md の「handler の意味」)。

use eml_diagnostics::{Diagnostic, Label, TextRange};
use eml_syntax::{SyntaxToken, ast};

use super::expr::BodyLowering;
use crate::builtin::Builtin;
use crate::codes;
use crate::hir::*;

/// 節を読みながら集める情報。
#[derive(Default)]
struct Clauses {
    /// 最初に解決できた節の操作のエフェクトと、その節の操作名の範囲。
    effect: Option<(EffectId, TextRange)>,
    /// 節を書いた操作と、その節の操作名の範囲。引数の個数を誤った節も含める。節のない操作として二重に報告しないため。
    seen: Vec<(OperationId, TextRange)>,
    /// 操作の節を書いたか。解決できなかった節も含める。
    any_operation: bool,
    clauses: Vec<OpClause>,
    ret: Option<ReturnClause>,
}

impl BodyLowering<'_> {
    pub(super) fn lower_handle(&mut self, handle: &ast::HandleExpr, range: TextRange) -> ExprId {
        if let Some(from) = handle.from_keyword() {
            return self.unsupported(
                from.text_range(),
                "handlers with `from` are not supported yet",
            );
        }
        let body = self.lower_expr(handle.body(), range);
        let mut lowered = Clauses::default();
        for clause in handle.clauses() {
            match clause {
                ast::Clause::OpClause(clause) => self.op_clause(&clause, &mut lowered),
                ast::Clause::ReturnClause(clause) => self.return_clause(&clause, &mut lowered),
            }
        }
        self.missing_clauses(handle.keyword_range(), &lowered);
        let Clauses {
            effect,
            clauses,
            ret,
            ..
        } = lowered;
        self.alloc(
            ExprKind::Handle {
                body,
                effect: effect.map(|(effect, _)| effect),
                clauses,
                ret,
            },
            range,
        )
    }

    fn op_clause(&mut self, clause: &ast::OpClause, out: &mut Clauses) {
        out.any_operation = true;
        // 引数のスコープは節の本体だけである
        let mark = self.scope.len();
        let params: Vec<PatId> = clause
            .params()
            .map(|pat| self.lower_pat(Some(pat), clause.range()))
            .collect();
        let body = self.lower_expr(clause.body(), clause.range());
        self.scope.truncate(mark);
        // 名前がなければパーサが報告済み
        let Some(name) = clause.name() else {
            return;
        };
        let name_range = name.text_range();
        let Some(op) = self.items.operation(name.text()) else {
            self.unknown_operation(&name);
            return;
        };
        let operation = &self.operations[op];
        let effect = operation.effect;
        let never = operation.multiplicity == OpMultiplicity::Never;
        let arity = operation.arity;
        let text = name.text();
        if let Some(&(_, first)) = out.seen.iter().find(|(seen, _)| *seen == op) {
            self.diagnostics.push(
                Diagnostic::error(
                    codes::DUPLICATE_CLAUSE,
                    format!("`{text}` has more than one clause in this handler"),
                    Label::new(self.file, name_range, format!("another clause for `{text}`")),
                )
                .with_secondary(Label::new(self.file, first, "the first clause")),
            );
            return;
        }
        match out.effect {
            None => out.effect = Some((effect, name_range)),
            Some((handled, first)) if handled != effect => {
                let handled = &self.effects[handled].name;
                let other = &self.effects[effect].name;
                self.diagnostics.push(
                    Diagnostic::error(
                        codes::MIXED_EFFECTS_IN_HANDLER,
                        "a handler can handle only one effect",
                        Label::new(
                            self.file,
                            name_range,
                            format!("`{text}` is an operation of `{other}`"),
                        ),
                    )
                    .with_secondary(Label::new(
                        self.file,
                        first,
                        format!("this handler handles `{handled}` because of this clause"),
                    ))
                    .with_help("handle each effect with its own `handle`"),
                );
                return;
            }
            Some(_) => {}
        }
        out.seen.push((op, name_range));
        let expected = arity + usize::from(!never);
        if params.len() != expected {
            let note = if never {
                format!(
                    "`{text}` is a `never` operation, so its clause takes only the arguments of the operation"
                )
            } else {
                format!("the clause takes the arguments of `{text}` and then the continuation `k`")
            };
            self.diagnostics.push(
                Diagnostic::error(
                    codes::CLAUSE_ARITY,
                    format!(
                        "the clause for `{text}` takes {}, but this one has {}",
                        parameters(expected),
                        params.len()
                    ),
                    Label::new(self.file, name_range, "this clause"),
                )
                .with_note(note),
            );
            return;
        }
        let mut params = params;
        let k = if never { None } else { params.pop() };
        out.clauses.push(OpClause {
            op,
            params,
            k,
            body,
            range: clause.range(),
        });
    }

    fn return_clause(&mut self, clause: &ast::ReturnClause, out: &mut Clauses) {
        let mark = self.scope.len();
        let params: Vec<PatId> = clause
            .params()
            .map(|pat| self.lower_pat(Some(pat), clause.range()))
            .collect();
        let body = self.lower_expr(clause.body(), clause.range());
        self.scope.truncate(mark);
        let range = clause.range();
        if let Some(first) = &out.ret {
            self.diagnostics.push(
                Diagnostic::error(
                    codes::DUPLICATE_CLAUSE,
                    "this handler has more than one `return` clause",
                    Label::new(self.file, range, "another `return` clause"),
                )
                .with_secondary(Label::new(self.file, first.range, "the first `return` clause")),
            );
            return;
        }
        let [param] = params.as_slice() else {
            let mut diagnostic = Diagnostic::error(
                codes::CLAUSE_ARITY,
                format!(
                    "the `return` clause takes 1 parameter, but this one has {}",
                    params.len()
                ),
                Label::new(self.file, range, "this clause"),
            );
            if params.len() == 2 {
                diagnostic = diagnostic
                    .with_note("a second parameter receives the state, which needs `handle ... from ...`");
            }
            self.diagnostics.push(diagnostic);
            return;
        };
        out.ret = Some(ReturnClause {
            param: *param,
            body,
            range,
        });
    }

    /// 節の先頭の名前が操作でない。`IO` の操作なら、handle できないことを伝える (docs/spec/effects.md の「組み込みの `IO`」)。
    fn unknown_operation(&mut self, name: &SyntaxToken) {
        let text = name.text();
        let range = name.text_range();
        let diagnostic = if Builtin::from_name(text).is_some_and(Builtin::is_io_operation) {
            Diagnostic::error(
                codes::UNHANDLEABLE_EFFECT,
                "`IO` cannot be handled",
                Label::new(
                    self.file,
                    range,
                    format!("`{text}` is an operation of the built-in `IO`"),
                ),
            )
            .with_note("the runtime handles `IO` itself")
        } else {
            Diagnostic::error(
                codes::UNDEFINED_NAME,
                format!("cannot find effect operation `{text}`"),
                Label::new(self.file, range, "not an operation of any effect"),
            )
        };
        self.diagnostics.push(diagnostic);
    }

    /// 扱うエフェクトの操作のうち、節のないものを報告する。操作の節が1つもない handler も報告する。解決できなかった
    /// 節があってエフェクトが決まらないときは、報告済みなので何も言わない。
    fn missing_clauses(&mut self, keyword: TextRange, clauses: &Clauses) {
        let (effects, operations) = (self.effects, self.operations);
        match clauses.effect {
            Some((effect, _)) => {
                let missing: Vec<&Operation> = effects[effect]
                    .operations
                    .iter()
                    .filter(|&&op| !clauses.seen.iter().any(|(seen, _)| *seen == op))
                    .map(|&op| &operations[op])
                    .collect();
                if missing.is_empty() {
                    return;
                }
                let names: Vec<String> = missing
                    .iter()
                    .map(|operation| format!("`{}`", operation.name))
                    .collect();
                let examples: Vec<String> = missing
                    .iter()
                    .map(|operation| format!("`{}`", clause_example(operation)))
                    .collect();
                let diagnostic = Diagnostic::error(
                    codes::MISSING_CLAUSE,
                    format!(
                        "this handler has no clause for {} of `{}`",
                        names.join(", "),
                        effects[effect].name
                    ),
                    Label::new(self.file, keyword, "this handler"),
                )
                .with_help(format!("add {}", examples.join(" and ")));
                self.diagnostics.push(diagnostic);
            }
            None if !clauses.any_operation => self.diagnostics.push(
                Diagnostic::error(
                    codes::MISSING_CLAUSE,
                    "this handler has no operation clauses",
                    Label::new(self.file, keyword, "this handler"),
                )
                .with_note("a handler handles all the operations of one effect"),
            ),
            None => {}
        }
    }

    pub(super) fn lower_resume(&mut self, resume: &ast::ResumeExpr, range: TextRange) -> ExprId {
        let args = self.keyword_args(resume.args());
        match args.as_slice() {
            [k, arg] => self.alloc(ExprKind::Resume { k: *k, arg: *arg }, range),
            // パラメータ付き handler の3引数の形 (docs/spec/expressions.md の「パラメータ付き handler」)
            [_, _, _] => self.unsupported(
                resume.keyword_range(),
                "`resume` with a handler state is not supported yet",
            ),
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    codes::KEYWORD_ARITY,
                    format!(
                        "`resume` takes a continuation and a value, but {} given",
                        arguments(args.len())
                    ),
                    Label::new(self.file, resume.keyword_range(), "this `resume`"),
                ));
                self.alloc(ExprKind::Missing, range)
            }
        }
    }

    pub(super) fn lower_drop(&mut self, drop: &ast::DropExpr, range: TextRange) -> ExprId {
        let args = self.keyword_args(drop.args());
        match args.as_slice() {
            [value] => self.alloc(ExprKind::Drop(*value), range),
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    codes::KEYWORD_ARITY,
                    format!("`drop` takes one value, but {} given", arguments(args.len())),
                    Label::new(self.file, drop.keyword_range(), "this `drop`"),
                ));
                self.alloc(ExprKind::Missing, range)
            }
        }
    }

    /// 個数の誤りを報告する前に引数を変換する。引数の中の名前の誤りも報告するため。
    fn keyword_args(&mut self, args: impl Iterator<Item = ast::Expr>) -> Vec<ExprId> {
        args.map(|arg| {
            let range = arg.range();
            self.lower_expr(Some(arg), range)
        })
        .collect()
    }
}

/// E1013 の help に出す節の書き方。
fn clause_example(operation: &Operation) -> String {
    let mut example = format!("| {}", operation.name);
    for _ in 0..operation.arity {
        example.push_str(" _");
    }
    if operation.multiplicity != OpMultiplicity::Never {
        example.push_str(" k");
    }
    example + " -> ..."
}

fn parameters(n: usize) -> String {
    if n == 1 {
        "1 parameter".to_string()
    } else {
        format!("{n} parameters")
    }
}

fn arguments(n: usize) -> String {
    if n == 1 {
        "1 argument was".to_string()
    } else {
        format!("{n} arguments were")
    }
}
```

`expr.rs` の `scope` と `alloc`、`unsupported` は、このファイルから使えるように `pub(super)` にしておく (`alloc` と `unsupported` はすでに `pub(super)`)。

- [ ] **Step 6: handler を表示する**

`crates/eml_hir/src/pretty.rs` の `expr` の `match` に腕を足す。

```rust
            ExprKind::Handle {
                body: handled,
                clauses,
                ret,
                ..
            } => {
                let mut s = format!("(handle {} with", self.expr(body, *handled, indent));
                for clause in clauses {
                    write!(s, " | {}", self.module.operations[clause.op].name).unwrap();
                    for pat in clause.patterns() {
                        write!(s, " {}", self.pat(body, pat)).unwrap();
                    }
                    write!(s, " -> {}", self.expr(body, clause.body, indent)).unwrap();
                }
                if let Some(ret) = ret {
                    write!(
                        s,
                        " | return {} -> {}",
                        self.pat(body, ret.param),
                        self.expr(body, ret.body, indent)
                    )
                    .unwrap();
                }
                s + ")"
            }
            ExprKind::Resume { k, arg } => format!(
                "(resume {} {})",
                self.expr(body, *k, indent),
                self.expr(body, *arg, indent)
            ),
            ExprKind::Drop(value) => format!("(drop {})", self.expr(body, *value, indent)),
```

- [ ] **Step 7: 下流の crate をビルドできる状態に保つ**

`crates/eml_types/src/check/body.rs` の `infer_expr` の `match &expr.kind` に仮の腕を足す。Task 5 で handle、`resume`、`drop` の検査に置き換える。

```rust
            ExprKind::Handle { .. } | ExprKind::Resume { .. } | ExprKind::Drop(_) => {
                self.table.error
            }
```

`crates/eml_types/src/usage.rs` の `expr` の `match` に仮の腕を足す。Task 6 で使用回数の数え方に置き換える。

```rust
            ExprKind::Handle { .. } | ExprKind::Resume { .. } | ExprKind::Drop(_) => Uses::new(),
```

`crates/eml_core_ir/src/lower.rs` の `atom` の `match` に仮の腕を足す。Task 8 で変換に置き換える。

```rust
            ExprKind::Handle { .. } | ExprKind::Resume { .. } | ExprKind::Drop(_) => {
                unreachable!("handlers are not lowered to Core IR yet")
            }
```

- [ ] **Step 8: テストが通ることを確かめる**

Run: `cargo test -p eml_hir`
Expected: PASS。`structure.rs` の `a_lambda_captures_what_its_nested_lambdas_capture` も、`lambda_captures` を `captures` で書き直した後で変わらずに通る

- [ ] **Step 9: 全体を確かめてコミットする**

```bash
cargo test && cargo clippy --all-targets && cargo fmt
git add crates
git commit -m "Lower handlers, resume, and drop to HIR and check their clauses"
```

---

### Task 4: 継続の型と操作のスキームを型検査に入れる

型の表に継続の型を足し、エフェクトの多重度を操作から決める。操作のスキームを作り、操作の参照と呼び出しを型検査する。後の段階のために、`TypedModule` に操作のスキームを、`BodyTypes` にパターンの型を足す。

**Files:**
- Modify: `crates/eml_types/src/ty.rs`
- Modify: `crates/eml_types/src/table/mod.rs`、`unify.rs`、`kinds.rs`、`copy.rs`、`export.rs`、`tests.rs`
- Modify: `crates/eml_types/src/scheme.rs`
- Modify: `crates/eml_types/src/check/mod.rs`、`body.rs`、`report.rs`
- Modify: `crates/eml_types/src/lib.rs`
- Modify: `crates/eml_core_ir/src/lower.rs` (`boxed` に継続の型を足す)
- Test: `crates/eml_types/tests/effects.rs` (新規)、`crates/eml_types/src/table/tests.rs`、`crates/eml_types/src/ty.rs`

**Interfaces:**
- Consumes: Task 2 の `Operation`、`OpMultiplicity`、`Res::Operation`
- Produces:
  - `TyShape::Cont { arg: Ty, lin: ArrowLin, row: Row, ret: Ty }` (操作の結果の型、継続の線形性、handle の外側の row、handle の結果の型)
  - `Type::Cont { arg: Box<Type>, ret: Box<Type>, effects: Vec<EffectLabel>, tail: Option<RowTail>, linearity: Linearity }`。表示は `Cont Int Unit <IO>`
  - `Table::new(lang, types, effects, operations)`
  - `scheme::lower_operation(table, operation, rigids) -> Ty`
  - `TypedModule::operations: ArenaMap<OperationId, Scheme>`、`BodyTypes::pats: ArenaMap<PatId, Type>`
  - `dump` は操作のスキームを関数より先に `名前 : 型` で出す

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/effects.rs` を作る。

```rust
mod common;

use common::check_text;

#[test]
fn operations_have_the_row_of_their_effect() {
    let text = "effect Log where\n  log : String -> String -> Unit\n  never fail : String -> a\n\nwarn : String -> <Log> Unit\nwarn m = log \"warn\" m\n\nstop : Unit -> <Log> Int\nstop () = fail \"no\"";
    insta::assert_snapshot!(check_text(text), @r"
    log : String -> String -> <Log> Unit
    fail : String -> <Log> a
    warn : String -> <Log> Unit
      m#0 : String
    stop : Unit -> <Log> Int
    ");
}

#[test]
fn an_unhandled_operation_is_not_in_the_row() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (ask ()))";
    insta::assert_snapshot!(check_text(text), @r"
    ask : Unit -> <Ask> Int
    main : Unit -> <IO> Unit
    ---
    E2002 5:30 `ask` performs `Ask`, which the signature of `main` does not allow
      5:30 this call performs `Ask`
      4:8 the row of this signature does not include it
      help: add `Ask` to the row of the signature of `main`, as in `-> <Ask> ...`
    ");
}

#[test]
fn operations_are_values_and_can_be_partially_applied() {
    let text = "effect Log where\n  log : String -> String -> Unit\n\neach : (String -> <e> Unit) -> <e> Unit\neach f = f \"a\"\n\nrun : Unit -> <Log> Unit\nrun () =\n  let info = log \"info\"\n  each info\n  each (log \"warn\")";
    insta::assert_snapshot!(check_text(text), @r"
    log : String -> String -> <Log> Unit
    each : (String -> <e> Unit) -> <e> Unit
      f#0 : String -> <e> Unit
    run : Unit -> <Log> Unit
      info#0 : String -> <Log> Unit
    ");
}
```

`crates/eml_types/src/table/tests.rs` に足す。

```rust
#[test]
fn continuations_unify_by_their_parts_and_are_linear() {
    let mut table = new_table();
    let (int, string) = (table.int, table.string);
    let lin = ArrowLin::Known(Linearity::Lin);
    let k = table.alloc(TyShape::Cont {
        arg: int,
        lin,
        row: Row::pure(),
        ret: string,
    });
    let v = table.fresh_var();
    let same = table.alloc(TyShape::Cont {
        arg: v,
        lin,
        row: Row::pure(),
        ret: string,
    });
    assert_eq!(table.unify(k, same), Ok(()));
    assert_eq!(table.unify(v, int), Ok(()));
    let other = table.alloc(TyShape::Cont {
        arg: string,
        lin,
        row: Row::pure(),
        ret: string,
    });
    assert_eq!(table.unify(k, other), Err(UnifyError::Mismatch));
    assert_eq!(table.unify(k, int), Err(UnifyError::Mismatch));
    assert_eq!(table.kind_bounds(k), vec![Bound::Const(Linearity::Lin)]);
}
```

`crates/eml_types/src/ty.rs` の `tests` に足す。

```rust
    #[test]
    fn continuations_are_displayed_with_their_row() {
        let mut types = Arena::new();
        let mut effects = Arena::new();
        let mut con = |name: &str| Type::Con {
            id: types.alloc(TypeDef {
                name: name.to_string(),
            }),
            name: name.to_string(),
        };
        let int = con("Int");
        let unit = Type::unit();
        let io = EffectLabel {
            id: effects.alloc(EffectDef {
                name: "IO".to_string(),
                operations: Vec::new(),
            }),
            name: "IO".to_string(),
        };
        let k = Type::Cont {
            arg: Box::new(int.clone()),
            ret: Box::new(unit.clone()),
            effects: vec![io],
            tail: None,
            linearity: Linearity::Lin,
        };
        assert_eq!(k.to_string(), "Cont Int Unit <IO>");
        let pure = Type::Cont {
            arg: Box::new(Type::Fn {
                param: Box::new(int.clone()),
                linearity: Linearity::Unr,
                effects: vec![],
                tail: None,
                ret: Box::new(int),
            }),
            ret: Box::new(unit),
            effects: vec![],
            tail: Some(RowTail::Rigid("e".to_string())),
            linearity: Linearity::Lin,
        };
        assert_eq!(pure.to_string(), "Cont (Int -> Int) Unit <e>");
    }
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types`
Expected: コンパイルエラー (`no variant named Cont`)

- [ ] **Step 3: 外に出す型に継続の型を足す**

`crates/eml_types/src/ty.rs` の `Type` に足す。

```rust
    /// 継続 `k` の型 (docs/spec/effects.md)。表面の構文に名前を持たず、診断の表示だけに使う。
    Cont {
        /// 操作の結果の型。`resume` に渡す値の型である。
        arg: Box<Type>,
        /// handle の結果の型。`resume` の値の型である。
        ret: Box<Type>,
        /// handle の外側の row。`resume` が起こすエフェクトである。
        effects: Vec<EffectLabel>,
        tail: Option<RowTail>,
        linearity: Linearity,
    },
```

`contains_error` に腕を足す。

```rust
            Type::Cont { arg, ret, tail, .. } => {
                arg.contains_error()
                    || ret.contains_error()
                    || matches!(tail, Some(RowTail::Error))
            }
```

`Display` の `Type::Fn` の腕の row の書き方を `row_text` に移し、`Type::Cont` の腕を足す。

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
                // 空の閉じた row は書かない。省略した row が `<>` だから (docs/spec/types.md)
                let row = row_text(effects, tail);
                if !row.is_empty() {
                    write!(f, "<{row}> ")?;
                }
                write!(f, "{ret}")
            }
            // 継続の row は、空でも書く。何も起こさない `resume` であることを示すため
            Type::Cont {
                arg,
                ret,
                effects,
                tail,
                ..
            } => write!(
                f,
                "Cont {} {} <{}>",
                atomic(arg),
                atomic(ret),
                row_text(effects, tail)
            ),
```

`impl fmt::Display for Type` の後に補助関数を足す。

```rust
/// row の中身。`<` と `>` は呼び出し側が付ける。
fn row_text(effects: &[EffectLabel], tail: &Option<RowTail>) -> String {
    let names: Vec<&str> = effects.iter().map(|e| e.name.as_str()).collect();
    let tail = match tail {
        Some(RowTail::Rigid(name)) => Some(name.as_str()),
        Some(RowTail::Flexible) => Some("_"),
        Some(RowTail::Error) => Some("{error}"),
        None => None,
    };
    match tail {
        Some(tail) if names.is_empty() => tail.to_string(),
        Some(tail) => format!("{} | {tail}", names.join(", ")),
        None => names.join(", "),
    }
}

/// 型の適用の引数の位置に置く形。関数型と継続の型は括弧で囲む。
fn atomic(ty: &Type) -> String {
    match ty {
        Type::Fn { .. } | Type::Cont { .. } => format!("({ty})"),
        _ => ty.to_string(),
    }
}
```

- [ ] **Step 4: 型の表に継続の型とエフェクトの多重度を足す**

`crates/eml_types/src/table/mod.rs` の `TyShape` に足す。

```rust
    /// 継続 `k` の型。`lin` は継続の線形性で、`once` の操作の `k` は `Lin` である (docs/spec/effects.md)。`resume` の
    /// 検査では、まだ決まらない継続を推論用の変数で表すので、矢印と同じ `ArrowLin` を使う。
    Cont {
        arg: Ty,
        lin: ArrowLin,
        row: Row,
        ret: Ty,
    },
```

`Table` に `effect_multiplicities: ArenaMap<EffectId, Multiplicity>` を足し、`new` が操作のアリーナを受け取って作る。`effect_multiplicity` を置き換える。

```rust
    pub fn new(
        lang: LangItems,
        types: &Arena<TypeDef>,
        effects: &Arena<EffectDef>,
        operations: &Arena<Operation>,
    ) -> Table {
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
                        .map(|&op| match operations[op].multiplicity {
                            OpMultiplicity::Never => Multiplicity::Never,
                            OpMultiplicity::Once => Multiplicity::Once,
                        })
                        .max()
                        .unwrap_or(Multiplicity::Never)
                };
                (id, multiplicity)
            })
            .collect();
        let mut table = Table {
            // ...今のフィールド...
            effect_multiplicities,
        };
        // ...今の続き...
    }

    /// エフェクトがその row に入れる操作の上限。操作の多重度の最大である (docs/spec/types.md の「Kind」)。
    pub fn effect_multiplicity(&self, effect: EffectId) -> Multiplicity {
        self.effect_multiplicities[effect]
    }
```

`use eml_hir::{...}` に `OpMultiplicity` と `Operation` を足す。`table/tests.rs` の `new_table` は `Table::new(lang, &types, &effects, &Arena::new())` にする (種類3)。

`crates/eml_types/src/table/unify.rs` の `unify` の `match` で、`TyShape::Fn` の腕の後に足す。

```rust
            (
                TyShape::Cont {
                    arg: a1,
                    lin: l1,
                    row: r1,
                    ret: t1,
                },
                TyShape::Cont {
                    arg: a2,
                    lin: l2,
                    row: r2,
                    ret: t2,
                },
            ) => {
                self.unify(a1, a2)?;
                self.unify_arrow_lin(l1, l2)?;
                self.unify_row(&r1, &r2)?;
                self.unify(t1, t2)
            }
```

`occurs` の腕を `TyShape::Fn { param, ret, .. } | TyShape::Cont { arg: param, ret, .. } => self.occurs(var, *param) || self.occurs(var, *ret),` にする。

`crates/eml_types/src/table/kinds.rs` の `kind_bounds` の `TyShape::Fn { lin, .. }` の腕を `TyShape::Fn { lin, .. } | TyShape::Cont { lin, .. }` にする。`kind_vars` に腕を足す。

```rust
                TyShape::Cont {
                    arg,
                    lin: m,
                    row,
                    ret,
                } => {
                    if let ArrowLin::Var(v) = m {
                        push_unique(&mut lin, *v);
                    }
                    if let Tail::Var(tail) = self.resolve_row(row).tail
                        && self.is_rigid_row(tail)
                    {
                        push_unique(&mut mult, self.row_multiplicity_var(tail));
                    }
                    work.push(*ret);
                    work.push(*arg);
                }
```

`crates/eml_types/src/table/copy.rs` の `copy_type` に腕を足す。

```rust
            TyShape::Cont { arg, lin, row, ret } => {
                let arg = self.copy_type(arg, subst);
                let ret = self.copy_type(ret, subst);
                let lin = match lin {
                    ArrowLin::Var(v) => ArrowLin::Var(subst.lin.get(&v).copied().unwrap_or(v)),
                    known => known,
                };
                let row = self.resolve_row(&row);
                let tail = match row.tail {
                    Tail::Var(tail) => Tail::Var(subst.rows.get(&tail).copied().unwrap_or(tail)),
                    other => other,
                };
                self.alloc(TyShape::Cont {
                    arg,
                    lin,
                    row: Row {
                        labels: row.labels,
                        tail,
                    },
                    ret,
                })
            }
```

`crates/eml_types/src/table/export.rs` の `to_type` で、矢印の線形性と row の変換を補助関数に移し、`TyShape::Cont` の腕を足す。

```rust
            TyShape::Fn {
                param,
                lin,
                row,
                ret,
            } => {
                let (effects, tail) = self.export_row(&row);
                Type::Fn {
                    param: Box::new(self.to_type(param, solved)),
                    linearity: self.export_lin(lin, solved),
                    effects,
                    tail,
                    ret: Box::new(self.to_type(ret, solved)),
                }
            }
            TyShape::Cont { arg, lin, row, ret } => {
                let (effects, tail) = self.export_row(&row);
                Type::Cont {
                    arg: Box::new(self.to_type(arg, solved)),
                    ret: Box::new(self.to_type(ret, solved)),
                    effects,
                    tail,
                    linearity: self.export_lin(lin, solved),
                }
            }
```

```rust
    fn export_lin(&self, lin: ArrowLin, solved: bool) -> Linearity {
        match lin {
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
        }
    }

    fn export_row(&self, row: &Row) -> (Vec<EffectLabel>, Option<RowTail>) {
        let row = self.resolve_row(row);
        let effects = row
            .labels
            .into_iter()
            .map(|id| EffectLabel {
                id,
                name: self.effect_names[id].clone(),
            })
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

`kind_names` の `match` に `TyShape::Cont { arg, ret, .. } => { work.push(*ret); work.push(*arg); }` を足す。

`crates/eml_core_ir/src/lower.rs` の `boxed` の `Type::Fn { .. } | Type::Rigid(_) | Type::Flexible => true` に `Type::Cont { .. }` を足す。継続はヒープのオブジェクトだからである。

- [ ] **Step 5: 操作のスキームを作る**

`crates/eml_types/src/scheme.rs` に足す。

```rust
/// 操作のスキームの型。シグネチャの、引数の個数の分だけたどった最後の矢印に、操作のエフェクトだけの row を付ける
/// (docs/spec/effects.md)。操作のシグネチャの外側の矢印には row を書けないので (E1007)、ほかの外側の矢印の row は
/// 空である。
pub(crate) fn lower_operation(table: &mut Table, operation: &Operation, rigids: &Rigids) -> Ty {
    let ty = lower_signature(table, &operation.signature, rigids);
    with_effect(table, ty, operation.arity, operation.effect)
}

fn with_effect(table: &mut Table, ty: Ty, arity: usize, effect: EffectId) -> Ty {
    if arity == 0 {
        return ty;
    }
    let TyShape::Fn {
        param,
        lin,
        row,
        ret,
    } = table.shape(ty).clone()
    else {
        return ty;
    };
    let (row, ret) = if arity == 1 {
        let row = Row {
            labels: vec![effect],
            tail: row.tail,
        };
        (row, ret)
    } else {
        (row, with_effect(table, ret, arity - 1, effect))
    };
    table.function_with(param, lin, row, ret)
}
```

`use eml_hir::{...}` に `EffectId` と `Operation` を足す。

- [ ] **Step 6: 操作のスキームを検査と結果に通す**

`crates/eml_types/src/lib.rs` の `TypedModule` と `BodyTypes` にフィールドを足す。

```rust
    /// エフェクトの操作のスキーム。Core IR が、操作を包む関数の変数を boxed にするかを決めるのに使う。
    pub operations: ArenaMap<OperationId, Scheme>,
```

```rust
#[derive(Debug, Default)]
pub struct BodyTypes {
    pub exprs: ArenaMap<ExprId, Type>,
    pub locals: ArenaMap<LocalId, Type>,
    /// パターンが受けた値の型。Core IR が、handler の節の引数の変数を作るのに使う。
    pub pats: ArenaMap<PatId, Type>,
}
```

`use eml_hir::{...}` に `OperationId` と `PatId` を足す。`dump` の先頭 (`for (id, function) in ...` の前) に、操作のスキームを出すループを足す。

```rust
    for (id, operation) in module.operations.iter() {
        if let Some(scheme) = typed.operations.get(id) {
            writeln!(out, "{} : {}", operation.name, scheme.ty).unwrap();
        }
    }
```

`crates/eml_types/src/check/mod.rs` を次のように直す。

1. `Table::new(module.lang, &module.types, &module.effects, &module.operations)` にする。
2. 組み込みのスキームを作るループの後に、操作のスキームを作る。

```rust
    // 操作は本体を持たない値なので、組み込みと同じく作ってすぐ多相化する
    let mut operations: ArenaMap<OperationId, Scheme> = ArenaMap::default();
    for (id, operation) in module.operations.iter() {
        let rigids = Rigids::new(&mut table, &operation.signature.generics);
        let ty = lower_operation(&mut table, operation, &rigids);
        table.closure_kinds(ty, operation.arity, &[]);
        let mut scheme = Scheme::new(ty, &rigids);
        scheme.generalize(&table);
        operations.insert(id, scheme);
    }
```

3. `BodyCheck { ... }` に `operations: &operations,` を足す。
4. `typed.builtins` を作るループの後に、操作のスキームを書き出す。

```rust
    for (id, scheme) in operations.iter() {
        typed.operations.insert(
            id,
            crate::Scheme {
                ty: table.export(scheme.ty),
                constraints: kind_constraints(&table, scheme),
            },
        );
    }
```

5. 本体の型を書き出すループで、`locals` の後にパターンの型を書き出す。

```rust
        for (pat, &ty) in typing.pats.iter() {
            types.pats.insert(pat, table.export(ty));
        }
```

`use` に `OperationId` (`eml_hir`) と `lower_operation` (`crate::scheme`) を足す。

`crates/eml_types/src/check/body.rs` の `BodyCheck` にフィールドを足す。

```rust
    /// エフェクトの操作のスキーム。
    pub(super) operations: &'a ArenaMap<OperationId, Scheme>,
```

`value` の仮の腕 (Task 2) を置き換える。

```rust
            Res::Operation(operation) => {
                let ty = match self.operations.get(operation) {
                    Some(scheme) => scheme.instantiate(self.table),
                    None => self.table.error,
                };
                self.table.open_spine(ty)
            }
```

`crates/eml_types/src/check/report.rs` の `callee_subject` に腕を足す。

```rust
        ExprKind::Path(Res::Operation(operation)) => module.operations[*operation].name.as_str(),
```

- [ ] **Step 7: テストが通ることを確かめる**

Run: `cargo test -p eml_types`
Expected: PASS

- [ ] **Step 8: 全体を確かめてコミットする**

```bash
cargo test && cargo clippy --all-targets && cargo fmt
git add crates
git commit -m "Type operations with the row of their effect and add continuation types"
```

---
### Task 5: handle、`resume`、`drop` を型検査する

handle 式の本体を「扱うエフェクト + 今の row」で検査し、節を今の row で検査する。`k` に継続の型を付け、操作の型変数を節の中では rigid にする。`resume` を継続の型の呼び出しとして検査する。

**Files:**
- Create: `crates/eml_types/src/check/handle.rs`
- Modify: `crates/eml_types/src/check/mod.rs` (`mod handle;`)
- Modify: `crates/eml_types/src/check/body.rs`
- Modify: `crates/eml_types/src/check/report.rs`
- Test: `crates/eml_types/tests/effects.rs`

**Interfaces:**
- Consumes: Task 3 の `ExprKind::{Handle, Resume, Drop}`、`OpClause`、`ReturnClause`。Task 4 の `TyShape::Cont`、`lower_operation`
- Produces:
  - `BodyCheck::handle(effect, handled, clauses, ret) -> Ty`、`BodyCheck::resume(id, k, arg) -> Ty`
  - `Origin::{HandlerClause, Continuation, ResumeValue}`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/effects.rs` に足す。

```rust
#[test]
fn a_handler_removes_its_effect_and_types_the_continuation() {
    let text = "effect Ask where\n  ask : String -> Int\n\nrun : Unit -> <Ask, IO> Int\nrun () =\n  let n = ask \"n\"\n  println \"asked\"\n  n + 1\n\nmain : Unit -> <IO> Unit\nmain () =\n  let total =\n    handle run () with\n      | ask key k -> resume k 41\n      | return x -> show_int x\n  println total";
    insta::assert_snapshot!(check_text(text), @r"
    ask : String -> <Ask> Int
    run : Unit -> <Ask, IO> Int
      n#0 : Int
    main : Unit -> <IO> Unit
      key#0 : String
      k#1 : Cont Int String <IO>
      x#2 : Int
      total#3 : String
    ");
}

#[test]
fn type_variables_of_an_operation_are_rigid_in_its_clause() {
    let text = "effect Pick where\n  pick : a -> a -> a\n\nfirst : Unit -> Int\nfirst () =\n  handle pick 1 2 with\n    | pick x y k -> resume k x\n\nwrong : Unit -> Int\nwrong () =\n  handle pick 1 2 with\n    | pick x y k -> resume k 0";
    insta::assert_snapshot!(check_text(text), @r"
    pick : a -> a -> <Pick> a
    first : Unit -> Int
      x#0 : a
      y#1 : a
      k#2 : Cont a Int <>
    wrong : Unit -> Int
      x#0 : a
      y#1 : a
      k#2 : Cont a Int <>
    ---
    E2001 12:30 mismatched types
      12:30 expected `a`, found `Int`
      note: `resume` passes this value as the result of the operation
    ");
}

#[test]
fn resume_needs_a_continuation_and_drop_takes_any_value() {
    let text = "f : Int -> Int\nf n =\n  drop n\n  resume n 1";
    insta::assert_snapshot!(check_text(text), @r"
    f : Int -> Int
      n#0 : Int
    ---
    E2001 4:10 mismatched types
      4:10 expected `Cont _ _ <_>`, found `Int`
      note: the first argument of `resume` must be a continuation
    ");
}

#[test]
fn the_body_of_a_handler_may_perform_only_the_handled_effect_and_the_outer_row() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nnoisy : Unit -> <IO> Int\nnoisy () =\n  println \"x\"\n  1\n\nf : Unit -> Int\nf () =\n  handle noisy () with\n    | ask () k -> resume k 1";
    insta::assert_snapshot!(check_text(text), @r"
    ask : Unit -> <Ask> Int
    noisy : Unit -> <IO> Int
    f : Unit -> Int
      k#0 : Cont Int Int <>
    ---
    E2002 11:10 `noisy` performs `IO`, which the signature of `f` does not allow
      11:10 this call performs `IO`
      9:5 the row of this signature does not include it
      help: add `IO` to the row of the signature of `f`, as in `-> <IO> ...`
    ");
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --test effects`
Expected: FAIL。handle 式の型が `Error` で、局所変数の型と診断が出ない

- [ ] **Step 3: 型の不一致の由来を足す**

`crates/eml_types/src/check/report.rs` の `Origin` に足す。

```rust
    /// handler の節の本体。handle 式全体の型を持つ。
    HandlerClause,
    /// `resume` の最初の引数。
    Continuation,
    /// `resume` に渡す値。
    ResumeValue,
```

`mismatch` の `match origin` に腕を足す。

```rust
            Origin::HandlerClause => diagnostic.with_note(
                "each clause of a handler must have the type of the whole `handle` expression",
            ),
            Origin::Continuation => {
                diagnostic.with_note("the first argument of `resume` must be a continuation")
            }
            Origin::ResumeValue => {
                diagnostic.with_note("`resume` passes this value as the result of the operation")
            }
```

- [ ] **Step 4: handle と `resume` の検査を書く**

`crates/eml_types/src/check/handle.rs` を作る。

```rust
//! handle、`resume` の検査 (docs/spec/effects.md の「handler の意味」)。

use eml_hir::{EffectId, ExprId, OpClause, ReturnClause};

use crate::scheme::{Rigids, lower_operation};
use crate::table::{ArrowLin, Row, Tail, Ty, TyShape};
use crate::ty::Linearity;

use super::body::BodyCheck;
use super::report::Origin;

impl BodyCheck<'_> {
    /// handle 式の型は、`return` の節があればその本体の型、なければ本体の型である。本体は今の row `ρ` の前に扱う
    /// エフェクトを足した row で、節と `return` の節は `ρ` で検査する。deep handler の節は handler の外側で動くため。
    pub(super) fn handle(
        &mut self,
        effect: Option<EffectId>,
        handled: ExprId,
        clauses: &[OpClause],
        ret: Option<&ReturnClause>,
    ) -> Ty {
        let Some(effect) = effect else {
            return self.broken_handle(handled, clauses, ret);
        };
        let outer = self.ambient.clone();
        // scoped labels なので、同じエフェクトの handler を入れ子にすると内側が処理する
        let inner = Row {
            labels: std::iter::once(effect)
                .chain(outer.labels.iter().copied())
                .collect(),
            tail: outer.tail,
        };
        let source = self.ambient_source.clone();
        let handled_ty = self.with_ambient(inner, source, |this| this.infer_expr(handled));
        let result = match ret {
            Some(ret) => {
                self.bind_pat(ret.param, handled_ty);
                self.infer_expr(ret.body)
            }
            None => handled_ty,
        };
        for clause in clauses {
            self.op_clause(clause, result, &outer);
        }
        result
    }

    /// 節の引数は操作の引数の型で、`k` は「操作の結果を受け、handle 式の値を返し、外側の row のエフェクトを起こす」
    /// 継続である。`once` の操作の `k` は `Lin` である (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。
    fn op_clause(&mut self, clause: &OpClause, result: Ty, outer: &Row) {
        let operation = &self.module.operations[clause.op];
        // 操作の型変数は節の中では rigid である。handler は、操作がどの型で呼ばれても動かなければならないため
        let rigids = Rigids::new(self.table, &operation.signature.generics);
        let mut ty = lower_operation(self.table, operation, &rigids);
        for &param in &clause.params {
            match self.table.shape(ty).clone() {
                TyShape::Fn {
                    param: expected,
                    ret,
                    ..
                } => {
                    self.bind_pat(param, expected);
                    ty = ret;
                }
                _ => {
                    let error = self.table.error;
                    self.bind_pat(param, error);
                    ty = error;
                }
            }
        }
        if let Some(k) = clause.k {
            let continuation = self.table.alloc(TyShape::Cont {
                arg: ty,
                lin: ArrowLin::Known(Linearity::Lin),
                row: outer.clone(),
                ret: result,
            });
            self.bind_pat(k, continuation);
        }
        self.check_expr(clause.body, result, Origin::HandlerClause);
    }

    /// 扱うエフェクトが決まらない handler は HIR が報告済みである。本体のエフェクトをすべて受け入れ、型を `Error` に
    /// して、診断を連鎖させない。
    fn broken_handle(
        &mut self,
        handled: ExprId,
        clauses: &[OpClause],
        ret: Option<&ReturnClause>,
    ) -> Ty {
        let source = self.ambient_source.clone();
        self.with_ambient(Row::error(), source, |this| this.infer_expr(handled));
        let error = self.table.error;
        for clause in clauses {
            for pat in clause.patterns() {
                self.bind_pat(pat, error);
            }
            self.check_expr(clause.body, error, Origin::HandlerClause);
        }
        if let Some(ret) = ret {
            self.bind_pat(ret.param, error);
            self.check_expr(ret.body, error, Origin::HandlerClause);
        }
        error
    }

    /// `resume k v` は、`k` の継続の型を関数型 `a -<ρ'>-> b` のように呼ぶ。`k` の型がまだ決まらない場合 (ラムダの
    /// 引数など) にも検査できるよう、推論用の変数でできた継続の型と単一化する。
    pub(super) fn resume(&mut self, id: ExprId, k: ExprId, arg: ExprId) -> Ty {
        let value = self.table.fresh_var();
        let result = self.table.fresh_var();
        let row = Row {
            labels: Vec::new(),
            tail: Tail::Var(self.table.fresh_row_var()),
        };
        let lin = self.table.fresh_arrow_lin();
        let expected = self.table.alloc(TyShape::Cont {
            arg: value,
            lin,
            row: row.clone(),
            ret: result,
        });
        self.check_expr(k, expected, Origin::Continuation);
        self.check_expr(arg, value, Origin::ResumeValue);
        let range = self.body.exprs[id].range;
        self.include_call_row(row, range, "`resume`", true);
        result
    }
}
```

`crates/eml_types/src/check/mod.rs` の `mod body;` の後に `mod handle;` を足す。`crates/eml_types/src/check/body.rs` で、`check_expr`、`infer_expr`、`bind_pat`、`with_ambient` を `pub(super)` にする。

`infer_expr` の仮の腕 (Task 3) を置き換える。

```rust
            ExprKind::Handle {
                body: handled,
                effect,
                clauses,
                ret,
            } => self.handle(*effect, *handled, clauses, ret.as_ref()),
            ExprKind::Resume { k, arg } => self.resume(id, *k, *arg),
            // `drop` はどんな値も受け取る。値を捨てることは使用の1回に数える (docs/spec/linearity.md)
            ExprKind::Drop(value) => {
                self.infer_expr(*value);
                self.table.unit
            }
```

- [ ] **Step 5: テストが通ることを確かめる**

Run: `cargo test -p eml_types`
Expected: PASS

- [ ] **Step 6: 全体を確かめてコミットする**

```bash
cargo test && cargo clippy --all-targets && cargo fmt
git add crates
git commit -m "Type-check handlers, resume, and drop"
```

---

### Task 6: Kind の制約の由来を記録し、違反を E3001 にする

Kind の制約に由来を記録し、`solve_kinds` が破れた制約の由来を返す。`check` は今の `debug_assert` の代わりに E3001 を出す。使用回数のパスに handle、`resume`、`drop` を足し、操作の節が捕まえる変数に `Unr` の制約を出す。

**Files:**
- Modify: `crates/eml_types/src/kind.rs`
- Modify: `crates/eml_types/src/table/mod.rs`、`crates/eml_types/src/table/kinds.rs`
- Modify: `crates/eml_types/src/check/mod.rs`、`body.rs`、`report.rs`
- Modify: `crates/eml_types/src/usage.rs`
- Modify: `crates/eml_types/src/lib.rs` (`codes`)
- Test: `crates/eml_types/tests/effects.rs`

**Interfaces:**
- Consumes: Task 3 の `Body::captures`、`OpClause::patterns`。Task 5 の handle の検査
- Produces:
  - `kind::{KindOrigin { range: TextRange, reason: KindReason }, KindReason}`。`KindReason::{UsedMoreThanOnce(String), NotUsed(String), Discarded, CapturedByClause(String), CapturedByLambda, Passed(String), Unified}`
  - `Lattice::set_origin(Option<KindOrigin>)`、`Lattice::origin(usize) -> Option<&KindOrigin>`
  - `Table::set_kind_origin(Option<KindOrigin>) -> Option<KindOrigin>` (前の由来を返す)、`Table::solve_kinds() -> Vec<KindOrigin>`
  - `eml_types::codes::LINEAR_VALUE_MISUSED` (E3001)、`report::linear_misuse(file, &KindOrigin) -> Diagnostic`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/effects.rs` に足す。

```rust
#[test]
fn a_continuation_of_a_once_operation_must_be_used_exactly_once() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\ntwice : Unit -> Int\ntwice () =\n  handle ask () with\n    | ask () k -> resume k 1 + resume k 2\n\nunused : Unit -> Int\nunused () =\n  handle ask () with\n    | ask () k -> 0\n\ndiscarded : Unit -> Int\ndiscarded () =\n  handle ask () with\n    | ask () _ -> 0\n\ncaptured : Unit -> <Ask> Int\ncaptured () =\n  handle ask () with\n    | ask () k ->\n        handle ask () with\n          | ask () inner -> resume k (resume inner 1)";
    insta::assert_snapshot!(check_text(text), @r"
    ask : Unit -> <Ask> Int
    twice : Unit -> Int
      k#0 : Cont Int Int <>
    unused : Unit -> Int
      k#0 : Cont Int Int <>
    discarded : Unit -> Int
    captured : Unit -> <Ask> Int
      k#0 : Cont Int Int <Ask>
      inner#1 : Cont Int Int <Ask>
    ---
    E3001 7:14 `k` must be used exactly once, but it may be used more than once
      7:14 `k` is bound here
      note: linear values, such as the continuation of a `once` operation and closures that capture one, must be used exactly once
    E3001 12:14 `k` must be used exactly once, but some paths do not use it
      12:14 `k` is bound here
      note: linear values, such as the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: pass `k` to `drop` on the paths that do not use it
    E3001 17:14 a linear value cannot be discarded with `_`
      17:14 this pattern discards it
      note: linear values, such as the continuation of a `once` operation and closures that capture one, must be used exactly once
      help: bind it to a name and pass the name to `drop`
    E3001 22:14 `k` must be used exactly once, but an operation clause captures it
      22:14 `k` is bound here
      note: linear values, such as the continuation of a `once` operation and closures that capture one, must be used exactly once
      note: an operation clause runs each time its operation is performed
    ");
}

#[test]
fn a_closure_capturing_a_continuation_cannot_be_used_twice() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\ntwice : (Int -> Int) -> Int\ntwice f = f (f 0)\n\ng : Unit -> Int\ng () =\n  handle ask () with\n    | ask () k -> twice (fn n -> resume k n)";
    insta::assert_snapshot!(check_text(text), @r"
    ask : Unit -> <Ask> Int
    twice : (Int -> Int) -> Int
      kinds: (Int -> Int) <= Unr
      f#0 : Int -> Int
    g : Unit -> Int
      k#0 : Cont Int Int <>
      n#1 : Int
    ---
    E3001 10:19 a linear value is passed to `twice`, which may use it more than once or not at all
      10:19 `twice` is used here
      note: linear values, such as the continuation of a `once` operation and closures that capture one, must be used exactly once
    ");
}

#[test]
fn a_body_with_a_reported_error_does_not_report_linear_values() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nf : Unit -> Int\nf () =\n  handle ask () with\n    | ask () k -> resume k";
    insta::assert_snapshot!(check_text(text), @r"
    ask : Unit -> <Ask> Int
    f : Unit -> Int
      k#0 : Cont Int Int <>
    ---
    E1011 7:19 `resume` takes a continuation and a value, but 1 argument was given
      7:19 this `resume`
    ");
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --test effects`
Expected: FAIL。debug ビルドでは `a kind constraint was violated without linear types` の `debug_assert` で panic する

- [ ] **Step 3: 束に由来を記録する**

`crates/eml_types/src/kind.rs` の先頭に `use eml_diagnostics::TextRange;` を足し、由来の型を足す。

```rust
/// Kind の制約の由来。制約が破れたときに E3001 が指す場所と理由である (docs/spec/diagnostics.md の「線形性の診断」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KindOrigin {
    pub range: TextRange,
    pub reason: KindReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KindReason {
    /// ある経路で2回以上使った変数。名前は変数の名前である。
    UsedMoreThanOnce(String),
    /// ある経路で使わなかった変数。
    NotUsed(String),
    /// `_` で受けた値。
    Discarded,
    /// 操作の節が捕まえた変数。
    CapturedByClause(String),
    /// ラムダが捕まえた値。
    CapturedByLambda,
    /// トップレベルの関数、組み込み、操作のスキームから複写した制約。名前は参照した値の名前である。
    Passed(String),
    /// 型の単一化で出た制約。
    Unified,
}
```

`Lattice` に由来の列と今の由来を足す。

```rust
#[derive(Debug)]
pub(crate) struct Lattice<T> {
    bottom: T,
    vars: usize,
    constraints: Vec<(Bound<T>, Bound<T>)>,
    /// 制約ごとの由来。`constraints` と同じ順に並ぶ。
    origins: Vec<Option<KindOrigin>>,
    /// これから作る制約に記録する由来。制約を作る側の引数を増やさずに済むよう、型の表が設定する。
    current: Option<KindOrigin>,
}
```

`new` で `origins: Vec::new(), current: None` を初期化する。`require` と `copy_constraints` で、制約を足すたびに `self.origins.push(self.current.clone());` を足す。次の2つの関数を足す。

```rust
    pub fn set_origin(&mut self, origin: Option<KindOrigin>) {
        self.current = origin;
    }

    pub fn origin(&self, index: usize) -> Option<&KindOrigin> {
        self.origins[index].as_ref()
    }
```

- [ ] **Step 4: 型の表が今の由来を持ち、破れた制約の由来を返す**

`crates/eml_types/src/table/mod.rs` の `Table` に `kind_origin: Option<KindOrigin>` を足し (`new` では `None`)、関数を足す。

```rust
    /// これから作る Kind の制約の由来を設定し、前の由来を返す。呼び出し側は、制約を作る処理の後で前の由来に戻す。
    pub fn set_kind_origin(&mut self, origin: Option<KindOrigin>) -> Option<KindOrigin> {
        self.linearity.set_origin(origin.clone());
        self.multiplicity.set_origin(origin.clone());
        std::mem::replace(&mut self.kind_origin, origin)
    }
```

`use crate::kind::{...}` に `KindOrigin` を足す。

`crates/eml_types/src/table/kinds.rs` の `solve_kinds` を次にする。

```rust
    /// すべての Kind の制約を解き、線形性の解を覚える。`export` が式ごとに解き直さずに済むようにするため。
    /// 定数の上限を超えた制約の由来を、位置の順に重複なく返す。由来のない制約は、報告済みの誤りのある本体から出た
    /// ものなので返さない。誤りのあるプログラムは実行しないので、困ることはない。
    pub fn solve_kinds(&mut self) -> Vec<KindOrigin> {
        let (lin, lin_violated) = self.linearity.solve();
        let (_, mult_violated) = self.multiplicity.solve();
        self.lin_solution = Some(lin);
        let mut origins: Vec<KindOrigin> = lin_violated
            .iter()
            .filter_map(|&index| self.linearity.origin(index).cloned())
            .chain(
                mult_violated
                    .iter()
                    .filter_map(|&index| self.multiplicity.origin(index).cloned()),
            )
            .collect();
        origins.sort_by_key(|origin| (origin.range.start(), origin.range.end()));
        origins.dedup();
        origins
    }
```

`use` に `crate::kind::KindOrigin` を足す (`use super::*;` で入らない場合)。

- [ ] **Step 5: 違反を E3001 にする**

`crates/eml_types/src/lib.rs` の `codes` に足す。

```rust
    pub const LINEAR_VALUE_MISUSED: ErrorCode = ErrorCode(3001);
```

`crates/eml_types/src/check/report.rs` の最後に足す。`use` に `eml_diagnostics::FileId` と `crate::kind::{KindOrigin, KindReason}` を足す。

```rust
/// 線形な値の誤った使い方 (E3001)。破れた Kind の制約の由来を指す。指し方を docs/spec/diagnostics.md の「線形性の
/// 診断」の表どおりにする (二重使用の2か所など) のは、線形性の検査パスを分ける段階5で行う。
pub(super) fn linear_misuse(file: FileId, origin: &KindOrigin) -> Diagnostic {
    let (message, label) = match &origin.reason {
        KindReason::UsedMoreThanOnce(name) => (
            format!("`{name}` must be used exactly once, but it may be used more than once"),
            format!("`{name}` is bound here"),
        ),
        KindReason::NotUsed(name) => (
            format!("`{name}` must be used exactly once, but some paths do not use it"),
            format!("`{name}` is bound here"),
        ),
        KindReason::Discarded => (
            "a linear value cannot be discarded with `_`".to_string(),
            "this pattern discards it".to_string(),
        ),
        KindReason::CapturedByClause(name) => (
            format!("`{name}` must be used exactly once, but an operation clause captures it"),
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
        KindReason::Unified => (
            "a linear value is used where an unrestricted value is expected".to_string(),
            "this expression".to_string(),
        ),
    };
    let mut diagnostic = Diagnostic::error(
        codes::LINEAR_VALUE_MISUSED,
        message,
        Label::new(file, origin.range, label),
    )
    .with_note(
        "linear values, such as the continuation of a `once` operation and closures that capture one, must be used exactly once",
    );
    match &origin.reason {
        KindReason::NotUsed(name) => {
            diagnostic =
                diagnostic.with_help(format!("pass `{name}` to `drop` on the paths that do not use it"));
        }
        KindReason::Discarded => {
            diagnostic = diagnostic.with_help("bind it to a name and pass the name to `drop`");
        }
        KindReason::CapturedByClause(_) => {
            diagnostic =
                diagnostic.with_note("an operation clause runs each time its operation is performed");
        }
        _ => {}
    }
    diagnostic
}
```

`crates/eml_types/src/check/mod.rs` の `let violated = table.solve_kinds();` と、その後の `debug_assert!` を次に置き換える。

```rust
    // Kind の制約の違反は、線形な値の誤った使い方である (docs/spec/linearity.md)
    for origin in table.solve_kinds() {
        diagnostics.push(report::linear_misuse(module.file, &origin));
    }
```

- [ ] **Step 6: 本体の検査で由来を設定する**

`crates/eml_types/src/check/body.rs` に補助関数を足す。`use` に `crate::kind::{KindOrigin, KindReason}` を足す。

```rust
    /// `check` の間だけ、作る Kind の制約の由来を設定する。
    fn with_kind_origin<T>(
        &mut self,
        range: TextRange,
        reason: KindReason,
        check: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let previous = self
            .table
            .set_kind_origin(Some(KindOrigin { range, reason }));
        let result = check(self);
        self.table.set_kind_origin(previous);
        result
    }
```

`expect` の `match self.table.unify(found, expected)` を次にする。

```rust
        let unified = self.with_kind_origin(range, KindReason::Unified, |this| {
            this.table.unify(found, expected)
        });
        match unified {
```

`value` に参照の範囲を渡し、スキームの具体化で複写する制約に `Passed` の由来を付ける。`infer_expr` の `ExprKind::Path(res) => self.value(*res),` を `ExprKind::Path(res) => self.value(*res, expr.range),` にし、`value` を次にする。

```rust
    /// 関数と組み込みの参照は、具体化した後に戻り値の側の閉じた row を開く。純粋な関数を、エフェクトを持つ関数型の
    /// 引数に渡せるようにするため (docs/spec/types.md の「推論」)。局所変数の型は開かない。スキームから複写する Kind
    /// の制約は、参照した場所を由来にする。
    fn value(&mut self, res: Res, range: TextRange) -> Ty {
        let module = self.module;
        let ty = match res {
            Res::Local(local) => {
                return self
                    .typing
                    .locals
                    .get(local)
                    .copied()
                    .unwrap_or(self.table.error);
            }
            Res::Function(function) => {
                let name = module.functions[function].name.clone();
                self.with_kind_origin(range, KindReason::Passed(name), |this| {
                    this.reference(function)
                })
            }
            // コンストラクタは Prelude にない。段階4で `data Bool` に置き換える
            Res::Builtin(Builtin::True | Builtin::False) => self.table.bool,
            Res::Builtin(builtin) => {
                let reason = KindReason::Passed(builtin.name().to_string());
                self.with_kind_origin(range, reason, |this| match this.builtins.get(&builtin) {
                    Some(scheme) => scheme.instantiate(this.table),
                    None => this.table.error,
                })
            }
            Res::Operation(operation) => {
                let name = module.operations[operation].name.clone();
                self.with_kind_origin(range, KindReason::Passed(name), |this| {
                    match this.operations.get(operation) {
                        Some(scheme) => scheme.instantiate(this.table),
                        None => this.table.error,
                    }
                })
            }
        };
        self.table.open_spine(ty)
    }
```

- [ ] **Step 7: 使用回数のパスに handler を足し、由来を設定する**

`crates/eml_types/src/usage.rs` を次のように直す。

1. `use` に `eml_diagnostics::TextRange` と `crate::kind::{KindOrigin, KindReason}` を足す。
2. `constrain` と `Usage` を次にする。

```rust
pub(crate) fn constrain(body: &Body, typing: &BodyTyping, table: &mut Table) {
    // 報告済みの誤りの跡 (`Missing`) がある本体では、捨てた式の中の使用が数えられない。E3001 を連鎖させないよう、
    // 由来を記録せずに制約だけを出す (docs/spec/types.md の「エラーの扱い」)
    let reliable = !body
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
        usage.check_pat(param, &uses);
    }
}

struct Usage<'a> {
    body: &'a Body,
    typing: &'a BodyTyping,
    table: &'a mut Table,
    reliable: bool,
}
```

3. `expr` の `ExprKind::Lambda` の腕で、捕まえた変数の数え方とクロージャの Kind を次にする。

```rust
                for &local in &captured {
                    self.count(local, inner[&local]);
                    if let Some(&ty) = self.typing.locals.get(local) {
                        captured_types.push(ty);
                    }
                }
                if let Some(&ty) = self.typing.exprs.get(id) {
                    let range = body.exprs[id].range;
                    self.with_origin(range, KindReason::CapturedByLambda, |table| {
                        table.closure_kinds(ty, params.len(), &captured_types)
                    });
                }
```

4. 仮の腕 (Task 3) を置き換える。

```rust
            // handle の本体と節は、捕まえた変数を先頭の引数に持つ関数に持ち上げる (docs/spec/core-ir.md)。ラムダと
            // 同じく、捕まえることを handle 式の位置での1回の使用に数え、中の使用を別に数える
            ExprKind::Handle {
                body: handled,
                clauses,
                ret,
                ..
            } => {
                let mut uses = Uses::new();
                let inner = self.expr(*handled);
                let captured = self.captured_once(*handled, &[], inner);
                sequence(&mut uses, captured);
                if let Some(ret) = ret {
                    let inner = self.expr(ret.body);
                    let captured = self.captured_once(ret.body, &[ret.param], inner);
                    sequence(&mut uses, captured);
                }
                for clause in clauses {
                    let mut inner = self.expr(clause.body);
                    let bound: Vec<PatId> = clause.patterns().collect();
                    for &pat in &bound {
                        self.check_pat(pat, &inner);
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
                        captured.into_iter().map(|local| (local, (1, 1))).collect(),
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
            ExprKind::Drop(value) => self.expr(*value),
```

5. `check_pat` と `unr_local` を次にし、補助関数を足す。

```rust
    /// パターンが束縛した変数を数え終える。どこかの経路で0回か2回以上なら、その型の Kind に `Unr` の制約を出す。
    /// `_` で受けた値も使わない値なので同じ扱いにする (docs/spec/linearity.md の「基本の規則」)。
    fn check_pat(&mut self, pat: PatId, uses: &Uses) {
        match &self.body.pats[pat].kind {
            PatKind::Bind(local) => {
                self.count(*local, uses.get(local).copied().unwrap_or((0, 0)));
            }
            PatKind::Wildcard => {
                if let Some(&ty) = self.typing.pats.get(pat) {
                    let range = self.body.pats[pat].range;
                    self.with_origin(range, KindReason::Discarded, |table| {
                        table.kind_at_most(ty, Bound::Const(Linearity::Unr))
                    });
                }
            }
            PatKind::Annot { pat, .. } => self.check_pat(*pat, uses),
            PatKind::Unit | PatKind::Missing => {}
        }
    }

    /// 1回だけ動く部分 (handle の本体と `return` の節) の使用回数を、捕まえた変数の1回の使用にまとめる。
    fn captured_once(&mut self, root: ExprId, params: &[PatId], mut inner: Uses) -> Uses {
        for &param in params {
            self.check_pat(param, &inner);
            remove_bound(self.body, param, &mut inner);
        }
        let mut captured: Vec<LocalId> = inner.keys().copied().collect();
        captured.sort();
        // 捕まえた変数の集合は、Core IR の変換が使う `captures` と同じでなければならない
        debug_assert_eq!(captured, self.body.captures(root, params));
        for &local in &captured {
            self.count(local, inner[&local]);
        }
        captured.into_iter().map(|local| (local, (1, 1))).collect()
    }

    /// 経路ごとの使用回数が1回でなければ、`Unr` の制約を出す。
    fn count(&mut self, local: LocalId, (min, max): (u8, u8)) {
        if (min, max) == (1, 1) {
            return;
        }
        let name = self.body.locals[local].name.clone();
        let reason = if max >= 2 {
            KindReason::UsedMoreThanOnce(name)
        } else {
            KindReason::NotUsed(name)
        };
        self.unr_local(local, reason);
    }

    fn unr_local(&mut self, local: LocalId, reason: KindReason) {
        if let Some(&ty) = self.typing.locals.get(local) {
            let range = self.body.locals[local].range;
            self.with_origin(range, reason, |table| {
                table.kind_at_most(ty, Bound::Const(Linearity::Unr))
            });
        }
    }

    /// `constrain` の間だけ、作る Kind の制約の由来を設定する。誤りのある本体では由来を記録しない。
    fn with_origin(&mut self, range: TextRange, reason: KindReason, constrain: impl FnOnce(&mut Table)) {
        let origin = self.reliable.then_some(KindOrigin { range, reason });
        let previous = self.table.set_kind_origin(origin);
        constrain(&mut *self.table);
        self.table.set_kind_origin(previous);
    }
```

今の `ExprKind::Lambda` の腕で `if inner[&local] != (1, 1) { self.unr_local(local); }` としていた箇所は、上の 3. の `self.count(local, inner[&local])` に置き換わる。

- [ ] **Step 8: テストが通ることを確かめる**

Run: `cargo test -p eml_types`
Expected: PASS。既存の `check.rs` のスナップショットは変わらない (由来は診断の出る制約にしか効かず、`Lin` の型は段階3a まで現れない)

- [ ] **Step 9: 全体を確かめてコミットする**

```bash
cargo test && cargo clippy --all-targets && cargo fmt
git add crates
git commit -m "Record the origins of kind constraints and report violations as E3001"
```

---
### Task 7: Core IR に handle、`perform`、`resume`、`drop` の命令を足す

`Call` に `Handle`、`Perform`、`Resume` を、`Rhs` に `Drop` を足し、`IO` の操作の `Rhs::Perform` を `Rhs::Io` に改名する。`Program` にエフェクトの表を持たせ、verifier と表示を新しい命令に広げる。インタプリタは `Drop` を実行し、新しい `Call` は Task 9 まで仮の分岐にする。

**Files:**
- Modify: `crates/eml_core_ir/src/lib.rs`
- Modify: `crates/eml_core_ir/src/lower.rs` (`Rhs::Io` への改名、`Program::effects`)
- Modify: `crates/eml_core_ir/src/pretty.rs`
- Modify: `crates/eml_core_ir/src/verify.rs`
- Modify: `crates/eml_interp/src/lib.rs`
- Modify: `crates/eml_core_ir/tests/verify.rs`、`crates/eml_interp/tests/closures.rs`、`crates/eml_interp/tests/run.rs` (種類3)
- Test: `crates/eml_core_ir/tests/verify.rs`

**Interfaces:**
- Consumes: Task 2 の `Module::operations`、`OpMultiplicity`
- Produces:
  - `Call::Handle { effect: u32, body: Atom, clauses: Vec<Atom>, ret: Option<Atom> }`、`Call::Perform { effect: u32, op: u32, args: Vec<Atom> }`、`Call::Resume { k: Atom, arg: Atom }`
  - `Rhs::Io(IoOp, Vec<Atom>)` (今の `Rhs::Perform`)、`Rhs::Drop(Atom)`
  - `Program::effects: Vec<EffectInfo>`、`EffectInfo { name: String, operations: Vec<OperationInfo> }`、`OperationInfo { name: String, resumable: bool }`。エフェクトの番号は `EffectId` の添字である
  - 表示: `handle Ask(c2) {ask: c3} return c4`、`perform Ask.ask(s1)`、`resume k1(1)`、`drop s0`。`IO` の操作は今と同じく `perform println(s1)`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/verify.rs` の `use` に `EffectInfo` と `OperationInfo` を足す。既存の `check` の `Program { ... }` に `effects: Vec::new(),` を足す (種類3)。ファイルの最後に足す。

```rust
fn check_with_effects(functions: Vec<CoreFn>, effects: Vec<EffectInfo>) -> Result<(), String> {
    let program = Program {
        functions,
        entry: FnIdx(0),
        strings: Vec::new(),
        effects,
    };
    verify(&program).map_err(|error| error.to_string())
}

fn ask_effect() -> Vec<EffectInfo> {
    vec![EffectInfo {
        name: "Ask".to_string(),
        operations: vec![OperationInfo {
            name: "ask".to_string(),
            resumable: true,
        }],
    }]
}

/// `handle ask 1 with | ask x k -> resume k x` を持ち上げた形。
fn handler_program() -> Vec<CoreFn> {
    let main = function(
        "main",
        0,
        vec![string("c"), string("c"), int("t")],
        vec![
            CExpr::Return(var(2)),
            CExpr::Let {
                var: VarId(2),
                rhs: Rhs::Call {
                    call: Call::Handle {
                        effect: 0,
                        body: var(0),
                        clauses: vec![var(1)],
                        ret: None,
                    },
                    saved: Vec::new(),
                },
                body: CExprId(0),
            },
            CExpr::Let {
                var: VarId(1),
                rhs: Rhs::MakeClosure(FnIdx(2), Vec::new()),
                body: CExprId(1),
            },
            CExpr::Let {
                var: VarId(0),
                rhs: Rhs::MakeClosure(FnIdx(1), Vec::new()),
                body: CExprId(2),
            },
        ],
        &[],
    );
    let body = function(
        "main$handle0",
        1,
        vec![int("p")],
        vec![CExpr::TailCall(Call::Perform {
            effect: 0,
            op: 0,
            args: vec![Atom::Int(1)],
        })],
        &[],
    );
    let clause = function(
        "main$handle0$ask",
        2,
        vec![int("x"), string("k")],
        vec![CExpr::TailCall(Call::Resume {
            k: var(1),
            arg: var(0),
        })],
        &[],
    );
    vec![main, body, clause]
}

#[test]
fn handlers_operations_and_resume_are_calls() {
    assert_eq!(check_with_effects(handler_program(), ask_effect()), Ok(()));
}

#[test]
fn a_handler_has_a_clause_for_each_operation() {
    let mut effects = ask_effect();
    effects[0].operations.push(OperationInfo {
        name: "tell".to_string(),
        resumable: true,
    });
    assert_eq!(
        check_with_effects(handler_program(), effects),
        Err("a handler of `Ask` has clauses for 1 operations, but the effect has 2 in `main`".to_string())
    );
}

#[test]
fn perform_names_an_operation_of_its_effect() {
    let f = function(
        "f",
        0,
        Vec::new(),
        vec![CExpr::TailCall(Call::Perform {
            effect: 0,
            op: 1,
            args: Vec::new(),
        })],
        &[],
    );
    assert_eq!(
        check_with_effects(vec![f], ask_effect()),
        Err("`perform` names operation 1 of `Ask`, which has 1 operations in `f`".to_string())
    );
}

#[test]
fn drop_takes_the_ownership_of_its_value() {
    let once = function(
        "f",
        1,
        vec![string("s"), int("t")],
        vec![
            CExpr::Return(var(1)),
            CExpr::Let {
                var: VarId(1),
                rhs: Rhs::Drop(var(0)),
                body: CExprId(0),
            },
        ],
        &[],
    );
    assert_eq!(check_with_effects(vec![once], Vec::new()), Ok(()));
    let twice = function(
        "g",
        1,
        vec![string("s"), int("t"), int("u")],
        vec![
            CExpr::Return(var(2)),
            CExpr::Let {
                var: VarId(2),
                rhs: Rhs::Drop(var(0)),
                body: CExprId(0),
            },
            CExpr::Let {
                var: VarId(1),
                rhs: Rhs::Drop(var(0)),
                body: CExprId(1),
            },
        ],
        &[],
    );
    assert_eq!(
        check_with_effects(vec![twice], Vec::new()),
        Err("`s0` is used after it was moved in `g`".to_string())
    );
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test verify`
Expected: コンパイルエラー (`no variant named Handle`、`cannot find type EffectInfo`)

- [ ] **Step 3: Core IR の型を足す**

`crates/eml_core_ir/src/lib.rs` を次のように直す。

`Program` にフィールドを足し、エフェクトの表の型を足す。

```rust
#[derive(Debug)]
pub struct Program {
    pub functions: Vec<CoreFn>,
    /// 実行の入口。`main` を `()` で呼ぶ、引数のない関数 (docs/spec/core-ir.md)。
    pub entry: FnIdx,
    /// 文字列リテラルの定数表。`ConstString` が添字で引き、実行のたびに新しい文字列をヒープに作る。
    pub strings: Vec<String>,
    /// エフェクトの表。添字は `Call::Handle` と `Call::Perform` のエフェクトの番号で、HIR の `EffectId` の添字と同じである。
    pub effects: Vec<EffectInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectInfo {
    pub name: String,
    /// 宣言した順の操作。`Call::Handle` の節と `Call::Perform` の操作の番号は、この順の添字である。
    pub operations: Vec<OperationInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationInfo {
    pub name: String,
    /// `never` の操作は再開しないので、継続を作らずに捨てる (docs/spec/effects.md)。
    pub resumable: bool,
}
```

`Rhs` の `Perform(IoOp, Vec<Atom>)` を次の2つにする。

```rust
    /// `IO` の操作。最下部の組み込みの handler が必ずすぐに1回再開するので、継続を遡らずにその場で実行する
    /// (docs/spec/core-ir.md)。
    Io(IoOp, Vec<Atom>),
    /// 値の所有権を受け取って捨てる。値は `()` である (docs/spec/core-ir.md の `drop x`)。
    Drop(Atom),
```

`Rhs::atoms` の腕を `Rhs::MakeClosure(_, args) | Rhs::Prim(_, args) | Rhs::Io(_, args) => args.clone(),` にし、`Rhs::Drop(atom) => vec![*atom],` を足す。

`Call` に足す。

```rust
    /// handler フレームを積み、本体のクロージャに `()` を適用する。本体と節と `return` の節は、捕まえた変数を先頭の
    /// 引数に持つ関数のクロージャである (docs/spec/core-ir.md)。節はエフェクトの操作の順に並ぶ。`ret` が `None` なら、
    /// 本体の値をそのまま返す。
    Handle {
        effect: u32,
        body: Atom,
        clauses: Vec<Atom>,
        ret: Option<Atom>,
    },
    /// ユーザーのエフェクトの操作。継続を遡って handler を探し、その節を呼ぶ。
    Perform {
        effect: u32,
        op: u32,
        args: Vec<Atom>,
    },
    /// 継続を再開する。値は handle 式の値である。
    Resume {
        k: Atom,
        arg: Atom,
    },
```

`Call::atoms` を次にする。

```rust
    /// 呼び出しが使う値。関数値の呼び出しでは、呼ばれる値が先に来る。
    pub fn atoms(&self) -> Vec<Atom> {
        match self {
            Call::Direct(_, args) | Call::Perform { args, .. } => args.clone(),
            Call::Apply(callee, args) => std::iter::once(*callee)
                .chain(args.iter().copied())
                .collect(),
            Call::Handle {
                body, clauses, ret, ..
            } => std::iter::once(*body)
                .chain(clauses.iter().copied())
                .chain(*ret)
                .collect(),
            Call::Resume { k, arg } => vec![*k, *arg],
        }
    }
```

- [ ] **Step 4: 変換を改名とエフェクトの表に合わせる**

`crates/eml_core_ir/src/lower.rs` で、`Rhs::Perform(` を `Rhs::Io(` に置き換える (`wrapper` と `call_builtin` の2か所)。`lower` の最後の `Program { ... }` に `effects: effect_table(module),` を足し、関数を足す。`use` に `EffectInfo`、`OperationInfo` (`crate`) と `OpMultiplicity` (`eml_hir`) を足す。

```rust
/// エフェクトの表。`EffectId` の添字の順に並べ、エフェクトの番号を `EffectId` の添字と同じにする。
fn effect_table(module: &Module) -> Vec<EffectInfo> {
    module
        .effects
        .iter()
        .map(|(_, effect)| EffectInfo {
            name: effect.name.clone(),
            operations: effect
                .operations
                .iter()
                .map(|&op| {
                    let operation = &module.operations[op];
                    OperationInfo {
                        name: operation.name.clone(),
                        resumable: operation.multiplicity != OpMultiplicity::Never,
                    }
                })
                .collect(),
        })
        .collect()
}
```

- [ ] **Step 5: 表示と verifier を新しい命令に広げる**

`crates/eml_core_ir/src/pretty.rs` の `rhs_text` を次のように直す。

```rust
        Rhs::Call { call, saved } => {
            let text = match call {
                Call::Direct(..) => format!("call {}", call_text(program, function, call)),
                _ => call_text(program, function, call),
            };
            // ...今の `saved` の表示のまま...
        }
        // ...
        Rhs::Io(IoOp::Println, a) => format!("perform println({})", args(a)),
        Rhs::Drop(a) => format!("drop {}", atom(function, a)),
```

`call_text` に腕を足す。

```rust
        Call::Handle {
            effect,
            body,
            clauses,
            ret,
        } => {
            let info = &program.effects[*effect as usize];
            let clauses: Vec<String> = info
                .operations
                .iter()
                .zip(clauses)
                .map(|(op, clause)| format!("{}: {}", op.name, atom(function, clause)))
                .collect();
            let ret = ret.map_or(String::new(), |ret| format!(" return {}", atom(function, &ret)));
            format!(
                "handle {}({}) {{{}}}{ret}",
                info.name,
                atom(function, body),
                clauses.join(", ")
            )
        }
        Call::Perform { effect, op, args: a } => {
            let info = &program.effects[*effect as usize];
            format!(
                "perform {}.{}({})",
                info.name,
                info.operations[*op as usize].name,
                args(a)
            )
        }
        Call::Resume { k, arg } => {
            format!("resume {}({})", atom(function, k), atom(function, arg))
        }
```

`crates/eml_core_ir/src/verify.rs` の `check_call` を次にする。

```rust
    fn check_call(&self, state: &mut State, call: &Call) -> Result<(), String> {
        match call {
            Call::Direct(target, args) => {
                let target = self.program.function(*target);
                if args.len() != target.params.len() {
                    return Err(format!(
                        "a direct call to `{}` passes {} arguments, but it takes {}",
                        target.name,
                        args.len(),
                        target.params.len()
                    ));
                }
            }
            Call::Handle {
                effect, clauses, ..
            } => {
                let info = self.effect(*effect)?;
                if clauses.len() != info.operations.len() {
                    return Err(format!(
                        "a handler of `{}` has clauses for {} operations, but the effect has {}",
                        info.name,
                        clauses.len(),
                        info.operations.len()
                    ));
                }
            }
            Call::Perform { effect, op, .. } => {
                let info = self.effect(*effect)?;
                if *op as usize >= info.operations.len() {
                    return Err(format!(
                        "`perform` names operation {op} of `{}`, which has {} operations",
                        info.name,
                        info.operations.len()
                    ));
                }
            }
            Call::Apply(..) | Call::Resume { .. } => {}
        }
        for atom in call.atoms() {
            self.consume(state, &atom)?;
        }
        Ok(())
    }

    fn effect(&self, effect: u32) -> Result<&EffectInfo, String> {
        self.program
            .effects
            .get(effect as usize)
            .ok_or_else(|| format!("effect {effect} is not in the effect table"))
    }
```

`use crate::{...}` に `EffectInfo` を足す。

- [ ] **Step 6: インタプリタを改名と `Drop` に合わせる**

`crates/eml_interp/src/lib.rs` で次のように直す。

1. `bind` の `Rhs::Perform(IoOp::Println, args)` を `Rhs::Io(IoOp::Println, args)` にする。
2. `bind` に腕を足す。

```rust
            // 所有している参照を1つ手放す。継続も RC が1のオブジェクトなので、これで解放される (docs/spec/core-ir.md)
            Rhs::Drop(atom) => {
                if let Value::Obj(obj) = self.atom(atom)? {
                    self.heap.decref(obj).map_err(Fault::Heap)?;
                }
                Value::Unit
            }
```

3. `call` の `let prepared = match call { ... }` に仮の腕を足す。Task 9 で handler の実行に置き換える。

```rust
            Call::Handle { .. } | Call::Perform { .. } | Call::Resume { .. } => {
                return Err(Fault::Internal("effects are not run yet"));
            }
```

`Program` を組み立てるテスト (`crates/eml_interp/tests/closures.rs` と `run.rs`) に `effects: Vec::new(),` を足し、`closures.rs` の `Rhs::Perform(` を `Rhs::Io(` にする (種類3。期待値は変えない)。

- [ ] **Step 7: テストが通ることを確かめる**

Run: `cargo test -p eml_core_ir && cargo test -p eml_interp`
Expected: PASS。既存の Core IR のスナップショットは変わらない (`IO` の操作の表示は `perform println(...)` のまま)

- [ ] **Step 8: 全体を確かめてコミットする**

```bash
cargo test && cargo clippy --all-targets && cargo fmt
git add crates
git commit -m "Add handle, perform, resume, and drop to Core IR"
```

---

### Task 8: 操作、handle、`resume`、`drop` を Core IR に変換する

操作の呼び出しを `Perform` に、値として使う操作を包む関数のクロージャにする。handle の本体と節をラムダと同じ方法で持ち上げ、`Call::Handle` にする。`resume` と `drop` を変換する。ラムダの持ち上げと handler の持ち上げは、1つの `lift` にまとめる。

**Files:**
- Modify: `crates/eml_core_ir/src/lower.rs`
- Test: `crates/eml_core_ir/tests/lower.rs`

**Interfaces:**
- Consumes: Task 3 の HIR の式と `Body::captures`。Task 4 の `TypedModule::operations` と `BodyTypes::pats`。Task 7 の命令
- Produces:
  - 持ち上げた関数の名前: handle の本体は `外側の名前$handleN`、操作の節は `外側の名前$handleN$操作名`、`return` の節は `外側の名前$handleN$return`。操作を包む関数は `op$操作名`
  - `FnLowering::lower` の引数は `params: &[(Option<PatId>, Type)]` になる (`None` は、パターンのない引数で、handle の本体の `()` に使う)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/lower.rs` に足す。

```rust
#[test]
fn handlers_are_lifted_to_closures() {
    let text = "effect Ask where\n  ask : String -> Int\n\nmain : Unit -> <IO> Unit\nmain () =\n  let prefix = \"n = \"\n  let n =\n    handle ask \"x\" with\n      | ask key k -> resume k 1\n      | return x -> x + 1\n  println (prefix ++ show_int n)";
    insta::assert_snapshot!(core_text(text), @r#"
    fn main(p0) {
      let s1 = const "n = "
      let c2 = closure main$handle0()
      let c3 = closure main$handle0$ask()
      let c4 = closure main$handle0$return()
      let t5 = handle Ask(c2) {ask: c3} return c4 [s1]
      let t6 = prim show_int(t5)
      let t7 = prim ++(s1, t6)
      let t8 = perform println(t7)
      return t8
    }
    fn main$handle0(p0) {
      let s1 = const "x"
      tailcall perform Ask.ask(s1)
    }
    fn main$handle0$ask(key0, k1) {
      decref key0
      tailcall resume k1(1)
    }
    fn main$handle0$return(x0) {
      let t1 = prim +(x0, 1)
      return t1
    }
    fn entry$main() {
      tailcall main(())
    }
    "#);
}

#[test]
fn operations_as_values_and_drop() {
    let text = "effect Log where\n  log : String -> String -> Unit\n\nrun : Unit -> <Log> Unit\nrun () =\n  let info = log \"info\"\n  info \"a\"\n\ndiscard : String -> Unit\ndiscard s = drop s\n\nmain : Unit -> <IO> Unit\nmain () = println \"x\"";
    insta::assert_snapshot!(core_text(text), @r#"
    fn run(p0) {
      let s1 = const "info"
      let c2 = closure op$log(s1)
      let s3 = const "a"
      tailcall apply c2(s3)
    }
    fn discard(s0) {
      let t1 = drop s0
      return t1
    }
    fn main(p0) {
      let s1 = const "x"
      let t2 = perform println(s1)
      return t2
    }
    fn op$log(p0, p1) {
      tailcall perform Log.log(p0, p1)
    }
    fn entry$main() {
      tailcall main(())
    }
    "#);
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test lower`
Expected: FAIL。`operations are not lowered to Core IR yet` などの `unreachable!` で panic する

- [ ] **Step 3: 操作を包む関数を作る**

`crates/eml_core_ir/src/lower.rs` の `ProgramBuilder` にフィールドを足す。

```rust
    /// 操作のスキームの型。操作を包む関数の変数が boxed かどうかを決める。
    operation_types: HashMap<OperationId, Type>,
    operation_wrappers: HashMap<OperationId, FnIdx>,
```

`ProgramBuilder::new` で、`operation_types: typed.operations.iter().map(|(id, scheme)| (id, scheme.ty.clone())).collect()` と `operation_wrappers: HashMap::new()` を初期化する。関数を足す。

```rust
    /// 操作を値や部分適用で使うときに、`perform` を末尾で呼ぶだけの関数を作る。操作ごとに1つだけ作る。
    fn operation_wrapper(&mut self, module: &Module, op: OperationId) -> FnIdx {
        if let Some(&function) = self.operation_wrappers.get(&op) {
            return function;
        }
        let operation = &module.operations[op];
        let arity = operation.arity;
        let ty = self
            .operation_types
            .get(&op)
            .expect("every operation has a scheme");
        let (param_types, _) = split_arrows(ty, arity);
        let vars = param_types
            .iter()
            .map(|ty| var_info("p", ty, &self.lang))
            .collect();
        let function = self.reserve(arity);
        self.operation_wrappers.insert(op, function);
        let params: Vec<VarId> = (0..arity as u32).map(VarId).collect();
        let args = params.iter().map(|&param| Atom::Var(param)).collect();
        let core = CoreFn {
            name: format!("op${}", operation.name),
            params,
            vars,
            body: CExprId(0),
            exprs: vec![CExpr::TailCall(perform_call(module, op, args))],
            joins: Vec::new(),
        };
        self.finish(function, core);
        function
    }
```

ファイルの関数の並びに補助関数を足す。

```rust
/// エフェクトの番号は `EffectId` の添字である (`Program::effects`)。
fn effect_index(effect: EffectId) -> u32 {
    u32::from(effect.into_raw())
}

/// 操作の番号は、エフェクトの宣言の中の順番である。
fn perform_call(module: &Module, op: OperationId, args: Vec<Atom>) -> Call {
    let effect = module.operations[op].effect;
    let index = module.effects[effect]
        .operations
        .iter()
        .position(|&other| other == op)
        .expect("an operation belongs to its effect");
    Call::Perform {
        effect: effect_index(effect),
        op: index as u32,
        args,
    }
}
```

`use eml_hir::{...}` に `EffectId` と `OperationId` を足す。

- [ ] **Step 4: 持ち上げを1つにまとめる**

`FnLowering` にフィールド `handlers: &'a mut u32` (handle の数。持ち上げた関数の名前に使う) を足す。`FnLowering::lower` の引数を次にする。

```rust
    /// ラムダと handle の本体と節は、捕まえた変数を先頭の引数に持つ (docs/spec/core-ir.md)。トップレベルの関数では
    /// `captured` は空である。引数のパターンが `None` なら、名前のない引数 (handle の本体が受ける `()`) である。
    fn lower(
        mut self,
        name: &str,
        captured: &[(LocalId, Type)],
        params: &[(Option<PatId>, Type)],
        root: ExprId,
    ) -> CoreFn {
        let body = self.body;
        let mut vars = Vec::new();
        for (local, ty) in captured {
            let var = self.new_var(&body.locals[*local].name, ty);
            self.locals.insert(*local, Atom::Var(var));
            vars.push(var);
        }
        for (pat, ty) in params {
            let local = pat.and_then(|pat| body.pat_bindings(pat).first().copied());
            let name = local.map_or("p", |local| body.locals[local].name.as_str());
            let var = self.new_var(name, ty);
            if let Some(local) = local {
                self.locals.insert(local, Atom::Var(var));
            }
            vars.push(var);
        }
        // ...残りは今のまま...
```

トップレベルの関数を変換するループは、`let mut handlers = 0;` を `lambdas` の隣に足し、`FnLowering { ... }` に `handlers: &mut handlers,` を足し、引数を組み立てて渡す。

```rust
        let (param_types, _) = split_arrows(signature, body.params.len());
        let params: Vec<(Option<PatId>, Type)> = body
            .params
            .iter()
            .map(|&pat| Some(pat))
            .zip(param_types)
            .collect();
        // ...
        .lower(&function.name, &[], &params, body.root);
```

`FnLowering` に `lift` を足す。

```rust
    /// `root` を、捕まえた変数を先頭の引数に持つ関数に持ち上げ、そのクロージャを作る (docs/spec/core-ir.md)。ラムダと、
    /// handle の本体と節に使う。関数の番号は持ち上げる前に取るので、入れ子の持ち上げは外側より後ろの番号になる。
    fn lift(
        &mut self,
        name: String,
        captured: Vec<LocalId>,
        params: &[(Option<PatId>, Type)],
        root: ExprId,
        ty: &Type,
        out: &mut Bindings,
    ) -> Atom {
        let captured: Vec<(LocalId, Type)> = captured
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
        let function = self.program.reserve(captured.len() + params.len());
        let core = FnLowering {
            module: self.module,
            body: self.body,
            types: self.types,
            indices: self.indices,
            program: &mut *self.program,
            root_name: self.root_name,
            lambdas: &mut *self.lambdas,
            handlers: &mut *self.handlers,
            exprs: Vec::new(),
            vars: Vec::new(),
            locals: ArenaMap::default(),
            joins: Vec::new(),
        }
        .lower(&name, &captured, params, root);
        self.program.finish(function, core);
        let atoms = captured
            .iter()
            .map(|(local, _)| self.locals[*local])
            .collect();
        self.bind(out, "c", ty, Rhs::MakeClosure(function, atoms))
    }

    fn pat_type(&self, pat: PatId) -> Type {
        self.types
            .pats
            .get(pat)
            .cloned()
            .expect("every pattern is typed")
    }
```

`atom` の `ExprKind::Lambda` の腕を `lift` で書き直す。名前を付ける順と番号を取る順は今と同じなので、既存のスナップショットは変わらない。

```rust
            ExprKind::Lambda {
                params,
                body: lambda_body,
            } => {
                let lambda_ty = self.ty(id);
                let (param_types, _) = split_arrows(&lambda_ty, params.len());
                let params: Vec<(Option<PatId>, Type)> = params
                    .iter()
                    .map(|&pat| Some(pat))
                    .zip(param_types)
                    .collect();
                let name = format!("{}$lambda{}", self.root_name, *self.lambdas);
                *self.lambdas += 1;
                let captured = body.lambda_captures(id);
                self.lift(name, captured, &params, *lambda_body, &lambda_ty, out)
            }
```

- [ ] **Step 5: 操作、handle、`resume`、`drop` を変換する**

`FnLowering` に操作の呼び出しを足す。

```rust
    /// 引数が操作の引数の個数に揃えば `perform` にし、足りなければ操作を包む関数のクロージャにする。操作の引数の個数は
    /// シグネチャの外側の矢印の数なので、型検査を通った呼び出しで引数が余ることはない。
    fn call_operation(
        &mut self,
        op: OperationId,
        args: Vec<Atom>,
        ty: &Type,
        out: &mut Bindings,
    ) -> Atom {
        let arity = self.module.operations[op].arity;
        if args.len() < arity {
            let wrapper = self.program.operation_wrapper(self.module, op);
            return self.bind(out, "c", ty, Rhs::MakeClosure(wrapper, args));
        }
        let call = perform_call(self.module, op, args);
        self.bind(out, "t", ty, Rhs::call(call))
    }
```

`atom` の仮の腕 (Task 2 の `Res::Operation` と Task 3 の handler) を置き換える。

```rust
            ExprKind::Path(Res::Operation(op)) => {
                let wrapper = self.program.operation_wrapper(self.module, *op);
                let ty = self.ty(id);
                self.bind(out, "c", &ty, Rhs::MakeClosure(wrapper, Vec::new()))
            }
```

`ExprKind::Call` の腕の `match &body.exprs[*callee].kind` に、`Res::Builtin` の腕の後に足す。

```rust
                    ExprKind::Path(Res::Operation(op)) => {
                        let args = self.call_args(args, first, out);
                        self.call_operation(*op, args, &ty, out)
                    }
```

handler、`resume`、`drop` の腕を足す。

```rust
            // 本体と節を、捕まえた変数を先頭の引数に持つ関数に持ち上げる (docs/spec/core-ir.md)。本体は `()` を受ける
            ExprKind::Handle {
                body: handled,
                effect,
                clauses,
                ret,
            } => {
                let effect = effect.expect("a program without errors handles a known effect");
                let ty = self.ty(id);
                let prefix = format!("{}$handle{}", self.root_name, *self.handlers);
                *self.handlers += 1;
                let unit = [(None, Type::unit())];
                let handled_closure = self.lift(
                    prefix.clone(),
                    body.captures(*handled, &[]),
                    &unit,
                    *handled,
                    &Type::Flexible,
                    out,
                );
                // 節はエフェクトの操作の順に並べる。インタプリタは操作の番号で節を引く
                let operations = self.module.effects[effect].operations.clone();
                let mut closures = Vec::new();
                for op in operations {
                    let clause = clauses
                        .iter()
                        .find(|clause| clause.op == op)
                        .expect("a program without errors has a clause for every operation");
                    let bound: Vec<PatId> = clause.patterns().collect();
                    let params: Vec<(Option<PatId>, Type)> = bound
                        .iter()
                        .map(|&pat| (Some(pat), self.pat_type(pat)))
                        .collect();
                    let name = format!("{prefix}${}", self.module.operations[op].name);
                    let captured = body.captures(clause.body, &bound);
                    let closure =
                        self.lift(name, captured, &params, clause.body, &Type::Flexible, out);
                    closures.push(closure);
                }
                let ret = ret.as_ref().map(|ret| {
                    let params = [(Some(ret.param), self.pat_type(ret.param))];
                    let captured = body.captures(ret.body, &[ret.param]);
                    let name = format!("{prefix}$return");
                    self.lift(name, captured, &params, ret.body, &Type::Flexible, out)
                });
                let call = Call::Handle {
                    effect: effect_index(effect),
                    body: handled_closure,
                    clauses: closures,
                    ret,
                };
                self.bind(out, "t", &ty, Rhs::call(call))
            }
            ExprKind::Resume { k, arg } => {
                let k = self.atom(*k, out);
                let arg = self.atom(*arg, out);
                let ty = self.ty(id);
                self.bind(out, "t", &ty, Rhs::call(Call::Resume { k, arg }))
            }
            ExprKind::Drop(value) => {
                let value = self.atom(*value, out);
                self.bind(out, "t", &Type::unit(), Rhs::Drop(value))
            }
```

`ret.as_ref().map(|ret| { ... self.lift(...) })` の中で `self` を可変で借りるので、借用の都合で書けない場合は `match ret { Some(ret) => Some(...), None => None }` で書く。

- [ ] **Step 6: テストが通ることを確かめる**

Run: `cargo test -p eml_core_ir`
Expected: PASS。既存のスナップショット (ラムダの持ち上げを含む) は変わらない

- [ ] **Step 7: 全体を確かめてコミットする**

```bash
cargo test && cargo clippy --all-targets && cargo fmt
git add crates
git commit -m "Lower operations, handlers, resume, and drop to Core IR"
```

---

### Task 9: handler フレームと継続オブジェクトでエフェクトを実行する

ランタイムに handler フレームと継続オブジェクトを足し、インタプリタで handle、`perform`、`resume` を実行する。`run/` の UI テストで、`debug_heap` を有効にしてリークも解放済みアクセスもないことを確かめる。

**Files:**
- Modify: `crates/eml_runtime/src/heap.rs`
- Modify: `crates/eml_interp/src/lib.rs`
- Create: `tests/ui/run/effect_abort.em`、`effect_resume.em`、`effect_deep.em`、`effect_drop_k.em`、`continuation_values.em`、`operation_values.em`、`effect_loop.em`
- Create: `crates/eml_cli/tests/snapshots/ui__run@*.em.snap` (上のファイルごとに、`cargo insta accept` で作る)

**Interfaces:**
- Consumes: Task 7 の命令と `Program::effects`
- Produces:
  - `Frame::Handler { effect: u32, clauses: Vec<Value>, ret: Option<Value>, next: Option<ObjRef> }`。`next` が `None` なのは、継続に捕まえられて外側から切り離されている間である
  - `Payload::Continuation { top: ObjRef, handler: ObjRef }`。記述子は `Continuation`。`top` だけを所有し、`handler` は所有せずに指す

- [ ] **Step 1: ランタイムの失敗するテストを書く**

`crates/eml_runtime/src/heap.rs` の `tests` に足す。

```rust
    fn handler(heap: &mut Heap, clauses: Vec<Value>, ret: Option<Value>, next: Option<ObjRef>) -> ObjRef {
        heap.alloc(Payload::Frame(Frame::Handler {
            effect: 1,
            clauses,
            ret,
            next,
        }))
    }

    #[test]
    fn a_handler_frame_releases_its_clauses_and_the_rest_of_the_continuation() {
        let mut heap = Heap::new();
        let clause = heap.alloc(Payload::Closure(Closure {
            function: 0,
            args: vec![],
        }));
        let ret = heap.alloc(Payload::Closure(Closure {
            function: 1,
            args: vec![],
        }));
        let end = bottom(&mut heap);
        let frame = handler(
            &mut heap,
            vec![Value::Obj(clause)],
            Some(Value::Obj(ret)),
            Some(end),
        );
        heap.decref(frame).unwrap();
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn releasing_a_continuation_stops_at_its_detached_handler() {
        let mut heap = Heap::new();
        // handler の外側は機械の継続が持っている
        let outside = bottom(&mut heap);
        let s = string(&mut heap, "saved");
        let detached = handler(&mut heap, vec![], None, None);
        let top = frame(&mut heap, vec![(0, Value::Obj(s))], detached);
        let k = heap.alloc(Payload::Continuation {
            top,
            handler: detached,
        });
        assert_eq!(
            heap.live_objects(),
            vec![
                ("Continuation".to_string(), 1),
                ("Frame".to_string(), 3),
                ("String".to_string(), 1),
            ]
        );
        heap.decref(k).unwrap();
        assert_eq!(heap.live_objects(), vec![("Frame".to_string(), 1)]);
        heap.decref(outside).unwrap();
        assert!(heap.live_objects().is_empty());
    }
```

Run: `cargo test -p eml_runtime`
Expected: コンパイルエラー (`no variant named Handler`)

- [ ] **Step 2: handler フレームと継続オブジェクトを足す**

`crates/eml_runtime/src/heap.rs` を次のように直す。

```rust
impl DescId {
    const STRING: DescId = DescId(0);
    const FRAME: DescId = DescId(1);
    const CLOSURE: DescId = DescId(2);
    const CONTINUATION: DescId = DescId(3);
}

const DESCRIPTORS: [Descriptor; 4] = [
    Descriptor { name: "String" },
    Descriptor { name: "Frame" },
    Descriptor { name: "Closure" },
    Descriptor {
        name: "Continuation",
    },
];

#[derive(Debug, PartialEq)]
pub enum Payload {
    Str(String),
    Closure(Closure),
    Frame(Frame),
    /// `perform` で捕まえた継続。先頭のフレーム `top` から `next` をたどった先に `handler` のフレームがある。`top`
    /// だけを所有し、`handler` は所有せずに指す。`handler` の `next` は捕まえられている間 `None` なので、継続を
    /// 解放すると `top` から `handler` までの区間だけが解放される (docs/spec/runtime.md)。
    Continuation { top: ObjRef, handler: ObjRef },
}
```

`Payload::desc` に `Payload::Continuation { .. } => DescId::CONTINUATION,` を足す。`Frame` に足す。

```rust
    /// handle の handler。節のクロージャはエフェクトの操作の順に並ぶ。`next` が `None` なのは、継続に捕まえられて
    /// handle の外側から切り離されている間である (docs/spec/core-ir.md)。
    Handler {
        effect: u32,
        clauses: Vec<Value>,
        ret: Option<Value>,
        next: Option<ObjRef>,
    },
```

`copy` に腕を足す。

```rust
        Payload::Frame(Frame::Handler {
            effect,
            clauses,
            ret,
            next,
        }) => Payload::Frame(Frame::Handler {
            effect: *effect,
            clauses: clauses.clone(),
            ret: *ret,
            next: *next,
        }),
        Payload::Continuation { top, handler } => Payload::Continuation {
            top: *top,
            handler: *handler,
        },
```

`children` に腕を足す。

```rust
        Payload::Frame(Frame::Handler {
            clauses, ret, next, ..
        }) => {
            work.extend(clauses.iter().filter_map(object));
            work.extend(ret.as_ref().and_then(object));
            work.extend(*next);
        }
        // `handler` は所有しない。`top` からたどれる
        Payload::Continuation { top, .. } => work.push(*top),
```

Run: `cargo test -p eml_runtime`
Expected: PASS (インタプリタの `ret` の `match` が網羅的でなくなるので、`eml_interp` は次のステップまでビルドできない)

- [ ] **Step 3: `run/` の失敗する UI テストを書く**

次の7つのファイルを作る。

`tests/ui/run/effect_abort.em`:

```haskell
-- `never` operations abort the handled body. The frames of the aborted part are freed with the values they saved,
-- including a continuation that a clause saved before a `never` operation aborted it.
effect Fail where
  never fail : String -> a

effect Ask where
  ask : Unit -> Int

check_positive : Int -> <Fail> Int
check_positive n =
  if n > 0 then n else fail "not positive"

describe : Int -> <Fail> String
describe n =
  let label = "value: "
  let checked = check_positive n
  label ++ show_int checked

safe : Int -> String
safe n =
  handle describe n with
    | fail message -> "error: " ++ message

checked : Int -> <Fail> Int
checked n = if n > 10 then fail "too big" else n

sum_two : Unit -> <Ask> Int
sum_two () = ask () + ask ()

ask_checked : Int -> <Fail> Int
ask_checked answer =
  handle sum_two () with
    | ask () k ->
        let n = checked answer
        resume k n

bounded : Int -> String
bounded answer =
  handle show_int (ask_checked answer) with
    | fail message -> "aborted: " ++ message

main : Unit -> <IO> Unit
main () =
  println (safe 3)
  println (safe 0)
  println (bounded 4)
  println (bounded 20)
```

`tests/ui/run/effect_resume.em`:

```haskell
-- `once` operations resumed in tail position and not in tail position. After a resume that is not in tail
-- position, the clause runs the rest of its body with the value of the whole handler.
effect Ask where
  ask : String -> String

greet : Unit -> <Ask> String
greet () =
  let name = ask "name"
  let place = ask "place"
  "hello " ++ name ++ " from " ++ place

with_answers : Unit -> String
with_answers () =
  handle greet () with
    | ask key k -> resume k (key ++ "!")

logged : Unit -> <IO> String
logged () =
  handle greet () with
    | ask key k ->
        let answer = resume k key
        println ("asked " ++ key)
        answer
    | return result -> "[" ++ result ++ "]"

main : Unit -> <IO> Unit
main () =
  println (with_answers ())
  println (logged ())
```

`tests/ui/run/effect_deep.em`:

```haskell
-- Deep handlers also handle the operations performed after `resume`. With nested handlers of one effect, the
-- inner one handles the operation. A clause runs outside its handler, so its operations go to the outer one. An
-- operation walks past the handlers of other effects.
effect Counter where
  tick : Unit -> Int

effect Log where
  log : String -> Unit

count_three : Unit -> <Counter> Int
count_three () =
  let a = tick ()
  let b = tick ()
  let c = tick ()
  a + b + c

inner_and_outer : Unit -> <Counter> Int
inner_and_outer () =
  let outer = tick ()
  let inner =
    handle tick () + tick () with
      | tick () k -> resume k 100
  outer + inner

logged_tick : Unit -> <Counter, Log> Int
logged_tick () =
  log "before"
  tick ()

main : Unit -> <IO> Unit
main () =
  let total =
    handle count_three () with
      | tick () k ->
          println "tick"
          resume k 1
  println (show_int total)
  let mixed =
    handle inner_and_outer () with
      | tick () k -> resume k 1
  println (show_int mixed)
  let relayed =
    handle (handle tick () with | tick () k -> resume k (tick () + 10)) with
      | tick () k -> resume k 5
  println (show_int relayed)
  let walked =
    handle (handle logged_tick () with | tick () k -> resume k 7) with
      | log message k ->
          println ("log: " ++ message)
          resume k ()
  println (show_int walked)
```

`tests/ui/run/effect_drop_k.em`:

```haskell
-- `drop k` discards a continuation. The frames it holds are freed with the values they saved. `drop` also
-- discards other values.
effect Choose where
  choose : String -> Bool

pick : Unit -> <Choose> String
pick () =
  let first = "apple"
  let second = "banana"
  if choose "which" then first else second

main : Unit -> <IO> Unit
main () =
  let taken =
    handle pick () with
      | choose question k ->
          drop question
          resume k True
  println taken
  let dropped =
    handle pick () with
      | choose question k ->
          drop k
          "dropped " ++ question
  println dropped
```

`tests/ui/run/continuation_values.em`:

```haskell
-- A continuation is a value: it can be passed to a function and captured by a closure before it is resumed.
effect Ask where
  ask : Unit -> Int

add_two : Unit -> <Ask> Int
add_two () = ask () + ask ()

main : Unit -> <IO> Unit
main () =
  let passed =
    handle add_two () with
      | ask () k ->
          let go = fn cont value -> resume cont value
          go k 20
  println (show_int passed)
  let captured =
    handle add_two () with
      | ask () k ->
          let later = fn n -> resume k n
          later 7
  println (show_int captured)
```

`tests/ui/run/operation_values.em`:

```haskell
-- Operations are values: they can be passed to functions, and a curried operation can be partially applied. A
-- partial application that is never called is freed.
effect Log where
  log : String -> String -> Unit
  note : String -> Unit

apply_twice : (String -> <e> Unit) -> <e> Unit
apply_twice f =
  f "first"
  f "second"

run : Unit -> <Log> Unit
run () =
  let unused = log "unused"
  apply_twice note
  apply_twice (log "info")

main : Unit -> <IO> Unit
main () =
  handle run () with
    | log level message k ->
        println (level ++ ": " ++ message)
        resume k ()
    | note message k ->
        println ("note: " ++ message)
        resume k ()
```

`tests/ui/run/effect_loop.em`:

```haskell
-- A hundred thousand operations resumed in tail position do not grow the continuation. The values saved across
-- each operation, including one bound by a join point, are freed exactly once.
effect Ask where
  ask : Unit -> Int

sum_asks : Int -> Int -> String -> <Ask> String
sum_asks n acc prefix =
  if n == 0 then prefix ++ show_int acc
  else
    let bonus = if n % 2 == 0 then 1 else 0
    let x = ask ()
    sum_asks (n - 1) (acc + x + bonus) prefix

main : Unit -> <IO> Unit
main () =
  let result =
    handle sum_asks 100000 0 "total: " with
      | ask () k -> resume k 2
  println result
```

- [ ] **Step 4: インタプリタで handler を実行する**

`crates/eml_interp/src/lib.rs` を次のように直す。

`Prepared` に足す。

```rust
enum Prepared {
    Direct(FnIdx, Vec<Value>),
    Apply(Value, Vec<Value>),
    Handle {
        effect: u32,
        body: Value,
        clauses: Vec<Value>,
        ret: Option<Value>,
    },
    Perform {
        effect: u32,
        op: u32,
        args: Vec<Value>,
    },
    Resume {
        k: Value,
        arg: Value,
    },
}
```

`call` を次にする (Task 7 の仮の腕を置き換える)。

```rust
    fn call(&mut self, call: &Call, resume: Option<Resume<'p>>) -> Result<Step, Fault> {
        let prepared = match call {
            Call::Direct(callee, args) => Prepared::Direct(*callee, self.atoms(args)?),
            Call::Apply(callee, args) => {
                let callee = self.atom(callee)?;
                Prepared::Apply(callee, self.atoms(args)?)
            }
            Call::Handle {
                effect,
                body,
                clauses,
                ret,
            } => Prepared::Handle {
                effect: *effect,
                body: self.atom(body)?,
                clauses: self.atoms(clauses)?,
                ret: ret.map(|ret| self.atom(&ret)).transpose()?,
            },
            Call::Perform { effect, op, args } => Prepared::Perform {
                effect: *effect,
                op: *op,
                args: self.atoms(args)?,
            },
            Call::Resume { k, arg } => Prepared::Resume {
                k: self.atom(k)?,
                arg: self.atom(arg)?,
            },
        };
        if let Some(resume) = resume {
            self.push_frame(resume)?;
        }
        match prepared {
            Prepared::Direct(callee, args) => {
                self.enter(callee, args);
                Ok(Step::Continue)
            }
            Prepared::Apply(callee, args) => self.apply_and_continue(callee, args),
            Prepared::Handle {
                effect,
                body,
                clauses,
                ret,
            } => {
                let frame = Frame::Handler {
                    effect,
                    clauses,
                    ret,
                    next: Some(self.cont),
                };
                self.cont = self.heap.alloc(Payload::Frame(frame));
                self.apply_and_continue(body, vec![Value::Unit])
            }
            Prepared::Perform { effect, op, args } => self.perform(effect, op, args),
            Prepared::Resume { k, arg } => self.resume(k, arg),
        }
    }

    /// 関数値を適用する。関数に入らずに値ができたら (足りない引数のクロージャ)、その値を継続に返す。
    fn apply_and_continue(&mut self, callee: Value, args: Vec<Value>) -> Result<Step, Fault> {
        match self.apply(callee, args)? {
            Applied::Entered => Ok(Step::Continue),
            Applied::Value(value) => self.ret(value),
        }
    }

    /// 継続の連結リストを先頭から読み、同じエフェクトの一番内側の handler フレームを探す (docs/spec/core-ir.md)。
    fn find_handler(&self, effect: u32) -> Result<ObjRef, Fault> {
        let mut current = self.cont;
        loop {
            let Payload::Frame(frame) = self.heap.get(current).map_err(Fault::Heap)? else {
                return Err(Fault::Internal("the continuation is not a frame"));
            };
            current = match frame {
                Frame::Handler { effect: other, .. } if *other == effect => return Ok(current),
                Frame::Handler { next, .. } => {
                    next.ok_or(Fault::Internal("a detached handler is in the continuation"))?
                }
                Frame::Return { next, .. } | Frame::Apply { next, .. } => *next,
                Frame::Io => return Err(Fault::Internal("an operation without a handler")),
            };
        }
    }

    /// handler フレームの外側を切り離して機械の継続に戻し、節を呼ぶ。先頭から handler フレームまでの区間が継続で、
    /// `once` の操作はそれを継続オブジェクトにして `k` として渡す。`never` の操作は再開しないので、区間をここで
    /// 解放する。区間のフレームが退避した値も、子をたどる解放で1回ずつ解放される (docs/spec/core-ir.md)。
    fn perform(&mut self, effect: u32, op: u32, mut args: Vec<Value>) -> Result<Step, Fault> {
        let handler = self.find_handler(effect)?;
        let Payload::Frame(Frame::Handler { clauses, next, .. }) =
            self.heap.get_mut(handler).map_err(Fault::Heap)?
        else {
            return Err(Fault::Internal("a handler that is not a handler frame"));
        };
        let clause = *clauses
            .get(op as usize)
            .ok_or(Fault::Internal("an operation without a clause"))?;
        let outside = next
            .take()
            .ok_or(Fault::Internal("performing through a detached handler"))?;
        // 節のクロージャは handler フレームにも残るので、呼ぶ分の参照を足す
        if let Value::Obj(obj) = clause {
            self.heap.dup(obj).map_err(Fault::Heap)?;
        }
        let top = std::mem::replace(&mut self.cont, outside);
        let resumable = self
            .program
            .effects
            .get(effect as usize)
            .and_then(|info| info.operations.get(op as usize))
            .ok_or(Fault::Internal("an unknown operation"))?
            .resumable;
        if resumable {
            let k = self.heap.alloc(Payload::Continuation { top, handler });
            args.push(Value::Obj(k));
        } else {
            self.heap.decref(top).map_err(Fault::Heap)?;
        }
        self.apply_and_continue(clause, args)
    }

    /// 継続オブジェクトの handler フレームの外側に今の継続をつなぎ、先頭のフレームに値を返す。末尾でない `resume`
    /// では、その前に呼び出しのフレームが積まれている。`once` の継続は一意なので、取り出して書き換えてよい。
    fn resume(&mut self, k: Value, value: Value) -> Result<Step, Fault> {
        let Value::Obj(obj) = k else {
            return Err(Fault::Internal("resuming a value that is not a continuation"));
        };
        let Payload::Continuation { top, handler } = self.heap.take(obj).map_err(Fault::Heap)?
        else {
            return Err(Fault::Internal("resuming an object that is not a continuation"));
        };
        let current = self.cont;
        match self.heap.get_mut(handler).map_err(Fault::Heap)? {
            Payload::Frame(Frame::Handler { next, .. }) => *next = Some(current),
            _ => {
                return Err(Fault::Internal(
                    "a continuation whose handler is not a handler frame",
                ));
            }
        }
        self.cont = top;
        self.ret(value)
    }
```

`ret` の `match frame` に腕を足す。

```rust
                // 本体が値を返したので handler を外す。節のクロージャはもう呼ばない
                Frame::Handler {
                    clauses,
                    ret: on_return,
                    next,
                    ..
                } => {
                    for clause in clauses {
                        if let Value::Obj(obj) = clause {
                            self.heap.decref(obj).map_err(Fault::Heap)?;
                        }
                    }
                    self.cont =
                        next.ok_or(Fault::Internal("a detached handler received a value"))?;
                    if let Some(on_return) = on_return {
                        match self.apply(on_return, vec![value])? {
                            Applied::Entered => return Ok(Step::Continue),
                            Applied::Value(result) => value = result,
                        }
                    }
                }
```

`ret` の先頭のコメント「段階2までは継続を複製しないので、フレームは常に一意である。共有されたフレームは段階3の `multi` で扱う」を「`once` の継続までは継続を複製しないので、フレームは常に一意である。共有されたフレームは段階3b の `multi` で扱う」にする。

- [ ] **Step 5: UI テストを実行し、出力を確かめて受け入れる**

Run: `cargo test -p eml_cli --test ui run`
Expected: 新しいスナップショットの `.snap.new` ができて FAIL する。`cat crates/eml_cli/tests/snapshots/ui__run@effect_*.snap.new crates/eml_cli/tests/snapshots/ui__run@continuation_values.em.snap.new crates/eml_cli/tests/snapshots/ui__run@operation_values.em.snap.new` で、stdout が次のとおりで、stderr が空であることを確かめる。

| ファイル | stdout |
|---|---|
| `effect_abort.em` | `value: 3`、`error: not positive`、`8`、`aborted: too big` |
| `effect_resume.em` | `hello name! from place!`、`asked place`、`asked name`、`[hello name from place]` |
| `effect_deep.em` | `tick`、`tick`、`tick`、`3`、`201`、`15`、`log: before`、`7` |
| `effect_drop_k.em` | `apple`、`dropped which` |
| `continuation_values.em` | `40`、`14` |
| `operation_values.em` | `note: first`、`note: second`、`info: first`、`info: second` |
| `effect_loop.em` | `total: 250000` |

一致したら受け入れる。

```bash
cargo insta accept
cargo test -p eml_cli --test ui
```

Expected: PASS。実行の結果が `Ok(())` でないファイルがあれば、`assert_eq!(result, Ok(()))` で失敗する (リークや解放済みアクセスを含む)

- [ ] **Step 6: 全体を確かめてコミットする**

```bash
cargo test && cargo clippy --all-targets && cargo fmt
git add crates tests
git commit -m "Run handlers with handler frames and continuation objects"
```

---
### Task 10: `check-fail/` の UI テストを足す

HIR と型検査の診断を、`eml check` の表示で確かめる。Task 2〜6 で実装済みなので、ここではテストを足してスナップショットを確かめるだけである。

**Files:**
- Create: `tests/ui/check-fail/handler_clauses.em`、`operation_declarations.em`、`handle_io.em`、`resume_and_drop_arity.em`、`continuation_misuse.em`、`unhandled_effect.em`、`later_stage_effects.em`
- Create: `crates/eml_cli/tests/snapshots/ui__check_fail@*.em.snap` (上のファイルごとに、`cargo insta accept` で作る)

**Interfaces:**
- Consumes: Task 2〜6 の診断

- [ ] **Step 1: テストのファイルを作る**

`tests/ui/check-fail/handler_clauses.em`:

```haskell
-- E1010, E1012, E1013, E1014: a clause with the wrong number of parameters, clauses of two effects, a missing
-- clause, duplicate clauses, and a handler without operation clauses are reported in one run.
effect Ask where
  ask : String -> String
  ask_twice : String -> String

effect Log where
  log : String -> Unit

effect Fail where
  never fail : String -> a

run : Unit -> <Ask, Log, Fail> String
run () = ask "x"

arity : Unit -> <Log, Fail> String
arity () =
  handle run () with
    | ask key -> key
    | ask_twice key k -> resume k key

mixed : Unit -> <Ask, Fail> String
mixed () =
  handle run () with
    | log message k -> resume k ()
    | ask key k -> resume k key

missing : Unit -> <Log, Fail> String
missing () =
  handle run () with
    | ask key k -> resume k key

duplicate : Unit -> <Log, Fail> String
duplicate () =
  handle run () with
    | ask key k -> resume k key
    | ask_twice key k -> resume k key
    | ask key k -> resume k "again"
    | return x -> x
    | return y -> y

never_with_k : Unit -> <Ask, Log> String
never_with_k () =
  handle run () with
    | fail message k -> message

only_return : Unit -> <Ask, Log, Fail> String
only_return () =
  handle run () with
    | return x -> x
```

`tests/ui/check-fail/operation_declarations.em`:

```haskell
-- E1007, E1008, E1003: a row on the outermost arrows of an operation, an operation that is not a function, a
-- `never` operation whose result is not a free type variable, and an operation named like a function.
effect Bad where
  with_row : Int -> <IO> Int
  constant : Int
  never stop : a -> a
  helper : Int -> Int

helper : Int -> Int
helper x = x
```

`tests/ui/check-fail/handle_io.em`:

```haskell
-- E1009: the built-in `IO` cannot be handled.
quiet : Unit -> Unit
quiet () =
  handle println "hi" with
    | println text k -> resume k ()
```

`tests/ui/check-fail/resume_and_drop_arity.em`:

```haskell
-- E1011: `resume` takes a continuation and a value, and `drop` takes one value. The continuation used only by the
-- wrong `resume` or `drop` is not reported again.
effect Ask where
  ask : Unit -> Int

one : Unit -> Int
one () =
  handle ask () with
    | ask () k -> resume k

two : Unit -> Int
two () =
  handle ask () with
    | ask () k ->
        drop k 1
        0
```

`tests/ui/check-fail/continuation_misuse.em`:

```haskell
-- E3001: the continuation of a `once` operation must be used exactly once. Resuming it twice, not using it,
-- discarding it with `_`, and capturing it in an operation clause, which may run more than once, are errors.
effect Ask where
  ask : Unit -> Int

twice : Unit -> Int
twice () =
  handle ask () with
    | ask () k -> resume k 1 + resume k 2

unused : Unit -> Int
unused () =
  handle ask () with
    | ask () k -> 0

discarded : Unit -> Int
discarded () =
  handle ask () with
    | ask () _ -> 0

captured : Unit -> <Ask> Int
captured () =
  handle ask () with
    | ask () k ->
        handle ask () with
          | ask () inner -> resume k (resume inner 1)
```

`tests/ui/check-fail/unhandled_effect.em`:

```haskell
-- E2002: `main` performs a user-defined effect that no handler handles.
effect Ask where
  ask : Unit -> Int

main : Unit -> <IO> Unit
main () = println (show_int (ask ()))
```

`tests/ui/check-fail/later_stage_effects.em`:

```haskell
-- E0004: `multi` operations, effect type parameters, effects with type arguments, and handlers with `from` come
-- in later stages.
effect Choice where
  multi choose : Unit -> Bool

effect State s where
  get : Unit -> s

counter : Unit -> <State Int> Int
counter () = 0

with_state : Int -> Int
with_state n =
  handle counter () from n with
    | return x st -> x
```

- [ ] **Step 2: UI テストを実行し、診断を確かめて受け入れる**

Run: `cargo test -p eml_cli --test ui check_fail`
Expected: 新しいスナップショットの `.snap.new` ができて FAIL する。各 `.snap.new` に、次の診断がこの順で、これだけ出ていることを確かめる (位置は `ファイル:行:列`)。

| ファイル | 診断 |
|---|---|
| `handler_clauses.em` | E1010 19:7、E1012 26:7、E1013 30:3、E1014 38:7、E1014 40:5、E1010 45:7、E1013 49:3 |
| `operation_declarations.em` | E1007 4:21、E1007 5:14、E1008 6:21、E1003 9:1 |
| `handle_io.em` | E1009 5:7 |
| `resume_and_drop_arity.em` | E1011 9:19、E1011 15:9 |
| `continuation_misuse.em` | E3001 9:14、E3001 14:14、E3001 19:14、E3001 24:14 |
| `unhandled_effect.em` | E2002 6:30 |
| `later_stage_effects.em` | E0004 4:3、E0004 6:14、E0004 9:20、E0004 14:21 |

一致したら受け入れる。

```bash
cargo insta accept
cargo test -p eml_cli --test ui
```

Expected: PASS

- [ ] **Step 3: 全体を確かめてコミットする**

```bash
cargo test && cargo clippy --all-targets && cargo fmt
git add crates tests
git commit -m "Add UI tests for the diagnostics of effects and handlers"
```

---

### Task 11: 文書を更新する

spec の「文書」の表に従って、`docs/spec/` と `docs/implementation/` を更新する。日本語を書く前に `yomiyasu:yomiyasu` スキルを読む。

**Files:**
- Modify: `docs/spec/effects.md`、`expressions.md`、`core-ir.md`、`linearity.md`、`runtime.md`、`diagnostics.md`
- Modify: `docs/implementation/architecture.md`、`docs/implementation/status.md`

- [ ] **Step 1: `docs/spec/effects.md` の「handler の意味」に足す**

箇条書きの「継続は `resume k v` で再開し、`drop k` で捨てる。」の後に足す。

```markdown
- 1つの handler は1つのエフェクトを扱い、そのエフェクトのすべての操作に節を書く。複数のエフェクトは handler を入れ子にして扱う。別のエフェクトの節が混ざったら E1012、節のない操作があれば E1013、同じ操作の節が2つあれば E1014 にする ([診断](diagnostics.md))。
- `k` は第一級の値である。変数に束縛し、関数に渡し、クロージャで捕まえられる。再開は `resume`、破棄は `drop` だけで行う。`k` の型 (継続の型) は表面の構文に名前を持たず、シグネチャには書けない。診断では `Cont Int Unit <IO>` のように、操作の結果の型、handle の結果の型、handle の外側の row の順に表示する。
- 操作の節は、handler が生きている間、操作が起きるたびに呼ばれる。そのため、節が捕まえる変数には `Unr` の制約が付く。handle の本体と `return` の節は高々1回しか動かないので、`Lin` の値も捕まえられる ([線形性](linearity.md))。
```

- [ ] **Step 2: `docs/spec/expressions.md` の「handler」を直す**

「節の引数の個数は、... 個数は HIR で検査する (E1xxx)」の `(E1xxx)` を `(E1010)` にする。「節の先頭の名前は、エフェクトの操作だけから解決する ([モジュールと名前解決](modules.md))」の後に `。操作でない名前は E1001 に、組み込みの \`IO\` の操作は E1009 にする` を足して1文にする。箇条書きの最後に「1つの handler は1つのエフェクトのすべての操作に節を書く ([エフェクトと handler](effects.md) の「handler の意味」)」を足す。「パラメータ付き handler」の「`resume` の引数の個数 (2 または 3) は HIR で検査する (E1xxx)」の `(E1xxx)` を `(E1011)` にする。

- [ ] **Step 3: `docs/spec/core-ir.md` を直す**

「Core IR」の箇条書きの「perform し得る各呼び出しに、線形性の検査パスが出したクリーンアップ情報 (その時点で生きている `Lin` 変数の集合) を付ける ([線形性](linearity.md))。」を、次の2項目に置き換える。

```markdown
- `handle`、`perform`、`resume` は呼び出しの一種である。呼び出しと同じく後で使う変数を退避し、末尾の位置ではフレームを積まない。
- handle の本体、操作の節、`return` の節は、ラムダと同じく、捕まえた変数を先頭の引数に持つ関数に持ち上げ、そのクロージャを `handle` に渡す。本体の関数は `()` を受ける。節はエフェクトの操作の順に並べる。操作を値として使うときは、`perform` を呼ぶだけの関数で包む。
```

「インタプリタ (CEK 機械)」の「`perform` は、連結リストを遡って対応する handler を探す。...」と「`drop k` は、継続の各フレームのクリーンアップ情報に従って、...」の2項目を、次の4項目に置き換える。

```markdown
- `handle` は、節のクロージャを持つ handler フレームを積み、本体のクロージャに `()` を適用する。本体の値が handler フレームに届いたら、フレームを外し、`return` の節があれば値に適用する。
- `perform` は、連結リストを遡って、同じエフェクトの一番内側の handler フレームを探す。先頭からその handler フレームまでの区間が継続になる。handler フレームの次 (handle の外側) を切り離して機械の継続に戻し、節を呼ぶ。`once` の操作は区間を継続オブジェクトにして `k` として渡し、`never` の操作は区間をその場で解放する。
- `resume k v` は、継続オブジェクトの handler フレームの次に今の継続をつなぎ、区間の先頭に `v` を返す。`once` の区間のフレームは一意なので、つなぎ直しは書き換え1回で済む。一意なオブジェクトの書き換えは観測できないので、フレームをイミュータブルとして扱う前提と両立する (Perceus の reuse と同じ理屈)。`multi` の再開 (段階3b) は区間を写す。
- `drop k` と `never` の操作による中断は、継続の区間を解放する。フレームは退避した値だけを所有するので、子をたどる解放が、捕まっていた値を1回ずつ解放する。`Lin` の値の破棄処理は、記述子に従ってこの解放の中で呼ぶ (段階5)。
```

- [ ] **Step 4: `docs/spec/linearity.md` を直す**

「線形性の検査パス」の「出力は、perform し得る各呼び出しについての ... Core IR に付く ([Core IR とインタプリタ](core-ir.md))。」を消す。中断時の後始末はクリーンアップ情報を使わず、継続の区間の解放で行うためである ([Core IR とインタプリタ](core-ir.md))。

「基本の規則」の「ラムダが変数を捕まえることは、...」の後に足す。

```markdown
- handle の本体と `return` の節が変数を捕まえることも、handle 式の位置での1回の使用に数える。操作の節は操作が起きるたびに呼ばれるので、節が捕まえる変数の Kind には、使用の回数によらず `Unr` の制約を加える ([エフェクトと handler](effects.md) の「handler の意味」)。
```

「線形性の検査パス」の最後の「違反は E3xxx の診断として報告する。」の前に、次の1文を足す。

```markdown
段階3a では、使用回数のパスが出した制約と Kind の制約の違反を E3001 として報告する。違反した制約の由来 (変数の束縛、`_`、操作の節の捕獲、スキームの具体化、型の単一化) を指す。[診断](diagnostics.md) の「線形性の診断」の表どおりの指し方は、線形性の検査パスを分ける段階5で入れる。
```

- [ ] **Step 5: `docs/spec/runtime.md` に足す**

「ランタイムの API」の「継続のフレームと環境も、同じ RC で管理するランタイムのオブジェクトにする。」の後に足す。

```markdown
- 継続オブジェクトは、`perform` で捕まえた区間の先頭のフレームを所有し、区間の最後の handler フレームを所有せずに指す。handler フレームの次 (handle の外側) は、捕まえられている間は空である。そのため、継続を解放すると、子をたどる解放は handler フレームで止まる ([Core IR とインタプリタ](core-ir.md))。
```

- [ ] **Step 6: `docs/spec/diagnostics.md` を直す**

「割り当て済みの番号」の前文の「E3xxx と E4xxx の番号は、線形性と網羅性の検査を実装するときに割り当てる。」を「E3001 は `eml_types::codes` に置く。E3xxx の残りと E4xxx の番号は、線形性と網羅性の検査を実装するときに割り当てる。」にする。表の E1006 の行の後と E2005 の行の後に足す。

```markdown
| E1007 | `INVALID_OPERATION_SIGNATURE` | 操作のシグネチャの一番外側の `->` に row を書いた。または、シグネチャが関数型でない |
| E1008 | `NEVER_RESULT_NOT_FREE` | `never` の操作の結果の型が、引数に現れない型変数でない |
| E1009 | `UNHANDLEABLE_EFFECT` | handler に組み込みの `IO` の操作の節を書いた |
| E1010 | `CLAUSE_ARITY` | handler の節の引数の個数の誤り |
| E1011 | `KEYWORD_ARITY` | `resume` と `drop` の引数の個数の誤り |
| E1012 | `MIXED_EFFECTS_IN_HANDLER` | 1つの handler に別のエフェクトの操作の節が混ざった |
| E1013 | `MISSING_CLAUSE` | 節のない操作がある。操作の節が1つもない handler も含む。primary は `handle` で、節の追加を help で示す |
| E1014 | `DUPLICATE_CLAUSE` | 同じ操作の節、または `return` の節が2つある |
```

```markdown
| E3001 | `LINEAR_VALUE_MISUSED` | 線形な値 (`once` の操作の `k` と、それを捕まえたクロージャ) を、ちょうど1回でなく使った。違反した Kind の制約の由来を指す |
```

「番号を割り当てていない診断」の E1xxx の行から「、handler の節の引数の個数、`resume` の引数の個数」を消す。

- [ ] **Step 7: `docs/implementation/architecture.md` を直す**

「`eml_hir` の内部」を次のように直す。

- 「型とエフェクトは item ... 今は組み込みの `Int`、`String`、`Bool`、`Unit` と `IO` だけを、変換のはじめに登録する。」を「型とエフェクトは item (`Module::types`、`Module::effects`) で、ID (`TypeDefId`、`EffectId`) で参照する。組み込みの `Int`、`String`、`Bool`、`Unit` と `IO` を変換のはじめに登録し、続けて `effect` の宣言を変換する (`lower/effect.rs`)。エフェクトの名前をすべて登録してから操作を変換するので、操作の引数の型の row は後ろで宣言したエフェクトも引ける。操作は `Module::operations` (`OperationId`) に置き、シグネチャと引数の個数 (外側の矢印の数) を持つ」にする。
- 「名前は `Res::{Local, Function, Builtin}` に解決する。」を「名前は `Res::{Local, Function, Operation, Builtin}` に解決する。」にする。
- 「`Body` は走査関数を持つ。...」の項目を「`Body` は走査関数を持つ。`walk_child_exprs` は式の直接の子を辿り、`pat_bindings` はパターンが束縛する変数を、`captures` は式の中で束縛していない変数 (ラムダ、handle の本体と節が捕まえる変数) を返す。段階4で式やパターンの種類を足すときは、これらを直す」にする。
- 項目を足す。「handler の変換と節の検査は `lower/handler.rs` にある。節の先頭の名前は `ItemScope::operation` で操作だけから引く。誤った節 (引数の個数の誤り、重複、別のエフェクトの節) は診断を出して `ExprKind::Handle::clauses` に入れず、扱うエフェクトが決まらなければ `effect` を `None` にする。型検査はそれを見て診断を連鎖させない」

「`eml_types` の内部」に項目を足す。

```markdown
- 継続の型は `TyShape::Cont` (操作の結果の型、継続の線形性、handle の外側の row、handle の結果の型) で、外に出す型は `Type::Cont` である。`once` の操作の `k` の線形性は `Lin` である
- 操作のスキームは、組み込みと同じ経路で作る。シグネチャの外側の最後の矢印に、操作のエフェクトだけの row を付ける (`scheme::lower_operation`)。エフェクトの多重度は操作の多重度の最大である
- handle の検査は `check/handle.rs` にある。本体は今の row の前に扱うエフェクトを足した row で、節は今の row で検査する。節では操作の型変数を新しい rigid 変数にする。`resume` は、推論用の変数でできた継続の型と単一化してから、関数の呼び出しと同じく row を今の row に含める
- Kind の制約は由来 (`KindOrigin`) を持つ。型の表が「今の由来」を持ち、制約を作るときに記録する。本体の検査は単一化と参照の具体化の前後で、使用回数のパスは `Unr` の制約の前後で、今の由来を設定する。`solve_kinds` は破れた制約の由来を返し、`check` が E3001 にする。報告済みの誤りの跡 (`Missing`) がある本体では、使用回数のパスは由来を記録しない
```

「`eml_core_ir`、`eml_runtime`、`eml_interp` の内部」に項目を足す。

```markdown
- handle の本体と節は、ラムダと同じ `lift` で持ち上げる (`外側の名前$handleN`、`外側の名前$handleN$操作名`、`外側の名前$handleN$return`)。操作を値として使うときは、`perform` を呼ぶだけの関数 (`op$名前`) で包む。`Program::effects` はエフェクトごとの操作の表 (名前と、再開できるか) で、番号は `EffectId` の添字である
- `Call::{Handle, Perform, Resume}` は呼び出しの一種なので、`saved` と末尾の位置の扱い、Perceus と verifier の規則を、ほかの呼び出しと共有する。`IO` の操作は `Rhs::Io` で、その場で実行する
- handler は `Frame::Handler` (エフェクトの番号、節のクロージャ、`return` の節、次のフレーム) で、継続は `Payload::Continuation` (区間の先頭のフレームと、所有しない handler フレームへの参照) である。`perform` は handler フレームの次を切り離し、`resume` はそこに今の継続をつなぐ。`never` の操作は区間をその場で解放する
```

- [ ] **Step 8: `docs/implementation/status.md` を直す**

1. 冒頭の日付は今日の日付のままにする。
2. 「名前解決以降の実装段階」の表の段階3の行を、次の2行に置き換える。

```markdown
| 3a | `effect` (型引数なし、`never` / `once`)、`handle` (deep handler)、`resume`、`drop` (`drop k` を含む)、操作の値と部分適用、Kind の制約の違反の E3001 | 完了 |
| 3b | `multi` と multi-shot の再開、エフェクトの型引数 | 未着手 |
```

3. 「各 crate の実装状況」の `eml_hir`、`eml_types`、`eml_core_ir`、`eml_runtime`、`eml_interp` の行の「段階2まで実装済み」を「段階3a まで実装済み」にし、それぞれの行の最後に次を足す。

| crate | 足す文 |
|---|---|
| `eml_hir` | `effect` の宣言と操作の item、handler、`resume`、`drop`、E1007〜E1014 |
| `eml_types` | 継続の型、操作のスキーム、handle の検査、Kind の制約の由来と E3001 |
| `eml_core_ir` | handle の本体と節の持ち上げ、操作を包む関数、`Call::{Handle, Perform, Resume}`、`Rhs::Drop` |
| `eml_runtime` | handler のフレーム (`Frame::Handler`) と継続オブジェクト (`Payload::Continuation`) |
| `eml_interp` | handle、`perform`、`resume`、`drop` の実行 |

4. 「次の作業の注意点」を次のように直す。
   - 「段階3と4: HIR の `Module` に、エフェクトと `data` の item を足し、...」の項目を「段階4: HIR の `Module` に `data` の item を足し、宣言の型変数の表を `Generics` として持たせる。コンストラクタは値の名前空間 (`ItemScope`) に置く。型構成子と row のラベルは ID だけを持ち、型の引数の置き場所はまだない。引数を持つ `data` は段階4で、引数を持つ `effect` は段階3b で、引数の単一化と一緒に入れる」に置き換える。
   - 「段階3の `drop k` は、...」の項目を消す (段階3a で実装し、文書に移した)。
   - 「段階3の注意: multi-shot の `resume` では、...」の項目を「段階3b: multi-shot の `resume` では、継続の区間のフレームを `Heap::take_or_copy` で写し、写した区間の最後の handler フレームの次に今の継続をつなぐ。写すときと解放するときで、数える子は `children` で一致させてある。`Payload::Continuation` の `copy` は今は参照をそのまま写すだけなので、区間を写す処理を足す」に置き換える。
   - 「段階3: row 変数の多重度 `σ` は、...」の項目の「段階3:」を「段階3b と5:」にする。
   - 「段階5: Kind の制約の違反は、段階2では `debug_assert` だけで、...」の項目を「段階5: Kind の制約の違反は、段階3a で E3001 にした。指し方は違反した制約の由来 (変数の束縛など) で、[診断](../spec/diagnostics.md) の「線形性の診断」の表どおりの指し方 (二重使用の2か所、扱い忘れの節など) は、線形性の検査パスを分けるときに入れる。由来のない制約が破れても診断を出さない。報告済みの誤りのある本体の制約だけが由来を持たないことを、検査パスを分けるときに確かめ直す」に置き換える。
   - 「段階5: Core IR の `VarInfo::linearity` は、今はつねに `Unr` である。」の項目の後に、「段階5: 操作の節のクロージャは、操作を起こすたびに handler フレームから複製して呼ぶ。節が捕まえる変数には `Unr` の制約が付くので、複製は正しい。`File` が入ったら、handler フレームの解放で `Lin` の値の破棄処理を呼ぶことを確かめる」を足す。
5. 「spec に反映済みで、実装は後の段階で扱うもの」から、段階3で実装した次の3項目を消す。「操作のカリー化に合わせた handler の節の引数の個数」、「操作名の重複は、...」、「`resume` の引数の個数の診断は E1xxx とする。...」。3つ目の項目のうち、パラメータ付き handler の3引数の形は段階6で加えることを、「パラメータ付き handler の `resume` の3引数の形と `return` の節の状態の引数は段階6で加える。今は E0004 と E1010 にする」として残す。
6. 「完了した作業」の表の最後に行を足す。

```markdown
| 縦の貫通 段階3a | ユーザー定義のエフェクトの `never` と `once` の操作、deep handler、`resume`、`drop` を通した。handle の本体と節をクロージャに持ち上げ、`perform` で handler フレームの外側を切り離し、`resume` でつなぎ直す。`drop k` は継続の区間の解放で済ませ、クリーンアップ情報をなくした。Kind の制約に由来を持たせ、違反を E3001 にした |
```

- [ ] **Step 9: 文書を確かめてコミットする**

`yomiyasu` のリンター (`python3 <スキルの配置先>/scripts/yomiyasu_lint.py <ファイル>`) を、書き足した文書にかける。英単語の前後の半角空白の指摘は、リポジトリの文書の書き方なので直さない。

```bash
git add docs
git commit -m "Document stage 3a in the specs, architecture, and status"
```

---

### Task 12: 仕上げの確認

- [ ] **Step 1: 仮の分岐が残っていないことを確かめる**

```bash
grep -rn "not lowered to Core IR yet\|effects are not run yet" crates
```

Expected: 何も出ない

- [ ] **Step 2: 全体のテスト、clippy、fmt を確かめる**

```bash
cargo test
cargo clippy --all-targets
cargo fmt --check
```

Expected: どれも成功する。clippy の警告がない

- [ ] **Step 3: spec の成功の条件を確かめる**

`docs/superpowers/specs/2026-10-04-stage-3a-effects-design.md` の「成功の条件」を1項目ずつ確かめる。

- `effect`、操作の呼び出し、`handle`、`resume`、`drop` が E0004 にならないこと。`grep -rn "are not supported yet" crates/eml_hir/src` の結果に、`effect` の宣言、handler、`resume`、`drop` の文言がなく、`from`、`multi`、型引数、`resume` の3引数の形だけが残っていれば満たす
- `debug_heap` を有効にした `run/` の UI テストが通る (Task 9)
- E1007〜E1014 と E3001 が UI テストで報告される (Task 10)。Kind の制約の違反で debug ビルドが panic しない (`a kind constraint was violated without linear types` の `debug_assert` がない)
- 変わったテストが spec の「変わるテスト」の表のものだけであること。`git diff main --stat -- 'crates/*/tests' 'crates/*/src/**/tests.rs'` と、`src` の中の `#[cfg(test)]` の変更を見て確かめる

- [ ] **Step 4: 作業の文書を消す準備をする**

spec の「位置づけ」のとおり、残す価値のある内容は Task 11 で `docs/spec/` と `docs/implementation/` に移した。作業の spec と計画の削除は、ブランチを統合するときに行う (`superpowers:finishing-a-development-branch`)。

## 完了後の後始末

ブランチを統合した後、`docs/superpowers/specs/2026-10-04-stage-3a-effects-design.md` と、この計画 `docs/superpowers/plans/2026-10-04-stage-3a-effects.md` を削除し、「Remove the work documents of stage 3a」としてコミットする。内容は git の履歴で参照する。
