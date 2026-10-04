# リファクタリング R4: Core IR のパスの構成 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `eml_core_ir` のパスの順番を1か所に置き、途中のパスで止めた IR をテストから見られるようにする。あわせて、パスの間の `captures` の約束、verifier の2つの度合い、`lower.rs` の分割を入れる。

**Architecture:** まず `lower.rs` を `translate/` に機械的に分け、パスを呼ぶ `pipeline.rs` を作る (Task 1)。verifier に Perceus より前の IR を確かめる `verify_scopes` を足す (Task 2)。`pipeline.rs` に `Pass` と `lower_until` を入れ、パスの間で `captures` を埋め直し、各パスの後に検査をかける (Task 3)。Core IR のテストを、確かめるパスごとのファイルに組み替える (Task 4)。最後に文書を直す (Task 5)。

**Tech Stack:** Rust (edition 2024)、la-arena 0.3、insta 1.49。新しい外部 crate は足さない。

**Spec:** `docs/superpowers/specs/2026-10-05-refactor-r4-core-ir-passes-design.md` (R4 の設計)。規範は `docs/spec/core-ir.md`。テストの運用は `docs/implementation/testing.md`。

## Global Constraints

- 作業は `main` から切ったブランチ `refactor-r4` で行う。タスクごとにコミットする (Task 4 は2回)。コミットメッセージは英語で、末尾に次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01SYHi6StxQEaEz8JneipqQK
  ```

- 外部 crate は増やさない。`unsafe` を書かない
- コードのコメントは日本語で、何をするかではなく理由を書く。規則を指すときは `docs/` のパスを書く。日本語を書く前に `yomiyasu:yomiyasu` スキルを読み、その規則に従う (CLAUDE.md)
- 言語の観測できる振る舞いは変えない。UI テスト、CLI テスト、`eml_interp` のテスト、`tests/verify.rs` の既存のテストの期待値は1文字も変えない
- `lower` の結果 (Perceus の後の IR) は変えない。`tests/perceus.rs` に移す4件の期待値が変わったら、R4 の誤りとして扱う
- 既存のテストで変えてよいのは、Task 4 に挙げたものだけである。それ以外の期待値が変わったら、変えずに止まり、差分と理由をユーザーに示して承認を得る。設計を曲げてテストを守ることはしない
- debug ビルドの各パスの後の検査 (Task 3) が、既存のテストで panic したら、`simplify` か変換に元からあった誤りが見つかったことになる。検査を緩めずに止まり、panic の文言と再現するソースをユーザーに示す
- 各タスクの最後に `cargo test`、`cargo clippy --all-targets`、`cargo fmt` を通す。clippy の警告を残さない
- 各段階は `fn stage(&In) -> (Out, Vec<Diagnostic>)` の純粋関数のままにする。`eml_core_ir` は今までどおり誤りのないプログラムだけを受け取り `Program` を返す
- 長い連鎖をたどる処理はループで書く。Rust の再帰を新しく増やさない

## Review Focus

- 2万個の文の `if` が続く関数。`captures` の埋め直しと `verify_scopes` が各パスの後に加わっても、文の数に比例する時間で終わらなければならない → 既存の `tests/verify.rs` の `a_long_run_of_if_statements_is_verified_in_linear_time` (`core` を通るので、Task 3 の後は追加の検査も含めて測る)
- B2 が作った join point (`()` を受ける引数を持つ) にも、`Simplify` の直後に正しい `captures` が出なければならない → Task 4 の `tests/simplify.rs` の `an_arm_reached_twice_stays_a_join_point` (`join j0(u7) [s2]` が残ることを差分の検査で確かめる)
- `lower_until` で途中で止めたときも、止めたパスまでの検査がかからなければならない → Task 3 の `tests/support.rs` の `core_until_stops_after_the_named_pass` (debug ビルドで `Translate` と `Simplify` で止める)
- Perceus より前の IR では、呼び出しの後も変数が範囲に残る。`verify_scopes` が呼び出しの後で範囲を区切り直すと、正しい IR を拒んでしまう → Task 2 の `scopes_keep_variables_in_scope_after_a_call`
- Perceus より前の IR では、同じ変数を `dup` なしで2回使う。`verify_scopes` が所有を数えると、正しい IR を拒んでしまう → Task 2 の `scopes_accept_a_value_used_twice_without_dup`

---

### Task 1: `lower.rs` を `translate/` に分け、パスを呼ぶ `pipeline.rs` を作る

関数の中身を変えずに、置き場所と可視性だけを変える。テストの期待値は変わらない (種類3)。

**Files:**
- Delete: `crates/eml_core_ir/src/lower.rs`
- Create: `crates/eml_core_ir/src/pipeline.rs`
- Create: `crates/eml_core_ir/src/translate/mod.rs`
- Create: `crates/eml_core_ir/src/translate/expr.rs`
- Create: `crates/eml_core_ir/src/translate/program.rs`
- Create: `crates/eml_core_ir/src/translate/types.rs`
- Modify: `crates/eml_core_ir/src/lib.rs:3-13` (`mod` と `pub use`)

**Interfaces:**
- Consumes: なし
- Produces:
  - `pub(crate) fn translate::translate(module: &eml_hir::Module, typed: &eml_types::TypedModule) -> Program`。HIR から Core IR への変換だけを行い、`simplify` も Perceus もかけない
  - `pub fn pipeline::lower(module: &Module, typed: &TypedModule) -> Program`。R4 の前の `lower` と同じ結果を返す。`lib.rs` が `pub use pipeline::lower;` で公開する

- [ ] **Step 1: 今のテストが通ることを確かめる**

Run: `cargo test`
Expected: すべて PASS。ここで落ちるテストがあれば、R4 を始める前にユーザーに知らせる。

- [ ] **Step 2: ファイルを分ける**

`crates/eml_core_ir/src/lower.rs` (940行) の中身を、次の表のとおりに移す。行番号は今の `lower.rs` の行である。関数と型の本体、コメント、doc コメントはそのまま写す。

| 今の行 | 中身 | 移す先 |
|---|---|---|
| 16-79 | `lower` の doc コメントと、本体の `simplify::simplify` の直前まで (`Program { ... }` を組み立てるところまで) | `translate/mod.rs` の `pub(crate) fn translate(module: &Module, typed: &TypedModule) -> Program`。最後の `let mut program = Program { ... };` は `Program { ... }` を返す式にする |
| 80-88 | `simplify` と Perceus の呼び出し、debug ビルドの verifier | `pipeline.rs` の `lower` (下のコード) |
| 91-106 | `Strings` | `translate/program.rs` |
| 108-290 | `ProgramBuilder` と、その `impl` | `translate/program.rs` |
| 292-323 | `boxed`、`var_info`、`split_arrows` | `translate/types.rs` |
| 325-360 | `Lowering`、`lowering` | `translate/types.rs` |
| 362-387 | `Binding`、`Bindings`、`Exit`、`exit_with` | `translate/mod.rs` |
| 389-405 | `FnLowering` | `translate/mod.rs` |
| 407-604 | `impl FnLowering` のうち `lower`、`lift`、`pat_type`、`new_var`、`push`、`seq`、`tail`、`tail_expr`、`stmts` | `translate/mod.rs` の `impl FnLowering<'_>` |
| 605-898 | `impl FnLowering` のうち `ty`、`bind`、`call_known`、`call_builtin`、`call_operation`、`atom`、`call_args`、`bind_pat` | `translate/expr.rs` の `impl FnLowering<'_>` (同じ型の2つ目の `impl` ブロック) |
| 900-940 | `effect_index`、`perform_call`、`effect_table` | `translate/program.rs` |

