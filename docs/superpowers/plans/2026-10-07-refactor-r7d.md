# リファクタリング R7d 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `IO` を Prelude で宣言したエフェクトにし、`not`、`&&`、`||`、`|>`、`<|`、`>>`、`<<` を Prelude の eml の本体にし、`|>` と `<|` の脱糖をやめ、Core IR には入口から届く関数だけを `Prelude.` の名前で入れる。

**Architecture:** Core IR の変換 (`translate`) は、入口の関数から `Res::Function` の参照をたどって届く関数だけを変換し、Prelude の関数には `Prelude.` を付ける。`IO` の操作は `Res::Operation` に解決し、Core IR は操作のエフェクトが lang item の `IO` なら `perform` ではなく `Rhs::Io` を作る。HIR は `&&` と `||` だけを `if` に脱糖し、`|>` と `<|` は Prelude の関数の普通の呼び出しにする。`Bool` のタグは Core IR の定数 `FALSE` と `TRUE` のまま、テストで Prelude と照らし合わせる。

**Tech Stack:** Rust (edition 2024)、`insta`、`cargo test`。

**Spec:** `docs/superpowers/specs/2026-10-06-refactor-r7-design.md` の 3.3、3.4、4.1〜4.6、6.1、6.2、7.1〜7.4 の R7d の行。

## Global Constraints

