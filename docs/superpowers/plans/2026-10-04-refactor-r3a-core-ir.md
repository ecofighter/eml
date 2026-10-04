# リファクタリング R3a: Core IR の形 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Core IR の `Rhs::Nested` を join point に替えて末尾呼び出しを入れ、Perceus を独立したパスにして verifier を足し、boxed の判定と組み込みの変換を1か所にまとめ、入口の関数を Core IR の側で作る。言語の観測できる振る舞いは変えない。

**Architecture:** まず型検査が組み込みのスキームを外に出す (Task 1)。次に呼び出しを `Call` にまとめて Perceus をプログラム全体のパスにし (Task 2)、boxed の判定と組み込みの変換を1か所にする (Task 3)。その上で join point (Task 4)、末尾呼び出し (Task 5)、入口の関数 (Task 6) を入れ、最後に verifier を足す (Task 7)。インタプリタは、各タスクで新しい命令を実行するのに要る分だけ追随させる。

**Tech Stack:** Rust (edition 2024)、la-arena、insta

**Spec:** `docs/superpowers/specs/2026-10-04-refactor-r3a-core-ir-design.md`

## Global Constraints

- 期待値は、このプランで名前を挙げたテストだけを変える (docs/implementation/testing.md の「テストの変更の運用」)
  - 種類2 (spec で承認済み): `crates/eml_core_ir/tests/lower.rs` の既存の9件。各タスクが新しい期待値の全文を示す
  - 種類3 (期待値を変えない機械的な追随): `crates/eml_interp/tests/run.rs` と `crates/eml_interp/tests/closures.rs` の手書きの Core IR の組み立て
  - このプランで足したテストの期待値は、後のタスクで示すとおりに変えてよい
- 上に挙げていないテスト (UI テスト、HIR と型のスナップショットを含む) の期待値が変わったら、変えずに止まる。差分と理由をユーザーに示し、承認を得てから変え、`testing.md` に記録する
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

- 分岐の中の `let` にある `if` (内側の join point の本体が、外側の join point に `jump` する) で、文字列の所有が釣り合い、リークも二重の解放もない (Task 4 の `tests/ui/run/join_points.em`)
- `else` のない `if` を文として使い、値 `()` を join point に渡す (Task 4 の `join_points.em`)
- どの枝も join point の本体も使わない文字列を、join point の前から持っている。どこかで1回だけ解放する (Task 4 の `join_points.em` の `unused`)
- 文の列の各文が `if` で、join point が本体の中に長く入れ子になる。後段のどれもスタックをあふれさせない (Task 4 の `long_sequence_of_if_statements_does_not_overflow_the_stack`)
- 余った引数を持つ末尾の呼び出し (`adder a b` で、戻った関数値に残りを適用する) が正しい値を返し、リークしない (Task 5 の `tests/ui/run/tail_calls.em`)

---

### Task 1: 組み込みのスキームを外に出す

**Files:**
- Modify: `crates/eml_types/src/lib.rs` (`TypedModule`)
- Modify: `crates/eml_types/src/check/mod.rs` (`TypedModule` の組み立て)
- Test: `crates/eml_types/tests/check.rs`

**Interfaces:**
- Consumes: なし
- Produces: `eml_types::TypedModule::builtins: HashMap<eml_hir::builtin::Builtin, eml_types::Scheme>`。`True` と `False` は含まない

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/check.rs` の先頭の `use` に `use eml_hir::builtin::Builtin;` を足し、末尾に足す。

```rust
#[test]
fn builtin_schemes_are_exported() {
    let checked = eml_test_support::check("main : Unit -> <IO> Unit\nmain () = ()");
    let ty = |builtin| checked.typed.builtins[&builtin].ty.to_string();
    assert_eq!(ty(Builtin::Println), "String -> <IO> Unit");
    assert_eq!(ty(Builtin::IntAdd), "Int -> Int -> Int");
    assert_eq!(
        ty(Builtin::ComposeFwd),
        "(a -> <e> b) -> (b -> <e> c) -> a -> <e> c"
    );
    // コンストラクタは Prelude にない (段階4で `data Bool` にする)
    assert!(!checked.typed.builtins.contains_key(&Builtin::True));
}
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_types --test check builtin_schemes_are_exported`
Expected: コンパイルエラー (`no field builtins on type TypedModule`)

- [ ] **Step 3: 実装する**

`crates/eml_types/src/lib.rs`:

- `use std::collections::HashMap;` と `use eml_hir::builtin::Builtin;` を足す。
- `TypedModule` の `main` の後に足す。

```rust
    /// Prelude のシグネチャから作った組み込みのスキーム。Core IR が、組み込みを包む関数の変数を boxed にするかを
    /// 決めるのに使う。コンストラクタ (`True`、`False`) は含まない。
    pub builtins: HashMap<Builtin, Scheme>,
```

`crates/eml_types/src/check/mod.rs` の `check_module` で、`typed.signatures.insert(...)` のループの後に足す。

```rust
    for (&builtin, scheme) in &builtins {
        typed.builtins.insert(
            builtin,
            crate::Scheme {
                ty: table.export(scheme.ty),
                constraints: kind_constraints(&table, scheme),
            },
        );
    }
```

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_types --test check builtin_schemes_are_exported`
Expected: PASS

- [ ] **Step 5: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る。`git diff --no-ext-diff --stat -- '*.snap' crates/*/tests` は `crates/eml_types/tests/check.rs` だけを出す

```bash
git add crates/eml_types
git commit -m "Export builtin schemes from type checking"
```

---

### Task 2: 呼び出しを `Call` にまとめ、Perceus をプログラム全体のパスにする

**Files:**
- Modify: `crates/eml_core_ir/src/lib.rs`、`lower.rs`、`perceus.rs`、`pretty.rs`
- Modify: `crates/eml_interp/src/lib.rs`
- Modify: `crates/eml_interp/tests/closures.rs` (種類3)

**Interfaces:**
- Consumes: なし
- Produces:
  - `eml_core_ir::Call { Direct(FnIdx, Vec<Atom>), Apply(Atom, Vec<Atom>) }`、`Call::atoms(&self) -> Vec<Atom>`
  - `Rhs::Call(Call)` (`Rhs::CallDirect` と `Rhs::Apply` は無くなる)、`Rhs::atoms(&self) -> Vec<Atom>`
  - `perceus::insert(program: &mut Program)` (crate の中だけ)
  - インタプリタの `Machine::call(&mut self, call: &Call, resume: Option<(VarId, CExprId)>) -> Result<bool, String>`

振る舞いも Core IR の表示も変えない。

- [ ] **Step 1: 命令の形を変える**

`crates/eml_core_ir/src/lib.rs` の `Rhs` を次にし、`Call` を足す。

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rhs {
    Atom(Atom),
    Call(Call),
    /// 関数と先頭の引数の並びからクロージャを作る。並びの値の所有権はクロージャに移る。ラムダの捕獲と部分適用は、
    /// どちらもこの形になる (docs/spec/core-ir.md)。
    MakeClosure(FnIdx, Vec<Atom>),
    Prim(PrimOp, Vec<Atom>),
    ConstString(u32),
    Perform(IoOp, Vec<Atom>),
    /// 値を返す入れ子の式。中の `Return` が、この `Let` の変数に値を渡す。
    Nested(CExprId),
}

impl Rhs {
    /// 右辺が使う値。関数、プリミティブ、`perform` の引数は、どれも所有権を受け取る (docs/spec/core-ir.md)。
    pub fn atoms(&self) -> Vec<Atom> {
        match self {
            Rhs::Atom(atom) => vec![*atom],
            Rhs::Call(call) => call.atoms(),
            Rhs::MakeClosure(_, args) | Rhs::Prim(_, args) | Rhs::Perform(_, args) => args.clone(),
            Rhs::ConstString(_) | Rhs::Nested(_) => Vec::new(),
        }
    }
}

/// 呼び出し。クロージャと引数の所有権は呼び出しに移る。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    /// 呼ぶ相手が分かっていて、引数の個数が揃っている呼び出し。
    Direct(FnIdx, Vec<Atom>),
    /// 関数値の呼び出し。実行時に引数の個数を比べる (eval/apply)。
    Apply(Atom, Vec<Atom>),
}

impl Call {
    /// 呼び出しが使う値。関数値の呼び出しでは、呼ばれる値が先に来る。
    pub fn atoms(&self) -> Vec<Atom> {
        match self {
            Call::Direct(_, args) => args.clone(),
            Call::Apply(callee, args) => std::iter::once(*callee).chain(args.iter().copied()).collect(),
        }
    }
}
```

`crates/eml_core_ir/src/lower.rs` で、`Rhs::CallDirect(f, args)` をすべて `Rhs::Call(Call::Direct(f, args))` に、`Rhs::Apply(f, args)` をすべて `Rhs::Call(Call::Apply(f, args))` にする (`use crate::{...}` に `Call` を足す)。

`crates/eml_core_ir/src/pretty.rs` の `rhs_text` の2つの腕を次にする (表示は今と同じ)。

```rust
        Rhs::Call(Call::Direct(callee, a)) => {
            format!("call {}({})", program.function(*callee).name, args(a))
        }
        Rhs::Call(Call::Apply(callee, a)) => format!("apply {}({})", atom(function, callee), args(a)),
```

- [ ] **Step 2: Perceus をプログラム全体のパスにする**

`crates/eml_core_ir/src/perceus.rs`:

- `uses` を、右辺の `atoms()` を使う形にする。

```rust
    /// 右辺が使う変数を、使う回数の分だけ並べる。
    fn uses(&self, rhs: &Rhs) -> Vec<VarId> {
        rhs.atoms()
            .iter()
            .filter_map(|atom| self.atom_var(atom))
            .collect()
    }
```

- 先頭に足し、`insert_rc` を `fn insert_rc` (非公開) にする。

```rust
/// 変換の後に、プログラム全体にかける。変換の途中の関数ごとではなく、独立したパスにする (docs/spec/core-ir.md)。
pub(crate) fn insert(program: &mut Program) {
    for function in &mut program.functions {
        insert_rc(function);
    }
}
```

(`use crate::{...}` に `Program` を足す。)

`crates/eml_core_ir/src/lower.rs`:

- `ProgramBuilder::finish` から `perceus::insert_rc(&mut core);` を消し、`fn finish(&mut self, function: FnIdx, core: CoreFn)` にする。
- `lower` の最後で `Program` を `let mut program = Program { ... };` として作り、`perceus::insert(&mut program);` を呼んでから返す。変数名が `ProgramBuilder` の `program` と重なるので、`ProgramBuilder` の方を `builder` に改名する。

- [ ] **Step 3: インタプリタを追随させる**

`crates/eml_interp/src/lib.rs`:

- `use eml_core_ir::{...}` に `Call` を足す。
- `bind` の `Rhs::CallDirect` と `Rhs::Apply` の2つの腕を、次の1つにする。

```rust
            Rhs::Call(call) => return self.call(call, Some((var, body))),
```

- ファイルの先頭 (`enum Applied` の後) に足す。

```rust
/// 読み終えた呼び出し。環境を退避する前に引数を読むために、呼び出しを2段に分ける。
enum Prepared {
    Direct(FnIdx, Vec<Value>),
    Apply(Value, Vec<Value>),
}
```

- `enter` の前に足す。

```rust
    /// 呼び出す。引数は、環境を退避する前に読む。`resume` は、戻った値を受ける変数と再開する位置で、`None` なら
    /// フレームを積まない (末尾呼び出し)。
    fn call(&mut self, call: &Call, resume: Option<(VarId, CExprId)>) -> Result<bool, String> {
        let prepared = match call {
            Call::Direct(callee, args) => Prepared::Direct(*callee, self.atoms(args)?),
            Call::Apply(callee, args) => {
                let callee = self.atom(callee)?;
                Prepared::Apply(callee, self.atoms(args)?)
            }
        };
        if let Some((var, body)) = resume {
            self.push_frame(var, body, true);
        }
        match prepared {
            Prepared::Direct(callee, args) => {
                self.enter(callee, args);
                Ok(false)
            }
            Prepared::Apply(callee, args) => match self.apply(callee, args)? {
                Applied::Entered => Ok(false),
                Applied::Value(value) => self.ret(value),
            },
        }
    }