可視性は次の規則で付ける。

- `translate/mod.rs` で定義した型と、そのフィールドは、子のモジュール (`expr.rs`、`program.rs`、`types.rs`) から見えるので、`pub` を付けなくてよい
- `expr.rs`、`program.rs`、`types.rs` で定義した関数、型、メソッド、フィールドのうち、ほかのファイルから使うものには `pub(super)` を付ける。例えば `expr.rs` の `atom` は `mod.rs` の `tail_expr` が呼ぶので `pub(super) fn atom` にする。`program.rs` の `ProgramBuilder` は、`mod.rs` が `builder.functions` や `builder.strings.values` を読むので、そのフィールドにも `pub(super)` を付ける
- 迷ったら、まず付けずにビルドし、`cargo build -p eml_core_ir` のエラーが出たものにだけ付ける

各ファイルの `use` は、そのファイルで使うものだけにする。`translate/mod.rs` は子のモジュールを宣言し、使うものを取り込む。

```rust
mod expr;
mod program;
mod types;

use program::{ProgramBuilder, effect_table};
use types::{split_arrows, var_info};
```

(実際に要る名前は、ビルドのエラーに合わせて足し引きする。)

各ファイルの先頭には、次の doc コメントを置く。

`translate/mod.rs`:

```rust
//! 型付き HIR から Core IR への変換 (docs/spec/core-ir.md)。式の値の渡し先と join point の組み立てという、制御の
//! 骨組みをここに置く。式ごとの変換は `expr.rs`、関数の表と包む関数は `program.rs`、型から決まる変数の性質と
//! 組み込みの変換の種類は `types.rs` にある。
```

`translate/expr.rs`:

```rust
//! 式ごとの変換と、呼び出しの引数の個数による場合分け (docs/spec/core-ir.md の eval/apply)。
```

`translate/program.rs`:

```rust
//! 変換の途中で関数を足していく表と、組み込みと操作を包む関数、入口の関数、エフェクトの表。
```

`translate/types.rs`:

```rust
//! 型から決まる変数の性質 (boxed かどうか) と、組み込みを Core IR のどの命令にするか。
```

`crates/eml_core_ir/src/pipeline.rs` は次の内容にする。

```rust
//! Core IR のパスの順番 (docs/spec/core-ir.md)。順番を知っているのはこのファイルだけにする。

use eml_hir::Module;
use eml_types::TypedModule;

use crate::{Program, perceus, simplify, translate};

/// 診断のエラーがないプログラムだけを受け取る。エラーがあれば `eml_cli` は Core IR を作らない
/// (docs/implementation/architecture.md)。
pub fn lower(module: &Module, typed: &TypedModule) -> Program {
    let mut program = translate::translate(module, typed);
    simplify::simplify(&mut program);
    perceus::insert(&mut program);
    // Perceus の誤りを、実行した経路だけでなく変換のたびに見つける (docs/spec/core-ir.md)
    #[cfg(debug_assertions)]
    if let Err(error) = crate::verify(&program) {
        panic!("internal error: invalid Core IR: {error}");
    }
    program
}
```

`translate::translate` の doc コメントは、今の `lower` の doc コメント (16-17行) を写さず、次にする。

```rust
/// 誤りのない型付き HIR を、RC の命令のない Core IR にする。`captures` は空のままでよい (docs/spec/core-ir.md)。
```

`crates/eml_core_ir/src/lib.rs` の 3-13行を次にする。

```rust
mod liveness;
mod perceus;
mod pipeline;
mod pretty;
mod simplify;
mod translate;
mod verify;

pub use eml_types::Linearity;
pub use pipeline::lower;
pub use pretty::pretty;
pub use verify::{VerifyError, verify};
```

最後に `git rm crates/eml_core_ir/src/lower.rs` で元のファイルを消す。

- [ ] **Step 3: ビルドとテストを通す**

Run: `cargo build -p eml_core_ir && cargo test`
Expected: すべて PASS。スナップショットは1つも変わらない (`cargo insta pending-snapshots` が空)。

- [ ] **Step 4: 移動だけであることを確かめる**

Run: `git diff --cached --stat; git diff --stat`
Expected: 変わったのは `lib.rs`、`lower.rs` (削除)、`pipeline.rs` と `translate/` の4ファイル (追加) だけ。

次のコマンドで、関数の本体の行が失われていないことを確かめる。消えた `lower.rs` の行のうち、空白を除いて新しいファイルのどこにもない行が、`use` の行、`pub(super)` を付けた宣言の行、`lower` の本体の組み替えの行だけであることを目で確かめる。

```bash
SCRATCH=<スクラッチのディレクトリ>
git show HEAD:crates/eml_core_ir/src/lower.rs | sed 's/^ *//' | sort -u > "$SCRATCH/old.txt"
cat crates/eml_core_ir/src/pipeline.rs crates/eml_core_ir/src/translate/*.rs | sed 's/^ *//; s/pub(super) //; s/pub(crate) //' | sort -u > "$SCRATCH/new.txt"
comm -23 "$SCRATCH/old.txt" "$SCRATCH/new.txt"
```


- [ ] **Step 5: clippy と fmt**

Run: `cargo clippy --all-targets && cargo fmt`
Expected: 警告なし。

- [ ] **Step 6: コミット**

```bash
git add -A crates/eml_core_ir/src
git commit -m "Split Core IR lowering into translate/ and a pipeline module

<末尾の2行>"
```

---

### Task 2: Perceus より前の IR を確かめる `verify_scopes` を足す

