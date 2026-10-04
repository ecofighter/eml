# リファクタリング R3b: ランタイムとインタプリタ 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 継続のフレームを種類の enum にし、呼び出しの後で使う変数だけをフレームに退避して `Owned::refs` をなくし、共有されたオブジェクトの複製をランタイムの手続きにし、実行時エラーに型を付ける。言語の観測できる振る舞いは変えない。

**Architecture:** まず実行時エラーに型を付ける (Task 1)。次に Core IR の呼び出しに退避する変数を記録し、verifier が確かめる (Task 2)。その記録を使って、ランタイムのフレームを種類の enum にし、インタプリタが退避する変数だけをフレームに置く (Task 3)。最後に共有されたオブジェクトの複製を `take_or_copy` にまとめ、`Clone` をなくす (Task 4)。

**Tech Stack:** Rust (edition 2024)、insta

**Spec:** `docs/superpowers/specs/2026-10-04-refactor-r3b-runtime-design.md`

## Global Constraints

- 期待値は、このプランで名前を挙げたテストだけを変える (docs/implementation/testing.md の「テストの変更の運用」)
  - 種類1 (spec で承認済み): `crates/eml_runtime/src/heap.rs` の `registered_descriptors_are_counted_by_name` と `a_slot_with_two_references_releases_both` の削除
  - 種類2 (spec で承認済み): `crates/eml_core_ir/tests/lower.rs` の `partial_and_extra_arguments_use_closures`、`builtins_used_as_values_are_wrapped`、`lambdas_are_lifted_with_their_captures_first`、`calls_in_tail_position_are_tail_calls`。Task 2 が新しい期待値を示す
  - 種類3 (期待値を変えない機械的な追随): `eml_runtime` の単体テスト、`eml_interp/tests/closures.rs` と `run.rs`、`eml_core_ir/tests/verify.rs`、`eml_cli/tests/api.rs` と `ui.rs` (実行の結果の扱い)
- 上に挙げていないテスト (UI テストのスナップショット、HIR と型のスナップショットを含む) の期待値が変わったら、変えずに止まる。差分と理由をユーザーに示し、承認を得てから変え、`testing.md` に記録する
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

- 呼び出しの引数に渡し、呼び出しの後でも使う文字列 (`dup` してから渡し、残りの1つを退避する)。フレームが1つ、呼び出しが1つの参照を持ち、リークも二重の解放もない (Task 3 の `tests/ui/run/saved_across_calls.em`)
- 呼び出しの後で使う `Int` (RC の対象でない変数) が退避され、戻った後に正しい値で読める (Task 3 の `saved_across_calls.em`)
- 分岐の中の呼び出しの後に `jump` し、join point の本体が `if` の前の変数を使う。その変数が分岐の中の呼び出しで退避される (Task 2 の `calls_save_the_variables_used_after_them` と、Task 3 の `saved_across_calls.em`)
- 共有されたクロージャを2回呼ぶ (`take_or_copy` の共有の経路)。捕まえた文字列がリークも二重の解放もしない (既存の `tests/ui/run/closures.em` の `shout` と、`eml_interp/tests/closures.rs` の `a_shared_closure_keeps_its_captured_values`)
- 実行時エラーの表示が今と同じ文字列である (Task 1 の `runtime_errors_are_displayed_as_before` と、既存の `tests/ui/run-fail/`)

---

### Task 1: 実行時エラーに型を付ける

**Files:**
- Modify: `crates/eml_interp/src/lib.rs`
- Modify: `crates/eml_cli/src/lib.rs`、`crates/eml_cli/src/main.rs`
- Modify: `crates/eml_cli/tests/api.rs`、`crates/eml_cli/tests/ui.rs`、`crates/eml_interp/tests/run.rs` (種類3)

**Interfaces:**
- Consumes: なし
- Produces:
  - `eml_interp::RuntimeError { Fault { fault: Fault, function: String }, Leak(Vec<(String, usize)>) }` (`Debug`、`Clone`、`PartialEq`、`Eq`、`Display`、`Error`)
  - `eml_interp::Fault { DivisionByZero, IntegerOverflow, Heap(HeapError), Output(String), Internal(&'static str) }` (`Debug`、`Clone`、`PartialEq`、`Eq`、`Display`)
  - インタプリタの中の `enum Step { Continue, Finished }`。`step`、`bind`、`call`、`ret` は `Result<Step, Fault>`、ほかの内部の関数は `Result<_, Fault>` を返す
  - `eml_cli::execute(program: Arc<Program>, config: &RunConfig, stdout: OutputSink) -> Result<(), RuntimeError>`、`eml_cli::RuntimeError` (再公開)。`eml_cli::RunResult` はなくなる

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_interp/src/lib.rs` の `mod tests` に足す。

```rust
    #[test]
    fn runtime_errors_are_displayed_as_before() {
        let fault = |fault| {
            RuntimeError::Fault {
                fault,
                function: "f".to_string(),
            }
            .to_string()
        };
        assert_eq!(fault(Fault::DivisionByZero), "division by zero in `f`");
        assert_eq!(fault(Fault::IntegerOverflow), "integer overflow in `f`");
        assert_eq!(
            fault(Fault::Heap(HeapError::UseAfterFree)),
            "use of a freed object in `f`"
        );
        assert_eq!(
            fault(Fault::Output("broken pipe".to_string())),
            "cannot write the output: broken pipe in `f`"
        );
        assert_eq!(
            fault(Fault::Internal("a switch without a matching arm")),
            "internal error: a switch without a matching arm in `f`"
        );
        let leak = RuntimeError::Leak(vec![("Closure".to_string(), 2), ("String".to_string(), 1)]);
        assert_eq!(
            leak.to_string(),
            "memory leak: objects were not freed: 2 Closure, 1 String"
        );
    }
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_interp --lib runtime_errors_are_displayed_as_before`
Expected: コンパイルエラー (`RuntimeError::Fault` も `Fault` もない)

- [ ] **Step 3: 型を足す**

`crates/eml_interp/src/lib.rs` の `pub struct RuntimeError(pub String);` とその `Display` の実装を、次にする。

```rust
/// 実行時エラー (docs/spec/core-ir.md の「実行時エラー」)。表示は CLI と UI テストが使う文言である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    /// 実行中の関数で止まった。
    Fault { fault: Fault, function: String },
    /// `debug_heap` で、終了時に解放されていないオブジェクトがあった。記述子の名前ごとの数。
    Leak(Vec<(String, usize)>),
}

/// 実行中の関数で起きた誤り。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    DivisionByZero,
    IntegerOverflow,
    Heap(HeapError),
    Output(String),
    /// 型検査と Core IR の変換が正しければ起きない誤り。
    Internal(&'static str),
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fault::DivisionByZero => f.write_str("division by zero"),
            Fault::IntegerOverflow => f.write_str("integer overflow"),
            Fault::Heap(error) => write!(f, "{error}"),
            Fault::Output(error) => write!(f, "cannot write the output: {error}"),
            Fault::Internal(what) => write!(f, "internal error: {what}"),
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeError::Fault { fault, function } => write!(f, "{fault} in `{function}`"),
            RuntimeError::Leak(live) => {
                let parts: Vec<String> =
                    live.iter().map(|(name, n)| format!("{n} {name}")).collect();
                write!(f, "memory leak: objects were not freed: {}", parts.join(", "))
            }
        }
    }
}
```

`enum Prepared` の後に足す。

```rust
/// 1つの命令を実行した後の状態。
enum Step {
    Continue,
    Finished,
}
```

- [ ] **Step 4: インタプリタの関数を型に合わせる**

`crates/eml_interp/src/lib.rs` で、次のように書き換える。

- `Machine::run` を次にする。

```rust
    fn run(&mut self) -> Result<(), RuntimeError> {
        loop {
            match self.step() {
                Ok(Step::Finished) => return Ok(()),
                Ok(Step::Continue) => {}
                Err(fault) => {
                    let function = self.program.function(self.function).name.clone();
                    return Err(RuntimeError::Fault { fault, function });
                }
            }
        }
    }