```

- [ ] **Step 4: 手書きの Core IR を追随させる (種類3)**

`crates/eml_interp/tests/closures.rs` の `Rhs::Apply(x, args)` をすべて `Rhs::Call(Call::Apply(x, args))` にし、`use eml_core_ir::{...}` に `Call` を足す。期待値は変えない。

- [ ] **Step 5: 確かめてコミットする**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED"`
Expected: 何も出ない

Run: `git diff --no-ext-diff --stat -- '*.snap' crates/*/tests`
Expected: `crates/eml_interp/tests/closures.rs` だけが出る

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates
git commit -m "Share one call type and run Perceus over the whole program"
```

---

### Task 3: boxed の判定と組み込みの変換を1か所にする

**Files:**
- Modify: `crates/eml_core_ir/src/lower.rs`

**Interfaces:**
- Consumes: Task 1 の `TypedModule::builtins`、Task 2 の `Call`
- Produces (どれも `lower.rs` の中だけ):
  - `fn boxed(ty: &Type, lang: &LangItems) -> bool`
  - `fn var_info(name: &str, ty: &Type, lang: &LangItems) -> VarInfo`
  - `fn split_arrows(ty: &Type, count: usize) -> (Vec<Type>, Type)` (`param_types` の置き換え)
  - `enum Lowering { Prim(PrimOp), Perform(IoOp), Compose { forward: bool }, Constructor(u32) }` と `fn lowering(builtin: Builtin) -> Lowering`
  - `ProgramBuilder::new(module: &Module, typed: &TypedModule) -> ProgramBuilder` (フィールド `lang: LangItems`、`builtin_types: HashMap<Builtin, Type>`)

振る舞いも Core IR の表示も変えない。既存のスナップショットが、そのまま確かめるテストになる。

- [ ] **Step 1: 判定と変換の関数を足す**

`crates/eml_core_ir/src/lower.rs` の `param_types`、`builtin_params`、`builtin_result_boxed`、`prim` を消し、次を足す (`use eml_hir::LangItems;` を足す)。

```rust
/// ヒープに置く値の型。`Unr` でボックス化した変数が RC の対象になる。関数値と型変数の値は、ヒープのクロージャや
/// 文字列かもしれない。インタプリタの `dup` / `decref` はヒープにない値を無視するので、多めに対象にしても正しく動く
/// (docs/spec/core-ir.md)。
fn boxed(ty: &Type, lang: &LangItems) -> bool {
    match ty {
        Type::Con { id, .. } => *id == lang.string,
        Type::Fn { .. } | Type::Rigid(_) | Type::Flexible => true,
        Type::Record(_) | Type::Error => false,
    }
}

fn var_info(name: &str, ty: &Type, lang: &LangItems) -> VarInfo {
    VarInfo {
        name: name.to_string(),
        linearity: Linearity::Unr,
        boxed: boxed(ty, lang),
    }
}

/// 関数型の先頭の `count` 個の引数の型と、残りの型。
fn split_arrows(ty: &Type, count: usize) -> (Vec<Type>, Type) {
    let mut params = Vec::new();
    let mut ty = ty;
    for _ in 0..count {
        let Type::Fn { param, ret, .. } = ty else {
            unreachable!("the type checker matched parameters with arrows");
        };
        params.push((**param).clone());
        ty = ret;
    }
    (params, ty.clone())
}

/// 組み込みを Core IR のどの命令にするか。引数の数は `Builtin::arity` (eml_hir の表) から、引数と結果の型は Prelude の
/// スキームから引くので、ここには変換の種類だけを置く。
enum Lowering {
    Prim(PrimOp),
    Perform(IoOp),
    /// `>>` は `g (f x)`、`<<` は `f (g x)` である (docs/spec/declarations.md の演算子の表)。
    Compose { forward: bool },
    Constructor(u32),
}

fn lowering(builtin: Builtin) -> Lowering {
    match builtin {
        Builtin::Println => Lowering::Perform(IoOp::Println),
        Builtin::ShowInt => Lowering::Prim(PrimOp::ShowInt),
        Builtin::Not => Lowering::Prim(PrimOp::Not),
        Builtin::IntNeg => Lowering::Prim(PrimOp::IntNeg),
        Builtin::IntAdd => Lowering::Prim(PrimOp::IntAdd),
        Builtin::IntSub => Lowering::Prim(PrimOp::IntSub),
        Builtin::IntMul => Lowering::Prim(PrimOp::IntMul),
        Builtin::IntDiv => Lowering::Prim(PrimOp::IntDiv),
        Builtin::IntMod => Lowering::Prim(PrimOp::IntMod),
        Builtin::IntEq => Lowering::Prim(PrimOp::IntEq),
        Builtin::IntNe => Lowering::Prim(PrimOp::IntNe),
        Builtin::IntLt => Lowering::Prim(PrimOp::IntLt),
        Builtin::IntLe => Lowering::Prim(PrimOp::IntLe),
        Builtin::IntGt => Lowering::Prim(PrimOp::IntGt),
        Builtin::IntGe => Lowering::Prim(PrimOp::IntGe),
        Builtin::StrConcat => Lowering::Prim(PrimOp::StrConcat),
        Builtin::ComposeFwd => Lowering::Compose { forward: true },
        Builtin::ComposeBwd => Lowering::Compose { forward: false },
        Builtin::True => Lowering::Constructor(TRUE),
        Builtin::False => Lowering::Constructor(FALSE),
    }
}
```

- [ ] **Step 2: `ProgramBuilder` が組み込みの型を持つ**

`ProgramBuilder` から `#[derive(Default)]` を外し、フィールドと `new` を足す。`lower` では `ProgramBuilder::new(module, typed)` で作る。

```rust
struct ProgramBuilder {
    lang: LangItems,
    /// Prelude から作った組み込みの型。組み込みを包む関数の変数が boxed かどうかを決める。
    builtin_types: HashMap<Builtin, Type>,
    functions: Vec<Option<CoreFn>>,
    arities: Vec<usize>,
    strings: Strings,
    wrappers: HashMap<Builtin, FnIdx>,
}

impl ProgramBuilder {
    fn new(module: &Module, typed: &TypedModule) -> ProgramBuilder {
        ProgramBuilder {
            lang: module.lang,
            builtin_types: typed
                .builtins
                .iter()
                .map(|(&builtin, scheme)| (builtin, scheme.ty.clone()))
                .collect(),
            functions: Vec::new(),
            arities: Vec::new(),
            strings: Strings::default(),
            wrappers: HashMap::new(),
        }
    }
```

`wrapper` の、引数の表と個別の match を使っていた部分を次にする (本体を組み立てる後半は今のまま)。

```rust
    fn wrapper(&mut self, builtin: Builtin) -> FnIdx {
        if let Some(&function) = self.wrappers.get(&builtin) {
            return function;
        }
        let arity = builtin.arity();
        let ty = self
            .builtin_types
            .get(&builtin)
            .expect("every builtin function has a Prelude signature");
        let (param_types, result_type) = split_arrows(ty, arity);
        let lang = self.lang;
        let function = self.reserve(arity);
        self.wrappers.insert(builtin, function);
        let mut vars: Vec<VarInfo> = param_types
            .iter()
            .map(|ty| var_info("p", ty, &lang))
            .collect();
        let params: Vec<VarId> = (0..arity as u32).map(VarId).collect();
        let atoms: Vec<Atom> = params.iter().map(|&param| Atom::Var(param)).collect();
        let mut fresh = |ty: &Type| {
            vars.push(var_info("t", ty, &lang));
            VarId(vars.len() as u32 - 1)
        };
        let steps: Vec<(VarId, Rhs)> = match lowering(builtin) {
            Lowering::Prim(op) => vec![(fresh(&result_type), Rhs::Prim(op, atoms))],
            Lowering::Perform(op) => vec![(fresh(&result_type), Rhs::Perform(op, atoms))],
            Lowering::Compose { forward } => {
                let (inner, outer) = if forward { (0, 1) } else { (1, 0) };
                let (_, middle_type) = split_arrows(&param_types[inner], 1);
                let middle = fresh(&middle_type);
                let result = fresh(&result_type);
                vec![
                    (middle, Rhs::Call(Call::Apply(atoms[inner], vec![atoms[2]]))),
                    (
                        result,
                        Rhs::Call(Call::Apply(atoms[outer], vec![Atom::Var(middle)])),
                    ),
                ]
            }
            Lowering::Constructor(_) => unreachable!("constructors are values, not functions"),
        };
        // ここから下 (`let mut exprs = Vec::new();` から `function` を返すまで) は今のまま
```

- [ ] **Step 3: 型を見ずに boxed にしていた変数に型を与える**

`FnLowering`:

- `new_var` を `var_info` を使う形にする。

```rust
    fn new_var(&mut self, name: &str, ty: &Type) -> VarId {
        self.vars.push(var_info(name, ty, &self.module.lang));
        VarId(self.vars.len() as u32 - 1)
    }
```

- `bind_boxed` を消す。呼んでいたところは、次のように `bind` に型を渡す。
  - `lower` の `param_types(signature, ...)` と `Lambda` の `param_types(&lambda_ty, ...)` は `split_arrows(...).0` にする
  - `ExprKind::Path(Res::Builtin(builtin))` は、コンストラクタを `lowering` で見分ける形にする。`ExprKind::Path(Res::Builtin(Builtin::True))` と `False` の2つの腕は消す

```rust
            ExprKind::Path(Res::Builtin(builtin)) => match lowering(*builtin) {
                Lowering::Constructor(tag) => Atom::Tag(tag),
                _ => {
                    let wrapper = self.program.wrapper(*builtin);
                    let ty = self.ty(id);
                    self.bind(out, "c", &ty, Rhs::MakeClosure(wrapper, Vec::new()))
                }
            },
```

  - `ExprKind::Path(Res::Function(function))` の引数のある関数の腕は `let ty = self.ty(id); self.bind(out, "c", &ty, Rhs::MakeClosure(target, Vec::new()))`
  - `Lambda` の最後は `self.bind(out, "c", &lambda_ty, Rhs::MakeClosure(function, atoms))`

- `call_known` と `call_builtin` に、呼ばれる式の型 `callee_ty: &Type` を足す。`ExprKind::Call` の腕の先頭で `let callee_ty = self.ty(*callee);` を作り、`self.call_known(target, &callee_ty, args, &ty, out)` と `self.call_builtin(*builtin, &callee_ty, args, &ty, out)` のように渡す。