**Files:**
- Modify: `crates/eml_core_ir/src/verify.rs`
- Modify: `crates/eml_core_ir/src/lib.rs` (`pub use verify::{VerifyError, verify, verify_scopes};`)
- Test: `crates/eml_core_ir/tests/verify.rs`

**Interfaces:**
- Consumes: なし
- Produces: `pub fn eml_core_ir::verify_scopes(program: &Program) -> Result<(), VerifyError>`。範囲と引数の数を確かめ、`Dup`、`Decref`、空でない `saved` を拒む。所有は数えず、呼び出しの後に範囲を区切り直さない。誤りの文言は次の3つが新しい
  - `` `{var}` is duplicated before Perceus ``
  - `` `{var}` is released before Perceus ``
  - `a call saves [{vars}] before Perceus`

  (`VerifyError` の表示は、今までどおり末尾に `` in `{function}` `` が付く)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/verify.rs` の先頭の `use` に `verify_scopes` を足す。

```rust
use eml_core_ir::{
    Atom, CExpr, CExprId, Call, CoreFn, EffectInfo, FnIdx, JoinId, OperationInfo, PrimOp, Program,
    Rhs, VarId, VarInfo, verify, verify_scopes,
};
```

`fn check` の直後に補助関数を足す。

```rust
fn check_scopes(functions: Vec<CoreFn>) -> Result<(), String> {
    verify_scopes(&program(functions, 0, &["s"])).map_err(|error| error.to_string())
}
```

ファイルの末尾に次のテストを足す。

```rust
#[test]
fn scopes_accept_a_value_used_twice_without_dup() {
    // Perceus より前の IR には `dup` がないので、所有は数えない
    let exprs = vec![CExpr::Return(var(1)), concat(1, 0, 0, 0)];
    let twice = function("twice", 1, vec![boxed("s"), boxed("t")], exprs, &[]);
    assert_eq!(check_scopes(vec![twice]), Ok(()));
}

#[test]
fn scopes_accept_a_join_point_before_perceus() {
    assert_eq!(check_scopes(vec![pick(false)]), Ok(()));
}

#[test]
fn scopes_keep_variables_in_scope_after_a_call() {
    // `saved` は Perceus が決めるので、その前は呼び出しの後で範囲を区切り直さない
    let k = function("k", 1, vec![unboxed("a")], vec![CExpr::Return(var(0))], &[]);
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
    let f = function(
        "f",
        1,
        vec![unboxed("n"), unboxed("t"), unboxed("t")],
        exprs,
        &[],
    );
    assert_eq!(check_scopes(vec![k, f]), Ok(()));
}

#[test]
fn scopes_reject_a_dup() {
    let exprs = vec![
        CExpr::Return(var(1)),
        concat(1, 0, 0, 0),
        CExpr::Dup {
            var: VarId(0),
            body: CExprId(1),
        },
    ];
    let twice = function("twice", 1, vec![boxed("s"), boxed("t")], exprs, &[]);
    assert_eq!(
        check_scopes(vec![twice]),
        Err("`s0` is duplicated before Perceus in `twice`".to_string())
    );
}

#[test]
fn scopes_reject_a_decref() {
    let exprs = vec![
        CExpr::Return(Atom::Int(1)),
        CExpr::Decref {
            var: VarId(0),
            body: CExprId(0),
        },
    ];
    let ignore = function("ignore", 1, vec![boxed("s")], exprs, &[]);
    assert_eq!(
        check_scopes(vec![ignore]),
        Err("`s0` is released before Perceus in `ignore`".to_string())
    );
}

#[test]
fn scopes_reject_a_saved_list() {
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
    let f = function("f", 1, vec![boxed("s"), boxed("t")], exprs, &[]);
    assert_eq!(
        check_scopes(vec![identity(), f]),
        Err("a call saves [s0] before Perceus in `f`".to_string())
    );
}

#[test]
fn scopes_reject_a_variable_used_outside_its_scope() {
    let f = function("f", 0, vec![boxed("s")], vec![CExpr::Return(var(0))], &[]);
    assert_eq!(
        check_scopes(vec![f]),
        Err("`s0` is used outside its scope in `f`".to_string())
    );
}
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_core_ir --test verify`
Expected: コンパイルエラー (`verify_scopes` が `eml_core_ir` にない)。

- [ ] **Step 3: `verify.rs` に検査の度合いを入れる**

ファイルの先頭の doc コメントを次にする。

```rust
//! Core IR の不変条件の検査 (docs/spec/core-ir.md)。Perceus の後のプログラムについては、変数と join point の範囲、
//! 直接呼び出しとクロージャの引数の数、RC の対象の変数の所有権の釣り合いを確かめる (`verify`)。Perceus より前の
//! プログラムについては、範囲と引数の数を確かめ、RC の命令がまだないことを確かめる (`verify_scopes`)。どちらも
//! join point の `captures` を宣言として扱い、生存解析には頼らない。
```

`pub fn verify` を次の4つに置き換える。

```rust
pub fn verify(program: &Program) -> Result<(), VerifyError> {
    verify_at(program, Level::Ownership)
}

pub fn verify_scopes(program: &Program) -> Result<(), VerifyError> {
    verify_at(program, Level::Scopes)
}

/// 検査の度合い。Perceus より前の IR には、所有権を確かめる材料 (`dup`、`decref`、`saved`) がまだない。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Scopes,
    Ownership,
}

fn verify_at(program: &Program, level: Level) -> Result<(), VerifyError> {
    for function in &program.functions {
        Checker::new(program, function, level)
            .run()
            .map_err(|message| VerifyError {
                function: function.name.clone(),
                message,
            })?;
    }
    Ok(())
}
```

`Checker` に欄 `level: Level` を足し (`function` の次)、`Checker::new` を次にする。

```rust
    fn new(program: &'a Program, function: &'a CoreFn, level: Level) -> Self {
        // Perceus より前は所有を数えないので、どの変数も RC の対象として扱わない。束縛、使用、join point の入口、
        // `jump` の所有の検査は、これで範囲の検査だけになる
        let tracked = match level {
            Level::Ownership => tracked(function),
            Level::Scopes => vec![false; function.vars.len()],
        };
        Checker {
            program,
            function,
            level,
            tracked,
            bound: HashSet::new(),
            stamps: vec![None; function.vars.len()],
            scope_log: Vec::new(),
            epoch: 0,
            next_epoch: 1,
            defined_joins: HashSet::new(),
        }
    }
