# リファクタリング R7c 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 型検査の出力を宣言ごとの結果 (`TypedProgram` と `DeclType`) にそろえ、Kind の制約の由来にファイルを持たせ、入口の関数を `eml_core_ir::lower` の引数にする。

**Architecture:** `eml_hir::Program::main()` が入口のモジュールの `main` を返し、`eml_types` (E2004)、`eml_cli` (E2003 と `lower` の入口)、`eml_test_support` がこれを使う。`eml_types::check` は `TypedProgram { decls: HashMap<Decl, DeclType>, bodies }` を返し、`DeclType` は書き出した `Type` と、crate の外から読めない `Shape` と `KindScheme` を持つ。表示用の Kind の制約は `dump` の中で作る。`KindOrigin`、`CarriedInner`、`Provenance::Unattributed` の位置を `Span` (ファイルと範囲) にし、報告は入口のファイルの決め打ちをやめる。観測できるふるまいは変えない。

**Tech Stack:** Rust (edition 2024)、`insta`、`cargo test`。

**Spec:** `docs/superpowers/specs/2026-10-06-refactor-r7-design.md` の 1.1 (段階と入口)、1.4 (`main`)、5章 (R7c 型検査の出力)、7.1 の R7c の行、7.4 (完了の条件)。

## Global Constraints