```rust
    /// 呼ぶ相手の引数の個数と比べ、揃えば直接呼び、足りなければクロージャにし、余れば戻った関数値に残りを適用する
    /// (docs/spec/core-ir.md の eval/apply)。
    fn call_known(
        &mut self,
        target: FnIdx,
        callee_ty: &Type,
        mut args: Vec<Atom>,
        ty: &Type,
        out: &mut Bindings,
    ) -> Atom {
        let arity = self.program.arity(target);
        if args.len() < arity {
            return self.bind(out, "c", ty, Rhs::MakeClosure(target, args));
        }
        let rest = args.split_off(arity);
        if rest.is_empty() {
            return self.bind(out, "t", ty, Rhs::Call(Call::Direct(target, args)));
        }
        let (_, function_ty) = split_arrows(callee_ty, arity);
        let function = self.bind(out, "t", &function_ty, Rhs::Call(Call::Direct(target, args)));
        self.bind(out, "t", ty, Rhs::Call(Call::Apply(function, rest)))
    }

    fn call_builtin(
        &mut self,
        builtin: Builtin,
        callee_ty: &Type,
        mut args: Vec<Atom>,
        ty: &Type,
        out: &mut Bindings,
    ) -> Atom {
        let arity = builtin.arity();
        if args.len() < arity {
            let wrapper = self.program.wrapper(builtin);
            return self.bind(out, "c", ty, Rhs::MakeClosure(wrapper, args));
        }
        let rest = args.split_off(arity);
        let rhs = match lowering(builtin) {
            Lowering::Prim(op) => Rhs::Prim(op, args),
            Lowering::Perform(op) => Rhs::Perform(op, args),
            Lowering::Compose { .. } => {
                Rhs::Call(Call::Direct(self.program.wrapper(builtin), args))
            }
            Lowering::Constructor(_) => unreachable!("constructors are values, not functions"),
        };
        if rest.is_empty() {
            return self.bind(out, "t", ty, rhs);
        }
        let (_, function_ty) = split_arrows(callee_ty, arity);
        let function = self.bind(out, "t", &function_ty, rhs);
        self.bind(out, "t", ty, Rhs::Call(Call::Apply(function, rest)))
    }
```

- [ ] **Step 4: 確かめる**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED"`
Expected: 何も出ない

Run: `git diff --no-ext-diff --stat -- '*.snap' crates/*/tests`
Expected: 何も出ない

Run: `grep -n "bind_boxed\|builtin_params\|builtin_result_boxed\|fn prim\b\|param_types" crates/eml_core_ir/src/lower.rs`
Expected: 何も出ない

- [ ] **Step 5: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates/eml_core_ir
git commit -m "Decide boxing from types in one place and look up builtin lowering in one table"
```

---

### Task 4: join point

**Files:**
- Modify: `crates/eml_core_ir/src/lib.rs`、`lower.rs`、`perceus.rs`、`pretty.rs`
- Create: `crates/eml_core_ir/src/liveness.rs`
- Modify: `crates/eml_interp/src/lib.rs`
- Modify: `crates/eml_interp/tests/run.rs`、`closures.rs` (種類3。`CoreFn` に `joins` を足す)
- Test: `crates/eml_core_ir/tests/lower.rs`、`crates/eml_interp/tests/run.rs`、`tests/ui/run/join_points.em`

**Interfaces:**
- Consumes: Task 2 の `Call`、`Rhs::atoms`、Task 3 の `lower.rs` の関数
- Produces:
  - `eml_core_ir::JoinId(pub u32)`、`CExpr::Join { join: JoinId, param: VarId, body: CExprId, scope: CExprId }`、`CExpr::Jump { join: JoinId, arg: Atom }`
  - `CoreFn::joins: Vec<CExprId>` (`JoinId` から `Join` の式への索引)、`CoreFn::join(&self, join: JoinId) -> (VarId, CExprId)`
  - `liveness::{Vars, Liveness { exprs: Vec<Vars>, joins: HashMap<JoinId, Vars> }, liveness(&CoreFn, &[bool]) -> Liveness, tracked(&CoreFn) -> Vec<bool>}` (crate の中だけ。Task 7 の verifier も使う)
  - `Rhs::Nested` は無くなる

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/lower.rs` に足す。

```rust
#[test]
fn a_tail_if_returns_from_each_arm() {
    let text = "sign : Int -> String\nsign n = if n < 0 then \"negative\" else \"non-negative\"\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text), @r#"
    fn sign(n0) {
      let t1 = prim <(n0, 0)
      switch t1 {
        #0 ->
          let s3 = const "non-negative"
          return s3
        #1 ->
          let s2 = const "negative"
          return s2
      }
    }
    fn main(p0) {
      return ()
    }
    "#);
}

#[test]
fn ifs_in_a_condition_nest_join_points() {
    let text = "choose : Bool -> Bool -> Int\nchoose a b =\n  let n = if (if a then b else False) then 1 else 2\n  n + 1\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text), @r"
    fn choose(a0, b1) {
      join j1(t2) {
        join j0(t3) {
          let t4 = prim +(t3, 1)
          return t4
        }
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
    fn main(p0) {
      return ()
    }
    ");
}
```

既存の2件の期待値を次にする (種類2)。

`a_non_tail_if_keeps_strings_used_later`:

```
    fn pick(b0, s1) {
      join j0(t3) {
        let t4 = prim ++(t3, s1)
        return t4
      }
      switch b0 {
        #0 ->
          let s2 = const "none"
          jump j0(s2)
        #1 ->
          dup s1
          jump j0(s1)
      }
    }
    fn main(p0) {
      return ()
    }
```

`recursion_and_top_level_values`:

```
    fn answer() {
      return 42
    }
    fn count(n0) {
      let t1 = prim ==(n0, 0)
      switch t1 {
        #0 ->
          let t3 = prim -(n0, 1)
          let t4 = call count(t3)
          return t4
        #1 ->
          let answer2 = call answer()
          return answer2
      }
    }
    fn main(p0) {
      let t1 = call count(3)
      let t2 = prim show_int(t1)
      let t3 = perform println(t2)
      return t3
    }
```

`crates/eml_interp/tests/run.rs` の末尾に足す。

```rust
#[test]
fn long_sequence_of_if_statements_does_not_overflow_the_stack() {
    // 文の `if` は join point になり、続きの文はその本体に入れ子になる。後段は、この入れ子を再帰せずに処理しなければならない
    let mut body = "  if True then println \"x\"\n".repeat(5000);
    body.push_str("  println \"done\"");
    let expected = format!("{}done\n", "x\n".repeat(5000));
    assert_eq!(run(&main_with(&body)), (expected, Ok(())));
}
```

`tests/ui/run/join_points.em` を作る。

```
-- Non-tail `if`s become join points: an `if` inside a branch, an `if` without `else` used as a statement,
-- and strings that live across the joins or are dropped before them. Every string must be freed exactly once.
label : Bool -> Bool -> String -> String
label a b s =
  let unused = "dropped"
  let t =
    if a then
      let u = if b then s ++ "!" else "plain"
      u ++ "?"
    else s
  t ++ s

main : Unit -> <IO> Unit
main () =
  println (label True True "a")
  println (label True False "b")
  println (label False True "c")
  let s = "kept"
  if True then println "then"
  println s
```

`crates/eml_cli/tests/snapshots/ui__run@join_points.em.snap` を作る。

```
---
source: crates/eml_cli/tests/ui.rs
expression: "format!(\"--- stdout ---\\n{stdout}--- stderr ---\\n{stderr}\")"
input_file: tests/ui/run/join_points.em
---
--- stdout ---
a!?a
plain?b
cc
then
kept
--- stderr ---
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_core_ir --test lower 2>&1 | grep -E "^test "`
Expected: 新しい2件と、期待値を変えた2件が FAILED。ほかは ok

Run: `cargo test -p eml_interp --test run long_sequence_of_if_statements && cargo test -p eml_cli --test ui`
Expected: どちらも PASS (今の実装でも通る。回帰を防ぐためのテスト)

- [ ] **Step 3: 命令を足す**

`crates/eml_core_ir/src/lib.rs`:

- `Rhs::Nested` と、`Rhs::atoms` の `Rhs::Nested(_)` を消す。
- `CExprId` の後に足す。

```rust
/// 関数の中の join point の番号。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JoinId(pub u32);
```

- `CExpr` に足す (`Let` の後と `Switch` の後)。

```rust
    /// `scope` の中の `Jump` が `body` に入る。`param` は `Jump` が渡す値を受ける。末尾にない `if` の続きを、
    /// ヒープにフレームを積まずに実行するために使う (docs/spec/core-ir.md)。`body` の中からは `Jump` しない。
    Join {
        join: JoinId,
        param: VarId,
        body: CExprId,
        scope: CExprId,
    },
```

```rust
    Jump {
        join: JoinId,
        arg: Atom,
    },
```

- `CoreFn` に `joins` と `join` を足す。

```rust
    /// `JoinId` から `Join` の式を引く索引。式のアリーナを作り直すパスは、索引も作り直す。
    pub joins: Vec<CExprId>,
```

```rust
    /// `Jump` の行き先の、join point の引数と本体。
    pub fn join(&self, join: JoinId) -> (VarId, CExprId) {
        match self.expr(self.joins[join.0 as usize]) {
            CExpr::Join { param, body, .. } => (*param, *body),
            _ => unreachable!("the join index points at join points"),
        }
    }
```

- `mod liveness;` を足す。

- [ ] **Step 4: 生存解析を独立させる**

`crates/eml_core_ir/src/liveness.rs` を作る。

```rust
//! RC の対象の変数の生存 (docs/spec/core-ir.md)。Perceus の挿入と verifier が使う。

use std::collections::{BTreeSet, HashMap};

use crate::{Atom, CExpr, CExprId, CoreFn, JoinId, Linearity, VarId};

pub(crate) type Vars = BTreeSet<VarId>;

/// RC の対象 (`Unr` でボックス化した変数) かどうか。
pub(crate) fn tracked(function: &CoreFn) -> Vec<bool> {
    function
        .vars
        .iter()
        .map(|var| var.boxed && var.linearity == Linearity::Unr)
        .collect()
}

pub(crate) struct Liveness {
    /// 式ごとの、その式から先で使う RC の対象の変数。`Jump` の先の join point の本体で使う変数も含む。
    pub exprs: Vec<Vars>,
    /// join point ごとの、本体で使う RC の対象の変数 (引数を除く)。`Jump` の時点で、ちょうど1つずつ所有している。
    pub joins: HashMap<JoinId, Vars>,
}

enum Task {
    Visit(CExprId),
    Finish(CExprId),
    Needs {
        join: JoinId,
        param: VarId,
        body: CExprId,
    },
}

/// `Let` の連鎖と、join point の本体の連なりは長くなりうるので、再帰せずに作業の列で後順にたどる。join point の
/// 本体は範囲より先に求める。範囲の中の `Jump` が、本体で使う変数を要るためである。
pub(crate) fn liveness(function: &CoreFn, tracked: &[bool]) -> Liveness {
    let tracked_var = |atom: &Atom| match atom {
        Atom::Var(var) if tracked[var.0 as usize] => Some(*var),
        _ => None,
    };
    let mut exprs = vec![Vars::new(); function.exprs.len()];
    let mut joins: HashMap<JoinId, Vars> = HashMap::new();
    let mut work = vec![Task::Visit(function.body)];
    while let Some(task) = work.pop() {
        match task {
            Task::Visit(id) => {
                work.push(Task::Finish(id));
                match function.expr(id) {
                    CExpr::Let { body, .. } | CExpr::Dup { body, .. } | CExpr::Decref { body, .. } => {
                        work.push(Task::Visit(*body));
                    }
                    CExpr::Join {
                        join,
                        param,
                        body,
                        scope,
                    } => {
                        work.push(Task::Visit(*scope));
                        work.push(Task::Needs {
                            join: *join,
                            param: *param,
                            body: *body,
                        });
                        work.push(Task::Visit(*body));
                    }
                    CExpr::Switch { arms, .. } => {
                        work.extend(arms.iter().map(|&(_, arm)| Task::Visit(arm)));
                    }
                    CExpr::Return(_) | CExpr::Jump { .. } => {}
                }
            }
            Task::Needs { join, param, body } => {
                let mut needs = exprs[body.0 as usize].clone();
                needs.remove(&param);
                joins.insert(join, needs);
            }
            Task::Finish(id) => {
                let vars = match function.expr(id) {
                    CExpr::Let { var, rhs, body } => {
                        let mut vars = exprs[body.0 as usize].clone();
                        vars.remove(var);
                        vars.extend(rhs.atoms().iter().filter_map(tracked_var));
                        vars
                    }
                    CExpr::Dup { var, body } | CExpr::Decref { var, body } => {
                        let mut vars = exprs[body.0 as usize].clone();
                        if tracked[var.0 as usize] {
                            vars.insert(*var);
                        }
                        vars
                    }
                    CExpr::Join { join, scope, .. } => {
                        let mut vars = joins.get(join).cloned().unwrap_or_default();
                        vars.extend(exprs[scope.0 as usize].iter().copied());
                        vars
                    }
                    CExpr::Switch { scrutinee, arms } => {
                        let mut vars: Vars = tracked_var(scrutinee).into_iter().collect();
                        for &(_, arm) in arms {
                            vars.extend(exprs[arm.0 as usize].iter().copied());
                        }
                        vars
                    }
                    CExpr::Return(atom) => tracked_var(atom).into_iter().collect(),
                    // 壊れた Core IR (範囲の外の `Jump`) は verifier が報告するので、ここでは空として扱う
                    CExpr::Jump { join, arg } => {
                        let mut vars = joins.get(join).cloned().unwrap_or_default();
                        vars.extend(tracked_var(arg));
                        vars
                    }
                };
                exprs[id.0 as usize] = vars;
            }
        }
    }
    Liveness { exprs, joins }
}
```