```

`check` の `CExpr::Let` の腕を次にする。

```rust
                CExpr::Let { var, rhs, body } => {
                    self.check_rhs(&mut state, rhs)?;
                    if let Rhs::Call { saved, .. } = rhs {
                        match self.level {
                            Level::Scopes if !saved.is_empty() => {
                                return Err(format!(
                                    "a call saves {} before Perceus",
                                    self.names(saved)
                                ));
                            }
                            Level::Scopes => {}
                            Level::Ownership => {
                                self.check_saved(&state, saved)?;
                                // 呼び出しの後は、退避した変数だけが範囲に残る
                                self.epoch = self.next_epoch;
                                self.next_epoch += 1;
                                for &var in saved {
                                    self.enter_scope(var);
                                }
                            }
                        }
                    }
                    self.bind(&mut state, *var)?;
                    id = *body;
                }
```

`CExpr::Dup` と `CExpr::Decref` の腕の先頭に、RC の命令を許すかの検査を足す。

```rust
                CExpr::Dup { var, body } => {
                    self.rc_allowed(*var, "duplicated")?;
                    *self.count(&mut state, *var, "duplicated")? += 1;
                    id = *body;
                }
                CExpr::Decref { var, body } => {
                    self.rc_allowed(*var, "released")?;
                    self.give_up(&mut state, *var, "released")?;
                    id = *body;
                }
```

`give_up` の後ろに次のメソッドを足す。

```rust
    /// RC の命令は Perceus だけが入れる (docs/spec/core-ir.md のパスの表)。
    fn rc_allowed(&self, var: VarId, what: &str) -> Result<(), String> {
        if self.level == Level::Scopes {
            return Err(format!("`{}` is {what} before Perceus", self.name(var)));
        }
        Ok(())
    }
```

`crates/eml_core_ir/src/lib.rs` の `pub use verify::{VerifyError, verify};` を `pub use verify::{VerifyError, verify, verify_scopes};` にする。

- [ ] **Step 4: テストを通す**

Run: `cargo test -p eml_core_ir --test verify`
Expected: 新しい7件を含めてすべて PASS。既存のテストの期待値は変えていない。

Run: `cargo test`
Expected: すべて PASS。

- [ ] **Step 5: clippy と fmt**

Run: `cargo clippy --all-targets && cargo fmt`
Expected: 警告なし。

- [ ] **Step 6: コミット**

```bash
git add crates/eml_core_ir/src/verify.rs crates/eml_core_ir/src/lib.rs crates/eml_core_ir/tests/verify.rs
git commit -m "Add a scope-only verifier for Core IR before Perceus

<末尾の2行>"
```

---

### Task 3: `Pass` と `lower_until` を入れ、パスの間で `captures` を埋め直して検査する

**Files:**
- Modify: `crates/eml_core_ir/src/pipeline.rs`
- Modify: `crates/eml_core_ir/src/lib.rs` (`pub use pipeline::...`、`CExpr::Join::captures` のコメント)
- Modify: `crates/eml_core_ir/src/simplify.rs:1-2` (先頭のコメント)
- Modify: `crates/eml_core_ir/src/translate/mod.rs` (`translate` の doc コメント)
- Modify: `crates/eml_test_support/src/lib.rs` (`core_until`)
- Test: `crates/eml_test_support/tests/support.rs`

**Interfaces:**
- Consumes: Task 1 の `translate::translate`、Task 2 の `verify_scopes`、既存の `liveness::analyze(&mut CoreFn) -> BlockLiveness`、`simplify::simplify(&mut Program)`、`perceus::insert(&mut Program)`
- Produces:
  - `pub enum eml_core_ir::Pass { Translate, Simplify, Perceus }` (`Debug, Clone, Copy, PartialEq, Eq`)
  - `pub fn eml_core_ir::lower_until(module: &Module, typed: &TypedModule, last: Pass) -> Program`
  - `pub fn eml_core_ir::lower(module: &Module, typed: &TypedModule) -> Program` (= `lower_until(.., Pass::Perceus)`)
  - `pub fn eml_test_support::core_until(text: &str, last: eml_core_ir::Pass) -> Program` (feature `core`)
  - debug ビルドの panic の文言 `internal error: invalid Core IR after {translate|simplify|perceus}: {error}`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_test_support/tests/support.rs` の先頭の `use` を次にする。

```rust
use eml_core_ir::{Atom, CExpr, CExprId, CoreFn, FnIdx, Linearity, Pass, Rhs, VarId, pretty, verify};
use eml_diagnostics::{Diagnostic, ErrorCode, Label, TextRange};
use eml_test_support::ir::{boxed, program, unboxed, var};
use eml_test_support::{
    check, core, core_until, full, lower, lower_clean, parse, parse_clean, run, short, short_text,
    source, with_diagnostics,
};
```

`core_rejects_programs_with_errors` の後ろに次のテストを足す。

```rust
#[test]
fn core_until_stops_after_the_named_pass() {
    // `simplify` は、値を返すだけの join point を消す
    let joined = "pick : Bool -> Int\npick c =\n  let y = if c then 1 else 2\n  y\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    assert!(pretty(&core_until(joined, Pass::Translate)).contains("join j0"));
    assert!(!pretty(&core_until(joined, Pass::Simplify)).contains("join"));
    // `s` を2回使うので、Perceus の後にだけ `dup` が入る
    let twice = "twice : String -> String\ntwice s = s ++ s\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    assert!(!pretty(&core_until(twice, Pass::Translate)).contains("dup"));
    assert!(!pretty(&core_until(twice, Pass::Simplify)).contains("dup"));
    assert!(pretty(&core_until(twice, Pass::Perceus)).contains("dup s0"));
    assert_eq!(
        pretty(&core_until(twice, Pass::Perceus)),
        pretty(&core(twice))
    );
}
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_test_support --test support core_until`
Expected: コンパイルエラー (`core_until` と `Pass` がない)。

- [ ] **Step 3: `pipeline.rs` を書き換える**

`crates/eml_core_ir/src/pipeline.rs` を次の内容にする。