- 互換性は気にしない。後方互換のための分岐や別名は作らない (CLAUDE.md)
- コードのコメントと `docs/` は日本語で書く。コメントは「なぜ」を書き、`docs/` の規則を指すときはパスを書く。日本語を書くときは `yomiyasu:yomiyasu` スキルの規則に従う (CLAUDE.md)
- 設計をテストに合わせて曲げない。テストの変更は種類1 (振る舞い)、種類2 (内部表現のスナップショット)、種類3 (期待値が同じ機械的な追随) に分ける (docs/implementation/testing.md)。R7c は観測できるふるまいを変えないので、種類1と種類2の変更はない (spec 7.1)
- 既存のテストの期待値は変えない。期待値を変えない機械的な追随 (型の名前、欄の読み方、入口の引数) だけを許す。既存のテストの期待値が変わったら、受け入れる前に作業を止めて相談する
- `TypedProgram` の `shape` と `kinds` は crate の外から読めないようにする。後の段階は `ty` だけを読む (spec 5.1)
- 各タスクの終わりに `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` を通す
- コミットのメッセージは英語で書き、末尾に次の2行を付ける:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01RSkNuf5aBtsdHwHZ92gz6y
  ```
- `git diff` は外部ツールを使う設定なので、スクリプトで差分を見るときは `git diff --no-ext-diff` を使う
- 作業中にファイルを一時的に戻すときは `git checkout <file>` を使わない。戻す前にファイルを写しておき、写しから戻す

## spec からの補い

- spec 5.1 の `Scheme` (「`dump` の中だけで使う表示の型」) は、`dump` が `ty` と Kind の制約の並びを直接書くので要らなくなる。型ごと消す。`KindConstraint`、`KindTerm`、`RowTerm` は `pub(crate)` にして、`lib.rs` の `pub use` から外す
- `dump` は表示する Kind の制約を作るのに `Context` (型の名前) が要る。`dump` の中で `Context::new(program)` を作る。`dump` はテストの表示にしか使わないので、`Context` を2回作る費用は問題にしない
- `Decl` は `kind/problem.rs` から `lib.rs` に移して公開する。`TypedProgram` のキーで、crate の外から使う型だからである
- 入口の関数の Core IR の名前は `entry$<関数の名前>` にする。`main` を渡したときは今と同じ `entry$main` になり、既存のスナップショットは変わらない
- `report_violations` は、同じ値の持ち越しの違反を1回だけ報告するために、値の範囲 (`CarriedValue::key`) を集合に入れている。別のファイルの値が同じ範囲を持ちうるので、集合のキーをファイルと範囲の組にする
- `KindReason::order_key` の `CarriedThrough` の鍵に、`inner` のファイルを含める。同じ範囲で別のファイルを指す `inner` を、違う由来として区別するためである
- spec 7.1 は、由来が Prelude の中を指すテストを R7d に回した。Prelude の本体はテストの中で Prelude のソースに足せるので (`eml_hir/tests/structure.rs` の `prelude_functions_with_equations_are_not_intrinsic` と同じやり方)、R7c で `eml_types/tests/modules.rs` に足す。`Span` の変更を確かめるテストが、ほかにないためである

## Review Focus

- Prelude (入口でないモジュール) の本体の中の線形性の誤り: primary と secondary がすべて `Prelude.em` を指す。今は入口のファイルを指してしまう (Task 4 にテストを置く)
- Prelude の多相な関数が値をまたがせ、入口のモジュールから `File` を渡した場合: primary は `test.em` の呼び出しを、「is kept alive across this call」の secondary は `Prelude.em` の中を指す (Task 4 にテストを置く)
- 別のファイルで同じ範囲を持つ2つの持ち越しの違反: 両方とも報告する。範囲だけで重複を除くと1つが消える (Task 4 にテストを置く)
- Prelude にだけ `main` という関数がある場合: 入口のモジュールに `main` はないので、`Program::main()` は `None` を返す (Task 1 にテストを置く)
- `main` 以外の関数を入口として `lower` に渡した場合: その関数を呼ぶ `entry$<名前>` ができる。REPL がその回の式から作った関数を渡す使い方である (Task 2 にテストを置く)

---

## ファイルの構成

| ファイル | 変更 | タスク |
|---|---|---|
| `crates/eml_hir/src/program.rs` | `Program::main()` | 1 |
| `crates/eml_hir/tests/structure.rs` | `Program::main()` のテスト | 1 |
| `crates/eml_types/src/check/mod.rs` | `main` を `Program::main()` で引く。`TypedProgram` を組み立てる。違反の並べ方と重複の除き方にファイルを足す | 1〜4 |
| `crates/eml_types/src/lib.rs` | `TypedProgram`、`DeclType`、`Decl`。`Scheme` と `dump` を外す | 2、3 |
| `crates/eml_types/src/dump.rs` (新規) | `dump` と表示用の Kind の制約 | 3 |
| `crates/eml_types/src/kind/mod.rs`、`kind/solve.rs`、`kind/problem.rs` | `Span`、`KindOrigin::span`、`CarriedInner::span`、`Provenance::Unattributed(Span)`、`Decl` の移動 | 3、4 |
| `crates/eml_types/src/check/body.rs`、`check/report.rs`、`usage.rs`、`carry.rs`、`exhaustive.rs` | 由来にファイルを入れる、報告が由来のファイルを使う、`TypedProgram` を受け取る | 3、4 |
| `crates/eml_types/tests/check.rs` | `decls` から型を読む (種類3)。操作の型のテスト | 3 |
| `crates/eml_types/tests/modules.rs` (新規) | Prelude の本体を指す診断のテスト | 4 |
| `crates/eml_core_ir/src/pipeline.rs`、`translate/mod.rs`、`translate/program.rs` | `entry: FunctionId` の引数、`decls` から型を読む | 2、3 |
| `crates/eml_core_ir/tests/translate.rs` | 入口を渡すテスト | 2 |
| `crates/eml_cli/src/lib.rs` | `program.main()` で E2003 と `lower` の入口を決める | 2、3 |
| `crates/eml_test_support/src/lib.rs` | `core`、`core_until` が `program.main()` を渡す。`Checked::typed` の型 | 2、3 |
| `docs/implementation/*.md` | `TypedProgram`、`DeclType`、`Span`、`lower` の入口、R7c の状況 | 5 |

---

### Task 1: `Program::main()` を足す

**Files:**
- Modify: `crates/eml_hir/src/program.rs` (`impl Program`)
- Modify: `crates/eml_types/src/check/mod.rs` (`check_module` の `main` の探し方)
- Test: `crates/eml_hir/tests/structure.rs`

**Interfaces:**
- Produces: `eml_hir::Program::main(&self) -> Option<FunctionId>`。入口のモジュール (`Program::entry`) の関数のうち、名前が `main` の最初のもの

- [ ] **Step 1: テストを書く**

`crates/eml_hir/tests/structure.rs` の末尾に足す。`function_id` と `module` はこのファイルにある補助関数である。

```rust
#[test]
fn main_is_the_entry_function_named_main() {
    let program = module("f : Int\nf = 1\n\nmain : Unit -> <IO> Unit\nmain () = ()");
    let main = program.main().expect("main");
    assert_eq!(main.module, program.entry);
    assert_eq!(main, function_id(&program, "main"));
}

#[test]
fn a_program_without_main_has_no_entry_function() {
    assert_eq!(module("f : Int\nf = 1").main(), None);
}

#[test]
fn an_operation_named_main_is_not_the_entry_function() {
    assert_eq!(module("effect E where\n  main : Unit -> Unit").main(), None);
}

#[test]
fn a_main_in_the_prelude_is_not_the_entry_function() {
    // `main` は入口のモジュールからだけ探す (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 1.4)
    let mut files = eml_diagnostics::SourceFiles::new();
    let source = format!(
        "{}\nmain : Unit -> <IO> Unit\nmain () = ()\n",
        eml_hir::PRELUDE_SOURCE
    );
    let prelude = files.add(eml_hir::PRELUDE_PATH, source);
    let entry = files.add("test.em", "");
    let trees = [prelude, entry].map(|file| {
        let (parse, errors) = eml_syntax::parse(file, files.text(file));
        assert!(errors.is_empty(), "{errors:?}");
        eml_hir::item_tree(file, &parse.tree()).0
    });
    let (def_map, diagnostics) = eml_hir::def_map(&trees);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let (program, diagnostics) = eml_hir::lower(&def_map, &trees);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(program.functions().any(|(_, function)| function.name == "main"));
    assert_eq!(program.main(), None);
}
```

Run: `cargo test -p eml_hir --test structure main`
Expected: `no method named main found for struct Program` でコンパイルが通らず FAIL。

- [ ] **Step 2: `Program::main()` を書く**

`crates/eml_hir/src/program.rs` の `impl Program` の `file` の後に足す。

```rust
    /// `eml run` が実行を始める関数。入口のモジュールだけから探し、Prelude には置かない
    /// (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 1.4)。同じ名前の関数が重複したときは、最初の定義である。
    pub fn main(&self) -> Option<FunctionId> {
        self.modules[self.entry]
            .items
            .functions
            .iter()
            .find(|(_, function)| function.name == "main")
            .map(|(local, _)| ItemId::new(self.entry, local))
    }
```

- [ ] **Step 3: 型検査で使う**

`crates/eml_types/src/check/mod.rs` の `check_module` で、`main` を探している次の部分を置き換える。

```rust
    // `main` は入口のモジュールからだけ探す (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 1.4)
    let main = program
        .functions()
        .find(|(id, function)| id.module == program.entry && function.name == "main")
        .map(|(id, _)| id);
```

を、次にする。

```rust
    let main = program.main();
```

- [ ] **Step 4: テストを流す**

Run: `cargo test -p eml_hir --test structure main`
Expected: 4件とも PASS。

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて PASS。期待値の変わったテストはない。

- [ ] **Step 5: コミットする**

```bash
git add crates/eml_hir crates/eml_types
git commit -m "Look up main through the HIR program

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RSkNuf5aBtsdHwHZ92gz6y"
```

---

### Task 2: 入口の関数を `lower` の引数にする

**Files:**
- Modify: `crates/eml_core_ir/src/pipeline.rs` (`lower`、`lower_until`)
- Modify: `crates/eml_core_ir/src/translate/mod.rs` (`translate`)
- Modify: `crates/eml_core_ir/src/translate/program.rs` (`ProgramBuilder::entry`)
- Modify: `crates/eml_types/src/lib.rs` (`TypedModule::main` を消す)
- Modify: `crates/eml_types/src/check/mod.rs` (`typed_module` の `main` 引数を消す)
- Modify: `crates/eml_cli/src/lib.rs` (`Session::compile`)
- Modify: `crates/eml_test_support/src/lib.rs` (`core`、`core_until`)
- Test: `crates/eml_core_ir/tests/translate.rs`

**Interfaces:**
- Consumes: `eml_hir::Program::main()` (Task 1)
- Produces: `eml_core_ir::lower(hir: &HirProgram, typed: &TypedModule, entry: FunctionId) -> Program` と `eml_core_ir::lower_until(hir: &HirProgram, typed: &TypedModule, entry: FunctionId, last: Pass) -> Program`。`TypedModule` は `main` の欄を持たない

- [ ] **Step 1: テストを書く**

`crates/eml_core_ir/tests/translate.rs` の `hello_world` の後に足す。

```rust
#[test]
fn the_entry_function_is_chosen_by_the_caller() {
    // REPL は、`main` の代わりにその回の式から作った関数を入口にする
    // (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 5.3)
    let text = "main : Unit -> <IO> Unit\nmain () = println \"main\"\n\nalt : Unit -> <IO> Unit\nalt () = println \"alt\"";
    let checked = eml_test_support::check(text);
    let alt = checked
        .program
        .functions()
        .find(|(_, function)| function.name == "alt")
        .map(|(id, _)| id)
        .expect("alt");
    let program = eml_core_ir::lower_until(&checked.program, &checked.typed, alt, Pass::Translate);
    insta::assert_snapshot!(eml_core_ir::pretty(&program), @r#"
    fn main(p0) {
      let s1^ = const "main"
      let t2 = perform println(s1)
      return t2
    }
    fn alt(p0) {
      let s1^ = const "alt"
      let t2 = perform println(s1)
      return t2
    }
    fn entry$alt() {
      tailcall alt(())
    }
    "#);
}
```

Run: `cargo test -p eml_core_ir --test translate the_entry_function`
Expected: `lower_until` の引数の数が合わずコンパイルが通らず FAIL。

- [ ] **Step 2: `eml_core_ir` に入口の引数を足す**

`crates/eml_core_ir/src/pipeline.rs` の2つの関数を次にする (doc コメントは今のものを残す。`FunctionId` は `eml_hir` から `use` する)。

```rust
pub fn lower(hir: &HirProgram, typed: &TypedModule, entry: FunctionId) -> Program {
    lower_until(hir, typed, entry, Pass::Perceus)
}

pub fn lower_until(hir: &HirProgram, typed: &TypedModule, entry: FunctionId, last: Pass) -> Program {
    let mut program = translate::translate(hir, typed, entry);
```

(`lower_until` の残りの本体は変えない。)

`crates/eml_core_ir/src/translate/mod.rs` の `translate` の引数を `(hir: &HirProgram, typed: &TypedModule, entry: FunctionId)` にし、`main` を引いていた末尾を次にする。

```rust
    let entry_type = &typed
        .signatures
        .get(entry)
        .expect("the entry function has a signature")
        .ty;
    let entry = builder.entry(hir, indices[entry], entry, entry_type);
```

`crates/eml_core_ir/src/translate/program.rs` の `ProgramBuilder::entry` を次にする。関数の名前を入口の関数の名前から作るので、HIR の ID も受け取る。

```rust
    /// 入口の関数を `()` で呼ぶ関数を作る。名前は `entry$` に入口の関数の名前を続ける。
    pub(super) fn entry(
        &mut self,
        hir: &HirProgram,
        target: FnIdx,
        target_id: FunctionId,
        target_type: &Type,
    ) -> FnIdx {
        let function = self.reserve(0);
        let unit = vec![Atom::Unit];
        let mut builder = FnBuilder::new();
        let body = if self.arity(target) == 0 {
            let value = builder.var(var_info("f", target_type, hir));
            let apply = builder.push(CExpr::TailCall(Call::Apply(Atom::Var(value), unit)));
            builder.push(CExpr::Let {
                var: value,
                rhs: Rhs::call(Call::Direct(target, Vec::new())),
                body: apply,
            })
        } else {
            builder.push(CExpr::TailCall(Call::Direct(target, unit)))
        };
        let core = builder.finish(format!("entry${}", hir[target_id].name), Vec::new(), body);
        self.finish(function, core);
        function
    }
```

今の `entry` の doc コメントがあれば、上の1行に置き換える。

- [ ] **Step 3: `TypedModule::main` を消す**

`crates/eml_types/src/lib.rs` の `TypedModule` から `pub main: Option<FunctionId>,` を消す。`crates/eml_types/src/check/mod.rs` の `typed_module` から `main` の引数と `main,` の欄を消し、`check_module` の呼び出し (`typed_module(&context, &signatures, &schemes, bodies, main)`) から `main` を外す。`check_module` の `let main = program.main();` は、`check_main` を呼ぶためにそのまま残す。使わなくなった `use` (`FunctionId` など) は clippy の指摘に従って消す。

- [ ] **Step 4: 呼ぶ側を直す**

`crates/eml_cli/src/lib.rs` の `Session::compile` を次にする。

```rust
    pub fn compile(&self, entry: FileId) -> Compiled {
        let (program, typed, mut diagnostics) = self.front(entry);
        let main = program.main();
        // `main` がないことは実行するときだけ誤りにする。モジュール (S2) は `main` を持たないため (docs/spec/types.md)
        if main.is_none() {
            diagnostics.push(eml_types::missing_main(entry));
        }
        sort_diagnostics(&mut diagnostics);
        // Core IR は誤りのないプログラムだけを受け取る (docs/implementation/architecture.md)
        let program = match main {
            Some(main) if !has_errors(&diagnostics) => {
                Some(Arc::new(eml_core_ir::lower(&program, &typed, main)))
            }
            _ => None,
        };
        Compiled {
            diagnostics,
            program,
        }
    }
```

`crates/eml_test_support/src/lib.rs` の `core` と `core_until` を次にする。

```rust
#[cfg(feature = "core")]
pub fn core(text: &str) -> Program {
    let checked = check_without_errors(text);
    eml_core_ir::lower(&checked.program, &checked.typed, entry(&checked))
}

/// 確かめたいパスの直後の Core IR を見るテストのため (docs/implementation/testing.md)。
#[cfg(feature = "core")]
pub fn core_until(text: &str, last: Pass) -> Program {
    let checked = check_without_errors(text);
    eml_core_ir::lower_until(&checked.program, &checked.typed, entry(&checked), last)
}

/// `eml run` と同じく、入口のモジュールの `main` から実行する。
#[cfg(feature = "core")]
fn entry(checked: &Checked) -> eml_hir::FunctionId {
    checked
        .program
        .main()
        .expect("a program lowered to Core IR has `main`")
}
```

- [ ] **Step 5: テストを流す**

Run: `cargo test -p eml_core_ir --test translate the_entry_function`
Expected: PASS。スナップショットが違ったら、`entry$alt` が `alt` を呼んでいることと、`main` と `alt` の本体が `hello_world` の `main` と同じ形であることを確かめてから、インラインの期待値を実際の出力に直す (新しいテストなので、種類の記録は要らない)。

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて PASS。`git diff --no-ext-diff --stat crates/*/tests crates/eml_cli/tests/snapshots` で、Step 1 の新しいテストのほかに期待値が変わっていないことを確かめる。

- [ ] **Step 6: コミットする**

```bash
git add crates
git commit -m "Pass the entry function to Core IR lowering instead of reading main from the typed module

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RSkNuf5aBtsdHwHZ92gz6y"
```

---

### Task 3: `TypedProgram` と `DeclType` にする

**Files:**
- Modify: `crates/eml_types/src/lib.rs` (`TypedProgram`、`DeclType`、`Decl`。`TypedModule`、`Scheme`、`dump`、`write_kinds` を外す)
- Create: `crates/eml_types/src/dump.rs`
- Modify: `crates/eml_types/src/kind/problem.rs` (`Decl` を消して `crate::Decl` を使う)
- Modify: `crates/eml_types/src/kind/solve.rs`、`check/body.rs`、`check/mod.rs` (`Decl` の `use`、`typed_program`、`kind_constraints` の移動)
- Modify: `crates/eml_types/src/ty.rs` (`KindTerm`、`KindConstraint`、`RowTerm` を `pub(crate)` に)
- Modify: `crates/eml_types/src/exhaustive.rs` (`TypedProgram` を受け取る)
- Modify: `crates/eml_core_ir/src/pipeline.rs`、`translate/mod.rs`、`translate/program.rs`
- Modify: `crates/eml_cli/src/lib.rs`、`crates/eml_test_support/src/lib.rs`
- Test: `crates/eml_types/tests/check.rs`

**Interfaces:**
- Consumes: Task 2 の `lower(hir, typed, entry)`
- Produces:
  - `eml_types::TypedProgram { pub decls: HashMap<Decl, DeclType>, pub bodies: ItemMap<Function, BodyTypes> }`
  - `eml_types::Decl { Function(FunctionId), Operation(OperationId), Constructor(ConstructorId) }` (`Debug, Clone, Copy, PartialEq, Eq, Hash`)
  - `eml_types::DeclType { pub ty: Type, pub(crate) shape: Shape, pub(crate) kinds: KindScheme }`
  - `eml_types::check(&Program) -> (TypedProgram, Vec<Diagnostic>)`、`eml_types::dump(&Program, &TypedProgram) -> String`
  - `eml_core_ir::lower(&HirProgram, &TypedProgram, FunctionId)`、`lower_until(&HirProgram, &TypedProgram, FunctionId, Pass)`
  - `eml_test_support::Checked::typed: eml_types::TypedProgram`

- [ ] **Step 1: テストを書く**

`crates/eml_types/tests/check.rs` の `intrinsic_schemes_are_exported` を、`decls` から読む形にする (種類3。期待する文字列は変えない)。

```rust
#[test]
fn intrinsic_schemes_are_exported() {
    let checked = eml_test_support::check("main : Unit -> <IO> Unit\nmain () = ()");
    let ty = |name: &str| {
        let (id, _) = checked
            .program
            .functions()
            .find(|(_, function)| function.name == name)
            .unwrap();
        checked.typed.decls[&Decl::Function(id)].ty.to_string()
    };
    assert_eq!(ty("println"), "String -> <IO> Unit");
    assert_eq!(ty("+"), "Int -> Int -> Int");
    assert_eq!(ty(">>"), "(a -> <e> b) -> (b -> <e> c) -> a -> <e> c");
    // コンストラクタは intrinsic ではなく、Prelude の `data Bool` のスキームとして書き出す
    assert_eq!(
        checked.typed.decls[&Decl::Constructor(checked.program.lang.true_ctor)]
            .ty
            .to_string(),
        "Bool"
    );
}