- [ ] **Step 5: 変換が join point を作る**

`crates/eml_core_ir/src/lower.rs`:

- `type Bindings = Vec<(VarId, Rhs)>;` を次にする。

```rust
/// 値を計算する束縛と、join point の開始の並び。`seq` が後ろから組み立てる。
enum Binding {
    Let(VarId, Rhs),
    /// ここより後ろで組み立てる式を本体にし、`scope` (枝が `Jump` する `Switch`) を範囲にする join point。
    Join {
        join: JoinId,
        param: VarId,
        scope: CExprId,
    },
}

type Bindings = Vec<Binding>;

/// 式の値の渡し先。
#[derive(Clone, Copy)]
enum Exit {
    Return,
    Jump(JoinId),
}

fn exit_with(exit: Exit, value: Atom) -> CExpr {
    match exit {
        Exit::Return => CExpr::Return(value),
        Exit::Jump(join) => CExpr::Jump { join, arg: value },
    }
}
```

- `FnLowering` に `joins: Vec<Option<CExprId>>` を足す (2か所の `FnLowering { ... }` で `joins: Vec::new()`)。`lower` の最後の `CoreFn` に `joins: self.joins.into_iter().map(|join| join.expect("every join point is built")).collect()` を足し、`let root = self.tail(root, Exit::Return);` にする。
- `bind` は `out.push(Binding::Let(var, rhs));` にする。
- `ProgramBuilder::wrapper` の `CoreFn` に `joins: Vec::new()` を足す。
- `seq`、`tail` を次にし、`tail_expr` と `stmts` を足す。

```rust
    fn seq(&mut self, bindings: Bindings, last: CExpr) -> CExprId {
        let mut id = self.push(last);
        for binding in bindings.into_iter().rev() {
            id = match binding {
                Binding::Let(var, rhs) => self.push(CExpr::Let { var, rhs, body: id }),
                Binding::Join { join, param, scope } => {
                    let expr = self.push(CExpr::Join {
                        join,
                        param,
                        body: id,
                        scope,
                    });
                    self.joins[join.0 as usize] = Some(expr);
                    expr
                }
            };
        }
        id
    }

    /// 式の値を `exit` に渡すコード。
    fn tail(&mut self, expr: ExprId, exit: Exit) -> CExprId {
        let mut bindings = Vec::new();
        let last = self.tail_expr(expr, exit, &mut bindings);
        self.seq(bindings, last)
    }

    /// 式の値を `exit` に渡す最後の命令を返す。値の計算に要る束縛は `out` に積む。末尾の `if` は、枝が直接 `exit` に
    /// 渡す `Switch` にし、join point を作らない。
    fn tail_expr(&mut self, id: ExprId, exit: Exit, out: &mut Bindings) -> CExpr {
        let body = self.body;
        match &body.exprs[id].kind {
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let scrutinee = self.atom(*condition, out);
                let then_code = self.tail(*then_branch, exit);
                let else_code = match else_branch {
                    Some(else_branch) => self.tail(*else_branch, exit),
                    // `else` のない `if` の値は `()` である
                    None => self.push(exit_with(exit, Atom::Unit)),
                };
                CExpr::Switch {
                    scrutinee,
                    arms: vec![(FALSE, else_code), (TRUE, then_code)],
                }
            }
            ExprKind::Block { stmts, tail } => {
                self.stmts(stmts, out);
                match tail {
                    Some(tail) => self.tail_expr(*tail, exit, out),
                    None => exit_with(exit, Atom::Unit),
                }
            }
            ExprKind::Annot { expr, .. } => self.tail_expr(*expr, exit, out),
            _ => {
                let value = self.atom(id, out);
                exit_with(exit, value)
            }
        }
    }

    fn stmts(&mut self, stmts: &[Stmt], out: &mut Bindings) {
        for stmt in stmts {
            match stmt {
                Stmt::Let { pat, init, .. } => {
                    let value = self.atom(*init, out);
                    self.bind_pat(*pat, value);
                }
                // 式文の値は `Unit` なので捨ててよい
                Stmt::Expr(expr) => {
                    self.atom(*expr, out);
                }
            }
        }
    }
```

- `atom` の `ExprKind::If` と `ExprKind::Block` の腕を次にする。

```rust
            ExprKind::If { .. } => {
                // 続きの式を join point の本体にし、`if` の値をその引数で受ける
                let join = JoinId(self.joins.len() as u32);
                self.joins.push(None);
                let last = self.tail_expr(id, Exit::Jump(join), out);
                let scope = self.push(last);
                let ty = self.ty(id);
                let param = self.new_var("t", &ty);
                out.push(Binding::Join { join, param, scope });
                Atom::Var(param)
            }
            ExprKind::Block { stmts, tail } => {
                self.stmts(stmts, out);
                match tail {
                    Some(tail) => self.atom(*tail, out),
                    None => Atom::Unit,
                }
            }
```

(`use crate::{...}` に `JoinId` を足す。)

- [ ] **Step 6: Perceus が join point を扱う**

`crates/eml_core_ir/src/perceus.rs` を次にする。

```rust
//! Perceus の `dup` / `decref` の挿入 (docs/spec/core-ir.md)。変数を使うことを所有権の移動として扱い、後でも使う
//! 変数を複製し、使わなくなった変数をできるだけ早く捨てる。対象は `Unr` でボックス化した変数だけである。

use std::collections::{BTreeMap, HashMap};

use crate::liveness::{Liveness, Vars, liveness, tracked};
use crate::{Atom, CExpr, CExprId, CoreFn, JoinId, Program, Rhs, VarId};

/// 変換の後に、プログラム全体にかける。変換の途中の関数ごとではなく、独立したパスにする (docs/spec/core-ir.md)。
pub(crate) fn insert(program: &mut Program) {
    for function in &mut program.functions {
        insert_rc(function);
    }
}

fn insert_rc(function: &mut CoreFn) {
    let tracked = tracked(function);
    let Liveness { exprs, joins } = liveness(function, &tracked);
    let mut pass = Pass {
        old: &function.exprs,
        tracked: &tracked,
        free: exprs,
        needs: joins,
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
    /// 式ごとの、その式から先で使う変数。
    free: Vec<Vars>,
    /// join point ごとの、本体で使う変数。
    needs: HashMap<JoinId, Vars>,
    new: Vec<CExpr>,
    joins: Vec<Option<CExprId>>,
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
        atoms.iter().filter_map(|atom| self.atom_var(atom)).collect()
    }

    fn push(&mut self, expr: CExpr) -> CExprId {
        self.new.push(expr);
        CExprId(self.new.len() as u32 - 1)
    }

    /// `owned` は入口で所有している変数。どの経路も `Return` か `Jump` で終わり、その時点で渡すもの以外は所有して
    /// いない。
    ///
    /// `Let` の連鎖と join point の本体の連なりは長くなりうるので、その向きはループで歩いて各段を記録し、最後に
    /// 逆順で組み立てる。再帰するのは `Switch` の枝と join point の範囲だけで、深さは E0013 の入れ子の制限で抑えられる。
    fn transform(&mut self, id: CExprId, owned: &Vars) -> CExprId {
        let old = self.old;
        let mut steps: Vec<Step> = Vec::new();
        let mut id = id;
        let mut owned = owned.clone();
        let mut code = loop {
            match &old[id.0 as usize] {
                CExpr::Return(atom) => break self.transform_return(*atom, &owned),
                CExpr::Jump { join, arg } => break self.transform_jump(*join, *arg, &owned),
                CExpr::Switch { scrutinee, arms } => {
                    let arms = arms
                        .iter()
                        .map(|&(tag, arm)| (tag, self.transform(arm, &owned)))
                        .collect();
                    break self.push(CExpr::Switch {
                        scrutinee: *scrutinee,
                        arms,
                    });
                }
                CExpr::Join {
                    join,
                    param,
                    body,
                    scope,
                } => {
                    // 範囲は今の所有から始まる。本体は、引数と、本体で使う変数を1つずつ所有して始まる
                    let scope = self.transform(*scope, &owned);
                    steps.push(Step::Join {
                        join: *join,
                        param: *param,
                        scope,
                    });
                    owned = self.needs.get(join).cloned().unwrap_or_default();
                    if self.tracked[param.0 as usize] {
                        owned.insert(*param);
                    }
                    id = *body;
                }
                CExpr::Let { var, rhs, body } => {
                    let uses = self.uses(&rhs.atoms());
                    let mut after = self.free[body.0 as usize].clone();
                    after.remove(var);
                    let next: Vars = owned.intersection(&after).copied().collect();
                    steps.push(Step::Let {
                        var: *var,
                        rhs: rhs.clone(),
                        uses,
                        after,
                        owned: std::mem::replace(&mut owned, next),
                    });
                    if self.tracked[var.0 as usize] {
                        owned.insert(*var);
                    }
                    id = *body;
                }
                CExpr::Dup { .. } | CExpr::Decref { .. } => {
                    unreachable!("the pass runs once on code without RC instructions")
                }
            }
        };
        for step in steps.into_iter().rev() {
            match step {
                Step::Join { join, param, scope } => {
                    code = self.push(CExpr::Join {
                        join,
                        param,
                        body: code,
                        scope,
                    });
                    self.joins[join.0 as usize] = Some(code);
                }
                Step::Let {
                    var,
                    rhs,
                    uses,
                    after,
                    owned,
                } => {
                    code = self.push(CExpr::Let {
                        var,
                        rhs,
                        body: code,
                    });
                    code = self.release_and_duplicate(code, &owned, &uses, &after);
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
        after: &Vars,
    ) -> CExprId {
        for &dead in owned.iter().rev() {
            if !uses.contains(&dead) && !after.contains(&dead) {
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
            let dups = count - usize::from(!after.contains(&used));
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

    /// join point の本体は、本体で使う変数をちょうど1つずつ所有して始まる。それ以外を捨て、渡す値を本体でも使う
    /// なら複製する。
    fn transform_jump(&mut self, join: JoinId, arg: Atom, owned: &Vars) -> CExprId {
        let needs = self.needs.get(&join).cloned().unwrap_or_default();
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
        scope: CExprId,
    },
    Let {
        var: VarId,
        rhs: Rhs,
        uses: Vec<VarId>,
        after: Vars,
        /// この束縛の入口で所有している変数
        owned: Vars,
    },
}
```