```rust
//! Core IR のパスの順番 (docs/spec/core-ir.md)。順番を知っているのはこのファイルだけにする。テストは `lower_until`
//! で、確かめたいパスの直後の IR を見る。

use eml_hir::Module;
use eml_types::TypedModule;

use crate::{Program, liveness, perceus, simplify, translate, verify, verify_scopes};

/// `lower_until` で止める位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    Translate,
    Simplify,
    Perceus,
}

impl Pass {
    fn name(self) -> &'static str {
        match self {
            Pass::Translate => "translate",
            Pass::Simplify => "simplify",
            Pass::Perceus => "perceus",
        }
    }
}

/// 診断のエラーがないプログラムだけを受け取る。エラーがあれば `eml_cli` は Core IR を作らない
/// (docs/implementation/architecture.md)。
pub fn lower(module: &Module, typed: &TypedModule) -> Program {
    lower_until(module, typed, Pass::Perceus)
}

/// `last` の直後で止める。止めたパスまでの検査は、debug ビルドでかける。
pub fn lower_until(module: &Module, typed: &TypedModule, last: Pass) -> Program {
    let mut program = translate::translate(module, typed);
    settle(&mut program, Pass::Translate);
    if last == Pass::Translate {
        return program;
    }
    simplify::simplify(&mut program);
    settle(&mut program, Pass::Simplify);
    if last == Pass::Simplify {
        return program;
    }
    perceus::insert(&mut program);
    check(&program, Pass::Perceus);
    program
}

/// パスの中では `captures` が古くなってよいが、パスの間ではつねに正しくする (docs/spec/core-ir.md のパスの表)。
/// 途中で止めた IR の表示と、`verify_scopes` が `captures` を宣言として扱うためである。
fn settle(program: &mut Program, pass: Pass) {
    for function in &mut program.functions {
        liveness::analyze(function);
    }
    check(program, pass);
}

/// 誤りを、それを作ったパスの名前で報告する。実行した経路だけでなく、変換のたびに見つけるためである。
fn check(program: &Program, pass: Pass) {
    if !cfg!(debug_assertions) {
        return;
    }
    let result = match pass {
        Pass::Translate | Pass::Simplify => verify_scopes(program),
        Pass::Perceus => verify(program),
    };
    if let Err(error) = result {
        panic!(
            "internal error: invalid Core IR after {}: {error}",
            pass.name()
        );
    }
}
```

`crates/eml_core_ir/src/lib.rs` の `pub use pipeline::lower;` を `pub use pipeline::{Pass, lower, lower_until};` にする。

`lib.rs` の `CExpr::Join::captures` の doc コメントを次にする。

```rust
        /// 本体が使う外側の変数 (`param` を除く)。RC の対象かどうかによらずすべて入れ、`VarId` の昇順に並べる。
        /// パスの中では古くなってよく、パスの間ではパイプラインが埋め直す (docs/spec/core-ir.md のパスの表)。
        captures: Vec<VarId>,
```

`crates/eml_core_ir/src/simplify.rs` の先頭の2行を次にする。

```rust
//! join point を書き換える最適化 (docs/spec/core-ir.md)。変換の後、Perceus の前に置く。RC の命令がまだないので、
//! 所有権を扱わずに書き換えられる。`captures` は古くなりうるが、このパスの後にパイプラインが埋め直す。
```

`translate/mod.rs` の `translate` の doc コメントを次にする。

```rust
/// 誤りのない型付き HIR を、RC の命令のない Core IR にする。`captures` は空のままでよく、パイプラインが埋める
/// (docs/spec/core-ir.md のパスの表)。
```

- [ ] **Step 4: `eml_test_support` に `core_until` を足す**

`crates/eml_test_support/src/lib.rs` の `#[cfg(feature = "core")] use eml_core_ir::Program;` を次にする。

```rust
#[cfg(feature = "core")]
use eml_core_ir::{Pass, Program};
```

`pub fn core` の後ろに次を足す。`core` は変えない。

```rust
/// 確かめたいパスの直後の Core IR を見るテストのため (docs/implementation/testing.md)。
#[cfg(feature = "core")]
pub fn core_until(text: &str, last: Pass) -> Program {
    let checked = check(text);
    assert!(
        !has_errors(&checked.diagnostics),
        "{:#?}",
        checked.diagnostics
    );
    eml_core_ir::lower_until(&checked.module, &checked.typed, last)
}
```

- [ ] **Step 5: テストを通す**

Run: `cargo test -p eml_test_support --test support`
Expected: `core_until_stops_after_the_named_pass` を含めてすべて PASS。

Run: `cargo test`
Expected: すべて PASS。スナップショットは1つも変わらない。debug ビルドの検査で `internal error: invalid Core IR after ...` の panic が出たら、Global Constraints のとおり止まってユーザーに示す。

Run: `cargo test -p eml_core_ir --test verify a_long_run_of_if_statements_is_verified_in_linear_time`
Expected: PASS (10秒以内)。

- [ ] **Step 6: clippy と fmt**

Run: `cargo clippy --all-targets && cargo fmt`
Expected: 警告なし。

- [ ] **Step 7: コミット**

```bash
git add crates/eml_core_ir/src crates/eml_test_support/src/lib.rs crates/eml_test_support/tests/support.rs
git commit -m "Lower Core IR up to a named pass and verify after every pass

<末尾の2行>"
```

---

### Task 4: Core IR のテストを、確かめるパスごとのファイルに組み替える

2回に分けてコミットする。1回目は移動だけで、期待値を変えない (種類1)。2回目は止めるパスを替え、期待値を取り直す (種類2)。分けるのは、2回目の差分を「RC の命令と `saved` が消えただけ」と機械的に確かめるためである。

**Files:**
- Modify: `crates/eml_core_ir/tests/common/mod.rs`
- Rename: `crates/eml_core_ir/tests/lower.rs` → `crates/eml_core_ir/tests/translate.rs`
- Create: `crates/eml_core_ir/tests/perceus.rs`
- Modify: `crates/eml_core_ir/tests/simplify.rs`
- Modify: `docs/implementation/test-changes.md`

**Interfaces:**
- Consumes: Task 3 の `eml_test_support::core_until`、`eml_core_ir::Pass`
- Produces: `tests/common/mod.rs` の `pub fn core_text(text: &str, last: Pass) -> String`

- [ ] **Step 1: `core_text` にパスを渡す**

`crates/eml_core_ir/tests/common/mod.rs` を次の内容にする。

```rust
use eml_core_ir::Pass;

/// 誤りのないプログラムを、`last` のパスの直後の Core IR にして表示する。
pub fn core_text(text: &str, last: Pass) -> String {
    eml_core_ir::pretty(&eml_test_support::core_until(text, last))
}
```

- [ ] **Step 2: テストを移す (期待値は変えない)**

1. `git mv crates/eml_core_ir/tests/lower.rs crates/eml_core_ir/tests/translate.rs`
2. `translate.rs` から次の4件のテスト関数 (`#[test]` の行から閉じ括弧まで) を切り取り、新しい `crates/eml_core_ir/tests/perceus.rs` に、`translate.rs` での順に貼る。本体は変えない
   - `strings_are_dupped_and_decreffed`
   - `shadowed_and_discarded_strings`
   - `a_non_tail_if_keeps_strings_used_later`
   - `calls_save_the_variables_used_after_them`