#[test]
fn operation_types_are_exported() {
    let checked = eml_test_support::check("effect State where\n  get : Unit -> Int\n  put : Int -> Unit");
    let ty = |name: &str| {
        let (id, _) = checked
            .program
            .operations()
            .find(|(_, operation)| operation.name == name)
            .unwrap();
        checked.typed.decls[&Decl::Operation(id)].ty.to_string()
    };
    assert_eq!(ty("get"), "Unit -> <State> Int");
    assert_eq!(ty("put"), "Int -> <State> Unit");
}
```

ファイルの先頭の `use` に `use eml_types::Decl;` を足す。

Run: `cargo test -p eml_types --test check exported`
Expected: `Decl` と `decls` がないのでコンパイルが通らず FAIL。

- [ ] **Step 2: 出力の型を定義する**

`crates/eml_types/src/kind/problem.rs` の `Decl` の定義を消し、`crates/eml_types/src/lib.rs` に移して公開する。`TypedModule` と `Scheme` を消し、次を置く。

```rust
/// 型付き HIR。HIR は複製せず、型を別テーブルに持つ (docs/implementation/architecture.md)。宣言の結果を宣言ごとに
/// 持つのは、クエリ化と REPL で、宣言ごとに結果を使い回せるようにするため
/// (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 5.1)。
#[derive(Debug, Default)]
pub struct TypedProgram {
    /// シグネチャのある関数、操作、コンストラクタの型。
    pub decls: HashMap<Decl, DeclType>,
    /// シグネチャと等式の両方がある関数だけを含む。
    pub bodies: ItemMap<Function, BodyTypes>,
}