```

- `step`、`bind`、`call`、`ret` の戻り値の型を `Result<Step, Fault>` に、`apply` を `Result<Applied, Fault>` に、`take_closure` を `Result<Closure, Fault>` に、`atom` と `prim` を `Result<Value, Fault>` に、`atoms` を `Result<Vec<Value>, Fault>` に、`take_string` を `Result<String, Fault>` にする。
- `Ok(false)` を `Ok(Step::Continue)` に、`Ok(true)` を `Ok(Step::Finished)` にする。`step` の doc の「プログラムが終わったら真を返す」を消す。
- `internal("...")` を `Fault::Internal("...")` に、`.map_err(heap_error)` を `.map_err(Fault::Heap)` に、`.ok_or_else(|| internal("..."))` を `.ok_or(Fault::Internal("..."))` にする。関数 `internal` と `heap_error` を消す。
- `prim` の `let overflow = || "integer overflow".to_string();` を `let overflow = || Fault::IntegerOverflow;` に、`return Err("division by zero".to_string());` を `return Err(Fault::DivisionByZero);` にする。
- `println` の `.map_err(|error| format!("cannot write the output: {error}"))` を `.map_err(|error| Fault::Output(error.to_string()))` にする。
- `check_leaks` を次にする。

```rust
    fn check_leaks(&self) -> Result<(), RuntimeError> {
        let live = self.heap.live_objects();
        if live.is_empty() {
            return Ok(());
        }
        Err(RuntimeError::Leak(live))
    }
```

- [ ] **Step 5: CLI を型に合わせる**

`crates/eml_cli/src/lib.rs`:

- `pub use eml_interp::RunConfig;` を `pub use eml_interp::{RunConfig, RuntimeError};` にする。
- `enum RunResult` を消し、`execute` を次にする。

```rust
pub fn execute(
    program: Arc<Program>,
    config: &RunConfig,
    stdout: OutputSink,
) -> Result<(), RuntimeError> {
    eml_interp::run(program, config, &stdout)
}
```

`crates/eml_cli/src/main.rs`:

- `use eml_cli::{OutputSink, RunConfig, RunResult};` を `use eml_cli::{OutputSink, RunConfig};` にする。
- `execute` の結果の `match` を次にする。

```rust
            match eml_cli::execute(program, &config, OutputSink::stdout()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("runtime error: {error}");
                    ExitCode::from(1)
                }
            }