- 互換性は気にしない。後方互換のための分岐や別名は作らない (CLAUDE.md)
- コードのコメントと `docs/` は日本語で書く。コメントは「なぜ」を書き、`docs/` の規則を指すときはパスを書く。日本語を書くときは `yomiyasu:yomiyasu` スキルの規則に従う (CLAUDE.md)
- 設計をテストに合わせて曲げない。テストの変更は種類1 (振る舞い)、種類2 (内部表現のスナップショット)、種類3 (期待値が同じ機械的な追随) に分け、種類1と種類2は `docs/implementation/test-changes.md` の「リファクタリング R7d」の節に、タスクごとに追記する (docs/implementation/testing.md)
- 期待値を変えてよいのは、各タスクの「期待値の変わるテスト」に挙げた種類のものだけである。それ以外の期待値が変わったら、受け入れずに DONE_WITH_CONCERNS で報告し、変わったテストと差分を報告に書く
- 実行する UI テスト (`tests/ui/run/`) の出力は、どのタスクでも変えない。評価の順と実行の結果は R7d で変わらない (spec 4.4)
- 各タスクの終わりに `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` を通す
- コミットのメッセージは英語で書き、末尾に次の2行を付ける
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01RSkNuf5aBtsdHwHZ92gz6y
  ```
- `git diff` は外部ツールを使う設定なので、スクリプトで差分を見るときは `git diff --no-ext-diff` を使う
- 作業中にファイルを一時的に戻すときは `git checkout <file>` を使わない。戻す前にファイルを写しておき、写しから戻す
- スナップショットの更新は `cargo insta test --accept` を使わない。`cargo insta test` で差分を出し、各タスクの確かめ方で差分を見てから `cargo insta accept` する

## spec からの補い

- `IO` に操作が入ると、Core IR の表示 (`pretty`) の先頭に `effect IO { println/1, open/1, read_all/1, close/1 }` の行が出る。`pretty` は操作のあるエフェクトをすべて表示するためである。表示から `IO` を外す特別扱いは入れない (実装を簡潔に保つ方針)。そのため、ソースから作る Core IR のスナップショットのほとんどにこの1行が増える (種類2)。spec 7.1 の R7d の種類2の行に、この計画と同じコミットで書き足す
- `>>` と `<<` の本体 (`fn x -> g (f x)`) は、`g` を持ったまま `f` を呼ぶので、`>>` のスキームに持ち越しの制約が残る。`>>` と `<<` を使う関数の `eml_types` の `dump` の `kinds:` の行が変わることがある (種類2)。これも spec 7.1 の R7d の種類2の行に書き足す。今まで Compose の包む関数は持ち越しを検査されていなかったので、線形な `g` を `multi` の `f` に合成するプログラムは、新しく E3006 になる (健全になる方向の種類1)
- `|>` と `<|` の脱糖をやめると、`eml_hir/tests/eval.rs` の `|>` のテストの手順 (`call_steps`) が変わる。観測できる評価の順は変わらないので、テストの意図 (左辺を先に評価する) は保ったまま、手順の期待値とテストの名前を直す (種類1として記録する)
- 入口から届く関数だけを変換するので、`eml_test_support::core` と `core_until` で `main` 以外の関数を確かめていたテストでは、`main` から届かない関数が Core IR から消える (spec 7.1 の55件)
- `IO` の操作は、宣言した `effect IO` の操作になるので、`hir::Program::functions()` から `println` などが消え、`operations()` に現れる。`println` を関数として引いていたテストは、操作として引く形に直す

## Review Focus

- `|>` と `<|` の評価の順: `x |> f a` は今と同じく `x`、`f`、`a` の順に評価する。`tests/ui/run/basics/pipe_evaluation_order.em` と `tests/ui/run/functions/evaluation_order.em` の出力が変わらないことで確かめる (Task 4)
- ユーザーが Prelude と同じ名前の関数 (`not`) を定義した場合: Core IR にはユーザーの `not` だけが入り、`Prelude.not` は入らない。Prelude の関数を使えば `Prelude.not` が入り、名前は重ならない (Task 3 にテストを置く)
- `println` を値として渡した場合 (`each println`): 操作を包む関数 (`op$println`) の本体が `Rhs::Io` になり、実行すると印字する (Task 2 に UI テストを置く)
- ユーザーが `println` という関数を定義していても、handler の節 `| println s k` は Prelude の `IO` の操作を引いて E1009 になる (既存の `crates/eml_hir/tests/effects.rs` の `a_user_println_does_not_make_the_io_clause_handleable` が Task 2 の後も通ることで確かめる)
- 線形な値を捕まえた関数 `g` を、`multi` の操作を起こす `f` と `f >> g` で合成した場合: `>>` の本体が `g` を持ったまま `f` を呼ぶので E3006 になり、secondary が `Prelude.em` の中を指す (Task 3 にテストを置く)

---

## ファイルの構成

| ファイル | 変更 | タスク |
|---|---|---|
| `crates/eml_core_ir/src/translate/mod.rs` | 入口から届く関数だけを変換する。Prelude の関数の名前に `Prelude.` を付ける | 1、3 |
| `crates/eml_core_ir/src/translate/program.rs` | `operation_rhs`、`IO` の操作を包む関数、`Compose` の包む関数をなくす | 2、3 |
| `crates/eml_core_ir/src/translate/expr.rs` | `call_operation` が `operation_rhs` を使う。`Lowering::Io` と `Lowering::Compose` の分岐をなくす | 2、3 |
| `crates/eml_core_ir/src/translate/types.rs` | intrinsic の表から `IO`、`not`、`>>`、`<<` の行と `Lowering::Io`、`Lowering::Compose` をなくす。表のテスト | 2、3、4 |
| `crates/eml_core_ir/src/lib.rs`、`crates/eml_interp/src/prim.rs` | `PrimOp::Not` をなくす | 3 |
| `crates/eml_hir/src/prelude.em` | `effect IO`、`not`、`&&`、`||`、`>>`、`<<`、`|>`、`<|` の本体 | 2、3、4 |
| `crates/eml_hir/src/def_map.rs`、`hir.rs`、`lower/mod.rs`、`lower/handler.rs` | 合成の `IO`、`io_operations`、`prelude_function` をなくす。E1009 を操作のエフェクトで判定する。`Bool` のタグの assert をなくす。`pipe` と `apply` をなくす | 2、3、4 |
| `crates/eml_hir/src/lower/ops.rs`、`lower/expr.rs`、`eval.rs`、`pretty.rs` | `|>` と `<|` の脱糖と `evaluate_first` をなくす | 4 |
| `crates/eml_types/src/context.rs` | `IO` を `Once` にする特別扱いをなくす | 2 |
| テスト (`eml_core_ir/tests/translate.rs`、`eml_hir/tests/*.rs`、`eml_types/tests/*.rs`、`tests/ui/`) | 各タスクに挙げる | 1〜4 |
| `docs/spec/*.md`、`docs/implementation/*.md` | R7d の内容 | 5 |

---

### Task 1: 入口から届く関数だけを Core IR に変換する

**Files:**
- Modify: `crates/eml_core_ir/src/translate/mod.rs` (`translate`)
- Test: `crates/eml_core_ir/tests/translate.rs`
- Modify: `docs/implementation/test-changes.md` (「リファクタリング R7d」の節を作る)

**Interfaces:**
- Produces: `translate` が変換するのは、入口の関数から `ExprKind::Path(Res::Function(_))` の参照をたどって届く、本体のある関数だけである。関数の名前と番号の付け方は変えない

**期待値の変わるテスト:** `main` (または入口に渡した関数) から届かない関数を定義した Core IR のスナップショット (種類2)。下見では `perceus.rs` 9件、`simplify.rs` 27件、`translate.rs` 18件、`eml_test_support/tests/support.rs` 1件だった。`translate.rs` の `the_entry_function_is_chosen_by_the_caller` は `fn main` が消える。差分は、届かない関数の `fn … { … }` の塊がまるごと消えることだけでなければならない

- [ ] **Step 1: テストを書く**

`crates/eml_core_ir/tests/translate.rs` の `the_entry_function_is_chosen_by_the_caller` の後に足す。

```rust
#[test]
fn only_functions_reachable_from_the_entry_are_lowered() {
    // 使わない Prelude の関数を Core IR に入れないため、入口から届く関数だけを変換する
    // (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 4.5)
    let text = "used : Int -> Int\nused x = x\n\nunused : Int -> Int\nunused x = x\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (used 1))";
    let shown = core_text(text, Pass::Translate);
    assert!(shown.contains("fn used("), "{shown}");
    assert!(shown.contains("fn main("), "{shown}");
    assert!(!shown.contains("fn unused("), "{shown}");
}
```

Run: `cargo test -p eml_core_ir --test translate only_functions_reachable`
Expected: FAIL (`fn unused(` が表示に含まれる)。

- [ ] **Step 2: 届く関数を集める**

`crates/eml_core_ir/src/translate/mod.rs` に関数を足す (`use std::collections::HashSet;` と、`eml_hir` の `ExprKind`、`Res` の `use` を足す)。

```rust
/// 入口の関数から届く関数。使わない Prelude の関数を Core IR に入れないため、関数の本体の参照をたどって集める
/// (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 4.5)。
fn reachable(hir: &HirProgram, entry: FunctionId) -> HashSet<FunctionId> {
    let mut seen = HashSet::new();
    let mut work = vec![entry];
    while let Some(id) = work.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(body) = hir.body(id) else {
            continue;
        };
        for (_, expr) in body.exprs.iter() {
            if let ExprKind::Path(Res::Function(callee)) = expr.kind {
                work.push(callee);
            }
        }
    }
    seen
}
```

`translate` の `defined` を次にする。

```rust
    let reachable = reachable(hir, entry);
    // intrinsic は本体を持たず、呼び出しの位置で命令にするか、包む関数を作る (`program.rs` の `wrapper`)
    let defined = || {
        hir.functions()
            .filter(|(id, function)| !function.intrinsic && reachable.contains(id))
    };
```

- [ ] **Step 3: テストを流す**

Run: `cargo test -p eml_core_ir --test translate only_functions_reachable`
Expected: PASS。

Run: `cargo insta test -p eml_core_ir -p eml_test_support`
Expected: 「期待値の変わるテスト」の種類のスナップショットだけが差分になる。`cargo insta pending-snapshots` で一覧を出し、各差分が、`main` から届かない関数の `fn` の塊がまるごと消えるだけであることを確かめてから `cargo insta accept` する。それ以外の差分があれば受け入れずに報告する。

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて PASS。

- [ ] **Step 4: test-changes.md に記録する**

`docs/implementation/test-changes.md` の末尾に節を足す。変わったテストの名前は、Step 3 で受け入れた一覧をファイルごとに書く。

```markdown
### リファクタリング R7d

- 種類2: Core IR の変換が、入口の関数から届く関数だけを変換するようにした (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 4.5)。`main` から届かない関数を定義していた次のテストのスナップショットから、その関数がまるごと消えた。ほかの関数の形は変わらない。(ここにファイルごとのテスト名を並べる)
- 種類2: `translate.rs` の `the_entry_function_is_chosen_by_the_caller` は、入口に `alt` を渡すので `fn main` が消えた
```

- [ ] **Step 5: コミットする**

```bash
git add crates docs/implementation/test-changes.md
git commit -m "Lower only the functions reachable from the entry

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RSkNuf5aBtsdHwHZ92gz6y"
```

---

### Task 2: `IO` を Prelude で宣言したエフェクトにする

**Files:**
- Modify: `crates/eml_hir/src/prelude.em` (`IO` の4つのシグネチャを `pub effect IO` にする)
- Modify: `crates/eml_hir/src/def_map.rs` (`SyntheticEffect`、`PRELUDE_EFFECTS`、`synthetic_effects` をなくす。`io_operations` を作らない。`prelude_function` をなくす)
- Modify: `crates/eml_hir/src/hir.rs` (`LangItems::io_operations` をなくす)
- Modify: `crates/eml_hir/src/lower/mod.rs` (合成のエフェクトを置く処理をなくす)
- Modify: `crates/eml_hir/src/lower/handler.rs` (E1009 を、節の操作のエフェクトが `IO` かで判定する)
- Modify: `crates/eml_types/src/context.rs` (`IO` の特別扱いをなくす)
- Modify: `crates/eml_core_ir/src/translate/program.rs`、`expr.rs`、`types.rs` (`operation_rhs`、`Lowering::Io` をなくす)
- Test: `crates/eml_core_ir/src/translate/types.rs` (単体テスト)、`crates/eml_core_ir/tests/translate.rs`、`crates/eml_hir/tests/structure.rs`、`crates/eml_types/tests/check.rs`、`tests/ui/run/effects/io_operation_as_value.em` (新規)

**Interfaces:**
- Consumes: Task 1 の、入口から届く関数だけの変換
- Produces:
  - Prelude の `pub effect IO` の操作 `println`、`open`、`read_all`、`close` (どれも `once`)。`hir::Program::lang.io` はこのエフェクトである
  - `translate/program.rs` の `pub(super) fn operation_rhs(hir: &HirProgram, op: OperationId, args: Vec<Atom>) -> Rhs`。`IO` の操作なら `Rhs::Io`、ほかは `Rhs::call(perform_call(..))`
  - `LangItems` から `io_operations` がなくなる

**期待値の変わるテスト:**
- 種類2: ソースから作る Core IR のスナップショットの先頭に `effect IO { println/1, open/1, read_all/1, close/1 }` の行が増える。`println` などを値として使うスナップショットでは、包む関数が `builtin$println` から `op$println` になる。差分はこの2つだけでなければならない
- 種類1: `crates/eml_hir/tests/structure.rs` の `prelude_signatures_without_equations_are_intrinsic_functions` から `println` を外す。`crates/eml_types/tests/check.rs` の `intrinsic_schemes_are_exported` は、`println` を操作として引く。期待する型の文字列は変えない
- E1009 のテストと UI テスト (`check-fail/names/handle_io.em`) の期待値は変えない

- [ ] **Step 1: テストを書く**

`crates/eml_core_ir/tests/translate.rs` に足す。

```rust
#[test]
fn an_io_operation_used_as_a_value_is_wrapped_with_its_io_call() {
    // `IO` の操作は perform せずに実行時がその場で処理する (docs/spec/effects.md の「組み込みの `IO`」)
    let text = "each : (String -> <IO> Unit) -> <IO> Unit\neach f = f \"x\"\n\nmain : Unit -> <IO> Unit\nmain () = each println";
    let shown = core_text(text, Pass::Translate);
    assert!(shown.contains("fn op$println(p0) {"), "{shown}");
    assert!(shown.contains("perform println(p0)"), "{shown}");
    assert!(!shown.contains("perform IO.println"), "{shown}");
}
```

`tests/ui/run/effects/io_operation_as_value.em` を作る (ディレクトリがなければ作る。既存のカテゴリのディレクトリ名は `ls tests/ui/run` で確かめ、エフェクトの実行のテストが置かれているディレクトリに合わせる)。

```
-- `IO` の操作を値として渡しても、その場で実行される。
each : (String -> <IO> Unit) -> <IO> Unit
each f =
  f "a"
  f "b"

main : Unit -> <IO> Unit
main () = each println
```

E1009 は、既存の `crates/eml_hir/tests/effects.rs` の `file_operations_cannot_be_handled`、`a_user_println_does_not_make_the_io_clause_handleable`、`clause_errors_are_reported_together` が確かめる。期待値は変えない。

Run: `cargo test -p eml_core_ir --test translate an_io_operation`
Expected: FAIL (`fn op$println(` がない。今は `builtin$println`)。

- [ ] **Step 2: Prelude と HIR を直す**

`crates/eml_hir/src/prelude.em` の4行

```
pub println : String -> <IO> Unit
pub open : String -> <IO> File
pub read_all : File -> <IO> (File, String)
pub close : File -> <IO> Unit
```

を消し、`pub data Bool` の宣言の後に次を置く。

```
-- 組み込みの `IO`。操作は実行時がその場で処理するので、ユーザーは handle できない (docs/spec/effects.md の「組み込みの `IO`」)
pub effect IO where
  println : String -> Unit
  open : String -> File
  read_all : File -> (File, String)
  close : File -> Unit
```

ファイルの先頭のコメントの「型は docs/spec/declarations.md の標準の演算子の表と、docs/spec/effects.md の組み込みの IO に従う」は、そのまま正しいので残す。

`crates/eml_hir/src/def_map.rs`:
- `SyntheticEffect`、`PRELUDE_EFFECTS`、`ModuleScope::synthetic_effects`、`DefMap::synthetic_effects`、`declare` の合成の item を登録するループを消す。`ModuleScope::new` の `(synthetic_effects, name)` は `name` だけにし、エフェクトの局所の番号を `item_id(module, k)` にする。`ModuleScope::new` の doc コメントの「合成の item を先に」を消す
- lang item の表 (`LangItems { … }`) から `io_operations` の行を消す。`IO` は `effect("IO")` が Prelude の `effect IO` の宣言を引く
- `Resolver::prelude_function` を消す (E1009 の判定のためだけにあった。ほかで使っていないことを `grep -rn prelude_function crates` で確かめる)

`crates/eml_hir/src/hir.rs` の `LangItems` から `io_operations` の欄と、その doc コメントを消す。

`crates/eml_hir/src/lower/mod.rs` の `lower_items` の、`def_map.synthetic_effects(module)` のループを消し、関数の doc コメントの「合成の item が先で、」を消す。

`crates/eml_hir/src/lower/handler.rs` の節の操作の解決を次にする (操作が見つかった後で `IO` かを判定する)。

```rust
        let op = match self.items.operation(name.text()) {
            Lookup::Found(op) => op,
            // 重複した effect の操作である。重複は E1003 で報告済み
            Lookup::Unusable => return,
            Lookup::NotFound => {
                self.diagnostics.push(Diagnostic::error(
                    codes::UNDEFINED_NAME,
                    format!("cannot find effect operation `{}`", name.text()),
                    Label::new(self.file, name_range, "not an operation of any effect"),
                ));
                return;
            }
        };
        let operation = self.operation(op);
        // `IO` の操作は実行時がその場で処理するので、handle できない (docs/spec/effects.md の「組み込みの `IO`」)
        if operation.effect == self.lang.io {
            let text = name.text();
            self.diagnostics.push(
                Diagnostic::error(
                    codes::UNHANDLEABLE_EFFECT,
                    "`IO` cannot be handled",
                    Label::new(
                        self.file,
                        name_range,
                        format!("`{text}` is an operation of the built-in `IO`"),
                    ),
                )
                .with_note("the runtime handles `IO` itself"),
            );
            return;
        }
```

`unknown_operation` は使われなくなるので消す。文言は今の `unknown_operation` のものと1字も変えない (E1009 と E1001 の期待値を変えないため)。借用の都合で `self.operation(op)` の結果を持ったまま `self.diagnostics` に積めなければ、先に `let is_io = self.operation(op).effect == self.lang.io;` を求める形にする。

- [ ] **Step 3: 型検査を直す**

`crates/eml_types/src/context.rs` の `effect_multiplicities` を次にする (`IO` も、ほかのエフェクトと同じく操作の多重度の最大になる。`IO` の操作は `once` なので `Once` になる)。

```rust
        let effect_multiplicities = program
            .effects()
            .map(|(id, effect)| {
                let multiplicity = effect
                    .operations
                    .iter()
                    .map(|&op| multiplicity(program[op].multiplicity))
                    .max()
                    .unwrap_or(Multiplicity::Never);
                (id, multiplicity)
            })
            .collect();
```

使わなくなった `lang` の束縛は clippy の指摘に従って消す。

- [ ] **Step 4: Core IR を直す**

`crates/eml_core_ir/src/translate/program.rs` の `perform_call` の前に足す (`IoOp` と `Rhs` の `use` を足す)。

```rust
/// 操作の呼び出し。`IO` の操作は実行時がその場で処理するので、`perform` ではなく `Rhs::Io` にする
/// (docs/spec/effects.md の「組み込みの `IO`」)。
pub(super) fn operation_rhs(hir: &HirProgram, op: OperationId, args: Vec<Atom>) -> Rhs {
    let operation = &hir[op];
    if operation.effect == hir.lang.io {
        let io = IoOp::from_name(&operation.name).expect("every `IO` operation has an `IoOp`");
        Rhs::Io(io, args)
    } else {
        Rhs::call(perform_call(hir, op, args))
    }
}
```

`operation_wrapper` の本体を作る行

```rust
        let body = builder.push(CExpr::TailCall(perform_call(hir, op, args)));
```

を次にする。`perform` の包む関数は今と同じく末尾呼び出しの形で作り、`IO` の操作は結果を返す形で作る。

```rust
        let (_, result_type) = split_arrows(ty, arity);
        let body = match operation_rhs(hir, op, args) {
            Rhs::Call { call, .. } => builder.push(CExpr::TailCall(call)),
            rhs => {
                let result = builder.var(var_info("t", &result_type, hir));
                let ret = builder.push(CExpr::Return(Atom::Var(result)));
                builder.push(CExpr::Let {
                    var: result,
                    rhs,
                    body: ret,
                })
            }
        };
```

`Rhs::Call` の欄の名前と `split_arrows` の戻り値の並びは、`crates/eml_core_ir/src/lib.rs` と `translate/types.rs` の定義に合わせる。

`crates/eml_core_ir/src/translate/expr.rs` の `call_operation` の最後の2行を次にする。

```rust
        let rhs = operation_rhs(self.hir, op, args);
        self.bind(out, "t", ty, rhs)
```

`crates/eml_core_ir/src/translate/types.rs`:
- `Lowering::Io` の変種と、`INTRINSICS` の `println`、`open`、`read_all`、`close` の4行を消す
- `expr.rs` と `program.rs` の `Lowering::Io` の分岐を消す

`types.rs` の単体テスト (`mod tests`) に、Prelude の `IO` の操作と `IoOp` の対応のテストを足す。`prelude_intrinsics` と同じく Prelude を構文解析する。

```rust
    /// Prelude の `effect IO` の操作の名前。
    fn prelude_io_operations() -> Vec<String> {
        let mut files = SourceFiles::new();
        let file = files.add(eml_hir::PRELUDE_PATH, eml_hir::PRELUDE_SOURCE);
        let (parse, errors) = eml_syntax::parse(file, eml_hir::PRELUDE_SOURCE);
        assert!(errors.is_empty(), "{errors:?}");
        parse
            .tree()
            .items()
            .filter_map(|item| match item {
                ast::Item::EffectItem(effect) if effect.name()?.text() == "IO" => Some(effect),
                _ => None,
            })
            .flat_map(|effect| effect.operations())
            .filter_map(|op| Some(op.name()?.text()))
            .collect()
    }

    #[test]
    fn every_io_operation_has_an_io_op_and_back() {
        let names = prelude_io_operations();
        // 壊れた Prelude で、確かめる名前が気づかないうちに減らないようにする
        assert_eq!(names.len(), IoOp::VARIANTS.len(), "{names:?}");
        for name in &names {
            assert!(IoOp::from_name(name).is_some(), "{name}");
        }
        for op in IoOp::VARIANTS {
            assert!(names.iter().any(|name| name == op.name()), "{}", op.name());
        }
    }
```

`Name::text` の戻り値の型と `IoOp::VARIANTS` の形は、`prelude_intrinsics` と `lib.rs` の既存のテストの使い方に合わせる。

- [ ] **Step 5: 種類1のテストを直す**

`crates/eml_hir/tests/structure.rs` の `prelude_signatures_without_equations_are_intrinsic_functions` の名前の並びから `"println"` を消す (`println` は関数ではなく操作になった)。

`crates/eml_types/tests/check.rs` の `intrinsic_schemes_are_exported` の `assert_eq!(ty("println"), "String -> <IO> Unit");` を、操作として引く形にする。

```rust
    let operation = |name: &str| {
        let (id, _) = checked
            .program
            .operations()
            .find(|(_, operation)| operation.name == name)
            .unwrap();
        checked.typed.decls[&Decl::Operation(id)].ty.to_string()
    };
    assert_eq!(operation("println"), "String -> <IO> Unit");
```

- [ ] **Step 6: テストを流す**

Run: `cargo test -p eml_core_ir --test translate an_io_operation && cargo test -p eml_hir --test effects && cargo test -p eml_core_ir --lib`
Expected: PASS。

Run: `cargo insta test`
Expected: 差分は「期待値の変わるテスト」の種類2の2つだけである。`cargo insta pending-snapshots` の各差分が、先頭の `effect IO { println/1, open/1, read_all/1, close/1 }` の1行の追加か、`builtin$println` などから `op$println` などへの包む関数の置き換え (本体は `perform println(..)` を返す形) だけであることを確かめてから `cargo insta accept` する。新しい UI テスト `io_operation_as_value` のスナップショットは、出力が `a` と `b` の2行であることを確かめて受け入れる。

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて PASS。

- [ ] **Step 7: test-changes.md に記録してコミットする**

`docs/implementation/test-changes.md` の「リファクタリング R7d」の節に足す。

```markdown
- 種類2: `IO` を Prelude の `effect IO` にしたので (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 4.3)、Core IR の表示の先頭に `effect IO { println/1, open/1, read_all/1, close/1 }` の行が増えた。`println` などを値として使うスナップショットでは、包む関数が `builtin$…` から `op$…` になった。(ここにファイルごとのテスト名を並べる)
- 種類1: `println` などが関数ではなく操作になったので、`eml_hir/tests/structure.rs` の `prelude_signatures_without_equations_are_intrinsic_functions` から `println` を外し、`eml_types/tests/check.rs` の `intrinsic_schemes_are_exported` は `println` を操作として引く。期待する型の文字列は変えていない
- 新しいテスト: `eml_core_ir/tests/translate.rs` の `an_io_operation_used_as_a_value_is_wrapped_with_its_io_call`、`eml_core_ir` の単体テスト `every_io_operation_has_an_io_op_and_back`、UI テスト `run/…/io_operation_as_value.em`
```

```bash
git add crates tests docs/implementation/test-changes.md
git commit -m "Declare IO as an effect of the Prelude and lower its operations to IO instructions

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RSkNuf5aBtsdHwHZ92gz6y"
```

---

### Task 3: `not`、`&&`、`||`、`>>`、`<<` を Prelude の本体にし、Prelude の関数の名前に `Prelude.` を付ける

**Files:**
- Modify: `crates/eml_hir/src/prelude.em`
- Modify: `crates/eml_hir/src/lower/mod.rs` (`Bool` のタグの `assert_eq!` をなくす)
- Modify: `crates/eml_core_ir/src/translate/mod.rs` (`Prelude.` の名前)
- Modify: `crates/eml_core_ir/src/translate/types.rs`、`expr.rs`、`program.rs` (`not`、`>>`、`<<` の行、`Lowering::Compose`、`builtin$>>` の包む関数をなくす)
- Modify: `crates/eml_core_ir/src/lib.rs`、`crates/eml_interp/src/prim.rs` (`PrimOp::Not` をなくす)
- Test: `crates/eml_core_ir/tests/translate.rs`、`crates/eml_core_ir/src/translate/types.rs` (単体テスト)、`crates/eml_hir/tests/structure.rs`、`crates/eml_types/tests/modules.rs`

**Interfaces:**
- Consumes: Task 1 の届く関数だけの変換、Task 2 の `effect IO`
- Produces: Prelude の関数は Core IR で `Prelude.<名前>` という名前になる (`Prelude.not`、`Prelude.>>`)。そのラムダの名前も、今と同じ規則でこの名前から作る。`Lowering::Compose` と `PrimOp::Not` がなくなる

**期待値の変わるテスト:**
- 種類2: `not`、`>>`、`<<` を使う Core IR のスナップショット (`prim not` が `call Prelude.not` になる。`builtin$>>` の包む関数が `Prelude.>>` とそのラムダになる)。`>>` と `<<` を使う関数の `eml_types` の `dump` の `kinds:` の行 (`>>` のスキームに持ち越しの制約が残るため)
- 種類1: `crates/eml_hir/tests/structure.rs` の `prelude_signatures_without_equations_are_intrinsic_functions` で、`>>` と `&&` は本体のある関数になる。`>>` の引数の数は 2 になる。`crates/eml_core_ir/src/translate/types.rs` の単体テストの `DESUGARED` から `&&` と `||` を外す (本体のある関数は intrinsic ではない)
- 種類1 (健全になる方向): 線形な値を捕まえた `g` を `multi` の操作を起こす `f` と合成するプログラムが E3006 になる。既存のテストにこの形があれば、その変化を報告に書く

- [ ] **Step 1: テストを書く**

`crates/eml_core_ir/tests/translate.rs` に足す。

```rust
#[test]
fn a_prelude_function_is_lowered_under_the_prelude_name() {
    // Prelude の関数の Core IR の名前には `Prelude.` を付け、ユーザーの関数と名前が重ならないようにする
    // (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 4.5)
    let text = "main : Unit -> <IO> Unit\nmain () = if not True then println \"a\" else println \"b\"";
    let shown = core_text(text, Pass::Translate);
    assert!(shown.contains("fn Prelude.not("), "{shown}");
    assert!(shown.contains("call Prelude.not("), "{shown}");
    assert!(!shown.contains("fn Prelude.&&("), "{shown}");
}

#[test]
fn a_user_function_hides_the_prelude_function_of_the_same_name() {
    let text = "not : Bool -> Bool\nnot b = b\n\nmain : Unit -> <IO> Unit\nmain () = if not True then println \"a\" else println \"b\"";
    let shown = core_text(text, Pass::Translate);
    assert!(shown.contains("fn not("), "{shown}");
    assert!(!shown.contains("Prelude.not"), "{shown}");
}

#[test]
fn the_prelude_bool_tags_match_the_core_ir_constants() {
    // Core IR は `Bool` を定数のタグで表す。タグは Prelude の宣言の順で決まる
    // (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 4.6)
    let program = eml_test_support::lower_clean("").program;
    assert_eq!(program[program.lang.false_ctor].tag, eml_core_ir::FALSE);
    assert_eq!(program[program.lang.true_ctor].tag, eml_core_ir::TRUE);
}
```

`crates/eml_types/tests/modules.rs` に、Prelude の本体の中の線形性の誤り (R7c で残した E3001〜E3004) と、`>>` を通る持ち越し (Review Focus) のテストを足す。`check_with_prelude` と `CHOICE` はこのファイルにある。

```rust
#[test]
fn linear_misuses_in_the_prelude_point_into_the_prelude() {
    let extra = "pub twice : File -> <IO> Unit\ntwice f =\n  close f\n  close f\n\npub dropped : File -> <IO> Unit\ndropped f = ()\n\npub discarded : File -> <IO> Unit\ndiscarded _ = ()\n\npub captured : File -> <IO> (Unit -> <IO> Unit)\ncaptured f = fn () -> close f\n";
    let shown = check_with_prelude(extra, "");
    // どの診断も、すべてのラベルが Prelude の中を指す
    for line in shown.lines().filter(|line| line.starts_with("  ")) {
        assert!(line.starts_with("  Prelude.em "), "{shown}");
    }
    for code in ["E3002", "E3003", "E3004"] {
        assert!(shown.contains(code), "{code}\n{shown}");
    }
}

#[test]
fn a_carry_over_through_a_composition_points_into_the_prelude() {
    // `>>` の本体は `g` を持ったまま `f` を呼ぶので、線形な `g` を `multi` の `f` と合成すると E3006 になる
    let text = "chooser : Unit -> <Choice> Unit\nchooser () =\n  let b = choose ()\n  ()\n\ncomposed : Unit -> <Choice, IO> Unit\ncomposed () =\n  let h = open \"a.txt\"\n  let k = chooser >> (fn u -> close h)\n  k ()";
    let shown = check_with_prelude(CHOICE, text);
    assert!(shown.starts_with("E3006 "), "{shown}");
    assert!(shown.contains("  test.em \">>\" "), "{shown}");
    assert!(shown.contains("  Prelude.em "), "{shown}");
}
```

2つ目のテストで、実装の後も E3006 が出ない場合は、`>>` のスキームに持ち越しの制約が残っていない。テストを直さずに、`dump` で `>>` の `kinds:` を確かめた結果を添えて DONE_WITH_CONCERNS で報告する。E3006 の前に型の誤りが出たら、テストのプログラムの型を直す (たとえば `chooser` と `fn u -> close h` の row をそろえる)。コンパイラは直さない。`captured` が E3001 にならないとき (戻り値の関数の Kind が `Lin` で許される場合) は、`captured` を `extra` から外してよい。E3001 は `E3002`〜`E3004` と同じ `linear_misuse` の経路を通るので、確かめる番号から外しても、ファイルの指し方は確かめられる。

Run: `cargo test -p eml_core_ir --test translate prelude && cargo test -p eml_core_ir --test translate a_user_function_hides && cargo test -p eml_types --test modules`
Expected: `a_prelude_function…` は FAIL (`not` は intrinsic なので `prim not` になる)。`the_prelude_bool_tags…` と `linear_misuses…` は今の実装でも PASS しうる。`a_carry_over_through_a_composition…` は FAIL (`>>` は持ち越しを検査されない intrinsic)。

- [ ] **Step 2: Prelude に本体を書く**

`crates/eml_hir/src/prelude.em` の次のシグネチャに、等式を足す (シグネチャの行はそのまま残す)。

```
pub not : Bool -> Bool
not True = False
not False = True
```

```
pub (>>) : (a -> <e> b) -> (b -> <e> c) -> a -> <e> c
f >> g = fn x -> g (f x)
pub (<<) : (b -> <e> c) -> (a -> <e> b) -> a -> <e> c
f << g = fn x -> f (g x)
```

```
-- HIR が `if` に脱糖するので、二項演算では本体を呼ばない。短絡して評価するためである
pub (&&) : Bool -> Bool -> Bool
a && b = if a then b else False
pub (||) : Bool -> Bool -> Bool
a || b = if a then True else b
```

上の3つ目のブロックの先頭のコメントは、`&&` と `||` のシグネチャの前に置く。`&&`、`||`、`|>`、`<|` の前にある既存のコメント (「HIR が脱糖する演算子。ユーザーが同じ演算子を定義すれば、普通の呼び出しになる」) は、`|>` と `<|` の前に移す。`|>` と `<|` のコメントは Task 4 で直す。

- [ ] **Step 3: Core IR と HIR を直す**

`crates/eml_core_ir/src/translate/mod.rs` の `translate` で、関数を変換するループの中で名前を作り、`root_name` と `lower` の両方に渡す。

```rust
        // Prelude の関数の名前には `Prelude.` を付け、ユーザーが同じ名前の関数を定義しても重ならないようにする
        // (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 4.5)
        let name = if id.module == hir.prelude {
            format!("Prelude.{}", function.name)
        } else {
            function.name.clone()
        };
```

`root_name: &function.name` と `.lower(&function.name, …)` を `&name` にする。

`crates/eml_core_ir/src/translate/types.rs`:
- `INTRINSICS` から `not`、`>>`、`<<` の行を消す
- `Lowering::Compose` の変種を消す。`expr.rs` と `program.rs` (`wrapper`) の `Lowering::Compose` の分岐を消す。`program.rs` の `wrapper` で、使わなくなった `split_arrows` などの束縛は clippy の指摘に従って消す
- 単体テストの `DESUGARED` を `&["|>", "<|"]` にし、doc コメントはそのまま残す

`crates/eml_core_ir/src/lib.rs` の `PrimOp` から `Not` を消し、`named_ops!` の `Not => "not"` の行を消す。`crates/eml_interp/src/prim.rs` の `PrimOp::Not` の分岐を消す。

`crates/eml_hir/src/lower/mod.rs` の、次の3行を消す (タグの確かめは Step 1 の `eml_core_ir` のテストに移した)。

```rust
    // Core IR は `Bool` を、タグ 0 の `False` と 1 の `True` で表す (docs/spec/core-ir.md)
    let tag = |id: ConstructorId| modules[id.module].items.constructors[id.local].tag;
    assert_eq!((tag(lang.false_ctor), tag(lang.true_ctor)), (0, 1));
```

使わなくなった `lang` の束縛と `ConstructorId` の `use` は clippy の指摘に従って消す。

`crates/eml_hir/tests/structure.rs` の `prelude_signatures_without_equations_are_intrinsic_functions` を次にする。

```rust
#[test]
fn prelude_signatures_without_equations_are_intrinsic_functions() {
    let module = module("f : Int\nf = 1");
    for name in ["show_int", "negate", "+", "==", "|>"] {
        let function = function(&module, name);
        assert!(function.intrinsic, "{name}");
        assert!(function.signature.is_some(), "{name}");
        assert!(module.body(function_id(&module, name)).is_none(), "{name}");
    }
    // eml で書いた Prelude の関数は intrinsic ではない (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 4.4)
    for name in ["not", "&&", "||", ">>", "<<"] {
        assert!(!function(&module, name).intrinsic, "{name}");
        assert!(module.body(function_id(&module, name)).is_some(), "{name}");
    }
    assert!(!function(&module, "f").intrinsic);
    assert_eq!(module.arity(function_id(&module, "+")), Some(2));
    assert_eq!(module.arity(function_id(&module, ">>")), Some(2));
}
```

- [ ] **Step 4: テストを流す**

Run: `cargo test -p eml_core_ir --test translate prelude && cargo test -p eml_core_ir --test translate a_user_function_hides && cargo test -p eml_types --test modules && cargo test -p eml_hir --test structure && cargo test -p eml_cli --test api the_prelude_alone`
Expected: PASS。`the_prelude_alone_has_no_diagnostics` は、Prelude の本体に誤りがないことを確かめる。

Run: `cargo insta test`
Expected: 差分は「期待値の変わるテスト」の種類2だけである。各差分が、`prim not(..)` から `call Prelude.not(..)` への置き換えと `fn Prelude.not` の追加、`builtin$>>` と `builtin$<<` の包む関数から `Prelude.>>` と `Prelude.<<` (とそのラムダ) への置き換え、`>>` と `<<` を使う関数の `kinds:` の行の変化のどれかであることを確かめてから `cargo insta accept` する。

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて PASS。UI テストの実行の出力は変わらない。

- [ ] **Step 5: test-changes.md に記録してコミットする**

「リファクタリング R7d」の節に足す。

```markdown
- 種類2: `not`、`>>`、`<<` を Prelude の eml の本体にし、Prelude の関数の Core IR の名前に `Prelude.` を付けた (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 4.4、4.5)。`prim not` が `call Prelude.not` になり、`builtin$>>` と `builtin$<<` が `Prelude.>>` と `Prelude.<<` になった。`>>` と `<<` を使う関数の `dump` の `kinds:` の行には、`>>` の本体の持ち越しの制約が現れる。(ここにファイルごとのテスト名を並べる)
- 種類1: `eml_hir/tests/structure.rs` の `prelude_signatures_without_equations_are_intrinsic_functions` で、`not`、`&&`、`||`、`>>`、`<<` を本体のある関数として確かめ、`>>` の引数の数を 2 にした。`eml_core_ir` の単体テストの `DESUGARED` から `&&` と `||` を外した
- 新しいテスト: `eml_core_ir/tests/translate.rs` の `a_prelude_function_is_lowered_under_the_prelude_name`、`a_user_function_hides_the_prelude_function_of_the_same_name`、`the_prelude_bool_tags_match_the_core_ir_constants`、`eml_types/tests/modules.rs` の `linear_misuses_in_the_prelude_point_into_the_prelude`、`a_carry_over_through_a_composition_points_into_the_prelude`
```

```bash
git add crates docs/implementation/test-changes.md
git commit -m "Write not, &&, ||, >> and << in the Prelude and name Prelude functions in Core IR

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RSkNuf5aBtsdHwHZ92gz6y"
```

---

### Task 4: `|>` と `<|` を Prelude の関数の呼び出しにする

**Files:**
- Modify: `crates/eml_hir/src/prelude.em` (`|>` と `<|` の本体)
- Modify: `crates/eml_hir/src/lower/ops.rs` (`pipe` と `apply` の分岐をなくす)
- Modify: `crates/eml_hir/src/lower/expr.rs` (`call` から `evaluate_first` をなくし、`pipe` をなくす)
- Modify: `crates/eml_hir/src/hir.rs` (`ExprKind::Call::evaluate_first`、`LangItems::pipe` と `apply`)
- Modify: `crates/eml_hir/src/def_map.rs` (lang item の表から `pipe` と `apply`)
- Modify: `crates/eml_hir/src/eval.rs`、`crates/eml_hir/src/pretty.rs`、`crates/eml_types/src/usage.rs` (`evaluate_first`)
- Modify: `crates/eml_core_ir/src/translate/types.rs` (単体テストの `DESUGARED` をなくす)
- Test: `crates/eml_hir/tests/eval.rs`、`crates/eml_hir/tests/operators.rs`、`crates/eml_hir/tests/structure.rs`、`crates/eml_types/tests/linearity.rs`、`tests/ui/check-fail/types/pipe_into_function_parameter.em`

**Interfaces:**
- Consumes: Task 3 の `Prelude.` の名前
- Produces: `ExprKind::Call { callee: ExprId, args: Vec<ExprId> }` (`evaluate_first` がない)。`lower/expr.rs` の `call(callee, args, range)` (今の第3引数 `evaluate_first` がない)。`LangItems` から `pipe` と `apply` がなくなる

**期待値の変わるテスト:**
- 種類2: `crates/eml_hir/tests/operators.rs` の `pipes_become_applications` (HIR の pretty)。`|>` と `<|` を使う Core IR のスナップショット (`x |> f a` が `call Prelude.|>` になる)
- 種類1: `crates/eml_hir/tests/eval.rs` の `the_left_of_a_pipe_is_evaluated_first`、`a_nested_pipe_is_a_callee`、`a_pipe_into_a_call_beyond_the_arity_applies_before_the_piped_arrow` (手順と名前)。`crates/eml_hir/tests/structure.rs` の intrinsic の並びから `|>` を外す。UI テスト `check-fail/types/pipe_into_function_parameter.em` の診断。`crates/eml_types/tests/linearity.rs` の `a_piped_value_is_kept_across_the_call` と `a_piped_value_is_kept_across_the_arrow_applied_before_a_later_argument` の診断 (変わった場合)
- 実行の UI テスト (`run/basics/pipe_evaluation_order.em`、`run/functions/evaluation_order.em`、`run/basics/operators.em`) の出力は変えない

- [ ] **Step 1: テストを直す**

`crates/eml_hir/tests/eval.rs` の3つのテストを次にする。`|>` は Prelude の関数の普通の呼び出しになり、左辺は最初の引数なので、呼ばれる式の `|>` の次に評価する。観測できる順 (左辺、関数、その引数) は変わらない。

```rust
#[test]
fn the_left_of_a_pipe_is_evaluated_first() {
    // `x |> f 1` は `(|>) x (f 1)` の呼び出しで、`x` を `f` と `1` より先に評価する
    assert_eq!(
        steps("t : Unit -> Int\nt () = g () |> f 1"),
        ["eval |>", "eval g ()", "eval f 1", "arrow 0", "arrow 1"]
    );
}

#[test]
fn a_nested_pipe_is_an_argument() {
    assert_eq!(
        steps("t : Unit -> Int\nt () = 1 |> (2 |> f)"),
        ["eval |>", "eval 1", "eval 2 |> f", "arrow 0", "arrow 1"]
    );
}

#[test]
fn a_pipe_passes_both_operands_together() {
    assert_eq!(
        steps("t : Unit -> Int\nt () = g () |> h 1"),
        ["eval |>", "eval g ()", "eval h 1", "arrow 0", "arrow 1"]
    );
}
```

`crates/eml_hir/tests/structure.rs` の `prelude_signatures_without_equations_are_intrinsic_functions` の intrinsic の並びから `"|>"` を消し、本体のある関数の並びに `"|>"` と `"<|"` を足す。

Run: `cargo test -p eml_hir --test eval pipe && cargo test -p eml_hir --test structure prelude_signatures`
Expected: FAIL (今は脱糖するので、手順の先頭が `eval g ()` になる。`|>` は intrinsic である)。

- [ ] **Step 2: Prelude に本体を書く**

`crates/eml_hir/src/prelude.em` の `|>` と `<|` のシグネチャに等式を足し、その前のコメントを直す。

```
-- 左辺を最初の引数に取る関数である。引数を左から評価するので、`x |> f a` は `x`、`f`、`a` の順に評価する
-- (docs/spec/expressions.md の「関数適用」)
pub (|>) : a -> (a -> <e> b) -> <e> b
x |> f = f x
pub (<|) : (a -> <e> b) -> a -> <e> b
f <| x = f x
```

- [ ] **Step 3: HIR の脱糖と `evaluate_first` をなくす**

`crates/eml_hir/src/lower/ops.rs` の `binary` から、`self.lang.pipe` と `self.lang.apply` の2つの腕を消す。関数の先頭のコメントを「解決した先が Prelude の `&&` か `||` のときだけ、短絡して評価するために `if` に脱糖する。ユーザーの定義は Prelude の演算子を隠すので、`&&` を定義すれば普通の呼び出しになる (docs/spec/declarations.md の「fixity」)」にする。`ExprKind::Call { … evaluate_first: None }` を作っている箇所から `evaluate_first: None,` を消す。

`crates/eml_hir/src/hir.rs` の `ExprKind::Call` から `evaluate_first` の欄とその doc コメントを消し、`Call` の doc コメントを「`(f a) b` は、引数を並べた1つの呼び出しにしてある。」にする。`LangItems` の `pipe` と `apply` の欄を消し、その doc コメントを `&&` と `||` だけの説明にする (「HIR が短絡して評価するために脱糖する演算子 `&&` と `||`。ユーザーが同じ演算子を定義すれば、それに解決して普通の呼び出しになる。」)。

`crates/eml_hir/src/def_map.rs` の lang item の表から `pipe: function("|>"),` と `apply: function("<|"),` を消す。

`crates/eml_hir/src/lower/expr.rs` の `call` と `pipe` を次の `call` だけにする。

```rust
    /// `(f a) b` を、引数の揃った1つの呼び出しとして型検査できるように、入れ子の呼び出しを平たくする。
    pub(super) fn call(&mut self, callee: ExprId, mut args: Vec<ExprId>, range: TextRange) -> ExprId {
        if let ExprKind::Call {
            callee: inner,
            args: inner_args,
        } = &self.exprs[callee].kind
        {
            let inner = *inner;
            let mut all = inner_args.clone();
            all.append(&mut args);
            return self.alloc(ExprKind::Call { callee: inner, args: all }, range);
        }
        self.alloc(ExprKind::Call { callee, args }, range)
    }
```

`call` を呼んでいる箇所 (`grep -rn "\.call(" crates/eml_hir/src/lower`) から第3引数を消す。

`crates/eml_hir/src/eval.rs` の `call_steps` を次にする。

```rust
pub fn call_steps(program: &Program, body: &Body, call: ExprId) -> Vec<EvalStep> {
    let ExprKind::Call { callee, args } = &body.exprs[call].kind else {
        panic!("call_steps takes a call");
    };
    let mut steps = vec![EvalStep::Eval(*callee)];
    let known = known_arity(program, body, *callee).unwrap_or(0);
    let mut pending = Vec::new();
    for (index, &arg) in args.iter().enumerate() {
        if !(index < known || is_value(program, body, arg)) {
            steps.extend(pending.drain(..).map(EvalStep::Arrow));
        }
        steps.push(EvalStep::Eval(arg));
        pending.push(index);
    }
    steps.extend(pending.into_iter().map(EvalStep::Arrow));
    steps
}
```

`crates/eml_hir/src/pretty.rs` の `ExprKind::Call` の表示から `evaluate_first` と `|>` の印を消す (`write!(s, " {}", self.expr(body, arg, indent))`)。

`crates/eml_types/src/usage.rs` の「使用回数は評価の順によらないので、先に評価する引数 (`evaluate_first`) は区別しない」のコメントを消す (区別するものがなくなった)。

ほかに `evaluate_first` を読む箇所がないことを `grep -rn evaluate_first crates` で確かめる (`ExprKind::Call { callee, args, .. }` の形で受けている箇所は `..` を外してよい)。

`crates/eml_core_ir/src/translate/types.rs` の単体テストから `DESUGARED` を消し、`every_prelude_intrinsic_has_an_implementation` を次にする。

```rust
    #[test]
    fn every_prelude_intrinsic_has_an_implementation() {
        for name in prelude_intrinsics() {
            assert!(intrinsic(&name).is_some(), "{name}");
        }
    }
```

`INTRINSICS` の doc コメントの「HIR が脱糖する `&&`、`||`、`|>`、`<|` は持たない。」を消す (どれも本体のある関数になった)。

- [ ] **Step 4: UI テストのコメントを直す**

`tests/ui/check-fail/types/pipe_into_function_parameter.em` の先頭のコメントを、新しい意味に合わせて次にする。

```
-- A lambda piped into a function is checked through the Prelude's `|>`, so the mismatch is reported on the function.
```

- [ ] **Step 5: テストを流す**

Run: `cargo test -p eml_hir --test eval pipe && cargo test -p eml_hir --test structure prelude_signatures`
Expected: PASS。

Run: `cargo test -p eml_cli --test ui`
Expected: 実行の UI テスト (`run/`) はすべて変わらずに PASS する。`pipe_evaluation_order.em` と `evaluation_order.em` が変わったら、評価の順が壊れているので実装を直す。`check-fail/types/pipe_into_function_parameter.em` のスナップショットは変わる。

Run: `cargo insta test`
Expected: 差分は「期待値の変わるテスト」のものだけである。次を確かめてから `cargo insta accept` する。
- `pipes_become_applications` は、`p` が `(@|> (@|> 1 @f) (@g 2))`、`q` が `(@<| (@g 1) (@f 2))` の形になる。テストの名前を `pipes_are_calls_of_prelude_functions` にする
- `pipe_into_function_parameter.em` は、E2001 か E2002 が1件だけ出て、primary が `each` か `|>` の位置を指す
- `linearity.rs` の2つのテストは、E3006 が出続けるか、出なくなるかを報告に書く。出なくなったら、その理由 (持ち越しの判定が `|>` のスキームを通るようになった) を報告に書き、受け入れる前に止まる
- Core IR のスナップショットは、`x |> f a` の形が `call Prelude.|>` と `f a` のクロージャになり、`fn Prelude.|>` が増えるだけである

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて PASS。

- [ ] **Step 6: test-changes.md に記録してコミットする**

「リファクタリング R7d」の節に足す。

```markdown
- 種類1: `|>` と `<|` の脱糖をやめ、Prelude の関数の普通の呼び出しにした (docs/superpowers/specs/2026-10-06-refactor-r7-design.md の 4.4)。観測できる評価の順は変わらない。`eml_hir/tests/eval.rs` の `|>` のテストの手順を、呼ばれる式 `|>` を先に評価する形にし、`a_nested_pipe_is_a_callee` を `a_nested_pipe_is_an_argument` に、`a_pipe_into_a_call_beyond_the_arity_applies_before_the_piped_arrow` を `a_pipe_passes_both_operands_together` にした。`eml_hir/tests/structure.rs` で `|>` と `<|` を本体のある関数として確かめる。UI テスト `check-fail/types/pipe_into_function_parameter.em` の診断が `|>` を通した型の不一致になった。(`linearity.rs` の2つのテストが変わったら、ここに書く)
- 種類2: `eml_hir/tests/operators.rs` の `pipes_become_applications` を `pipes_are_calls_of_prelude_functions` にし、HIR の表示から `|>` の印をなくした。`|>` と `<|` を使う Core IR のスナップショットが `call Prelude.|>` の形になった。(ここにファイルごとのテスト名を並べる)
```

```bash
git add crates tests docs/implementation/test-changes.md
git commit -m "Call the Prelude for |> and <| instead of desugaring them

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RSkNuf5aBtsdHwHZ92gz6y"
```

---

### Task 5: 文書を更新する

**Files:**
- Modify: `docs/spec/effects.md` (`IO` は Prelude の `effect IO` の宣言で、操作は `once`)
- Modify: `docs/spec/declarations.md` (標準の演算子の表の `|>` と `<|`、ユーザーの定義が隠せる演算子の段落)
- Modify: `docs/spec/expressions.md` (「関数適用」の評価の順の `x |> f a` の文、HIR の脱糖の一覧)
- Modify: `docs/spec/core-ir.md` (入口から届く関数だけの変換、`Prelude.` の名前、`builtin$>>` と `builtin$<<` がなくなること、`Bool` のタグの定数を Prelude と照らし合わせるテスト)
- Modify: `docs/implementation/architecture.md`、`docs/implementation/testing.md`、`docs/implementation/status.md`

文書は日本語で書き、書く前に `yomiyasu:yomiyasu` スキルを読み込んで規則に従う。`docs/implementation/test-changes.md` は Task 1〜4 で書いたので、ここでは名前の並べ漏れがないかだけを確かめる。

- [ ] **Step 1: spec を直す**

- `effects.md` の「組み込みの `IO`」に、`IO` は Prelude で `pub effect IO` として宣言したエフェクトで、操作は `once` であること、実行時がその場で処理するので handle できない (E1009) ことを書く。操作の表はそのまま残す
- `declarations.md` の標準の演算子の表の `|>` `<|` の行を「`x |> f` は Prelude の関数 `x |> f = f x` の呼び出しである。引数を左から評価するので、`x` を先に評価する。`f <| x` は `f <| x = f x` の呼び出し」にする。103行目付近の「`&&`、`||`、`|>`、`<|` も、ユーザーの定義が Prelude のものを隠せる」の段落を、`&&` と `||` だけが脱糖され、ユーザーの定義に隠されると短絡しない普通の関数になる形にする
- `expressions.md` の「関数適用」の「`x |> f a` の `x` だけは最初に評価する」を、`x |> f a` は `(|>) x (f a)` の呼び出しなので、引数の順に `x` を先に評価する、という説明にする。190行目付近の HIR の脱糖の一覧から `|>` と `<|` を外す
- `core-ir.md` に、`translate` は入口の関数から届く関数だけを変換すること、Prelude の関数の名前は `Prelude.` で始まること、`Bool` はタグ 0 の `False` と 1 の `True` で表し、Prelude の宣言の順と一致することをテストで確かめることを書く。包む関数の一覧 (`op$…`、`con$…`、`builtin$…`) の `builtin$…` は、intrinsic を値として使うときの包む関数として残る (`builtin$+` など) ので残す

- [ ] **Step 2: implementation の文書を直す**

- `architecture.md`: `IO` は Prelude の `effect IO` で、`DefMap` の合成の item はなくなった (137行目付近の「`IO` は R7d まで、`DefMap` が Prelude に足す操作のないエフェクト」を直す)。HIR の脱糖は `&&` と `||` だけになり、`evaluate_first` がなくなった。Core IR は入口から届く関数だけを `Prelude.` の名前で変換し、`IO` の操作を `Rhs::Io` にする (`operation_rhs`)
- `testing.md`: テストの地図で、`eml_types` の `modules.rs` の説明に Prelude の本体の中の線形性と `>>` を通る持ち越しを足す
- `status.md`: R7 の行を「R7a、R7b (R7b-1〜R7b-3)、R7c、R7d 完了。… R7d で `IO` を Prelude の `effect IO` にし、`not`、`&&`、`||`、`|>`、`<|`、`>>`、`<<` を eml で書き、`|>` と `<|` の脱糖をやめ、Core IR を入口から届く関数だけにした。R7e は未着手」の形にする。「R7 で直す項目」の組み込みの項目の「`Bool` のタグは R7d で扱う」を「R7d で、`Bool` のタグの定数を Prelude と照らし合わせるテストにした」にする。`eml_hir`、`eml_types`、`eml_core_ir` の行に R7d の変更を足す

- [ ] **Step 3: lint を流してコミットする**

Run: `python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py docs/spec/effects.md` (ほかの変えた文書も同じ)。英単語の前後の空白と箇条書きの比率の指摘は、`docs/` の既存の書き方に合わせて残してよい。文末のコロンは直す。

Run: `grep -rn "evaluate_first\|io_operations\|SyntheticEffect\|Lowering::Compose\|PrimOp::Not" docs --exclude-dir=superpowers`
Expected: 完了した作業の記録 (`status.md` と `test-changes.md` の過去の回の説明) だけが残る。

```bash
git add docs
git commit -m "Document refactor R7d: IO as a Prelude effect, Prelude bodies, pipes as calls, reachable Core IR

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RSkNuf5aBtsdHwHZ92gz6y"
```