/// スキームを持つ宣言。具体化の記録が、どの宣言のスキームを使うかも指す。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Decl {
    Function(FunctionId),
    Operation(OperationId),
    Constructor(ConstructorId),
}

/// 1つの宣言の型検査の結果。
#[derive(Debug)]
pub struct DeclType {
    /// 後の段階が読む、矢印の線形性のない型。`Type` は名前を持ち、名前は `Context` から引くので、検査の最後に1回だけ
    /// 書き出しておく。
    pub ty: Type,
    /// 後の段階は Kind を読まない (docs/implementation/architecture.md の「`Table::export`」) ので、外からは読めなくする。
    #[allow(dead_code)]
    pub(crate) shape: Shape,
    #[allow(dead_code)]
    pub(crate) kinds: KindScheme,
}
```

`#[allow(dead_code)]` は、Step 4 で `dump` が `shape` と `kinds` を読むようになったら外す。`use` に `std::collections::HashMap`、`eml_hir::{ConstructorId, OperationId}`、`crate::shape::Shape`、`crate::kind::problem::KindScheme` を足し、使わなくなった `Constructor`、`Operation` を外す。`Shape` と `KindScheme` は `pub(crate)` の型のままでよい (欄が `pub(crate)` なので外に漏れない)。

`kind/solve.rs`、`check/mod.rs`、`check/body.rs` の `use ... problem::{... Decl ...}` から `Decl` を外し、`use crate::Decl;` を足す。

- [ ] **Step 3: `TypedProgram` を組み立てる**

`crates/eml_types/src/check/mod.rs` の `typed_module` を次の `typed_program` に置き換え、`check_module` の戻り値の型と呼び出しを合わせる (`let typed = typed_program(&context, &signatures, schemes, bodies);`)。`kind_constraints` は Step 4 で `dump.rs` に移すので、ここでは消す。

```rust
/// 段0の形と段2のスキームを、宣言ごとの結果にまとめる。スキームのない宣言 (Kind の制約が残らなかった宣言) は、制約のない
/// スキームにする。
fn typed_program(
    context: &Context,
    signatures: &Signatures,
    mut schemes: HashMap<Decl, KindScheme>,
    bodies: ItemMap<Function, BodyTypes>,
) -> TypedProgram {
    let shapes = signatures
        .functions
        .iter()
        .map(|(id, shape)| (Decl::Function(id), shape))
        .chain(
            signatures
                .operations
                .iter()
                .map(|(id, shape)| (Decl::Operation(id), shape)),
        )
        .chain(
            signatures
                .constructors
                .iter()
                .map(|(id, shape)| (Decl::Constructor(id), shape)),
        );
    let decls = shapes
        .map(|(decl, shape)| {
            let declared = DeclType {
                ty: shape.export(context),
                shape: shape.clone(),
                kinds: schemes.remove(&decl).unwrap_or_default(),
            };
            (decl, declared)
        })
        .collect();
    TypedProgram { decls, bodies }
}
```