3. `perceus.rs` の先頭は次にする。

   ```rust
   //! Perceus の `dup` / `decref` の位置と、呼び出しの `saved` (docs/spec/core-ir.md)。

   mod common;

   use common::core_text;
   use eml_core_ir::Pass;
   ```

4. `translate.rs` の先頭は次にする。

   ```rust
   //! 型付き HIR から Core IR への変換 (docs/spec/core-ir.md)。`simplify` と Perceus より前の形を見る。

   mod common;

   use common::core_text;
   use eml_core_ir::Pass;
   ```

5. `simplify.rs` の `use common::core_text;` の後ろに `use eml_core_ir::Pass;` を足す
6. 3つのファイルの `core_text(text)` と `core_text("...")` を、すべて `core_text(text, Pass::Perceus)` (引数が文字列リテラルなら `core_text("...", Pass::Perceus)`) にする。この時点では全部 `Perceus` にして、期待値を変えない

- [ ] **Step 3: 期待値が変わらないことを確かめる**

Run: `cargo test -p eml_core_ir`
Expected: すべて PASS。`cargo insta pending-snapshots` が空。

Run: `cargo fmt && cargo clippy --all-targets`
Expected: 警告なし。

- [ ] **Step 4: 1回目のコミット**

```bash
git add -A crates/eml_core_ir/tests
git commit -m "Group the Core IR tests by the pass they check

<末尾の2行>"
```

- [ ] **Step 5: 止めるパスを替える**

- `translate.rs` の `Pass::Perceus` をすべて `Pass::Translate` にする
- `simplify.rs` の `Pass::Perceus` をすべて `Pass::Simplify` にする
- `perceus.rs` は変えない

Run: `cargo insta test -p eml_core_ir --accept`
Expected: `translate.rs` の12件と `simplify.rs` の7件のうち、RC の命令か `saved` を含んでいたものの期待値が書き換わる。`perceus.rs` と `verify.rs` は変わらない。

- [ ] **Step 6: 差分が RC の命令と `saved` だけであることを確かめる**

次のスクリプトをスクラッチのディレクトリに `check_rc_only.py` として保存し、リポジトリの根で実行する。

```python
"""止めるパスを替えた差分が、RC の命令の行と `saved` の並びを消しただけであることを確かめる。"""
import difflib
import re
import subprocess
import sys

files = ["crates/eml_core_ir/tests/translate.rs", "crates/eml_core_ir/tests/simplify.rs"]
diff = subprocess.run(
    ["git", "diff", "-U0", "HEAD", "--", *files],
    capture_output=True, text=True, check=True,
).stdout

removed, added = [], []
for line in diff.splitlines():
    if line.startswith(("---", "+++", "@@", "diff ", "index ")):
        continue
    body = line[1:]
    # 止めるパスの引数と、insta が選び直した生文字列の閉じ方は比べない
    if "core_text(" in body or re.fullmatch(r'\s*"#*\);', body):
        continue
    if line.startswith("-"):
        removed.append(body)
    elif line.startswith("+"):
        added.append(body)

expected = []
for line in removed:
    if re.fullmatch(r"\s*(dup|decref) \S+", line):
        continue
    if line.lstrip().startswith("let "):
        line = re.sub(r" \[[^\]]*\]$", "", line)
    expected.append(line)

if expected != added:
    sys.stdout.writelines(
        difflib.unified_diff(
            [l + "\n" for l in expected], [l + "\n" for l in added], "expected", "actual"
        )
    )
    sys.exit(1)
print("only RC instructions and saved lists were removed")
```

Run: `python3 <スクラッチ>/check_rc_only.py`
Expected: `only RC instructions and saved lists were removed`

食い違いが出たときの扱い:

- `translate.rs` だけで、`simplify` の書き換えが戻った形 (消えていた `join` が `Translate` の直後に現れる、など) なら、spec が認める変更である。どのテストのどの行かを、Step 9 の記録に書き足す
- `captures` の並び (`join` の行の `[...]`) が変わっていたら、R4 の誤りとして扱う。`simplify` が書き換えない join point の `captures` は、R4 の前と同じになるはずである
- それ以外の行が変わっていたら、止めて差分をユーザーに示す

- [ ] **Step 7: Review Focus の項目を目で確かめる**

`simplify.rs` の `an_arm_reached_twice_stays_a_join_point` の期待値に、`join j0(u7) [s2] {` の行がそのまま残っていることを確かめる。B2 が作った join point にも、`Simplify` の直後に正しい `captures` が出ていることの確認である。

`translate.rs` の `nested_join_points_capture_what_outer_join_points_need` の期待値が、`join j0(t9) [s2] {` と `join j1(t6) [s2] {` を持ち、`dup` の行がないことを確かめる。

- [ ] **Step 8: テスト全体を通す**

Run: `cargo test && cargo clippy --all-targets && cargo fmt`
Expected: すべて PASS、警告なし。

- [ ] **Step 9: 記録を足す**

`docs/implementation/test-changes.md` の末尾に、次の節を足す。Step 6 で `simplify` の書き換えが戻ったテストがあれば、2つ目の項目の後ろに、テストの名前と変わった形を1文ずつ書き足す。

```markdown
### リファクタリング R4

- `eml_core_ir/tests/lower.rs` を `translate.rs` に改名し、`dup` / `decref` の位置と `saved` を確かめる4件 (`strings_are_dupped_and_decreffed`、`shadowed_and_discarded_strings`、`a_non_tail_if_keeps_strings_used_later`、`calls_save_the_variables_used_after_them`) を新しい `perceus.rs` に移した (種類1)。`perceus.rs` は Perceus の直後の IR を見るので、期待値は変わっていない
- `translate.rs` の12件は変換の直後の IR を、`simplify.rs` の7件は `simplify` の直後の IR を見るようにした。期待値から `dup` / `decref` の行と、呼び出しの後ろの `saved` の並びが消えた (種類2)。ほかの行は変わっていないことを、差分から RC の命令と `saved` を除いて比べて確かめた。確かめる目的は、それぞれのテストの名前のとおりで変わらない
- `eml_test_support/tests/support.rs` に `core_until_stops_after_the_named_pass` を、`eml_core_ir/tests/verify.rs` に `verify_scopes` の7件を足した
```

- [ ] **Step 10: 2回目のコミット**

```bash
git add crates/eml_core_ir/tests docs/implementation/test-changes.md
git commit -m "Check each Core IR test right after the pass it is about

<末尾の2行>"
```

---

### Task 5: 文書を直す

**Files:**
- Modify: `docs/spec/core-ir.md`
- Modify: `docs/implementation/architecture.md`
- Modify: `docs/implementation/testing.md`
- Modify: `docs/implementation/status.md`
- Modify: `docs/future/roadmap.md`
- Modify: `CLAUDE.md`