- [ ] **Step 7: 表示とインタプリタを追随させる**

`crates/eml_core_ir/src/pretty.rs` の `expr` の `match` に足し、`rhs_text` の `Rhs::Nested` の腕と、`Let` の `if let Rhs::Nested(inner) = rhs` の分岐を消す (`Let` はつねに1行で表示する)。

```rust
            CExpr::Join {
                join,
                param,
                body,
                scope,
            } => {
                writeln!(out, "{pad}join j{}({}) {{", join.0, var(function, *param)).unwrap();
                expr(program, function, *body, indent + 1, out);
                writeln!(out, "{pad}}}").unwrap();
                id = *scope;
            }
            CExpr::Jump { join, arg } => {
                writeln!(out, "{pad}jump j{}({})", join.0, atom(function, arg)).unwrap();
                return;
            }
```

`crates/eml_interp/src/lib.rs`:

- `bind` の `Rhs::Nested(inner)` の腕を消す。
- `push_frame` から `save_env` を外し、つねに環境を退避する。

```rust
    /// 呼び出しでは環境ごと退避する。
    fn push_frame(&mut self, bind: VarId, resume: CExprId) {
        let frame = Frame {
            function: self.function.0,
            resume: resume.0,
            bind: bind.0,
            slots: Some(std::mem::take(&mut self.slots)),
            next: Some(self.cont),
        };
        self.cont = self.heap.alloc(DescId::FRAME, Payload::Frame(frame));
    }
```

(`call` の `self.push_frame(var, body, true)` を `self.push_frame(var, body)` にする。)

- `step` の `match` に足す。

```rust
            CExpr::Join { scope, .. } => self.control = *scope,
            CExpr::Jump { join, arg } => {
                // join point は同じ関数の中にあるので、環境をそのまま使い、フレームを積まない
                let value = self.atom(arg)?;
                let (param, body) = program.function(self.function).join(*join);
                self.slots[param.0 as usize] = Some(Owned::new(value));
                self.control = body;
            }
```

- [ ] **Step 8: 手書きの Core IR を追随させる (種類3)**

`crates/eml_interp/tests/closures.rs` の `function` が作る `CoreFn` と、`run.rs` の `leaking_program` の `CoreFn` に `joins: Vec::new()` を足す。期待値は変えない。

- [ ] **Step 9: 確かめる**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED"`
Expected: 何も出ない

Run: `git diff --no-ext-diff --stat -- '*.snap' crates/*/tests tests`
Expected: `crates/eml_core_ir/tests/lower.rs`、`crates/eml_interp/tests/run.rs`、`crates/eml_interp/tests/closures.rs` と、新しい `join_points.em` とそのスナップショットだけ

Run: `grep -rn "Nested\|save_env" crates/*/src`
Expected: 何も出ない

- [ ] **Step 10: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates tests
git commit -m "Lower non-tail ifs to join points instead of nested expressions"
```

---

### Task 5: 末尾呼び出し

**Files:**
- Modify: `crates/eml_core_ir/src/lib.rs`、`lower.rs`、`perceus.rs`、`liveness.rs`、`pretty.rs`
- Modify: `crates/eml_interp/src/lib.rs`
- Test: `crates/eml_core_ir/tests/lower.rs`、`tests/ui/run/tail_calls.em`

**Interfaces:**
- Consumes: Task 2 の `Call` と `Machine::call`、Task 4 の `Binding`、`Pass::release_and_duplicate`、`liveness`
- Produces: `CExpr::TailCall(Call)`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/lower.rs` に足す。

```rust
#[test]
fn calls_in_tail_position_are_tail_calls() {
    let text = "loop : Int -> Int -> Int\nloop n acc = if n == 0 then acc else loop (n - 1) (acc + 1)\n\ncall_twice : (Int -> Int) -> Int -> Int\ncall_twice f x = f (f x)\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text), @r"
    fn loop(n0, acc1) {
      let t2 = prim ==(n0, 0)
      switch t2 {
        #0 ->
          let t3 = prim -(n0, 1)
          let t4 = prim +(acc1, 1)
          tailcall loop(t3, t4)
        #1 ->
          return acc1
      }
    }
    fn call_twice(f0, x1) {
      dup f0
      let t2 = apply f0(x1)
      tailcall apply f0(t2)
    }
    fn main(p0) {
      return ()
    }
    ");
}
```

既存の3件の期待値を次にする (種類2)。

`recursion_and_top_level_values` の `count` を次にする (`answer` と `main` は Task 4 のまま)。

```
    fn count(n0) {
      let t1 = prim ==(n0, 0)
      switch t1 {
        #0 ->
          let t2 = prim -(n0, 1)
          tailcall count(t2)
        #1 ->
          tailcall answer()
      }
    }
```

`builtins_used_as_values_are_wrapped` の全体:

```
    fn apply(f0, x1) {
      tailcall apply f0(x1)
    }
    fn main(p0) {
      let c1 = closure builtin$not()
      let c2 = closure builtin$not()
      let c3 = closure builtin$>>(c1, c2)
      decref c3
      let c4 = closure builtin$println()
      let t5 = prim show_int(1)
      tailcall apply(c4, t5)
    }
    fn builtin$not(p0) {
      let t1 = prim not(p0)
      return t1
    }
    fn builtin$>>(p0, p1, p2) {
      let t3 = apply p0(p2)
      tailcall apply p1(t3)
    }
    fn builtin$println(p0) {
      let t1 = perform println(p0)
      return t1
    }
```

`lambdas_are_lifted_with_their_captures_first` の `apply` を次にする (`main` と `main$lambda0` は今のまま)。

```
    fn apply(f0, x1) {
      tailcall apply f0(x1)
    }
```

`tests/ui/run/tail_calls.em` を作る。

```
-- Tail calls do not push frames: a loop of a million iterations, and a call in tail position
-- whose result is applied to the remaining argument.
loop : Int -> Int -> Int
loop n acc = if n == 0 then acc else loop (n - 1) (acc + 1)

add : Int -> Int -> Int
add a b = a + b

adder : Int -> Int -> Int
adder x = add x

sum : Int -> Int -> Int
sum a b = adder a b

main : Unit -> <IO> Unit
main () =
  println (show_int (loop 1000000 0))
  println (show_int (sum 3 4))
```

`crates/eml_cli/tests/snapshots/ui__run@tail_calls.em.snap` を作る。

```
---
source: crates/eml_cli/tests/ui.rs
expression: "format!(\"--- stdout ---\\n{stdout}--- stderr ---\\n{stderr}\")"
input_file: tests/ui/run/tail_calls.em
---
--- stdout ---
1000000
7
--- stderr ---
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_core_ir --test lower 2>&1 | grep -E "^test "`
Expected: 新しい1件と、期待値を変えた3件が FAILED

Run: `cargo test -p eml_cli --test ui`
Expected: PASS (今の実装でもヒープのフレームで通る。回帰を防ぐためのテスト)

- [ ] **Step 3: 命令を足し、変換が末尾呼び出しを作る**

`crates/eml_core_ir/src/lib.rs` の `CExpr` の `Return` の後に足す。

```rust
    /// 関数の末尾の呼び出し。呼び出し元のフレームを積まない。
    TailCall(Call),
```

`crates/eml_core_ir/src/lower.rs` の `tail` を次にする。

```rust
    /// 式の値を `exit` に渡すコード。値を返すだけの呼び出しは、呼び出し元のフレームを積まない末尾呼び出しにする。
    fn tail(&mut self, expr: ExprId, exit: Exit) -> CExprId {
        let mut bindings = Vec::new();
        let mut last = self.tail_expr(expr, exit, &mut bindings);
        if let CExpr::Return(Atom::Var(returned)) = last
            && let Some(Binding::Let(bound, Rhs::Call(_))) = bindings.last()
            && *bound == returned
        {
            let Some(Binding::Let(_, Rhs::Call(call))) = bindings.pop() else {
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

`ProgramBuilder::wrapper` で、本体を「束縛の列と最後の命令」で作る形にし、合成の2回目の呼び出しを末尾呼び出しにする。

```rust
        let (steps, last): (Vec<(VarId, Rhs)>, CExpr) = match lowering(builtin) {
            Lowering::Prim(op) => {
                let result = fresh(&result_type);
                (vec![(result, Rhs::Prim(op, atoms))], CExpr::Return(Atom::Var(result)))
            }
            Lowering::Perform(op) => {
                let result = fresh(&result_type);
                (vec![(result, Rhs::Perform(op, atoms))], CExpr::Return(Atom::Var(result)))
            }
            Lowering::Compose { forward } => {
                let (inner, outer) = if forward { (0, 1) } else { (1, 0) };
                let (_, middle_type) = split_arrows(&param_types[inner], 1);
                let middle = fresh(&middle_type);
                (
                    vec![(middle, Rhs::Call(Call::Apply(atoms[inner], vec![atoms[2]])))],
                    CExpr::TailCall(Call::Apply(atoms[outer], vec![Atom::Var(middle)])),
                )
            }
            Lowering::Constructor(_) => unreachable!("constructors are values, not functions"),
        };
        let mut exprs = vec![last];
        let mut body = CExprId(0);
        for (var, rhs) in steps.into_iter().rev() {
            exprs.push(CExpr::Let { var, rhs, body });
            body = CExprId(exprs.len() as u32 - 1);
        }
```

(その後の `CoreFn { ... }` の組み立ては今のまま。`let last = steps.last()...` と `exprs.push(CExpr::Return(...))` の行は消す。)

- [ ] **Step 4: Perceus、生存解析、表示、インタプリタを追随させる**

`crates/eml_core_ir/src/liveness.rs`:

- `Task::Visit` の `CExpr::Return(_) | CExpr::Jump { .. }` を `CExpr::Return(_) | CExpr::Jump { .. } | CExpr::TailCall(_)` にする。
- `Task::Finish` に足す。

```rust
                    CExpr::TailCall(call) => call.atoms().iter().filter_map(tracked_var).collect(),
```

`crates/eml_core_ir/src/perceus.rs`:

- `transform` のループに足す。

```rust
                CExpr::TailCall(call) => break self.transform_tail_call(call, &owned),
```

- `transform_return` の後に足す (`use crate::{...}` に `Call` を足す)。

```rust
    /// 末尾呼び出しの後で使う変数はないので、呼び出しが使わない変数をすべて捨てる。
    fn transform_tail_call(&mut self, call: &Call, owned: &Vars) -> CExprId {
        let uses = self.uses(&call.atoms());
        let code = self.push(CExpr::TailCall(call.clone()));
        self.release_and_duplicate(code, owned, &uses, &Vars::new())
    }
```

- `transform` の doc の「どの経路も `Return` か `Jump` で終わり」を「どの経路も `Return`、`TailCall`、`Jump` で終わり」にする。

`crates/eml_core_ir/src/pretty.rs`:

- `rhs_text` の呼び出しの2つの腕を、`call_text` を使う形にする。

```rust
        Rhs::Call(call @ Call::Direct(..)) => format!("call {}", call_text(program, function, call)),
        Rhs::Call(call) => call_text(program, function, call),