`crates/eml_types/src/exhaustive.rs` の `check` の引数を `typed: &TypedProgram` にする (読むのは `typed.bodies` だけなので本体は変えない)。

- [ ] **Step 4: `dump` を `dump.rs` に移す**

`crates/eml_types/src/dump.rs` を作る。`lib.rs` の今の `dump` と `write_kinds`、`check/mod.rs` の `kind_constraints` を移し、`DeclType` から表示する形にする。`kind_constraints` の本体は今のまま写す。

```rust
//! テストで推論結果を確かめるための表示。

use std::fmt::Write;

use eml_hir::Program;

use crate::context::Context;
use crate::kind::problem::KindScheme;
use crate::kind::Bound;
use crate::shape::Shape;
use crate::ty::{KindConstraint, KindTerm, Linearity, Multiplicity, RowTerm};
use crate::{Decl, DeclType, TypedProgram};

/// 入口のモジュールだけを表示する。Prelude はどのプログラムにもあるので、テストの表示を Prelude に左右させないため。
pub fn dump(program: &Program, typed: &TypedProgram) -> String {
    // Kind の制約の表示に型の名前が要る。テストの表示にしか使わないので、`Context` を作り直す費用は問題にしない
    let context = Context::new(program);
    let mut out = String::new();
    for (id, operation) in program.operations() {
        if id.module != program.entry {
            continue;
        }
        if let Some(declared) = typed.decls.get(&Decl::Operation(id)) {
            writeln!(out, "{} : {}", operation.name, declared.ty).unwrap();
            write_kinds(&mut out, &context, declared);
        }
    }
    for (id, function) in program.functions() {
        if id.module != program.entry {
            continue;
        }
        if let Some(declared) = typed.decls.get(&Decl::Function(id)) {
            writeln!(out, "{} : {}", function.name, declared.ty).unwrap();
            write_kinds(&mut out, &context, declared);
        }
        let (Some(body), Some(types)) = (program.body(id), typed.bodies.get(id)) else {
            continue;
        };
        for (local, data) in body.locals.iter() {
            if let Some(ty) = types.locals.get(local) {
                writeln!(
                    out,
                    "  {}#{} : {ty}",
                    data.name,
                    u32::from(local.into_raw())
                )
                .unwrap();
            }
        }
    }
    out
}

fn write_kinds(out: &mut String, context: &Context, declared: &DeclType) {
    let constraints = kind_constraints(context, &declared.shape, &declared.kinds);
    if !constraints.is_empty() {
        let kinds: Vec<String> = constraints.iter().map(ToString::to_string).collect();
        writeln!(out, "  kinds: {}", kinds.join(", ")).unwrap();
    }
}

/// スキームに残った制約のうち、定数を片側に持つものを表示用にする。変数どうしの制約は出さない。テストで確かめたいのは
/// `Unr` の上限が付いたかどうかで、変数どうしの制約は部分適用のたびに増えて読みにくくなるため。
fn kind_constraints(context: &Context, shape: &Shape, scheme: &KindScheme) -> Vec<KindConstraint> {
    // (今の check/mod.rs の本体をそのまま写す)
}
```

最後の関数の本体は、`check/mod.rs` の今の `kind_constraints` の本体 (`let names = shape.kind_names(context);` から `constraints` を返すまで) を1字も変えずに写す。`use` のパスは、clippy とコンパイラの指摘に合わせて直す (`Bound` の場所など)。

`lib.rs` に `mod dump;` と `pub use dump::dump;` を足し、`lib.rs` の `dump`、`write_kinds`、`use std::fmt::Write;` を消す。`DeclType` の `#[allow(dead_code)]` を2つとも外す。`pub use ty::{...}` から `KindConstraint`、`KindTerm`、`RowTerm` を外し、`ty.rs` のこの3つの型を `pub(crate)` にする。外の crate がこの3つを使っていないことは、`grep -rn "KindConstraint\|KindTerm\|RowTerm" crates --include=*.rs | grep -v crates/eml_types/src` が何も出さないことで確かめる。

- [ ] **Step 5: 使う側を直す**

`crates/eml_core_ir/src/pipeline.rs`、`translate/mod.rs`、`translate/program.rs` の `TypedModule` を `TypedProgram` にする。

`translate/mod.rs` の関数の型の読み方を次にする (`use eml_types::Decl;` を足す)。

```rust
        let signature = &typed
            .decls
            .get(&Decl::Function(id))
            .expect("every function has a signature")
            .ty;
```

入口の型も同じにする。

```rust
    let entry_type = &typed
        .decls
        .get(&Decl::Function(entry))
        .expect("the entry function has a signature")
        .ty;
```

`translate/program.rs` の `ProgramBuilder::new` の3つの表を次にする。

```rust
            intrinsic_types: hir
                .functions()
                .filter(|(_, function)| function.intrinsic)
                .filter_map(|(id, _)| Some((id, typed.decls.get(&Decl::Function(id))?.ty.clone())))
                .collect(),
            // (functions、arities、strings、wrappers は今のまま)
            operation_types: typed
                .decls
                .iter()
                .filter_map(|(decl, declared)| match decl {
                    Decl::Operation(id) => Some((*id, declared.ty.clone())),
                    _ => None,
                })
                .collect(),
            operation_wrappers: HashMap::new(),
            constructor_types: typed
                .decls
                .iter()
                .filter_map(|(decl, declared)| match decl {
                    Decl::Constructor(id) => Some((*id, declared.ty.clone())),
                    _ => None,
                })
                .collect(),
```

`crates/eml_cli/src/lib.rs` の `front` の戻り値の型を `eml_types::TypedProgram` にする。`crates/eml_test_support/src/lib.rs` の `Checked::typed` の型を `eml_types::TypedProgram` にする。

- [ ] **Step 6: テストを流す**

Run: `cargo test -p eml_types --test check exported`
Expected: 2件とも PASS。

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて PASS。`dump` の表示 (`kinds:` の行を含む) を期待値に持つ `eml_types` のテストが、変わらずに通ること。`git diff --no-ext-diff --stat crates/*/tests crates/eml_cli/tests/snapshots` で、変わったテストが `check.rs` だけであることを確かめる。

- [ ] **Step 7: コミットする**

```bash
git add crates
git commit -m "Return per-declaration results from type checking as TypedProgram

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RSkNuf5aBtsdHwHZ92gz6y"
```