**Interfaces:**
- Consumes: Task 1〜4 で入れた名前 (`pipeline.rs`、`translate/`、`Pass`、`lower_until`、`verify_scopes`、`core_until`、`tests/translate.rs`、`tests/perceus.rs`)
- Produces: なし

文書は日本語で書く。書く前に `yomiyasu:yomiyasu` スキルを読む。下の文面はそのまま使ってよい。

- [ ] **Step 1: `docs/spec/core-ir.md`**

1. 「Core IR」の節の join point の項目 (「末尾にない分岐の値は join point で受ける。」で始まる項目) の中の、

   > Perceus の最初の解析が埋めるので、変換は空のままでよい。

   を次にする。

   > パスの中では古くなってよいが、パスの間ではつねに正しい (下の「パス」)。

2. 「Core IR」の節の最後の3項目 (「マイルストーン1 で入れるパスは」「`simplify` は、変換の後」「Perceus の挿入は」で始まる項目) を「Core IR」の節から除き、「## インタプリタ (CEK 機械)」の直前に、次の新しい節を置く。

   ```markdown
   ## パス

   型付き HIR から、実行する Core IR までを、次のパスの順に作る。順番は `eml_core_ir` の `pipeline.rs` だけが持ち、テストは `lower_until` で、確かめたいパスの直後の IR を見る。

   | パス | 受け取る IR | 渡す IR |
   |---|---|---|
   | 変換 (`translate`) | 誤りのない型付き HIR | ANF、join point、末尾呼び出し。RC の命令と `saved` はない |
   | `simplify` | RC の命令のない IR | 同じ形の IR (join point を書き換えた後) |
   | Perceus | RC の命令のない IR | `dup` / `decref` と `saved` が入った IR |

   - どのパスの後でも、join point の `captures` と `joins` の索引は正しい。パスの中では `captures` が古くなってよい。RC の命令を入れる前のパスの後で、パイプラインが生存解析で埋め直す。
   - マイルストーン1 で入れるパスは、`simplify` と Perceus の `dup` / `decref` の挿入にする。reuse analysis と借用パラメータの最適化は後で追加する ([ロードマップ](../future/roadmap.md))。
   - `simplify` は、変換の後、Perceus の前に置き、join point を書き換える。分かっているタグの `jump` を枝へ直接向ける (case-of-case)、本体が1命令の join point への `jump` をその命令にする、`jump` が1つの join point をその位置に戻す、`jump` のない join point を消す、の4つを、この順 (B2、B5、B3、B4) で1巡だけ行う。どの書き換えも、本体を、その `jump` に来たときだけ実行される位置へ動かすだけなので、エフェクトの順と短絡評価は変わらない。
   - Perceus の挿入は、`simplify` の後にプログラム全体にかける独立したパスである。最初に関数ごとに生存解析を行い、呼び出しの `saved` も Perceus が決める。
   - verifier は2つの度合いを持つ。どちらも生存解析を使わず、`captures` を宣言として扱う。
     - Perceus の後の IR には、変数と join point の範囲、直接呼び出しとクロージャの引数の数、RC の対象の変数の所有権の釣り合いを確かめる。所有権は、どの経路でも、関数の入口と束縛で得た参照と `dup` で増やした参照が、使用と `decref` でちょうど使い切られることを確かめる。呼び出しでは、退避する RC の対象の変数が、その時点で所有している変数とちょうど一致し (同じ変数を2回退避しない)、呼び出しの後は退避した変数と結果の変数だけが範囲にあることを確かめる。`Join` の時点で `captures` が範囲にあり昇順であること、本体が `captures` と引数だけが範囲にある状態から始まることも確かめる。`jump` では、`captures` が範囲にあり、そのうち RC の対象をちょうど1つずつ所有していることを確かめる。`captures` が最小であることは確かめない。
     - Perceus より前の IR には、範囲と引数の数を確かめ、`dup`、`decref`、空でない `saved` がないことを確かめる。所有の数は数えず、呼び出しの後に範囲を区切り直さない。区切り直しの規則が `saved` を前提にしているためである。
   - デバッグビルドでは、各パスの直後に、そのパスの後の IR に合う度合いで verifier を通す。誤りはパスの名前を付けて報告する。
   ```

- [ ] **Step 2: `docs/implementation/architecture.md`**

1. 「各段階の規律」の表の `eml_core_ir` の行を次にする。

   ```markdown
   | `eml_core_ir` | `lower(&Module, &TypedModule) -> Program`。途中のパスで止める `lower_until(&Module, &TypedModule, Pass) -> Program` |
   ```

2. 「`eml_core_ir`、`eml_runtime`、`eml_interp` の内部」の節の、「Core IR の関数は、ANF の木をアリーナに置き」で始まる項目の直前に、次の2項目を足す。

   ```markdown
   - パスの順番は `pipeline.rs` だけが持つ。`lower_until` は、変換 (`translate/`)、`simplify`、Perceus を順にかけ、指定したパスの直後で止める。RC の命令を入れる前のパスの後では、`liveness::analyze` で `captures` を埋め直してから `verify_scopes` をかけ、Perceus の後では `verify` をかける。検査はデバッグビルドだけで、誤りはパスの名前を付けた panic にする
   - 変換は `translate/` にある。`mod.rs` は式の値の渡し先と join point の組み立てという制御の骨組み、`expr.rs` は式ごとの変換と呼び出しの場合分け、`program.rs` は関数の表 (`ProgramBuilder`)、組み込みと操作を包む関数、入口の関数、エフェクトの表、`types.rs` は型から決まる変数の性質 (`boxed`) と組み込みの変換の種類 (`lowering`) を持つ
   ```

3. 「Perceus の挿入 (`perceus.rs`) は」で始まる項目の中の、

   > 生存解析 (`liveness.rs` の `analyze`) は関数ごとに1回だけ行い、

   を次にする。

   > Perceus は最初に関数ごとに生存解析 (`liveness.rs` の `analyze`) を行う。解析は、

   (続く「`Let` の連鎖と join point の本体の連なりが長くなりうるので、作業の列で後順にたどる。」につながる。)

4. 「verifier (`verify.rs`) は」で始まる項目を次にする。

   ```markdown
   - verifier (`verify.rs`) は2つの入口を持つ。`verify` は Perceus の後に、変数と join point の範囲、引数の数、所有権の釣り合いを確かめる。`verify_scopes` は Perceus より前に、範囲と引数の数を確かめ、RC の命令と `saved` がないことを確かめる。`verify_scopes` はどの変数も RC の対象として扱わないので、束縛、使用、join point の入口、`jump` の所有の検査が範囲の検査だけになる。どちらも `captures` を宣言として扱い、生存解析は使わない
   ```