```

```rust
fn call_text(program: &Program, function: &CoreFn, call: &Call) -> String {
    let args = |args: &[Atom]| {
        args.iter()
            .map(|a| atom(function, a))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match call {
        Call::Direct(callee, a) => format!("{}({})", program.function(*callee).name, args(a)),
        Call::Apply(callee, a) => format!("apply {}({})", atom(function, callee), args(a)),
    }
}
```

- `expr` の `match` に足す。

```rust
            CExpr::TailCall(call) => {
                writeln!(out, "{pad}tailcall {}", call_text(program, function, call)).unwrap();
                return;
            }
```

`crates/eml_interp/src/lib.rs` の `step` の `match` に足す。

```rust
            // 呼び出し元のフレームを積まない。verifier が、この時点で所有している参照が残っていないことを保証するので、
            // 今の環境はそのまま捨ててよい (docs/spec/core-ir.md)
            CExpr::TailCall(call) => return self.call(call, None),
```

- [ ] **Step 5: 確かめる**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED"`
Expected: 何も出ない

Run: `git diff --no-ext-diff --stat -- '*.snap' crates/*/tests tests`
Expected: `crates/eml_core_ir/tests/lower.rs` と、新しい `tail_calls.em` とそのスナップショットだけ

- [ ] **Step 6: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates tests
git commit -m "Lower calls in tail position to tail calls"
```

---

### Task 6: 入口の関数

**Files:**
- Modify: `crates/eml_core_ir/src/lib.rs`、`lower.rs`
- Modify: `crates/eml_interp/src/lib.rs`
- Modify: `crates/eml_interp/tests/run.rs`、`closures.rs` (種類3)
- Test: `crates/eml_core_ir/tests/lower.rs`

**Interfaces:**
- Consumes: Task 5 の `CExpr::TailCall`、Task 3 の `var_info`
- Produces: `Program::entry: FnIdx` (`Program::main` は無くなる)、`ProgramBuilder::entry(&mut self, main: FnIdx, main_type: &Type) -> FnIdx`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/lower.rs` に足す。

```rust
#[test]
fn the_entry_applies_a_point_free_main_to_unit() {
    let text = "main : Unit -> <IO> Unit\nmain = fn () -> println \"point-free\"";
    insta::assert_snapshot!(core_text(text), @r#"
    fn main() {
      let c0 = closure main$lambda0()
      return c0
    }
    fn main$lambda0(p0) {
      let s1 = const "point-free"
      let t2 = perform println(s1)
      return t2
    }
    fn entry$main() {
      let f0 = call main()
      tailcall apply f0(())
    }
    "#);
}
```

ほかのすべての Core IR のスナップショット (既存の9件と、Task 4 と Task 5 で足した3件) の末尾に、次の関数を足す (種類2。足したテストはこのプランの中の変更)。

```
    fn entry$main() {
      tailcall main(())
    }
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_core_ir --test lower 2>&1 | grep -E "^test "`
Expected: 13件すべてが FAILED

- [ ] **Step 3: 実装する**

`crates/eml_core_ir/src/lib.rs` の `Program` の `main` を次にする。

```rust
    /// 実行の入口。`main` を `()` で呼ぶ、引数のない関数 (docs/spec/core-ir.md)。
    pub entry: FnIdx,
```

`crates/eml_core_ir/src/lower.rs`:

- `ProgramBuilder` に足す。

```rust
    /// 実行の入口。`main : Unit -> <IO> Unit` を `()` で呼ぶ。等式に引数のない `main = fn () -> ...` は関数値を返す
    /// ので、返った値に `()` を適用する (docs/spec/core-ir.md)。
    fn entry(&mut self, main: FnIdx, main_type: &Type) -> FnIdx {
        let function = self.reserve(0);
        let unit = vec![Atom::Unit];
        let (vars, exprs) = if self.arity(main) == 0 {
            let value = VarId(0);
            (
                vec![var_info("f", main_type, &self.lang)],
                vec![
                    CExpr::TailCall(Call::Apply(Atom::Var(value), unit)),
                    CExpr::Let {
                        var: value,
                        rhs: Rhs::Call(Call::Direct(main, Vec::new())),
                        body: CExprId(0),
                    },
                ],
            )
        } else {
            (Vec::new(), vec![CExpr::TailCall(Call::Direct(main, unit))])
        };
        let core = CoreFn {
            name: "entry$main".to_string(),
            params: Vec::new(),
            vars,
            body: CExprId(exprs.len() as u32 - 1),
            exprs,
            joins: Vec::new(),
        };
        self.finish(function, core);
        function
    }
```

- `lower` の最後で、`let main = typed.main.expect(...)` の後に `let entry = builder.entry(indices[main], &typed.signatures.get(main).expect("`main` has a signature").ty);` を置き、`Program` の `main: indices[main]` を `entry` にする。

`crates/eml_interp/src/lib.rs` の `Machine::new` を次にする (`ApplyFrame` を使うのは `apply` だけになる)。

```rust
    fn new(program: &'p Program, out: &'p OutputSink) -> Self {
        let mut heap = Heap::new();
        let cont = heap.alloc(
            DescId::FRAME,
            Payload::Frame(Frame {
                function: IO_HANDLER,
                resume: 0,
                bind: 0,
                slots: None,
                next: None,
            }),
        );
        let entry = program.function(program.entry);
        Machine {
            program,
            out,
            heap,
            function: program.entry,
            control: entry.body,
            slots: vec![None; entry.vars.len()],
            cont,
        }
    }
```

- [ ] **Step 4: 手書きの Core IR を追随させる (種類3)**

- `crates/eml_interp/tests/closures.rs`: `run_program` の `Program` の `main: FnIdx(main)` を `entry: FnIdx(main)` にし、各テストの `function("main", 1, ...)` を `function("main", 0, ...)` にする (入口は引数を取らない。変数の表は変えない)。
- `crates/eml_interp/tests/run.rs`: `leaking_program` の `params: vec![VarId(0)]` を `params: vec![]` に、`main: FnIdx(0)` を `entry: FnIdx(0)` にする。

期待値は変えない。

- [ ] **Step 5: 確かめる**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED"`
Expected: 何も出ない

Run: `git diff --no-ext-diff --stat -- '*.snap' crates/*/tests tests`
Expected: `crates/eml_core_ir/tests/lower.rs`、`crates/eml_interp/tests/run.rs`、`crates/eml_interp/tests/closures.rs` だけ

Run: `grep -n "main.params\|ApplyFrame" crates/eml_interp/src/lib.rs`
Expected: `ApplyFrame` は `apply` と `ret` の中だけに出る。`main.params` は出ない

- [ ] **Step 6: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates
git commit -m "Build the entry function in Core IR instead of the interpreter"
```

---

### Task 7: verifier

**Files:**
- Create: `crates/eml_core_ir/src/verify.rs`
- Modify: `crates/eml_core_ir/src/lib.rs`、`lower.rs`
- Test: `crates/eml_core_ir/tests/verify.rs`

**Interfaces:**
- Consumes: Task 4 の `liveness`、`tracked`、Task 2 の `Rhs::atoms`、`Call::atoms`
- Produces: `eml_core_ir::verify(program: &Program) -> Result<(), VerifyError>`、`eml_core_ir::VerifyError { pub function: String, pub message: String }` (`Display` は `"{message} in `{function}`"`)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/verify.rs` を作る。

```rust
//! 手で組んだ Core IR で、verifier が正しいものを受け入れ、壊れたものを拒むことを確かめる (docs/spec/core-ir.md)。

use eml_core_ir::{
    Atom, CExpr, CExprId, Call, CoreFn, FnIdx, JoinId, Linearity, PrimOp, Program, Rhs, VarId,
    VarInfo, verify,
};

fn string(name: &str) -> VarInfo {
    VarInfo {
        name: name.to_string(),
        linearity: Linearity::Unr,
        boxed: true,
    }
}

fn int(name: &str) -> VarInfo {
    VarInfo {
        name: name.to_string(),
        linearity: Linearity::Unr,
        boxed: false,
    }
}

/// `exprs` は子を親より先に並べ、最後の式を本体にする。
fn function(name: &str, params: u32, vars: Vec<VarInfo>, exprs: Vec<CExpr>, joins: &[u32]) -> CoreFn {
    CoreFn {
        name: name.to_string(),
        params: (0..params).map(VarId).collect(),
        vars,
        body: CExprId(exprs.len() as u32 - 1),
        exprs,
        joins: joins.iter().map(|&id| CExprId(id)).collect(),
    }
}

fn check(functions: Vec<CoreFn>) -> Result<(), String> {
    let program = Program {
        functions,
        entry: FnIdx(0),
        strings: vec!["s".to_string()],
    };
    verify(&program).map_err(|error| error.to_string())
}

fn var(n: u32) -> Atom {
    Atom::Var(VarId(n))
}

fn concat(var: u32, left: u32, right: u32, body: u32) -> CExpr {
    CExpr::Let {
        var: VarId(var),
        rhs: Rhs::Prim(PrimOp::StrConcat, vec![self::var(left), self::var(right)]),
        body: CExprId(body),
    }
}

/// `pick b s = let t = if b then s else "none" in t ++ s` の Perceus の後の形。
fn pick(dup_before_jump: bool) -> CoreFn {
    let mut exprs = vec![
        CExpr::Return(var(4)),
        concat(4, 3, 1, 0),
        CExpr::Jump {
            join: JoinId(0),
            arg: var(2),
        },
        CExpr::Let {
            var: VarId(2),
            rhs: Rhs::ConstString(0),
            body: CExprId(2),
        },
        CExpr::Jump {
            join: JoinId(0),
            arg: var(1),
        },
    ];
    let then_arm = if dup_before_jump {
        exprs.push(CExpr::Dup {
            var: VarId(1),
            body: CExprId(4),
        });
        5
    } else {
        4
    };
    exprs.push(CExpr::Switch {
        scrutinee: var(0),
        arms: vec![(0, CExprId(3)), (1, CExprId(then_arm))],
    });
    let switch = exprs.len() as u32 - 1;
    exprs.push(CExpr::Join {
        join: JoinId(0),
        param: VarId(3),
        body: CExprId(1),
        scope: CExprId(switch),
    });
    let join = exprs.len() as u32 - 1;
    let vars = vec![int("b"), string("s"), string("s"), string("t"), string("t")];
    function("pick", 2, vars, exprs, &[join])
}

#[test]
fn a_duplicated_string_used_twice_is_accepted() {
    let exprs = vec![
        CExpr::Return(var(1)),
        concat(1, 0, 0, 0),
        CExpr::Dup {
            var: VarId(0),
            body: CExprId(1),
        },
    ];
    let twice = function("twice", 1, vec![string("s"), string("t")], exprs, &[]);
    assert_eq!(check(vec![twice]), Ok(()));
}

#[test]
fn a_join_point_is_accepted() {
    assert_eq!(check(vec![pick(true)]), Ok(()));
}

#[test]
fn a_tail_call_that_takes_every_owned_value_is_accepted() {
    let id = function("id", 1, vec![string("s")], vec![CExpr::Return(var(0))], &[]);
    let exprs = vec![CExpr::TailCall(Call::Direct(FnIdx(0), vec![var(0)]))];
    let caller = function("caller", 1, vec![string("s")], exprs, &[]);
    assert_eq!(check(vec![id, caller]), Ok(()));
}

#[test]
fn a_variable_bound_twice_is_rejected() {
    let exprs = vec![
        CExpr::Return(var(0)),
        CExpr::Let {
            var: VarId(0),
            rhs: Rhs::ConstString(0),
            body: CExprId(0),
        },
        CExpr::Let {
            var: VarId(0),
            rhs: Rhs::ConstString(0),
            body: CExprId(1),
        },
    ];
    let f = function("f", 0, vec![string("s")], exprs, &[]);
    assert_eq!(check(vec![f]), Err("`s0` is bound twice in `f`".to_string()));
}

#[test]
fn a_variable_used_outside_its_scope_is_rejected() {
    let f = function("f", 0, vec![string("s")], vec![CExpr::Return(var(0))], &[]);
    assert_eq!(
        check(vec![f]),
        Err("`s0` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn a_jump_outside_its_join_scope_is_rejected() {
    let exprs = vec![
        CExpr::Jump {
            join: JoinId(0),
            arg: Atom::Int(1),
        },
        CExpr::Jump {
            join: JoinId(0),
            arg: Atom::Int(2),
        },
        CExpr::Join {
            join: JoinId(0),
            param: VarId(0),
            body: CExprId(0),
            scope: CExprId(1),
        },
    ];
    let f = function("f", 0, vec![int("t")], exprs, &[2]);
    assert_eq!(
        check(vec![f]),
        Err("a jump to `j0` is outside its scope in `f`".to_string())
    );
}

#[test]
fn a_use_after_a_move_is_rejected() {
    let exprs = vec![CExpr::Return(var(1)), concat(1, 0, 0, 0)];
    let twice = function("twice", 1, vec![string("s"), string("t")], exprs, &[]);
    assert_eq!(
        check(vec![twice]),
        Err("`s0` is used after it was moved in `twice`".to_string())
    );
}

#[test]
fn a_missing_decref_is_rejected() {
    let ignore = function("ignore", 1, vec![string("s")], vec![CExpr::Return(Atom::Int(1))], &[]);
    assert_eq!(
        check(vec![ignore]),
        Err("`s0` is still owned at the end of the function in `ignore`".to_string())
    );
}

#[test]
fn a_double_decref_is_rejected() {
    let exprs = vec![
        CExpr::Return(Atom::Int(1)),
        CExpr::Decref {
            var: VarId(0),
            body: CExprId(0),
        },
        CExpr::Decref {
            var: VarId(0),
            body: CExprId(1),
        },
    ];
    let ignore = function("ignore", 1, vec![string("s")], exprs, &[]);
    assert_eq!(
        check(vec![ignore]),
        Err("`s0` is released after it was moved in `ignore`".to_string())
    );
}

#[test]
fn a_jump_that_owns_too_much_is_rejected() {
    let exprs = vec![
        CExpr::Return(var(1)),
        CExpr::Jump {
            join: JoinId(0),
            arg: Atom::Int(1),
        },
        CExpr::Join {
            join: JoinId(0),
            param: VarId(1),
            body: CExprId(0),
            scope: CExprId(1),
        },
    ];
    let f = function("f", 1, vec![string("s"), int("t")], exprs, &[2]);
    assert_eq!(
        check(vec![f]),
        Err("a jump to `j0` owns [s0] but its join needs [] in `f`".to_string())
    );
}

#[test]
fn a_jump_that_owns_too_little_is_rejected() {
    assert_eq!(
        check(vec![pick(false)]),
        Err("a jump to `j0` owns [] but its join needs [s1] in `pick`".to_string())
    );
}

#[test]
fn a_direct_call_with_the_wrong_number_of_arguments_is_rejected() {
    let g = function("g", 1, vec![int("a")], vec![CExpr::Return(var(0))], &[]);
    let exprs = vec![
        CExpr::Return(var(0)),
        CExpr::Let {
            var: VarId(0),
            rhs: Rhs::Call(Call::Direct(FnIdx(0), vec![Atom::Int(1), Atom::Int(2)])),
            body: CExprId(0),
        },
    ];
    let f = function("f", 0, vec![int("t")], exprs, &[]);
    assert_eq!(
        check(vec![g, f]),
        Err("a direct call to `g` passes 2 arguments, but it takes 1 in `f`".to_string())
    );
}
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_core_ir --test verify`
Expected: コンパイルエラー (`no function verify in the crate root`)

- [ ] **Step 3: 実装する**

`crates/eml_core_ir/src/verify.rs` を作る。

```rust
//! Core IR の不変条件の検査 (docs/spec/core-ir.md)。Perceus の挿入の後のプログラムについて、変数と join point の
//! 範囲、直接呼び出しとクロージャの引数の数、RC の対象の変数の所有権の釣り合いを確かめる。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

use crate::liveness::{Vars, liveness, tracked};
use crate::{Atom, CExpr, CExprId, Call, CoreFn, JoinId, Program, Rhs, VarId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyError {
    pub function: String,
    pub message: String,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} in `{}`", self.message, self.function)
    }
}

impl std::error::Error for VerifyError {}

pub fn verify(program: &Program) -> Result<(), VerifyError> {
    for function in &program.functions {
        Checker::new(program, function)
            .run()
            .map_err(|message| VerifyError {
                function: function.name.clone(),
                message,
            })?;
    }
    Ok(())
}

/// 経路ごとの状態。`Switch` の各枝と join point の範囲は、同じ状態の写しから始まる。
#[derive(Clone, Default)]
struct State {
    in_scope: BTreeSet<VarId>,
    /// RC の対象の変数ごとの、所有している参照の数。
    owned: BTreeMap<VarId, u32>,
    /// `Jump` してよい join point。
    joins: BTreeSet<JoinId>,
}

struct Checker<'a> {
    program: &'a Program,
    function: &'a CoreFn,
    tracked: Vec<bool>,
    /// join point ごとの、本体で使う RC の対象の変数。
    needs: HashMap<JoinId, Vars>,
    bound: HashSet<VarId>,
    defined_joins: HashSet<JoinId>,
}

impl<'a> Checker<'a> {
    fn new(program: &'a Program, function: &'a CoreFn) -> Self {
        let tracked = tracked(function);
        let needs = liveness(function, &tracked).joins;
        Checker {
            program,
            function,
            tracked,
            needs,
            bound: HashSet::new(),
            defined_joins: HashSet::new(),
        }
    }

    fn run(mut self) -> Result<(), String> {
        let mut state = State::default();
        for &param in &self.function.params {
            self.bind(&mut state, param)?;
        }
        self.check(self.function.body, state)
    }

    fn name(&self, var: VarId) -> String {
        format!("{}{}", self.function.vars[var.0 as usize].name, var.0)
    }

    fn names<'v>(&self, vars: impl IntoIterator<Item = &'v VarId>) -> String {
        let names: Vec<String> = vars.into_iter().map(|&var| self.name(var)).collect();
        format!("[{}]", names.join(", "))
    }

    /// `Let` の連鎖と、join point の本体の連なりはループで歩く。再帰するのは `Switch` の枝と join point の範囲だけで、
    /// 深さは E0013 の入れ子の制限で抑えられる。
    fn check(&mut self, id: CExprId, mut state: State) -> Result<(), String> {
        let function = self.function;
        let mut id = id;
        loop {
            match function.expr(id) {
                CExpr::Let { var, rhs, body } => {
                    self.check_rhs(&mut state, rhs)?;
                    self.bind(&mut state, *var)?;
                    id = *body;
                }
                CExpr::Dup { var, body } => {
                    *self.count(&mut state, *var, "duplicated")? += 1;
                    id = *body;
                }
                CExpr::Decref { var, body } => {
                    *self.count(&mut state, *var, "released")? -= 1;
                    id = *body;
                }
                CExpr::Return(atom) => {
                    self.consume(&mut state, atom)?;
                    return self.nothing_owned(&state);
                }
                CExpr::TailCall(call) => {
                    self.check_call(&mut state, call)?;
                    return self.nothing_owned(&state);
                }
                CExpr::Jump { join, arg } => return self.check_jump(state, *join, arg),
                CExpr::Switch { scrutinee, arms } => {
                    self.consume(&mut state, scrutinee)?;
                    let mut tags = HashSet::new();
                    for &(tag, arm) in arms {
                        if !tags.insert(tag) {
                            return Err(format!("a switch has two arms for tag {tag}"));
                        }
                        self.check(arm, state.clone())?;
                    }
                    return Ok(());
                }
                CExpr::Join {
                    join,
                    param,
                    body,
                    scope,
                } => {
                    if !self.defined_joins.insert(*join) {
                        return Err(format!("`j{}` is defined twice", join.0));
                    }
                    if function.joins.get(join.0 as usize) != Some(&id) {
                        return Err(format!("the join index does not point at `j{}`", join.0));
                    }
                    // 範囲は今の状態から始まり、この join point に `Jump` できる
                    let mut scope_state = state.clone();
                    scope_state.joins.insert(*join);
                    self.check(*scope, scope_state)?;
                    // 本体は、本体で使う変数を1つずつ所有し、引数を束縛して始まる
                    let needs = self.needs.get(join).cloned().unwrap_or_default();
                    state.owned = needs.iter().map(|&var| (var, 1)).collect();
                    self.bind(&mut state, *param)?;
                    id = *body;
                }
            }
        }
    }

    fn bind(&mut self, state: &mut State, var: VarId) -> Result<(), String> {
        if !self.bound.insert(var) {
            return Err(format!("`{}` is bound twice", self.name(var)));
        }
        state.in_scope.insert(var);
        if self.tracked[var.0 as usize] {
            state.owned.insert(var, 1);
        }
        Ok(())
    }

    fn visible(&self, state: &State, var: VarId) -> Result<(), String> {
        if state.in_scope.contains(&var) {
            Ok(())
        } else {
            Err(format!("`{}` is used outside its scope", self.name(var)))
        }
    }

    /// RC の対象の変数の、所有している参照の数。0 なら、`what` (使う、複製する、捨てる) ことはできない。
    fn count<'s>(&self, state: &'s mut State, var: VarId, what: &str) -> Result<&'s mut u32, String> {
        self.visible(state, var)?;
        if !self.tracked[var.0 as usize] {
            return Err(format!("`{}` is {what} but is not reference counted", self.name(var)));
        }
        let name = self.name(var);
        match state.owned.get_mut(&var) {
            Some(count) if *count > 0 => Ok(count),
            _ => Err(format!("`{name}` is {what} after it was moved")),
        }
    }

    /// 値を使う。RC の対象なら、所有権を1つ渡す。
    fn consume(&self, state: &mut State, atom: &Atom) -> Result<(), String> {
        let Atom::Var(var) = *atom else {
            return Ok(());
        };
        if !self.tracked[var.0 as usize] {
            return self.visible(state, var);
        }
        *self.count(state, var, "used")? -= 1;
        Ok(())
    }

    fn check_rhs(&self, state: &mut State, rhs: &Rhs) -> Result<(), String> {
        match rhs {
            Rhs::Call(call) => return self.check_call(state, call),
            Rhs::MakeClosure(target, args) => {
                let target = self.program.function(*target);
                if args.len() >= target.params.len() {
                    return Err(format!(
                        "a closure of `{}` has {} arguments, but it must have fewer than {}",
                        target.name,
                        args.len(),
                        target.params.len()
                    ));
                }
            }
            _ => {}
        }
        for atom in rhs.atoms() {
            self.consume(state, &atom)?;
        }
        Ok(())
    }

    fn check_call(&self, state: &mut State, call: &Call) -> Result<(), String> {
        if let Call::Direct(target, args) = call {
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
        for atom in call.atoms() {
            self.consume(state, &atom)?;
        }
        Ok(())
    }

    /// 渡す値を除き、行き先の join point の本体が使う変数を、ちょうど1つずつ所有している。
    fn check_jump(&self, mut state: State, join: JoinId, arg: &Atom) -> Result<(), String> {
        if !state.joins.contains(&join) {
            return Err(format!("a jump to `j{}` is outside its scope", join.0));
        }
        self.consume(&mut state, arg)?;
        let needs = self.needs.get(&join).cloned().unwrap_or_default();
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

    fn nothing_owned(&self, state: &State) -> Result<(), String> {
        match state.owned.iter().find(|&(_, &count)| count > 0) {
            Some((&var, _)) => Err(format!(
                "`{}` is still owned at the end of the function",
                self.name(var)
            )),
            None => Ok(()),
        }
    }
}
```

`crates/eml_core_ir/src/lib.rs` に `mod verify;` と `pub use verify::{VerifyError, verify};` を足す。

`crates/eml_core_ir/src/lower.rs` の `lower` で、`perceus::insert(&mut program);` の後に足す。

```rust
    // Perceus の誤りを、実行した経路だけでなく変換のたびに見つける (docs/spec/core-ir.md)
    #[cfg(debug_assertions)]
    if let Err(error) = crate::verify(&program) {
        panic!("internal error: invalid Core IR: {error}");
    }
```

- [ ] **Step 4: 通ることを確かめる**

Run: `cargo test -p eml_core_ir --test verify`
Expected: 12件が PASS

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED|invalid Core IR"`
Expected: 何も出ない (すべての Core IR のテストと UI テストが、変換のたびに verify を通る)

- [ ] **Step 5: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates/eml_core_ir
git commit -m "Verify Core IR scopes and ownership after Perceus in debug builds"
```

---

### Task 8: 文書

**Files:**
- Modify: `docs/spec/core-ir.md`、`docs/implementation/architecture.md`、`docs/implementation/status.md`、`docs/implementation/testing.md`

**Interfaces:**
- Consumes: Task 1〜7 の結果
- Produces: なし

日本語の文書を書く前に `yomiyasu:yomiyasu` スキルを読み込み、その規則に従う。

- [ ] **Step 1: `docs/spec/core-ir.md` を直す**

「Core IR」の命令の表の「値」の行の後に足す。

```markdown
| 制御 | `join j(x) { 本体 }` (join point)、`jump j(v)`、末尾呼び出し |
```

箇条の「`Bool` は、タグ 0 (`False`) と 1 (`True`) の引数のないコンストラクタとして表し、`if` は `match` と同じ分岐の命令に変換する。」の後に足す。

```markdown
- 末尾にない分岐の値は join point で受ける。各枝は値を `jump` で join point に渡し、join point の本体が続きを実行する。join point は同じ関数の中だけで使い、本体から自分へは `jump` しない。末尾の分岐は join point を作らず、各枝が直接返す。
- 関数の末尾の呼び出しは末尾呼び出しにし、呼び出し元のフレームを積まない。
- 変換は、`main` を `()` で呼ぶ入口の関数を作る。等式に引数のない `main` は関数値を返すので、入口の関数が返った値に `()` を適用する。
```

「マイルストーン1 で入れるパスは、Perceus の `dup` / `decref` の挿入だけにする。…」の後に足す。

```markdown
- Perceus の挿入は、変換の後にプログラム全体にかける独立したパスである。その後の verifier が、変数と join point の範囲、直接呼び出しとクロージャの引数の数、RC の対象の変数の所有権の釣り合いを確かめる。所有権は、どの経路でも、関数の入口と束縛で得た参照と `dup` で増やした参照が、使用と `decref` でちょうど使い切られることを確かめる。デバッグビルドの変換は、毎回 verifier を通す。
```

「インタプリタ (CEK 機械)」の「継続は、ヒープ上のイミュータブルなフレームの連結リストで表す。…」の後に足す。

```markdown
- `jump` は同じ関数の中で制御を移すだけで、フレームを積まない。実行は入口の関数から始める。
```

- [ ] **Step 2: `docs/implementation/architecture.md` を直す**

「`eml_core_ir`、`eml_runtime`、`eml_interp` の内部」:

- 「Core IR の関数は、ANF の木をアリーナに置き、`CExprId` で参照する。継続のフレームが再開する位置を ID で持てるようにするため。値を返す入れ子の式は `Rhs::Nested` で、`if` はその中の `Switch` にする」を、次にする。

```markdown
- Core IR の関数は、ANF の木をアリーナに置き、`CExprId` で参照する。継続のフレームが再開する位置を ID で持てるようにするため。変換は式の値の渡し先 (`Exit::Return` か `Exit::Jump`) を持って回り、末尾の `if` は各枝が返す `Switch` に、末尾にない `if` は続きを本体にした join point (`CExpr::Join`) にする。`CoreFn::joins` は `JoinId` から `Join` の式を引く索引で、アリーナを作り直す Perceus が作り直す。値を返すだけの呼び出しは `TailCall` にする
- 変数が boxed かどうかは、型から `boxed` の1か所で決める。組み込みの引数の数は `Builtin::arity` (R2 の表) から、引数と結果の型は `TypedModule::builtins` (Prelude のスキーム) から引き、変換の種類 (`Prim`、`Perform`、`Compose`、`Constructor`) は `lowering` の1つの match に置く
- 変換は、`main` を `()` で呼ぶ入口の関数 `entry$main` を足す (`Program::entry`)
```

- 「Perceus の挿入は、ANF の上の後ろ向きの生存解析で行う。…」を、次にする。

```markdown
- Perceus の挿入 (`perceus.rs`) は、変換の後にプログラム全体にかける独立したパスである。生存解析 (`liveness.rs`) は、`Let` の連鎖と join point の本体の連なりが長くなりうるので、作業の列で後順にたどる。join point の本体を範囲より先に求めるのは、範囲の中の `jump` が、本体で使う変数を要るためである。join point の本体は、本体で使う変数をちょうど1つずつ所有して始まり、`jump` の前で残りを捨てる。関数、プリミティブ、`perform` の引数は、どれも所有権を受け取る
- verifier (`verify.rs`) は、Perceus の後に、変数と join point の範囲、引数の数、所有権の釣り合いを確かめる。デバッグビルドの `lower` が毎回呼ぶ
```

- 「CEK 機械の継続は、ヒープ上のフレームの連結リストである。呼び出しのフレームは環境 (スロットの配列) を退避し、入れ子の式のフレームは環境をそのまま使い続ける。最下部に `IO` の handler のフレームを置く」を、次にする。

```markdown
- CEK 機械の継続は、ヒープ上のフレームの連結リストである。呼び出しのフレームは環境 (スロットの配列) を退避する。`jump` と末尾呼び出しはフレームを積まない。最下部に `IO` の handler のフレームを置く
```

- 「変数のスロットは所有する参照の数を持つ…Perceus の `dup` で 1 つの変数が複数の参照を持ち、入れ子の式は環境を共有するため、読み出しでスロットを空にするだけでは足りない。…」の「、入れ子の式は環境を共有するため」を消す。

- [ ] **Step 3: `docs/implementation/status.md` を直す**

- 「リファクタリング」の節の冒頭の「6つの回に分け、R0 → R1 → R2a → R2b-1 → R2b-2 → R3 の順に」を「7つの回に分け、R0 → R1 → R2a → R2b-1 → R2b-2 → R3a → R3b の順に」にする。
- 表の R3 の行を、次の2行にする。

```markdown
| R3a | Core IR の形 | join point と末尾呼び出し、Perceus のパスと verifier、boxed の判定、入口の関数、組み込みの変換。`eml_interp` も追随させる | 完了 |
| R3b | ランタイムとインタプリタ | `Frame` の種類の enum、記述子、共有されたオブジェクトの複製、`Owned::refs`、型を付けた `RuntimeError`。`eml_runtime`、`eml_interp`、`eml_cli` | 未着手 |
```

- 「テストを変えないために曲げた箇所」の表の1の行の「直す回」を `R3b` にする。
- `#### R3 Core IR とランタイム` の節を、次にする。

```markdown
#### R3a Core IR の形

R3a で済んだ。末尾にない `if` を join point にし、末尾の `if` と末尾呼び出しを入れた。Perceus を変換の後の独立したパスにし、所有権まで確かめる verifier を足した。boxed の判定と組み込みの変換を1か所にまとめ、入口の関数を Core IR の側で作るようにした。

#### R3b ランタイムとインタプリタ

- `Frame` を種類の enum にする。`ApplyFrame` を別のペイロードにするのをやめ (上の表の1)、番兵の `IO_HANDLER = u32::MAX` と、環境を退避しないフレームの名残の `slots: Option` をなくす。記述子はペイロードの種類から決める
- 共有されたオブジェクトを複製する手続きをランタイムに置き、参照を `dup` せずに複製する `Clone` をなくす
- 変数のスロットごとの参照の数 (`Owned::refs`) が要るかを確かめ、要らなければ除く。R3a で入れ子の式が環境を共有することはなくなったが、`dup` した変数を1つの右辺で2回読むことは残る
- `RuntimeError` と `step` の結果に型を付ける。`eml_cli` の `RunResult` が文字列で包み直すのをやめる
```

- 「各 crate の実装状況」の `eml_core_ir` の行の末尾に「join point と末尾呼び出し、Perceus の独立したパスと verifier、入口の関数」を足す。
- 「完了した作業」の表の末尾に足す。

```markdown
| リファクタリング R3a | 末尾にない `if` を join point に、値を返すだけの呼び出しを末尾呼び出しにした。Perceus を変換の後の独立したパスにし、範囲と所有権を確かめる verifier を足した。boxed の判定と組み込みの変換を1か所にまとめ、入口の関数を Core IR の側で作るようにした |
```

- [ ] **Step 4: `docs/implementation/testing.md` に記録する**

「テストの変更の記録」の末尾に足す。

```markdown
### リファクタリング R3a

- Core IR の命令を変えたので、`eml_core_ir/tests/lower.rs` の既存の9件のスナップショットが変わった (種類2)。末尾にない `if` が join point (`join` と `jump`) に、末尾の `if` が各枝で返す `switch` に、値を返すだけの呼び出しが `tailcall` になり、すべてに入口の関数 `entry$main` が加わった。`dup` と `decref` の位置で所有権を確かめる目的は変わらない
- `eml_interp/tests/run.rs` と `closures.rs` の手書きの Core IR は、`Rhs::Call`、`CoreFn::joins`、`Program::entry` に合わせて組み立てを書き換えた (種類3)。入口は引数を取らないので、手書きの `main` の引数を除いた。期待値は変えていない
```

- [ ] **Step 5: 文書を検査してコミットする**

Run: `python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py docs/spec/core-ir.md`
Expected: 書き足した部分の指摘を見直す。英単語の前後の半角空白と箇条書きの比率は直さない

Run: `cargo test`
Expected: PASS

```bash
git add docs/spec/core-ir.md docs/implementation
git commit -m "Document refactor R3a in the spec, architecture, and status"
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

Run: `grep -rn "Nested\|bind_boxed\|builtin_params\|builtin_result_boxed\|CallDirect\|save_env\|program.main\b" crates/*/src`
Expected: 何も出ない

Run: `test -e crates/eml_core_ir/src/verify.rs && test -e crates/eml_core_ir/src/liveness.rs && echo ok`
Expected: `ok`

- [ ] **Step 3: 変わったテストが名前を挙げたものだけであることを確かめる**

Run: `git diff --no-ext-diff main --stat | grep -E "tests/|\.snap"`
Expected: 次だけが出る
- `crates/eml_core_ir/tests/lower.rs`、`crates/eml_core_ir/tests/verify.rs`
- `crates/eml_types/tests/check.rs`
- `crates/eml_interp/tests/run.rs`、`crates/eml_interp/tests/closures.rs`
- `tests/ui/run/join_points.em`、`tests/ui/run/tail_calls.em` とそのスナップショット

Run: `git diff --no-ext-diff main -- crates/eml_cli/tests/snapshots | grep '^-' | grep -v '^---'`
Expected: 何も出ない (既存の UI のスナップショットは変わっていない)

## 完了後の後始末

ブランチ全体のレビューが済んだら、作業用の文書を削除する。

```bash
git rm docs/superpowers/specs/2026-10-04-refactor-r3a-core-ir-design.md docs/superpowers/plans/2026-10-04-refactor-r3a-core-ir.md
git commit -m "Remove the work documents of refactor R3a"
```