---

### Task 4: 由来にファイルを持たせる

**Files:**
- Modify: `crates/eml_types/src/kind/mod.rs` (`Span`、`KindOrigin`、`CarriedInner`、`Provenance::Unattributed`、`order_key`、単体テスト)
- Modify: `crates/eml_types/src/kind/solve.rs` (`reportable`、`unattributed_origin`、`copy_scheme`、単体テスト)
- Modify: `crates/eml_types/src/check/body.rs` (`with_kind_origin`)
- Modify: `crates/eml_types/src/check/mod.rs` (`check_body` の `Unattributed`、`usage::constrain` と `carry::constrain` の呼び出し、`report_violations`)
- Modify: `crates/eml_types/src/usage.rs`、`crates/eml_types/src/carry.rs` (`file` の引数)
- Modify: `crates/eml_types/src/check/report.rs` (`linear_misuse`、`carried_across`、`carried_through`、`misused`)
- Create: `crates/eml_types/tests/modules.rs`

**Interfaces:**
- Consumes: Task 3 の `eml_types::check` (`TypedProgram` を返す)
- Produces (crate の中だけ):
  - `kind::Span { pub file: FileId, pub range: TextRange }` (`Debug, Clone, Copy, PartialEq, Eq`)
  - `KindOrigin { pub span: Span, pub reason: KindReason }`
  - `CarriedInner { pub span: Span, pub label: InnerLabel }`
  - `Provenance::Unattributed(Span)`
  - `usage::constrain(file: FileId, body: &Body, typing: &BodyTyping, table: &mut Table, reliable: bool)`
  - `carry::constrain(program: &Program, file: FileId, body: &Body, typing: &BodyTyping, table: &mut Table<'_>, reliable: bool)`

- [ ] **Step 1: 結合テストを書く**

`crates/eml_types/tests/modules.rs` を作る。

```rust
//! モジュールをまたぐ型検査 (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 5.2)。Prelude の末尾に本体のある
//! 関数を足したプログラムで、診断が位置のあるファイルを指すことを確かめる。

use std::fmt::Write;

use eml_diagnostics::{Label, SourceFiles};

/// 線形性のテストで使うエフェクト。Prelude に足す。
const CHOICE: &str = "pub effect Choice where\n  multi choose : Unit -> Bool\n";

/// Prelude の末尾に `extra` を足したプログラムを検査し、診断を「番号 文言」と、ラベルごとの「ファイル "指す文字列" 文言」
/// の行にする。Prelude の行番号は Prelude を変えるたびに動くので、位置は指す文字列で示す。
fn check_with_prelude(extra: &str, text: &str) -> String {
    let mut files = SourceFiles::new();
    let prelude = files.add(
        eml_hir::PRELUDE_PATH,
        format!("{}\n{extra}", eml_hir::PRELUDE_SOURCE),
    );
    let entry = files.add("test.em", text);
    let mut diagnostics = Vec::new();
    let trees = [prelude, entry].map(|file| {
        let (parse, errors) = eml_syntax::parse(file, files.text(file));
        assert!(errors.is_empty(), "{errors:?}");
        let (tree, stage) = eml_hir::item_tree(file, &parse.tree());
        diagnostics.extend(stage);
        tree
    });
    let (def_map, stage) = eml_hir::def_map(&trees);
    diagnostics.extend(stage);
    let (program, stage) = eml_hir::lower(&def_map, &trees);
    diagnostics.extend(stage);
    let (_, stage) = eml_types::check(&program);
    diagnostics.extend(stage);
    eml_diagnostics::sort_diagnostics(&mut diagnostics);
    let mut out = String::new();
    for d in &diagnostics {
        writeln!(out, "{} {}", d.code, d.message).unwrap();
        for label in std::iter::once(&d.primary).chain(&d.secondary) {
            writeln!(out, "  {}", shown(&files, label)).unwrap();
        }
    }
    out
}

fn shown(files: &SourceFiles, label: &Label) -> String {
    let text = &files.text(label.file)[label.range];
    format!("{} {text:?} {}", files.path(label.file), label.message)
}

#[test]
fn a_linear_misuse_in_the_prelude_points_into_the_prelude() {
    let extra = format!(
        "{CHOICE}\npub held : Unit -> <Choice, IO> Unit\nheld () =\n  let f = open \"a.txt\"\n  let b = choose ()\n  close f\n"
    );
    insta::assert_snapshot!(check_with_prelude(&extra, ""), @r#"
    E3006 `f` must be used exactly once, but it is kept alive across a call that may resume more than once
      Prelude.em "choose ()" this call may perform `choose`, a `multi` operation
      Prelude.em "f" `f` is bound here
      Prelude.em "choose" `choose` is declared `multi` here
    "#);
}

#[test]
fn a_carry_over_through_a_prelude_function_points_into_the_prelude() {
    let extra = format!(
        "{CHOICE}\npub keep : a -> (Unit -> <e> Unit) -> <e> a\nkeep x action =\n  action ()\n  x\n"
    );
    let text = "chooser : Unit -> <Choice> Unit\nchooser () =\n  let b = choose ()\n  ()\n\nkept : Unit -> <Choice, IO> Unit\nkept () =\n  let f = open \"a.txt\"\n  let g = keep f chooser\n  close g";
    insta::assert_snapshot!(check_with_prelude(&extra, text), @r#"
    E3006 `keep` keeps a linear value alive across a call that may resume more than once
      test.em "keep" `keep` is used here
      Prelude.em "action ()" `x` is kept alive across this call
    "#);
}
```

`crates/eml_types/Cargo.toml` の dev-dependencies に `eml_diagnostics` がなければ足す (`eml_diagnostics.workspace = true`。`[dependencies]` にはあるので、結合テストからもそのまま使えるなら足さなくてよい)。

Run: `cargo test -p eml_types --test modules`
Expected: FAIL。今の報告は入口のファイルを決め打ちするので、Prelude の範囲を `test.em` で切り出そうとして panic するか、ファイルが `test.em` になる。

- [ ] **Step 2: 単体テストを書く**

`crates/eml_types/src/check/mod.rs` の `mod tests` に足す。

```rust
    use eml_diagnostics::TextRange;

    use super::report_violations;
    use crate::kind::{CallKind, CarriedValue, KindOrigin, KindReason, Span};

    #[test]
    fn carried_values_in_different_files_are_reported_separately() {
        // 範囲が同じでも、ファイルが違えば別の値である
        let program = crate::test_program("");
        let prelude = program.file(program.prelude);
        let entry = program.file(program.entry);
        let range = TextRange::new(0.into(), 1.into());
        let origin = |file| KindOrigin {
            span: Span { file, range },
            reason: KindReason::CarriedAcross {
                value: CarriedValue::Temporary(range),
                multi: None,
                call: CallKind::Call,
            },
        };
        let reported = report_violations(&program, vec![origin(entry), origin(prelude)]);
        let files: Vec<_> = reported.iter().map(|d| d.primary.file).collect();
        assert_eq!(files, vec![prelude, entry]);
    }
```