- [ ] **Step 3: `docs/implementation/testing.md`**

1. 「crate の中の置き方」の `crates/eml_test_support/` の項目の `(`parse`、`lower`、`check`、`core`、`run`、`execute`)` を `(`parse`、`lower`、`check`、`core`、`core_until`、`run`、`execute`)` にする。
2. 同じ節の「crate の結合テストが使う表示の関数は」で始まる項目の後ろに、次の項目を足す。

   ```markdown
   - Core IR の結合テストは、確かめるパスごとのファイルに置き、`eml_test_support::core_until` でそのパスの直後の IR を見る。後のパスの書き換えや RC の命令を、確かめたいことと一緒に期待値に入れないためである
   ```

3. 「今あるテストの地図」の `eml_core_ir` の行を次にする。

   ```markdown
   | `eml_core_ir` | `translate.rs` (Core IR への変換。変換の直後)、`simplify.rs` (`simplify` の書き換え。`simplify` の直後)、`perceus.rs` (`dup` / `decref` の位置と `saved`。Perceus の直後)、`verify.rs` (手書きの Core IR による verifier) | なし |
   ```

- [ ] **Step 4: `docs/implementation/status.md`**

1. 3行目の `2026-10-04 時点` を `2026-10-05 時点` にする。
2. 「リファクタリング」の節の表の R3b の行の後ろに、次の行を足す。

   ```markdown
   | R4 | Core IR のパスの構成 | パスの順番を持つ `pipeline.rs` と `lower_until`、パスの間の `captures`、verifier の2つの度合い (`verify_scopes`)、`translate/` への分割、パスごとのテスト | 完了 |
   ```

   表の直前の段落 (「段階3に入る前に、」で始まる段落) の後ろに、次の段落を足す。

   ```markdown
   段階4の前に、Core IR のパスの構成を整理する R4 を足した。段階4で `match` のコンパイルを変換に入れる前に、パスの順番を1か所に置き、途中のパスの IR をテストから見られるようにするためである。
   ```

3. 「各 crate の実装状況」の `eml_core_ir` の行の、

   > join point (`captures` を持つ) と末尾呼び出し、関数ごとに1回の生存解析、Perceus の独立したパス (`saved` を含む) と verifier、入口の関数。

   を次にする。

   > join point (`captures` を持つ) と末尾呼び出し、Perceus の独立したパス (`saved` を含む) と verifier、入口の関数。パスの順番を持つ `pipeline.rs` と、途中のパスで止める `lower_until`。パスの間で埋め直す `captures` と、Perceus より前の IR を確かめる `verify_scopes`。

4. 「次の作業の注意点」の、「段階4: `simplify` の B2 は、」で始まる項目の後ろに、次の項目を足す。

   ```markdown
   - 段階4: `match` のコンパイルは `translate/pattern.rs` に置き、決定木の枝の join point も `translate/mod.rs` の骨組み (`Exit::Jump` と `Binding::Join`) で作る。共有する枝の join point はパターン変数を受けるので、join point の引数を `Vec` にする。B2 の作り直し (上の項目) と一緒に設計する
   ```

5. 「完了した作業」の表の最後の行の後ろに、次の行を足す。

   ```markdown
   | リファクタリング R4 | Core IR のパスの順番を `pipeline.rs` に置き、途中のパスで止める `lower_until` を足した。パスの間では `captures` をつねに正しくし、Perceus より前の IR を範囲だけで確かめる `verify_scopes` を各パスの後にかける。`lower.rs` を `translate/` に分け、Core IR のテストを確かめるパスごとのファイルに組み替えた |
   ```

- [ ] **Step 5: `docs/future/roadmap.md`**

「処理系」の節の、「すぐに再開する handler の最適化」の項目の後ろに、次の項目を足す。

```markdown
- 再帰する join point (ループ化): 自己末尾呼び出しを、関数の中の自分へ `jump` する join point にする。トップレベルの関数を呼び出し元の join point にする contification も同じ枠で扱う。今は join point の本体から自分へ `jump` しない ([Core IR とインタプリタ](../spec/core-ir.md))
  - 入れる時期: マイルストーン1 では入れない。eml のローカルの `let` は再帰せず、自己末尾呼び出しはすでにフレームを積まないので、インタプリタでの効果が小さい。ループの中で変わらない値の `dup` / `decref` を減らすには、`captures` を借用として扱う Perceus の拡張も要る。借用パラメータ、evidence passing、ネイティブ化のどれかに着手するときに一緒に入れる
  - 残りの作業: verifier の「自分へ `jump` しない」制約を外すことと、変換にループ化を足すこと。join point の引数を複数にする変更は、段階4で `match` のために入れる予定である
  - 生存解析に不動点の計算は要らない。自分への `jump` の時点で生きている変数も「行き先の `captures` + 渡す値」で決まり、`captures` は本体の自由変数で決まるためである。互いに `jump` し合う join point の組は、別に設計する
```

- [ ] **Step 6: `CLAUDE.md`**

「Testing」の節の `eml_test_support` の項目の `(`parse` / `lower` / `check` / `core` / `run` / `execute`,` を `(`parse` / `lower` / `check` / `core` / `core_until` / `run` / `execute`,` にする。

- [ ] **Step 7: 文書の点検**

Run: `grep -rn "lower\.rs" docs/spec docs/implementation/architecture.md docs/implementation/testing.md docs/implementation/status.md`
Expected: `eml_core_ir` の `lower.rs` を指す箇所が残っていない (`eml_hir` の `lower.rs` と `lower/` は残ってよい)。

Run: `grep -n "関数ごとに1回" docs/spec/core-ir.md docs/implementation/architecture.md docs/implementation/status.md`
Expected: 生存解析が関数ごとに1回だと書いた箇所が残っていない。

書き足した日本語の段落を `yomiyasu` の規則で読み直す (Step 1〜5 の文面を変えた場合)。

- [ ] **Step 8: コミット**

```bash
git add docs CLAUDE.md
git commit -m "Document the Core IR pass pipeline and the recursive join point plan

<末尾の2行>"
```

---

## 全体の後始末

全体のレビューが済んだら、spec のとおり作業用の文書を消す。

```bash
git rm docs/superpowers/specs/2026-10-05-refactor-r4-core-ir-passes-design.md docs/superpowers/plans/2026-10-05-refactor-r4-core-ir-passes.md
git commit -m "Remove the working documents for refactor R4

<末尾の2行>"
```