```

- [ ] **Step 6: テストの組み立てを追随させる (種類3)**

- `crates/eml_cli/tests/api.rs`: `use` から `RunResult` を消し、`execute_runs_a_compiled_program` の期待値 `RunResult::Completed` を `Ok(())` にする。
- `crates/eml_cli/tests/ui.rs`:
  - `use eml_cli::{OutputSink, RunConfig, RunResult};` を `use eml_cli::{OutputSink, RunConfig, RuntimeError};` にする。
  - `compile_and_execute` の戻り値の型の `RunResult` を `Result<(), RuntimeError>` にする。
  - `run` の `assert_eq!(result, RunResult::Completed, "{stderr}");` を `assert_eq!(result, Ok(()), "{stderr}");` にする。
  - `run_fail` の `let RunResult::RuntimeError(message) = result else {` を `let Err(error) = result else {` にし、スナップショットの `{message}` を `{error}` にする (表示の文字列は同じ)。
- `crates/eml_interp/tests/run.rs` の `debug_heap_reports_leaks` の期待値を次にする。

```rust
    assert_eq!(
        execute(leaking_program(), true).1,
        Err(RuntimeError::Leak(vec![("String".to_string(), 1)]))
    );
```

- [ ] **Step 7: 確かめてコミットする**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED"`
Expected: 何も出ない (UI テストの run-fail のスナップショットも変わらない)

Run: `git diff --no-ext-diff --stat -- '*.snap' crates/*/tests`
Expected: `crates/eml_cli/tests/api.rs`、`crates/eml_cli/tests/ui.rs`、`crates/eml_interp/tests/run.rs` だけ

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates
git commit -m "Give runtime errors and interpreter steps types"
```

---

### Task 2: 呼び出しに退避する変数を記録する

**Files:**
- Create: `crates/eml_core_ir/src/saved.rs`
- Modify: `crates/eml_core_ir/src/lib.rs`、`lower.rs`、`pretty.rs`、`verify.rs`
- Modify: `crates/eml_interp/src/lib.rs` (パターンの追随だけ)
- Modify: `crates/eml_interp/tests/closures.rs`、`crates/eml_core_ir/tests/verify.rs` (種類3)
- Test: `crates/eml_core_ir/tests/lower.rs`、`crates/eml_core_ir/tests/verify.rs`

**Interfaces:**
- Consumes: R3a の `liveness::{liveness, tracked, Vars}`、`Call`
- Produces:
  - `Rhs::Call { call: Call, saved: Vec<VarId> }`、`Rhs::call(call: Call) -> Rhs` (`saved` が空の呼び出しを作る)
  - `saved::record(program: &mut Program)` (crate の中だけ。`lower` が Perceus の後、verify の前に呼ぶ)
  - 表示: `saved` が空でなければ `let t2 = call twice(s1) [s3]` のように後ろに付ける

インタプリタは、このタスクでは `saved` を使わない (Task 3 で使う)。

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/lower.rs` に足す。

```rust
#[test]
fn calls_save_the_variables_used_after_them() {
    let text = "around : Int -> String -> String\naround n s =\n  let m = n + 1\n  let t = if n > 0 then twice s else s\n  t ++ show_int m\n\ntwice : String -> String\ntwice s = s ++ s\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    insta::assert_snapshot!(core_text(text), @r"
    fn around(n0, s1) {
      let t2 = prim +(n0, 1)
      let t3 = prim >(n0, 0)
      join j0(t5) {
        let t6 = prim show_int(t2)
        let t7 = prim ++(t5, t6)
        return t7
      }
      switch t3 {
        #0 ->
          jump j0(s1)
        #1 ->
          let t4 = call twice(s1) [t2]
          jump j0(t4)
      }
    }
    fn twice(s0) {
      dup s0
      let t1 = prim ++(s0, s0)
      return t1
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

既存の4件の期待値を次のように変える (種類2)。

- `partial_and_extra_arguments_use_closures` の `main` の2行:
  - `let t3 = call adder(3)` を `let t3 = call adder(3) [t2]` に
  - `let t4 = apply t3(4)` を `let t4 = apply t3(4) [t2]` に
- `builtins_used_as_values_are_wrapped` の `builtin$>>` の `let t3 = apply p0(p2)` を `let t3 = apply p0(p2) [p1]` に
- `lambdas_are_lifted_with_their_captures_first` の `main` の `let t4 = call apply(c2, s3)` を `let t4 = call apply(c2, s3) [s1]` に
- `calls_in_tail_position_are_tail_calls` の `call_twice` の `let t2 = apply f0(x1)` を `let t2 = apply f0(x1) [f0]` に

`crates/eml_core_ir/tests/verify.rs`:

- 既存の `Rhs::Call(Call::Direct(FnIdx(0), vec![Atom::Int(1), Atom::Int(2)]))` を `Rhs::call(Call::Direct(FnIdx(0), vec![Atom::Int(1), Atom::Int(2)]))` にする (種類3)。
- 末尾に足す。

```rust
/// `g s = s`。
fn identity() -> CoreFn {
    function("g", 1, vec![string("s")], vec![CExpr::Return(var(0))], &[])
}

#[test]
fn a_call_that_does_not_save_an_owned_variable_is_rejected() {
    // f s = dup s; let t = g s; s を退避しないまま t ++ s
    let exprs = vec![
        CExpr::Return(var(2)),
        concat(2, 1, 0, 0),
        CExpr::Let {
            var: VarId(1),
            rhs: Rhs::call(Call::Direct(FnIdx(0), vec![var(0)])),
            body: CExprId(1),
        },
        CExpr::Dup {
            var: VarId(0),
            body: CExprId(2),
        },
    ];
    let f = function("f", 1, vec![string("s"), string("t"), string("t")], exprs, &[]);
    assert_eq!(
        check(vec![identity(), f]),
        Err("a call saves [] but owns [s0] in `f`".to_string())
    );
}

#[test]
fn a_call_that_saves_a_variable_it_does_not_own_is_rejected() {
    let exprs = vec![
        CExpr::Return(var(1)),
        CExpr::Let {
            var: VarId(1),
            rhs: Rhs::Call {
                call: Call::Direct(FnIdx(0), vec![var(0)]),
                saved: vec![VarId(0)],
            },
            body: CExprId(0),
        },
    ];
    let f = function("f", 1, vec![string("s"), string("t")], exprs, &[]);
    assert_eq!(
        check(vec![identity(), f]),
        Err("a call saves [s0] but owns [] in `f`".to_string())
    );
}

#[test]
fn a_variable_not_saved_by_a_call_is_out_of_scope_after_it() {
    let k = function("k", 1, vec![int("a")], vec![CExpr::Return(var(0))], &[]);
    let exprs = vec![
        CExpr::Return(var(2)),
        CExpr::Let {
            var: VarId(2),
            rhs: Rhs::Prim(PrimOp::IntAdd, vec![var(0), var(1)]),
            body: CExprId(0),
        },
        CExpr::Let {
            var: VarId(1),
            rhs: Rhs::call(Call::Direct(FnIdx(0), vec![var(0)])),
            body: CExprId(1),
        },
    ];
    let f = function("f", 1, vec![int("n"), int("t"), int("t")], exprs, &[]);
    assert_eq!(
        check(vec![k, f]),
        Err("`n0` is used outside its scope in `f`".to_string())
    );
}

#[test]
fn a_jump_after_a_call_needs_the_variables_of_the_join_body_in_scope() {
    let z = function("z", 0, vec![], vec![CExpr::Return(Atom::Int(1))], &[]);
    let exprs = vec![
        CExpr::Return(var(2)),
        CExpr::Let {
            var: VarId(2),
            rhs: Rhs::Prim(PrimOp::IntAdd, vec![var(0), var(1)]),
            body: CExprId(0),
        },
        CExpr::Jump {
            join: JoinId(0),
            arg: var(3),
        },
        CExpr::Let {
            var: VarId(3),
            rhs: Rhs::call(Call::Direct(FnIdx(0), vec![])),
            body: CExprId(2),
        },
        CExpr::Join {
            join: JoinId(0),
            param: VarId(1),
            body: CExprId(1),
            scope: CExprId(3),
        },
    ];
    let f = function("f", 1, vec![int("n"), int("t"), int("t"), int("t")], exprs, &[4]);
    assert_eq!(
        check(vec![z, f]),
        Err("`n0` is used outside its scope in `f`".to_string())
    );
}
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_core_ir 2>&1 | grep -E "^error|^test .*FAILED|test result"`
Expected: `verify.rs` が `Rhs::call` と `Rhs::Call { .. }` でコンパイルエラーになる。`lower.rs` は新しい1件と変えた4件が FAILED

- [ ] **Step 3: Core IR に退避する変数を足す**

`crates/eml_core_ir/src/lib.rs`:

- `Rhs::Call(Call)` を次にする。

```rust
    /// 呼び出しの後で使う変数 (`saved`) を、呼び出しのフレームに退避する (docs/spec/core-ir.md)。
    Call { call: Call, saved: Vec<VarId> },
```

- `Rhs::atoms` の `Rhs::Call(call) => call.atoms(),` を `Rhs::Call { call, .. } => call.atoms(),` にし、`impl Rhs` に足す。

```rust
    /// 退避する変数をまだ決めていない呼び出し。Perceus の後に、退避のパス (`saved.rs`) が埋める。
    pub fn call(call: Call) -> Rhs {
        Rhs::Call {
            call,
            saved: Vec::new(),
        }
    }
```

- `mod saved;` を足す。

`crates/eml_core_ir/src/saved.rs` を作る。

```rust
//! 呼び出しのフレームに退避する変数を決めるパス (docs/spec/core-ir.md)。Perceus の後に、各呼び出しの後で使う変数を
//! 記録する。RC の対象でない変数も、`Jump` の先の join point の本体で使う変数も含む。インタプリタはこの変数だけを
//! フレームに退避するので、フレームはちょうど所有している参照だけを持つ。

use crate::liveness::liveness;
use crate::{CExpr, Program, Rhs};

pub(crate) fn record(program: &mut Program) {
    for function in &mut program.functions {
        let all = vec![true; function.vars.len()];
        let live = liveness(function, &all).exprs;
        for expr in &mut function.exprs {
            if let CExpr::Let {
                var,
                rhs: Rhs::Call { saved, .. },
                body,
            } = expr
            {
                let mut after = live[body.0 as usize].clone();
                after.remove(var);
                *saved = after.into_iter().collect();
            }
        }
    }
}
```

`crates/eml_core_ir/src/lower.rs`:

- `tail` の末尾呼び出しの判定のパターン `Some(Binding::Let(bound, Rhs::Call(_)))` を `Some(Binding::Let(bound, Rhs::Call { .. }))` に、`let Some(Binding::Let(_, Rhs::Call(call))) = bindings.pop()` を `let Some(Binding::Let(_, Rhs::Call { call, .. })) = bindings.pop()` にする。
- ほかの `Rhs::Call(` をすべて `Rhs::call(` にする (呼び出しを作るところ)。
- `lower` の `perceus::insert(&mut program);` の後に `saved::record(&mut program);` を足す (`use crate::{...}` に `saved` を足す)。

`crates/eml_core_ir/src/pretty.rs` の `rhs_text` の呼び出しの2つの腕を次の1つにする。

```rust
        Rhs::Call { call, saved } => {
            let text = match call {
                Call::Direct(..) => format!("call {}", call_text(program, function, call)),
                Call::Apply(..) => call_text(program, function, call),
            };
            if saved.is_empty() {
                text
            } else {
                let names: Vec<String> = saved.iter().map(|&v| var(function, v)).collect();
                format!("{text} [{}]", names.join(", "))
            }
        }
```

`crates/eml_interp/src/lib.rs` の `Rhs::Call(call) =>` を `Rhs::Call { call, .. } =>` にする。

- [ ] **Step 4: verifier が退避する変数を確かめる**

`crates/eml_core_ir/src/verify.rs`:

- `Checker` のフィールド `in_scope` と `scope_log` を次にし、`uses` を足す。

```rust
    /// join point ごとの、本体で使う変数 (RC の対象でないものも含む)。呼び出しの後に `Jump` するとき、退避し忘れて
    /// いないことを確かめる。
    uses: HashMap<JoinId, Vars>,
    bound: HashSet<VarId>,
    /// 変数ごとの、範囲に入れたときの区間の番号。区間は呼び出しのたびに新しくなり、呼び出しで退避した変数を新しい
    /// 区間に入れ直す。今の区間の番号を持つ変数だけが範囲にある。枝ごとに写すと、文の `if` が続く関数で文の数の
    /// 2乗の時間がかかるので、変更を `scope_log` に記録し、枝や範囲を確かめ終えたら巻き戻す。
    stamps: Vec<Option<u32>>,
    scope_log: Vec<(VarId, Option<u32>)>,
    epoch: u32,
    next_epoch: u32,
```

- `Checker::new` で次のように作る。

```rust
        let tracked = tracked(function);
        let needs = liveness(function, &tracked).joins;
        let uses = liveness(function, &vec![true; function.vars.len()]).joins;
        Checker {
            program,
            function,
            tracked,
            needs,
            uses,
            bound: HashSet::new(),
            stamps: vec![None; function.vars.len()],
            scope_log: Vec::new(),
            epoch: 0,
            next_epoch: 1,
            defined_joins: HashSet::new(),
        }
```

- `check` の `CExpr::Let` の腕を次にする。

```rust
                CExpr::Let { var, rhs, body } => {
                    self.check_rhs(&mut state, rhs)?;
                    if let Rhs::Call { saved, .. } = rhs {
                        self.check_saved(&state, saved)?;
                        // 呼び出しの後は、退避した変数だけが範囲に残る
                        self.epoch = self.next_epoch;
                        self.next_epoch += 1;
                        for &var in saved {
                            self.enter_scope(var);
                        }
                    }
                    self.bind(&mut state, *var)?;
                    id = *body;
                }
```

- `check_branch`、`bind`、`visible` を次にし、`enter_scope` を足す。

```rust
    /// 枝や範囲を確かめ、その中での範囲の変更を巻き戻す。
    fn check_branch(&mut self, id: CExprId, state: State) -> Result<(), String> {
        let mark = self.scope_log.len();
        let epoch = self.epoch;
        self.check(id, state)?;
        for (var, stamp) in self.scope_log.drain(mark..).rev() {
            self.stamps[var.0 as usize] = stamp;
        }
        self.epoch = epoch;
        Ok(())
    }

    fn enter_scope(&mut self, var: VarId) {
        self.scope_log.push((var, self.stamps[var.0 as usize]));
        self.stamps[var.0 as usize] = Some(self.epoch);
    }

    fn bind(&mut self, state: &mut State, var: VarId) -> Result<(), String> {
        if !self.bound.insert(var) {
            return Err(format!("`{}` is bound twice", self.name(var)));
        }
        self.enter_scope(var);
        if self.tracked[var.0 as usize] {
            state.owned.insert(var, 1);
        }
        Ok(())
    }

    fn visible(&self, var: VarId) -> Result<(), String> {
        if self.stamps[var.0 as usize] == Some(self.epoch) {
            Ok(())
        } else {
            Err(format!("`{}` is used outside its scope", self.name(var)))
        }
    }
```

- `check_rhs` の `Rhs::Call(call) =>` を `Rhs::Call { call, .. } =>` にする。
- `check_saved` を足す。

```rust
    /// 退避する変数は範囲の中にあり、RC の対象のうち所有している変数とちょうど一致する。フレームがちょうど所有して
    /// いる参照だけを持つためである (docs/spec/core-ir.md)。
    fn check_saved(&self, state: &State, saved: &[VarId]) -> Result<(), String> {
        for &var in saved {
            self.visible(var)?;
        }
        let saved_tracked: Vars = saved
            .iter()
            .copied()
            .filter(|var| self.tracked[var.0 as usize])
            .collect();
        let owned: Vec<VarId> = state
            .owned
            .iter()
            .flat_map(|(&var, &count)| std::iter::repeat_n(var, count as usize))
            .collect();
        if owned.iter().copied().collect::<Vars>() != saved_tracked
            || owned.len() != saved_tracked.len()
        {
            return Err(format!(
                "a call saves {} but owns {}",
                self.names(&saved_tracked),
                self.names(&owned)
            ));
        }
        Ok(())
    }
```

- `check_jump` の先頭の `if !state.joins.contains(&join) { ... }` の後に足す。

```rust
        // 呼び出しの後に `Jump` する経路で、本体が使う変数を退避し忘れていないこと
        for &var in self.uses.get(&join).into_iter().flatten() {
            self.visible(var)?;
        }
```

- `State` の doc の「束縛の範囲は `Checker` が取り消しの記録で戻す」はそのまま残す。

- [ ] **Step 5: 手書きの Core IR を追随させる (種類3)**

`crates/eml_interp/tests/closures.rs` の `Rhs::Call(Call::Apply(x, args))` を `Rhs::call(Call::Apply(x, args))` にする。ただし `a_shared_closure_keeps_its_captured_values` の最初の呼び出し (`Step::Let(3, ...)`) は、後で `c2` を使うので次にする。

```rust
        Step::Let(
            3,
            Rhs::Call {
                call: Call::Apply(var(2), vec![Atom::Int(1)]),
                saved: vec![VarId(2)],
            },
        ),
```

期待値は変えない。

- [ ] **Step 6: 確かめる**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED|invalid Core IR"`
Expected: 何も出ない

Run: `git diff --no-ext-diff --stat -- '*.snap' crates/*/tests tests`
Expected: `crates/eml_core_ir/tests/lower.rs`、`crates/eml_core_ir/tests/verify.rs`、`crates/eml_interp/tests/closures.rs` だけ

- [ ] **Step 7: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates
git commit -m "Record the variables saved across each call and verify them"
```

---

### Task 3: フレームを種類の enum にし、退避する変数だけを置く

**Files:**
- Modify: `crates/eml_runtime/src/heap.rs`、`crates/eml_runtime/src/lib.rs`
- Modify: `crates/eml_interp/src/lib.rs`
- Test: `tests/ui/run/saved_across_calls.em` と `crates/eml_cli/tests/snapshots/ui__run@saved_across_calls.em.snap`

**Interfaces:**
- Consumes: Task 1 の `Fault`、`Step`、Task 2 の `Rhs::Call { call, saved }`
- Produces:
  - `eml_runtime::Payload { Str(String), Closure(Closure), Frame(Frame) }`
  - `eml_runtime::Frame { Return { function: u32, resume: u32, bind: u32, saved: Vec<(u32, Value)>, next: ObjRef }, Apply { args: Vec<Value>, next: ObjRef }, Io }`
  - `Heap::alloc(&mut self, payload: Payload) -> ObjRef`
  - `eml_runtime` の公開: `Closure, Frame, Heap, HeapError, ObjRef, Payload, Value` (`ApplyFrame`、`DescId`、`Descriptor`、`Owned` はなくなる)。`Heap::register` はなくなる

- [ ] **Step 1: 回帰を防ぐ UI テストを足す**

`tests/ui/run/saved_across_calls.em` を作る。

```
-- Values live across calls are saved in the call's frame: a string passed to a call and used again,
-- an integer used after a call, and a call inside a branch whose join point uses a value from before the `if`.
twice : String -> String
twice s = s ++ s

count : Int -> Int
count n = if n == 0 then 0 else 1 + count (n - 1)

label : Bool -> Int -> String -> String
label b n s =
  let t = if b then twice s else s
  t ++ show_int (n + count 3)

main : Unit -> <IO> Unit
main () =
  let s = "ab"
  let t = twice s
  println (t ++ s)
  let n = 40
  let m = count 2
  println (show_int (n + m))
  println (label True 1 "x")
  println (label False 2 "y")
```

`crates/eml_cli/tests/snapshots/ui__run@saved_across_calls.em.snap` を作る。

```
---
source: crates/eml_cli/tests/ui.rs
expression: "format!(\"--- stdout ---\\n{stdout}--- stderr ---\\n{stderr}\")"
input_file: tests/ui/run/saved_across_calls.em
---
--- stdout ---
ababab
42
xx4
y5
--- stderr ---
```

Run: `cargo test -p eml_cli --test ui`
Expected: PASS (今の実装でも通る。回帰を防ぐためのテスト)

- [ ] **Step 2: ランタイムのテストを新しい形で書く**

`crates/eml_runtime/src/heap.rs` の `mod tests` を次にする (種類3の書き換えと、種類1の2件の削除)。

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn string(heap: &mut Heap, text: &str) -> ObjRef {
        heap.alloc(Payload::Str(text.to_string()))
    }

    /// 継続の最下部のフレーム。
    fn bottom(heap: &mut Heap) -> ObjRef {
        heap.alloc(Payload::Frame(Frame::Io))
    }

    fn frame(heap: &mut Heap, saved: Vec<(u32, Value)>, next: ObjRef) -> ObjRef {
        heap.alloc(Payload::Frame(Frame::Return {
            function: 0,
            resume: 0,
            bind: 0,
            saved,
            next,
        }))
    }

    #[test]
    fn a_closure_releases_its_arguments() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        let closure = heap.alloc(Payload::Closure(Closure {
            function: 0,
            args: vec![Value::Obj(s), Value::Int(1)],
        }));
        heap.decref(closure).unwrap();
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn an_apply_frame_releases_its_arguments_and_the_rest_of_the_continuation() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        let end = bottom(&mut heap);
        let next = frame(&mut heap, vec![], end);
        let apply = heap.alloc(Payload::Frame(Frame::Apply {
            args: vec![Value::Obj(s)],
            next,
        }));
        heap.decref(apply).unwrap();
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn decref_to_zero_frees_the_object() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        heap.decref(s).unwrap();
        assert!(heap.live_objects().is_empty());
        assert_eq!(heap.get(s), Err(HeapError::UseAfterFree));
    }

    #[test]
    fn dup_keeps_the_object_alive() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        heap.dup(s).unwrap();
        heap.decref(s).unwrap();
        assert_eq!(heap.get(s).unwrap(), &Payload::Str("a".to_string()));
        heap.decref(s).unwrap();
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn a_reused_slot_does_not_revive_old_references() {
        let mut heap = Heap::new();
        let old = string(&mut heap, "a");
        heap.decref(old).unwrap();
        let new = string(&mut heap, "b");
        assert_ne!(old, new);
        assert_eq!(heap.get(old), Err(HeapError::UseAfterFree));
        assert_eq!(heap.decref(old), Err(HeapError::UseAfterFree));
        assert_eq!(heap.get(new).unwrap(), &Payload::Str("b".to_string()));
    }

    #[test]
    fn decref_releases_children() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        let inner = bottom(&mut heap);
        let outer = frame(
            &mut heap,
            vec![(0, Value::Obj(s)), (1, Value::Int(1))],
            inner,
        );
        heap.decref(outer).unwrap();
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn releasing_a_long_chain_does_not_overflow_the_stack() {
        let mut heap = Heap::new();
        let mut next = bottom(&mut heap);
        for _ in 0..200_000 {
            next = frame(&mut heap, vec![], next);
        }
        heap.decref(next).unwrap();
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn live_objects_are_counted_by_descriptor() {
        let mut heap = Heap::new();
        string(&mut heap, "a");
        string(&mut heap, "b");
        bottom(&mut heap);
        assert_eq!(
            heap.live_objects(),
            [("Frame".to_string(), 1), ("String".to_string(), 2)]
        );
    }

    #[test]
    fn take_requires_a_unique_object() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        heap.dup(s).unwrap();
        assert_eq!(heap.take(s), Err(HeapError::Shared));
        heap.decref(s).unwrap();
        assert_eq!(heap.take(s), Ok(Payload::Str("a".to_string())));
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn mark_shared_is_reserved_for_multicore() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        assert_eq!(
            heap.mark_shared(s),
            Err(HeapError::NotImplemented("mark_shared"))
        );
    }
}
```

Run: `cargo test -p eml_runtime`
Expected: コンパイルエラー (`alloc` の引数の数、`Frame::Io` がない)

- [ ] **Step 3: ランタイムのフレームと記述子を変える**

`crates/eml_runtime/src/heap.rs` の `mod tests` より前を、次の形にする (`HeapError` とその `Display`、`Header`、`Object`、`Slot`、`ObjRef`、`Value`、`Closure` は今のまま)。

- `DescId` を crate の外に出さない形にし、記述子の表を定数にする。

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct DescId(u32);

impl DescId {
    const STRING: DescId = DescId(0);
    const FRAME: DescId = DescId(1);
    const CLOSURE: DescId = DescId(2);
}

/// オブジェクトの種類。ヘッダから引けるようにし、後の段階でフィールドのレイアウトと `Lin` の破棄処理を足す
/// (docs/spec/runtime.md の「オブジェクトのヘッダ」)。段階4で、ユーザーの `data` の記述子を登録できるようにする。
struct Descriptor {
    name: &'static str,
}

const DESCRIPTORS: [Descriptor; 3] = [
    Descriptor { name: "String" },
    Descriptor { name: "Frame" },
    Descriptor { name: "Closure" },
];
```

- `Payload`、`Frame` を次にし、`ApplyFrame` と `Owned` を消す。

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum Payload {
    Str(String),
    Closure(Closure),
    Frame(Frame),
}

impl Payload {
    /// 記述子はペイロードの種類から決める。フレームはどの種類も継続の連結リストの要素なので、同じ記述子にする。
    fn desc(&self) -> DescId {
        match self {
            Payload::Str(_) => DescId::STRING,
            Payload::Closure(_) => DescId::CLOSURE,
            Payload::Frame(_) => DescId::FRAME,
        }
    }
}
```

```rust
/// CEK 機械の継続のフレーム。継続もランタイムのオブジェクトにする (docs/spec/runtime.md)。
#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    /// 呼び出し元の関数に戻る。呼び出しの後で使う変数だけを退避し、それぞれ参照を1つ所有する
    /// (docs/spec/core-ir.md)。
    Return {
        function: u32,
        resume: u32,
        bind: u32,
        saved: Vec<(u32, Value)>,
        next: ObjRef,
    },
    /// 戻った関数値に、余った引数を適用する (docs/spec/core-ir.md の eval/apply)。
    Apply { args: Vec<Value>, next: ObjRef },
    /// 継続の最下部にある `IO` の組み込みの handler (docs/spec/core-ir.md)。
    Io,
}
```

- `Heap` から `descriptors` を除き、`new`、`alloc`、`live_objects` を次にし、`register` を消す。

```rust
pub struct Heap {
    slots: Vec<Slot>,
    free: Vec<u32>,
}

impl Heap {
    pub fn new() -> Heap {
        Heap {
            slots: Vec::new(),
            free: Vec::new(),
        }
    }

    pub fn alloc(&mut self, payload: Payload) -> ObjRef {
        let object = Object {
            header: Header {
                rc: AtomicI32::new(1),
                desc: payload.desc(),
            },
            payload,
        };
        // ここから下 (`match self.free.pop()` から) は今のまま
```

```rust
    /// まだ解放されていないオブジェクトの数を、記述子の名前ごとに数える。`debug_heap` のリーク検出で使う。
    pub fn live_objects(&self) -> Vec<(String, usize)> {
        let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
        for object in self.slots.iter().filter_map(|slot| slot.object.as_ref()) {
            *counts
                .entry(DESCRIPTORS[object.header.desc.0 as usize].name)
                .or_default() += 1;
        }
        counts
            .into_iter()
            .map(|(name, count)| (name.to_string(), count))
            .collect()
    }
```

- `children` を次にする。

```rust
/// 子のオブジェクト。解放と、段階3以降の複製が、同じ子を数える。
fn children(payload: &Payload, work: &mut Vec<ObjRef>) {
    let object = |value: &Value| match value {
        Value::Obj(obj) => Some(*obj),
        _ => None,
    };
    match payload {
        // 退避した値はそれぞれ参照を1つ所有するので、1回ずつ解放する
        Payload::Frame(Frame::Return { saved, next, .. }) => {
            work.extend(saved.iter().filter_map(|(_, value)| object(value)));
            work.push(*next);
        }
        Payload::Frame(Frame::Apply { args, next }) => {
            work.extend(args.iter().filter_map(object));
            work.push(*next);
        }
        Payload::Closure(closure) => work.extend(closure.args.iter().filter_map(object)),
        Payload::Frame(Frame::Io) | Payload::Str(_) => {}
    }
}
```

`crates/eml_runtime/src/lib.rs` の公開を次にする。

```rust
pub use heap::{Closure, Frame, Heap, HeapError, ObjRef, Payload, Value};
```

Run: `cargo test -p eml_runtime`
Expected: PASS (11件)

- [ ] **Step 4: インタプリタが退避する変数だけをフレームに置く**

`crates/eml_interp/src/lib.rs`:

- `use eml_runtime::{...}` を `use eml_runtime::{Closure, Frame, Heap, HeapError, ObjRef, OutputSink, Payload, Value};` にする。`const IO_HANDLER` を消す。
- `enum Step` の後に足す。

```rust
/// 呼び出しから戻った後に再開するところと、呼び出しのフレームに退避する変数。
struct Resume<'p> {
    bind: VarId,
    resume: CExprId,
    saved: &'p [VarId],
}
```

- `Machine` の `slots` と `cont` の doc と型を次にする。

```rust
    /// 今の関数の環境。読み出しはスロットを書き換えない。参照の所有は Core IR の命令 (使用、`dup`、`decref`) が表し、
    /// verifier がその釣り合いを確かめる (docs/spec/core-ir.md)。
    slots: Vec<Option<Value>>,
    /// 継続の先頭のフレーム。最下部には常に `Frame::Io` がある。
    cont: ObjRef,
```

- `Machine::new` の最下部のフレームを `let cont = heap.alloc(Payload::Frame(Frame::Io));` にする。
- `step` の `Jump`、`Dup`、`Decref` の腕を次にする。

```rust
            CExpr::Jump { join, arg } => {
                // join point は同じ関数の中にあるので、環境をそのまま使い、フレームを積まない
                let value = self.atom(arg)?;
                let (param, body) = program.function(self.function).join(*join);
                self.slots[param.0 as usize] = Some(value);
                self.control = body;
            }
            CExpr::Dup { var, body } => {
                if let Value::Obj(obj) = self.read(*var)? {
                    self.heap.dup(obj).map_err(Fault::Heap)?;
                }
                self.control = *body;
            }
            CExpr::Decref { var, body } => {
                if let Value::Obj(obj) = self.read(*var)? {
                    self.heap.decref(obj).map_err(Fault::Heap)?;
                }
                self.control = *body;
            }
```

- `bind` の引数を `rhs: &'p Rhs` にし、呼び出しの腕を次にする。`Payload::Str` と `Payload::Closure` の確保は `self.heap.alloc(Payload::...)` にする (記述子の引数を除く)。最後の `Some(Owned::new(value))` を `Some(value)` にする。

```rust
            Rhs::Call { call, saved } => {
                let resume = Resume {
                    bind: var,
                    resume: body,
                    saved,
                };
                return self.call(call, Some(resume));
            }
```

- `call` の `resume` を `Option<Resume<'p>>` にし、フレームを積むところを `if let Some(resume) = resume { self.push_frame(resume)?; }` にする。
- `enter` の `Some(Owned::new(value))` を `Some(value)` にする。
- `apply` の余りのフレームを次にし、`Ordering::Less` の確保を `self.heap.alloc(Payload::Closure(closure))` にする。

```rust
                let frame = Frame::Apply {
                    args: rest,
                    next: self.cont,
                };
                self.cont = self.heap.alloc(Payload::Frame(frame));
```

- `push_frame` と `ret` を次にする。

```rust
    /// 呼び出しの後で使う変数だけをフレームに退避する。フレームは、ちょうど所有している参照だけを持つ
    /// (docs/spec/core-ir.md)。
    fn push_frame(&mut self, resume: Resume<'p>) -> Result<(), Fault> {
        let saved = resume
            .saved
            .iter()
            .map(|&var| Ok((var.0, self.read(var)?)))
            .collect::<Result<Vec<_>, Fault>>()?;
        let frame = Frame::Return {
            function: self.function.0,
            resume: resume.resume.0,
            bind: resume.bind.0,
            saved,
            next: self.cont,
        };
        self.cont = self.heap.alloc(Payload::Frame(frame));
        Ok(())
    }

    /// 継続の先頭のフレームに値を返す。最下部の `Frame::Io` に届いたら、プログラムが終わる。
    /// 余った引数のフレームが続く間はループで適用し、Rust の再帰を使わない。
    fn ret(&mut self, mut value: Value) -> Result<Step, Fault> {
        loop {
            // 段階2までは継続を複製しないので、フレームは常に一意である。共有されたフレームは段階3の `multi` で扱う
            let Payload::Frame(frame) = self.heap.take(self.cont).map_err(Fault::Heap)? else {
                return Err(Fault::Internal("the continuation is not a frame"));
            };
            match frame {
                Frame::Apply { args, next } => {
                    self.cont = next;
                    match self.apply(value, args)? {
                        Applied::Entered => return Ok(Step::Continue),
                        Applied::Value(result) => value = result,
                    }
                }
                Frame::Return {
                    function,
                    resume,
                    bind,
                    saved,
                    next,
                } => {
                    let function = FnIdx(function);
                    let mut slots = vec![None; self.program.function(function).vars.len()];
                    for (var, saved) in saved {
                        slots[var as usize] = Some(saved);
                    }
                    slots[bind as usize] = Some(value);
                    self.slots = slots;
                    self.function = function;
                    self.control = CExprId(resume);
                    self.cont = next;
                    return Ok(Step::Continue);
                }
                Frame::Io => {
                    if let Value::Obj(obj) = value {
                        self.heap.decref(obj).map_err(Fault::Heap)?;
                    }
                    return Ok(Step::Finished);
                }
            }
        }
    }
```

- `atom` を次にし、`read` を足す。`atom` と `atoms` は `&self` にする。

```rust
    /// 変数の値。読み出しはスロットを書き換えない。ヒープの値の所有権を渡すかどうかは Core IR の命令が決める
    /// (docs/spec/core-ir.md)。
    fn read(&self, var: VarId) -> Result<Value, Fault> {
        self.slots[var.0 as usize].ok_or(Fault::Internal("a variable read before it was bound"))
    }

    fn atom(&self, atom: &Atom) -> Result<Value, Fault> {
        Ok(match *atom {
            Atom::Var(var) => self.read(var)?,
            Atom::Int(n) => Value::Int(n),
            Atom::Unit => Value::Unit,
            Atom::Tag(tag) => Value::Tag(tag),
        })
    }

    fn atoms(&self, atoms: &[Atom]) -> Result<Vec<Value>, Fault> {
        atoms.iter().map(|atom| self.atom(atom)).collect()
    }
```

- `prim` の `ShowInt` と `StrConcat` の確保を `self.heap.alloc(Payload::Str(...))` にする。

- [ ] **Step 5: 確かめる**

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED"`
Expected: 何も出ない

Run: `git diff --no-ext-diff --stat -- '*.snap' crates/*/tests tests`
Expected: 新しい `saved_across_calls.em` とそのスナップショットだけ (`heap.rs` の単体テストは `src/` にある)

Run: `grep -rn "Owned\|ApplyFrame\|IO_HANDLER\|register\|DescId::" crates/*/src | grep -v "^crates/eml_runtime/src/heap.rs:.*DescId::\(STRING\|FRAME\|CLOSURE\)"`
Expected: 何も出ない

- [ ] **Step 6: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates tests
git commit -m "Make frames an enum that saves only the variables used after each call"
```

---

### Task 4: 共有されたオブジェクトの複製

**Files:**
- Modify: `crates/eml_runtime/src/heap.rs`
- Modify: `crates/eml_interp/src/lib.rs`

**Interfaces:**
- Consumes: Task 3 の `Payload`、`Frame`、`children`
- Produces: `Heap::take_or_copy(&mut self, obj: ObjRef) -> Result<Payload, HeapError>`。`Payload`、`Frame`、`Closure` は `Clone` を導出しない

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_runtime/src/heap.rs` の `mod tests` に足す。

```rust
    #[test]
    fn take_or_copy_takes_a_unique_object() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        assert_eq!(heap.take_or_copy(s), Ok(Payload::Str("a".to_string())));
        assert!(heap.live_objects().is_empty());
    }

    #[test]
    fn take_or_copy_copies_a_shared_object_and_its_children() {
        let mut heap = Heap::new();
        let s = string(&mut heap, "a");
        let closure = heap.alloc(Payload::Closure(Closure {
            function: 0,
            args: vec![Value::Obj(s), Value::Int(1)],
        }));
        heap.dup(closure).unwrap();
        let copy = heap.take_or_copy(closure).unwrap();
        assert_eq!(
            copy,
            Payload::Closure(Closure {
                function: 0,
                args: vec![Value::Obj(s), Value::Int(1)],
            })
        );
        // 元のクロージャと写した中身が、捕まえた文字列の参照を1つずつ持つ
        heap.decref(closure).unwrap();
        heap.decref(s).unwrap();
        assert!(heap.live_objects().is_empty());
    }
```

Run: `cargo test -p eml_runtime take_or_copy`
Expected: コンパイルエラー (`take_or_copy` がない)

- [ ] **Step 2: 実装する**

`crates/eml_runtime/src/heap.rs`:

- `Payload`、`Closure`、`Frame` の `#[derive(Debug, Clone, PartialEq)]` を `#[derive(Debug, PartialEq)]` にする。`Closure` の doc の「`Clone` は `ObjRef` を `dup` せずに複製するので、複製した側が参照を数え直す。」を消す。
- `take` の後に足す。

```rust
    /// オブジェクトの所有権を受け取って中身を使う側のための手続き。一意なら解放して中身を返す。共有されていれば
    /// 中身を写し、写した中身の子の参照を1つずつ増やしてから、元の参照を1つ手放す。子は解放と同じ `children` で
    /// 数えるので、写すときと解放するときで数える参照が一致する (docs/spec/runtime.md)。
    pub fn take_or_copy(&mut self, obj: ObjRef) -> Result<Payload, HeapError> {
        if self.is_unique(obj)? {
            return self.take(obj);
        }
        let copy = copy(&self.object(obj)?.payload);
        let mut shared = Vec::new();
        children(&copy, &mut shared);
        for child in shared {
            self.dup(child)?;
        }
        self.decref(obj)?;
        Ok(copy)
    }
```

- `children` の前に足す。

```rust
/// 中身の写し。子の参照は数え直さないので、`take_or_copy` だけが使う。
fn copy(payload: &Payload) -> Payload {
    match payload {
        Payload::Str(text) => Payload::Str(text.clone()),
        Payload::Closure(closure) => Payload::Closure(Closure {
            function: closure.function,
            args: closure.args.clone(),
        }),
        Payload::Frame(Frame::Return {
            function,
            resume,
            bind,
            saved,
            next,
        }) => Payload::Frame(Frame::Return {
            function: *function,
            resume: *resume,
            bind: *bind,
            saved: saved.clone(),
            next: *next,
        }),
        Payload::Frame(Frame::Apply { args, next }) => Payload::Frame(Frame::Apply {
            args: args.clone(),
            next: *next,
        }),
        Payload::Frame(Frame::Io) => Payload::Frame(Frame::Io),
    }
}
```

`crates/eml_interp/src/lib.rs` の `take_closure` を次にする。

```rust
    /// 呼び出しはクロージャの所有権を受け取る。共有されていれば、ランタイムが中身を写して子の参照を数え直す
    /// (docs/spec/runtime.md)。
    fn take_closure(&mut self, obj: ObjRef) -> Result<Closure, Fault> {
        match self.heap.take_or_copy(obj).map_err(Fault::Heap)? {
            Payload::Closure(closure) => Ok(closure),
            _ => Err(Fault::Internal("applying an object that is not a closure")),
        }
    }
```

- [ ] **Step 3: 確かめる**

Run: `cargo test -p eml_runtime take_or_copy`
Expected: 2件が PASS

Run: `cargo test --no-fail-fast 2>&1 | grep -E "^test .* FAILED|test result: FAILED"`
Expected: 何も出ない

Run: `grep -rn "derive(.*Clone" crates/eml_runtime/src/heap.rs`
Expected: `ObjRef`、`Value`、`DescId`、`HeapError` だけに出る

- [ ] **Step 4: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates
git commit -m "Copy shared objects through the runtime instead of deriving Clone"
```

---

### Task 5: 文書

**Files:**
- Modify: `docs/spec/core-ir.md`、`docs/spec/runtime.md`、`docs/implementation/architecture.md`、`docs/implementation/status.md`、`docs/implementation/testing.md`

**Interfaces:**
- Consumes: Task 1〜4 の結果
- Produces: なし

日本語の文書を書く前に `yomiyasu:yomiyasu` スキルを読み込み、その規則に従う。

- [ ] **Step 1: spec を直す**

`docs/spec/core-ir.md` の「変数の読み出しは所有権の移動 (move) とし、…フレームや環境を解放するときは、まだ残っている値だけを decref すればよい。」の最後の文を、次にする。

```markdown
呼び出しのフレームには、呼び出しの後で使う変数だけを退避する。Core IR の呼び出しは、その変数の並びを持つ。そのため、フレームはちょうど所有している参照だけを持ち、フレームを解放するときは退避した値を1回ずつ decref すればよい。
```

`docs/spec/runtime.md`:

- 「ヘッダから記述子 (フィールドのレイアウトと `Lin` の破棄処理) を引けるようにする。…」の後に足す。

```markdown
- 記述子はペイロードの種類から決める。継続のフレームは、どの種類も同じ記述子にする。
```

- 「確保、`dup` / `decref`、フィールドの読み出し、一意かどうかの判定を API とする。」を「確保、`dup` / `decref`、フィールドの読み出し、一意かどうかの判定、共有されたオブジェクトの複製を API とする。」にする。
- 「クロージャも、同じ RC で管理するオブジェクトである。…」の後に足す。

```markdown
- 共有されたオブジェクトの中身を使う側 (クロージャの呼び出し、段階3の multi-shot の再開) は、中身を写し、写した中身の子の参照を1つずつ増やしてから、元の参照を1つ手放す。写すときと解放するときで、数える子を一致させる。
```

- [ ] **Step 2: `docs/implementation/architecture.md` を直す**

- 「`execute(Arc<Program>, &RunConfig, stdout: OutputSink) -> RunResult`。`RunResult` は `Completed` / `RuntimeError` である」を「`execute(Arc<Program>, &RunConfig, stdout: OutputSink) -> Result<(), RuntimeError>`。`RuntimeError` は `eml_interp` の型を再公開したものである」にする。
- 「クロージャは `Payload::Closure(Closure)` …リファクタリング R3b でフレームの種類の enum に直す ([status.md](status.md) の「リファクタリング」)」を、次にする。

```markdown
- クロージャは `Payload::Closure(Closure)` (フィールドは `function` と `args`) で、関数値の呼び出し (`Call::Apply`) は eval/apply で行う。継続のフレームは `Payload::Frame(Frame)` で、`Frame` は種類の enum である。`Return` は呼び出し元に戻るフレームで、呼び出しの後で使う変数だけを退避する。`Apply` は余った引数を持ち、戻った関数値に適用する。`Io` は継続の最下部の `IO` の handler である。記述子はペイロードの種類から決める
```

- 「共有されたクロージャを呼ぶときは、捕まえた値の参照を複製してからクロージャを手放す」を、次にする。

```markdown
- 共有されたクロージャを呼ぶときは、`Heap::take_or_copy` で中身を写し、写した中身の子の参照を1つずつ増やしてから元の参照を手放す。`Payload` は `Clone` を導出しないので、`ObjRef` を `dup` せずに複製できない
```

- 「verifier (`verify.rs`) は、…デバッグビルドの `lower` が毎回呼ぶ」の後に足す。

```markdown
- 退避のパス (`saved.rs`) は、Perceus の後に、各呼び出し (`Rhs::Call`) の `saved` を、その呼び出しの後で使う変数で埋める。RC の対象でない変数と、`jump` の先の join point の本体で使う変数も含む。verifier は、`saved` の RC の対象の変数が所有している変数とちょうど一致すること、呼び出しの後は `saved` の変数と結果の変数だけが範囲にあることを確かめる
```

- 「変数のスロットは所有する参照の数を持つ (`eml_runtime::Owned { value, refs }`)。…文字どおり成り立つ」を、次にする。

```markdown
- 環境のスロットは値だけを持ち、読み出しはスロットを書き換えない。参照の所有は Core IR の命令 (使用、`dup`、`decref`) が表し、verifier が釣り合いを確かめる。呼び出しのフレームには `saved` の変数だけを退避するので、フレームはちょうど所有している参照だけを持ち、解放するときは退避した値を1回ずつ解放する
```

- 「CEK 機械の継続は、…最下部に `IO` の handler のフレームを置く」の最後の文を「最下部に `IO` の handler のフレーム (`Frame::Io`) を置く」にし、その後に足す。

```markdown
- 実行時エラーは `RuntimeError` (実行中の関数で止まった `Fault` と、`debug_heap` の `Leak`) で、`step` は `Result<Step, Fault>` を返す。`Fault` に関数の名前を付けるのは `run` である。表示の文言は CLI と UI テストが使う
```

- [ ] **Step 3: `docs/implementation/status.md` を直す**

- 「リファクタリング」の表の R3b の行の状態を「完了」にする。
- 「テストを変えないために曲げた箇所」の表の1の行の「今の負担」を「R3b でフレームの種類の enum (`Return`、`Apply`、`Io`) にした」にする。
- `#### R3b ランタイムとインタプリタ` の節の箇条を消し、次の1段落にする。

```markdown
R3b で済んだ。`Frame` を種類の enum にし、記述子をペイロードの種類から決めるようにした。呼び出しの後で使う変数を Core IR に記録してフレームにはその変数だけを退避し、`Owned::refs` をなくした。共有されたオブジェクトの複製を `take_or_copy` にまとめて `Clone` をなくし、`RuntimeError` に型を付けた。これで、段階3の前のリファクタリングはすべて済んだ。
```

- 「次の作業の注意点」の「段階3の `drop k` は、継続のフレームをヒープの子の走査で解放する。…がこれを保証する」を、次にする。

```markdown
- 段階3の `drop k` は、継続のフレームをヒープの子の走査で解放する。これは、フレームがちょうど所有している参照だけを持つことに頼っている。呼び出しのフレームが呼び出しの後で使う変数だけを退避すること (Core IR の `saved`) と、verifier がその一致を確かめることが、これを保証する。`perform` のフレームも、同じく後で使う変数だけを退避する
```

- 「段階3の注意: `Payload` / `Frame` は `Clone` を導出しているが、…なければならないため」を、次にする。

```markdown
- 段階3の注意: multi-shot の `resume` では、継続のフレームを `Heap::take_or_copy` で写す。写すときと解放するときで、数える子は `children` で一致させてある。また `perform` は、呼び出しと同じく後で使う変数を退避するフレームを積む必要がある。join point への `jump` と末尾呼び出しはフレームを積まないので、捕獲した継続から再開するには、`perform` の後で使う値がフレームに残っていなければならないため
```

- 「各 crate の実装状況」の `eml_runtime` の行の「クロージャのオブジェクトと、余った引数のフレーム」を「クロージャのオブジェクト、継続のフレームの種類 (`Return`、`Apply`、`Io`)、共有されたオブジェクトの複製」にし、`eml_interp` の行の末尾に「型を付けた実行時エラー」を足す。
- 「完了した作業」の表の末尾に足す。

```markdown
| リファクタリング R3b | 継続のフレームを種類の enum にし、記述子をペイロードの種類から決めるようにした。呼び出しの後で使う変数を Core IR に記録してフレームにはその変数だけを退避し、`Owned::refs` をなくした。共有されたオブジェクトの複製を `take_or_copy` にまとめて `Clone` をなくし、実行時エラーに型を付けた |
```

- [ ] **Step 4: `docs/implementation/testing.md` に記録する**

「テストの変更の記録」の末尾に足す。

```markdown
### リファクタリング R3b

- `eml_runtime` の `Heap::register` をなくしたので、`registered_descriptors_are_counted_by_name` を削除した (種類1)。段階4で `data` の記述子を足すときに、登録のテストを書き直す。スロットが複数の参照を持つことがなくなったので、`a_slot_with_two_references_releases_both` を削除した (種類1)。フレームが退避した値を1回ずつ解放することは `decref_releases_children` が確かめる
- Core IR の呼び出しが、呼び出しの後で使う変数を持つようになった。`eml_core_ir/tests/lower.rs` の `partial_and_extra_arguments_use_closures`、`builtins_used_as_values_are_wrapped`、`lambdas_are_lifted_with_their_captures_first`、`calls_in_tail_position_are_tail_calls` の呼び出しの後ろに `[...]` が付いた (種類2)
- `eml_runtime` の単体テスト、`eml_interp/tests/closures.rs` と `run.rs`、`eml_core_ir/tests/verify.rs` の手書きの Core IR、`eml_cli/tests/api.rs` と `ui.rs` の実行の結果の扱いを、新しい型に合わせて書き換えた (種類3)。期待値は変えていない
```

- [ ] **Step 5: 文書を検査してコミットする**

Run: `for f in docs/spec/core-ir.md docs/spec/runtime.md docs/implementation/architecture.md docs/implementation/status.md docs/implementation/testing.md; do python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py $f; done`
Expected: 書き足した部分の指摘を見直す。英単語の前後の半角空白と箇条書きの比率は直さない

Run: `cargo test`
Expected: PASS

```bash
git add docs/spec docs/implementation
git commit -m "Document refactor R3b in the specs, architecture, and status"
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

Run: `grep -rn "ApplyFrame\|IO_HANDLER\|Owned\|fn register\|RunResult\|RuntimeError(pub" crates/*/src crates/*/tests`
Expected: 何も出ない

Run: `grep -n "Frame::Io\|Frame::Return\|Frame::Apply" crates/eml_runtime/src/heap.rs | head -3`
Expected: 出る

- [ ] **Step 3: 変わったテストが名前を挙げたものだけであることを確かめる**

Run: `git diff --no-ext-diff main --stat | grep -E "tests/|\.snap"`
Expected: 次だけが出る
- `crates/eml_core_ir/tests/lower.rs`、`crates/eml_core_ir/tests/verify.rs`
- `crates/eml_interp/tests/closures.rs`、`crates/eml_interp/tests/run.rs`
- `crates/eml_cli/tests/api.rs`、`crates/eml_cli/tests/ui.rs`
- `tests/ui/run/saved_across_calls.em` とそのスナップショット

Run: `git diff --no-ext-diff main -- crates/eml_cli/tests/snapshots | grep '^-' | grep -v '^---'`
Expected: 何も出ない (既存の UI のスナップショットは変わっていない)

## 完了後の後始末

ブランチ全体のレビューが済んだら、作業用の文書を削除する。

```bash
git rm docs/superpowers/specs/2026-10-04-refactor-r3b-runtime-design.md docs/superpowers/plans/2026-10-04-refactor-r3b-runtime.md
git commit -m "Remove the work documents of refactor R3b"
```