`crates/eml_types/src/kind/mod.rs` の `distinct_reasons` (単体テスト) で `CarriedInner` を作っている `inner` の補助関数を、ファイルも受け取る形にし、同じ範囲で別のファイルを指す `inner` の由来を1つ足す。

```rust
        let mut files = eml_diagnostics::SourceFiles::new();
        let a = files.add("a.em", "");
        let b = files.add("b.em", "");
        let inner = |file, start, label| CarriedInner {
            span: Span {
                file,
                range: range(start, start + 1),
            },
            label,
        };
```

今の `inner(start, label)` の呼び出しはすべて `inner(a, start, label)` にし、`through(Some(inner(a, 1, InnerLabel::Value)))` の次の行に `through(Some(inner(b, 1, InnerLabel::Value)))` を足す。ファイルだけが違う2つの由来の鍵が、違うことを確かめるためである。

Run: `cargo test -p eml_types --lib`
Expected: `Span` がないのでコンパイルが通らず FAIL。

- [ ] **Step 3: `Span` を足し、由来の型を変える**

`crates/eml_types/src/kind/mod.rs` の `KindOrigin` の前に足し、`KindOrigin` を変える (`use eml_diagnostics::FileId;` を足す)。

```rust
/// ファイルを持つ位置。由来は別のモジュールの中を指しうる (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の
/// 5.2)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Span {
    pub file: FileId,
    pub range: TextRange,
}

/// Kind の制約の由来。制約が破れたときに E3001〜E3005 が指す場所と理由である (docs/spec/diagnostics.md の「線形性の診断」)。
/// `reason` の中の位置は、どれも由来を作った本体の中にあり、`span` と同じファイルである。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KindOrigin {
    pub span: Span,
    pub reason: KindReason,
}
```

`Provenance::Unattributed(TextRange)` を `Unattributed(Span)` にし、doc コメントの「値は検査している関数の名前の範囲」を「値は検査している関数の名前の位置」にする。

`CarriedInner` の `range: TextRange` を `span: Span` にし、doc コメントに「呼ばれた関数の中を指すので、呼んだ側の由来とは別のファイルでありうる」と足す。`CarriedInner::of` は `span: origin.span` にする。

`KeyPart` に `File(FileId)` を足し、`order_key` の `CarriedThrough` の `inner` の鍵を次にする。

```rust
                    Some(inner) => [
                        vec![number(1), KeyPart::File(inner.span.file)],
                        span(inner.span.range),
                        inner.label.order_key(),
                    ]
                    .concat(),
```

`KeyPart` の derive に `FileId` が必要とする `Ord` はそろっている (`FileId` は `PartialOrd, Ord` を derive している)。

- [ ] **Step 4: 由来を作る箇所を直す**

`crates/eml_types/src/check/body.rs` の `with_kind_origin` の由来を次にする。

```rust
        let span = Span {
            file: self.file(),
            range,
        };
        let previous = self
            .table
            .set_kind_origin(Provenance::At(KindOrigin { span, reason }));
```

`crates/eml_types/src/usage.rs` の `constrain` の先頭の引数に `file: FileId` を足して `Usage` に持たせ、`with_origin` の由来を `Provenance::At(KindOrigin { span: Span { file: self.file, range }, reason })` にする。

`crates/eml_types/src/carry.rs` の `constrain` の `program` の次の引数に `file: FileId` を足して `Carrying` に持たせ、`CarriedAcross` の由来を次にする。

```rust
            Provenance::At(KindOrigin {
                span: Span {
                    file: self.file,
                    range: self.body.exprs[at].range,
                },
                reason: KindReason::CarriedAcross {
```

`crates/eml_types/src/check/mod.rs` の `check_body` で、ファイルを1回だけ求めて使う。

```rust
    let file = program.file(id.module);
    // ここから後の制約は、由来を付け忘れたら Unattributed になり、違反すれば段2が見つける
    table.set_kind_origin(Provenance::Unattributed(Span {
        file,
        range: function.name_range,
    }));
```

`BodyCheck` の `file: program.file(id.module)` を `file` にし、`usage::constrain(file, body, &typing, &mut table, reliable);` と `carry::constrain(program, file, body, &typing, &mut table, reliable);` にする。

`crates/eml_types/src/kind/solve.rs` の `reportable` の `Unattributed(range)` を `Unattributed(span)` にし、`unattributed_origin(span: Span) -> KindOrigin` は `KindOrigin { span, reason: KindReason::Unified }` を返す。`copy_scheme` の `Passed` の置き換えは次にする。

```rust
                Provenance::At(KindOrigin {
                    span,
                    reason: KindReason::Passed(name),
                }) => Provenance::At(KindOrigin {
                    span: *span,
                    reason: KindReason::CarriedThrough {
                        name: name.clone(),
                        inner: carry.origin.origin().map(CarriedInner::of),
                    },
                }),
```

`solve.rs` の単体テストでは、`KindOrigin { range: range(a, b), ... }`、`CarriedInner { range: ..., ... }`、`Provenance::Unattributed(range)` を、ファイルを持つ形にする。`mod tests` に次の補助関数を足して使う。期待値の範囲の数字は変えない (種類3)。

```rust
    /// 単体テストの由来のファイル。どのテストも1つのファイルの中の位置だけを使う。
    fn at(start: u32, end: u32) -> Span {
        Span {
            file: eml_diagnostics::SourceFiles::new().add("test.em", ""),
            range: range(start, end),
        }
    }
```

`an_unattributed_violation_is_a_bug` の `let range = TextRange::new(0.into(), 1.into());` は `let span = at(0, 1);` にし、`Provenance::Unattributed(span)` を渡す。

- [ ] **Step 5: 報告が由来のファイルを使う**

`crates/eml_types/src/check/report.rs` を次のように直す。

- `linear_misuse` の先頭の、入口のファイルを決め打ちする2行のコメントと `let file = program.file(program.entry);` を、`let file = origin.span.file;` と `let range = origin.span.range;` にする。関数の中の `origin.range` をすべて `range` にする
- `CarriedAcross` の腕を `carried_across(program, origin.span, value, *multi, call)` にする。`carried_across` は `range: TextRange` の代わりに `span: Span` を受け取り、決め打ちの2行のコメントと `let file = program.file(program.entry);` を消して、`span.file` と `span.range` を使う。`value` の中の位置 (`binding`、`at`、`clause`、`init`) は `span.file` のファイルにある
- `CarriedThrough` の腕を `carried_through(origin.span, name, inner.as_ref())` にする。`carried_through` は `file` と `range` の代わりに `span: Span` を受け取る。`inner` の secondary は `Label::new(inner.span.file, inner.span.range, label)` にする。呼ばれた関数のファイルを指すためである
- `misused` は `Label::new(origin.span.file, origin.span.range, label)` にし、`file` の引数をなくす。呼んでいる4か所から `file` を外す
- `linear_misuse` の引数の `program` が `carried_across` のためだけに要るなら、そのままにする

`crates/eml_types/src/check/mod.rs` の `report_violations` を次にする。

```rust
fn report_violations(program: &Program, mut origins: Vec<KindOrigin>) -> Vec<Diagnostic> {
    origins.sort_by_cached_key(|origin| {
        (
            origin.span.file,
            origin.span.range.start(),
            origin.span.range.end(),
            origin.reason.order_key(),
        )
    });
    origins.dedup();
    let mut carried = HashSet::new();
    let mut out = Vec::new();
    for origin in origins {
        // 値の範囲は由来と同じファイルにある。別のファイルの値は、範囲が同じでも別の値である
        if let KindReason::CarriedAcross { value, .. } = &origin.reason
            && !carried.insert((origin.span.file, value.key()))
        {
            continue;
        }
        out.push(report::linear_misuse(program, &origin));
    }
    out
}
```

関数の doc コメントの「位置の順に並べ」を「ファイルと位置の順に並べ」にする。

- [ ] **Step 6: テストを流す**

Run: `cargo test -p eml_types --lib && cargo test -p eml_types --test modules`
Expected: すべて PASS。`modules.rs` のスナップショットが違ったら、ファイルの列 (`Prelude.em` と `test.em`) が Step 1 のとおりであることを確かめる。ファイルが合っていて、指す文字列か文言だけが違う場合は、同じ形の既存のテスト (`linearity.rs` の `a_file_kept_across_a_multi_operation` と `a_file_kept_by_a_polymorphic_function_across_a_multi_operation`) の行と列が指す文字列と照らし合わせ、合っていれば期待値を実際の出力に直す (新しいテストなので、種類の記録は要らない)。ファイルが違う場合は、実装を直す。

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて PASS。`git diff --no-ext-diff --stat crates/*/tests crates/eml_cli/tests/snapshots` で、既存のテストの期待値が変わっていないことを確かめる。

Run: `cargo test --release -p eml_types --test scaling -- --ignored`
Expected: PASS (5つの形とも、関数の数を4倍にしたときの時間の比が6以下)。

- [ ] **Step 7: コミットする**

```bash
git add crates
git commit -m "Give Kind constraint origins a file and report them in that file

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RSkNuf5aBtsdHwHZ92gz6y"
```

---

### Task 5: 文書を更新する

**Files:**
- Modify: `docs/implementation/architecture.md`
- Modify: `docs/implementation/testing.md`
- Modify: `docs/implementation/status.md`

文書は日本語で書き、書く前に `yomiyasu:yomiyasu` スキルを読み込んで規則に従う。R7c は種類1と種類2のテストの変更がないので、`test-changes.md` は変えない。

- [ ] **Step 1: `architecture.md` を直す**

`grep -n "TypedModule\|Scheme\|KindOrigin\|lower(\|lower_until\|main" docs/implementation/architecture.md` で出る箇所を、次の内容に合わせて直す。

- 段階の入口の表: `eml_types` は `check(&Program) -> (TypedProgram, Vec<Diagnostic>)`。`eml_core_ir` は `lower(&hir::Program, &TypedProgram, FunctionId) -> Program` と `lower_until(&hir::Program, &TypedProgram, FunctionId, Pass) -> Program`。入口の関数は呼ぶ側が渡し、`eml run` では `hir::Program::main()` (入口のモジュールの `main`) である
- 「各段階の規律」の型付き HIR の段落: `TypedProgram` は、宣言ごとの結果 (`decls: HashMap<Decl, DeclType>`) と本体ごとの推論結果 (`bodies`) を持つ。`DeclType` は後の段階が読む `ty` と、crate の外から読めない `Shape` と `KindScheme` を持つ
- `eml_types` の内部の `TypedModule::signatures`、`Scheme::constraints`、`TypedModule::constructors` を書いた箇所: 宣言の型は `DeclType` にあり、`dump` は `dump.rs` で `Shape` と `KindScheme` から `kinds:` の行を作る。持ち越しの制約の表示の書き方 (`a => <e> <= Once` など) の説明は残す
- `KindOrigin` の段落: 由来の位置は `Span` (ファイルと範囲) で、`KindReason` の中の位置は由来と同じファイルにある。`CarriedThrough` の `inner` は呼ばれた関数のファイルを指しうる。報告は由来のファイルを使う
- `eml_core_ir` の intrinsic の型を `TypedModule::signatures` から引くと書いた箇所: `TypedProgram::decls` から引く

- [ ] **Step 2: `testing.md` と `status.md` を直す**

- `testing.md` のテストの地図の `eml_types` に `modules.rs` (モジュールをまたぐ検査。Prelude に本体を足して、診断のファイルを確かめる) を足す
- `status.md` の R7 の行を「R7a、R7b (R7b-1〜R7b-3)、R7c 完了。…R7c で型検査の出力を宣言ごとの `TypedProgram` にし、Kind の制約の由来にファイルを持たせ、入口の関数を `lower` の引数にした。R7d と R7e は未着手」の形にする
- `status.md` の「R7 で直す項目」の型検査の出力の項目に「R7c で済んだ。表示用の `Scheme` はなくし、`dump` が `DeclType` から表示する」を足す
- `status.md` の「次の作業の注意点」の「S2 まで: 同じ名前の別の型を区別して表示しない」の項目に、`Type` から名前をなくし、表示する側が `Program` から名前を引いて修飾する案 (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 5.1) を足す。REPL で `data T` を定義し直すと同じことが起きることも書く
- `status.md` の `eml_types` の行に、R7c で `TypedProgram` と `DeclType`、由来の `Span` にしたことを足す

- [ ] **Step 3: lint を流してコミットする**

Run: `python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py docs/implementation/architecture.md` (`testing.md` と `status.md` も同じ)。英単語の前後の空白と箇条書きの比率の指摘は、`docs/` の既存の書き方に合わせて残してよい。文末のコロンは直す。

Run: `grep -rn "TypedModule" docs --exclude-dir=superpowers`
Expected: 完了した作業の記録 (`status.md` の R2b-2 などの過去の回の説明と `test-changes.md`) だけが残る。

```bash
git add docs
git commit -m "Document refactor R7c: per-declaration type results, origins with files, the entry as an argument

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RSkNuf5aBtsdHwHZ92gz6y"
```
