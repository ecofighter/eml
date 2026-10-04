# テスト補助コードの統一 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 各 crate が別々に書いているテスト補助の処理を `eml_test_support` と各 crate の `tests/common/` に寄せる。期待値は1文字も変えない。

**Architecture:** 先に `eml_test_support` に `with_diagnostics`、`short_text`、`parse_clean`、`lower_clean`、`ir` モジュールをテストつきで足す (Task 1)。次に上流の crate から順に、各テストの組み立てをそれに置き換える (Task 2〜5)。最後に文書を直し (Task 6)、作業用の文書を消す (Task 7)。

**Tech Stack:** Rust (edition 2024)、insta 1.49。新しい外部 crate は足さない。

**Spec:** `docs/superpowers/specs/2026-10-05-test-helpers-design.md`

## Global Constraints

- 作業は `main` から切ったブランチ `test-helpers` で行う。タスクごとにコミットする。コミットメッセージは英語で、末尾に次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
  ```

- 既存のテストの変更は種類3 (機械的な追随) だけにする。スナップショットの文字列と `assert_eq!` / `assert!` の期待値は1文字も変えない。例外は、「診断がない」ことを確かめる前提の `assert!` (`ast.rs` の `source`、`structure.rs` の `module`、`eml_hir/tests/effects.rs` の2か所) で、同じ条件のまま `parse_clean` / `lower_clean` に移る。期待値が変わったら、変えずに止まり、差分をユーザーに示す
- 外部 crate は増やさない。`eml_test_support` は `src/` の `#[cfg(test)]` から使わない
- コードのコメントは日本語で、何をするかではなく理由を書く。日本語を書く前に `yomiyasu:yomiyasu` スキルを読み、その規則に従う (CLAUDE.md)。英単語の前後の半角空白はリポジトリの書き方に合わせて残す
- 各タスクの最後に、`INSTA_UPDATE=no cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` を通す。clippy の警告を残さない。`cargo fmt --check` が差分を出したら `cargo fmt` で直してからコミットする
- 各タスクの最後に、スナップショットの書き残しがないことを確かめる。`.snap.new` と `.pending-snap` は `.gitignore` にあるので `find` で探す

  ```bash
  find . -path ./target -prune -o \( -name '*.snap.new' -o -name '*.pending-snap' \) -print
  ```

  Expected: 何も出ない。

## Review Focus

- `render` (ariadne) に空の診断を渡したときに空でない文字列が返ると、`lexer.rs` と `parser.rs` の誤りのないケースの表示に `---` が増える。`eml_diagnostics` の単体テスト `renders_nothing_for_no_diagnostics` が空を保証している → Task 2 の `INSTA_UPDATE=no cargo test`
- HIR の診断の並べ替えが安定でないと、同じ位置の診断の順が変わる。今と同じ `sort_by_key` (安定) を使う → Task 3 の `INSTA_UPDATE=no cargo test`
- `common` の関数のうち、あるテストファイルが使わないものが dead_code の警告になる → Task 3 の `cargo clippy --all-targets`
- `parse_clean` / `lower_clean` が警告だけのソースを通してしまうと、今の `source` / `module` より条件が緩くなる。条件は「診断が1件もない」のまま保つ → Task 1 の実装
- 種類3を外れた変更 (期待値の行の変化) が紛れ込む → Task 2〜5 の Step「種類3の確かめ」

---

### Task 1: eml_test_support に共通の部品を足す

**Files:**
- Modify: `crates/eml_test_support/src/lib.rs`
- Test: `crates/eml_test_support/tests/support.rs`

**Interfaces:**
- Consumes: なし
- Produces:
  - `pub fn with_diagnostics(dump: String, diagnostics: &str) -> String`
  - `pub fn short_text(files: &SourceFiles, diagnostics: &[Diagnostic]) -> String`
  - `pub fn parse_clean(text: &str) -> Parsed`
  - `#[cfg(feature = "hir")] pub fn lower_clean(text: &str) -> Lowered`
  - `#[cfg(feature = "core")] pub mod ir` に `pub fn var(n: u32) -> Atom`、`pub fn boxed(name: &str) -> VarInfo`、`pub fn unboxed(name: &str) -> VarInfo`、`pub fn program(functions: Vec<CoreFn>, entry: u32, strings: &[&str]) -> Program`

- [ ] **Step 1: ブランチを切る**

```bash
cd /Users/arakaki/Projects/eml
git switch -c test-helpers main
```

- [ ] **Step 2: 失敗するテストを書く**

`crates/eml_test_support/tests/support.rs` の `use` を次に置き換える。

```rust
use eml_core_ir::{Atom, CExpr, CExprId, CoreFn, FnIdx, Linearity, Rhs, VarId, verify};
use eml_diagnostics::{Diagnostic, ErrorCode, Label, TextRange};
use eml_test_support::ir::{boxed, program, unboxed, var};
use eml_test_support::{
    check, core, full, lower, lower_clean, parse, parse_clean, run, short, short_text, source,
    with_diagnostics,
};
```

ファイルの末尾に次のテストを足す。

```rust
#[test]
fn with_diagnostics_leaves_a_dump_without_diagnostics_as_it_is() {
    assert_eq!(with_diagnostics("tree\n".to_string(), ""), "tree\n");
}

#[test]
fn with_diagnostics_separates_the_diagnostics_with_a_line() {
    assert_eq!(
        with_diagnostics("tree\n".to_string(), "E0001 1:1 bad\n"),
        "tree\n---\nE0001 1:1 bad\n"
    );
}

#[test]
fn short_text_ends_every_line_with_a_newline() {
    let (files, file) = source("a\nbcd");
    let diagnostics = [
        Diagnostic::error(ErrorCode(1), "bad", Label::new(file, range(0, 1), "here")),
        Diagnostic::error(ErrorCode(2), "worse", Label::new(file, range(3, 4), "here")),
    ];
    assert_eq!(short_text(&files, &diagnostics), "E0001 1:1 bad\nE0002 2:2 worse\n");
    assert_eq!(short_text(&files, &[]), "");
}

#[test]
fn parse_clean_returns_a_tree_without_diagnostics() {
    assert!(parse_clean("x = 1").diagnostics.is_empty());
}

#[test]
#[should_panic]
fn parse_clean_rejects_a_syntax_error() {
    parse_clean("x = €");
}

#[test]
fn lower_clean_returns_a_module_without_diagnostics() {
    assert!(lower_clean("f : Int -> Int\nf x = x").diagnostics.is_empty());
}

#[test]
#[should_panic]
fn lower_clean_rejects_an_undefined_name() {
    lower_clean("f : Int -> Int\nf x = g x");
}

#[test]
fn ir_builds_a_program_the_verifier_accepts() {
    // main () = let s = "s" in let n = 1 in decref s; ()
    let main = CoreFn {
        name: "main".to_string(),
        params: vec![],
        vars: vec![boxed("s"), unboxed("n")],
        body: CExprId(3),
        exprs: vec![
            CExpr::Return(Atom::Unit),
            CExpr::Decref {
                var: VarId(0),
                body: CExprId(0),
            },
            CExpr::Let {
                var: VarId(1),
                rhs: Rhs::Atom(Atom::Int(1)),
                body: CExprId(1),
            },
            CExpr::Let {
                var: VarId(0),
                rhs: Rhs::ConstString(0),
                body: CExprId(2),
            },
        ],
        joins: Vec::new(),
    };
    let program = program(vec![main], 0, &["s"]);
    assert_eq!(verify(&program).map_err(|error| error.to_string()), Ok(()));
    assert_eq!(program.entry, FnIdx(0));
    assert_eq!(program.strings, ["s"]);
    assert!(program.effects.is_empty());
    assert_eq!(var(3), Atom::Var(VarId(3)));
    let s = &program.functions[0].vars[0];
    assert_eq!((s.name.as_str(), s.linearity, s.boxed), ("s", Linearity::Unr, true));
    let n = &program.functions[0].vars[1];
    assert_eq!((n.name.as_str(), n.linearity, n.boxed), ("n", Linearity::Unr, false));
}
```

`eml_test_support` の dev-dependencies に `eml_core_ir` がなければ、`crates/eml_test_support/Cargo.toml` に次を足す。

```toml
[dev-dependencies]
eml_core_ir.workspace = true
```

`CExpr::Decref` と `Rhs::Atom` の正確な形 (フィールド名、`Atom` が `PartialEq` と `Debug` を持つか、`Linearity` が `Copy` と `PartialEq` を持つか) は `crates/eml_core_ir/src/` で確かめる。形が違えば、同じ意味のプログラム (文字列を1つ束縛して `decref` し、`Unit` を返す) になるようにテストの組み立てだけを合わせる。

- [ ] **Step 3: テストが失敗することを確かめる**

Run: `cargo test -p eml_test_support --test support 2>&1 | tail -20`
Expected: コンパイルエラー。`with_diagnostics`、`short_text`、`parse_clean`、`lower_clean`、`ir` が見つからないという内容。

- [ ] **Step 4: 実装する**

`crates/eml_test_support/src/lib.rs` で、`#[cfg(feature = "hir")] pub fn lower` の後ろに次を足す。

```rust
/// 前提として診断のないソースを使うテストのため。条件を緩めないよう、警告も1件として数える。
pub fn parse_clean(text: &str) -> Parsed {
    let parsed = parse(text);
    assert_clean(&parsed.files, &parsed.diagnostics);
    parsed
}

#[cfg(feature = "hir")]
pub fn lower_clean(text: &str) -> Lowered {
    let lowered = lower(text);
    assert_clean(&lowered.files, &lowered.diagnostics);
    lowered
}

fn assert_clean(files: &SourceFiles, diagnostics: &[Diagnostic]) {
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics:\n{}",
        short_text(files, diagnostics)
    );
}
```

`pub fn short` の後ろに次を足す。

```rust
/// `full` と同じく文字列にして、段階の表示の後ろにそのまま足せるようにする。
pub fn short_text(files: &SourceFiles, diagnostics: &[Diagnostic]) -> String {
    short(files, diagnostics)
        .into_iter()
        .map(|line| line + "\n")
        .collect()
}

/// 診断の形式は段階のテストごとに違うので、区切り方だけをここでそろえる。
pub fn with_diagnostics(mut dump: String, diagnostics: &str) -> String {
    if !diagnostics.is_empty() {
        dump.push_str("---\n");
        dump.push_str(diagnostics);
    }
    dump
}
```

ファイルの末尾に次を足す。

```rust
/// フロントエンドからは作れない Core IR を手で組むテストが使う部品 (docs/implementation/testing.md の「テストの置き場所」)。
#[cfg(feature = "core")]
pub mod ir {
    use eml_core_ir::{Atom, CoreFn, FnIdx, Linearity, Program, VarId, VarInfo};

    pub fn var(n: u32) -> Atom {
        Atom::Var(VarId(n))
    }

    pub fn boxed(name: &str) -> VarInfo {
        var_info(name, true)
    }

    pub fn unboxed(name: &str) -> VarInfo {
        var_info(name, false)
    }

    fn var_info(name: &str, boxed: bool) -> VarInfo {
        VarInfo {
            name: name.to_string(),
            linearity: Linearity::Unr,
            boxed,
        }
    }

    /// エフェクトを持つプログラムは `Program { effects, ..program(...) }` で組む。
    pub fn program(functions: Vec<CoreFn>, entry: u32, strings: &[&str]) -> Program {
        Program {
            functions,
            entry: FnIdx(entry),
            strings: strings.iter().map(|s| s.to_string()).collect(),
            effects: Vec::new(),
        }
    }
}
```

- [ ] **Step 5: テストが通ることを確かめる**

Run: `cargo test -p eml_test_support --test support 2>&1 | grep -E '^test |test result'`
Expected: 既存の5件と新しい9件がすべて `ok`、`test result: ok. 14 passed`。

- [ ] **Step 6: 全体を確かめてコミットする**

```bash
INSTA_UPDATE=no cargo test 2>&1 | grep -E '^test result|FAILED|panicked' | grep -v ' 0 failed' ; echo "failures above (none expected)"
cargo clippy --all-targets 2>&1 | grep -E '^(warning|error)' ; echo "clippy above (none expected)"
cargo fmt --check && echo FMT_OK
find . -path ./target -prune -o \( -name '*.snap.new' -o -name '*.pending-snap' \) -print
git add crates/eml_test_support
git commit -F - <<'EOF'
Add shared test helpers to eml_test_support

with_diagnostics and short_text join a stage dump with its diagnostics, parse_clean and lower_clean assert a clean source, and ir builds hand-written Core IR. The crates still carry their own copies; the next commits switch them over.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```

Expected: failures と clippy の行は何も出ず、`FMT_OK`、find は何も出ない。

---

### Task 2: eml_syntax のテストを共通の部品に移す

**Files:**
- Modify: `crates/eml_syntax/tests/common/mod.rs` (`shape`)
- Modify: `crates/eml_syntax/tests/lexer.rs` (`dump`、`use`)
- Modify: `crates/eml_syntax/tests/parser.rs` (`dump`、`use`)
- Modify: `crates/eml_syntax/tests/ast.rs` (`source`)

**Interfaces:**
- Consumes: Task 1 の `with_diagnostics`、`short_text`、`parse_clean`
- Produces: なし

- [ ] **Step 1: common::shape を書き換える**

`crates/eml_syntax/tests/common/mod.rs` の `use eml_test_support::{parse, short};` を次にする。

```rust
use eml_test_support::{parse, short, short_text, with_diagnostics};
```

`shape` を次にする。ドキュメントコメントは変えない。

```rust
pub fn shape(text: &str) -> String {
    let parsed = parse(text);
    let mut out = String::new();
    write_node(&mut out, &parsed.parse.syntax(), 0);
    with_diagnostics(out, &short_text(&parsed.files, &parsed.diagnostics))
}
```

- [ ] **Step 2: lexer.rs の dump を書き換える**

`use eml_test_support::source;` を `use eml_test_support::{source, with_diagnostics};` にする。`dump` の末尾の

```rust
    if !diagnostics.is_empty() {
        out.push_str("---\n");
        out.push_str(&render(&diagnostics, &files));
    }
    out
```

を次にする。

```rust
    with_diagnostics(out, &render(&diagnostics, &files))
```

- [ ] **Step 3: parser.rs の dump を書き換える**

`use eml_test_support::parse;` を `use eml_test_support::{parse, with_diagnostics};` にし、`dump` を次にする。ドキュメントコメントは変えない。

```rust
fn dump(text: &str) -> String {
    let parsed = parse(text);
    with_diagnostics(
        debug_tree(&parsed.parse.syntax()),
        &render(&parsed.diagnostics, &parsed.files),
    )
}
```

- [ ] **Step 4: ast.rs の source を書き換える**

```rust
fn source(text: &str) -> SourceFile {
    eml_test_support::parse_clean(text).parse.tree()
}
```

- [ ] **Step 5: 種類3の確かめ**

```bash
git diff -U0 -- crates/eml_syntax | grep -E '^[-+].*(@"|assert)'
```

Expected: `ast.rs` の `-    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);` の1行だけが出る。

- [ ] **Step 6: 全体を確かめてコミットする**

```bash
INSTA_UPDATE=no cargo test 2>&1 | grep -E '^test result|FAILED|panicked' | grep -v ' 0 failed' ; echo "failures above (none expected)"
cargo clippy --all-targets 2>&1 | grep -E '^(warning|error)' ; echo "clippy above (none expected)"
cargo fmt --check && echo FMT_OK
find . -path ./target -prune -o \( -name '*.snap.new' -o -name '*.pending-snap' \) -print
git add crates/eml_syntax/tests
git commit -F - <<'EOF'
Build the syntax test dumps from the shared helpers

Expected values are unchanged.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```

Expected: Task 1 の Step 6 と同じ。

---

### Task 3: eml_hir のテストを共通の部品に移す

**Files:**
- Modify: `crates/eml_hir/tests/common/mod.rs`
- Modify: `crates/eml_hir/tests/effects.rs` (`errors` を除く、`use`、78〜79行と205〜206行)
- Modify: `crates/eml_hir/tests/structure.rs` (`module`)

**Interfaces:**
- Consumes: Task 1 の `with_diagnostics`、`short_text`、`lower_clean`、`Lowered`
- Produces: `common::diagnostics(text: &str) -> Vec<String>` (`eml_hir` のテストの中だけ)

- [ ] **Step 1: common を書き換える**

`crates/eml_hir/tests/common/mod.rs` の全体を次にする。

```rust
#![allow(dead_code)]

use eml_test_support::{Lowered, lower, short, short_text, with_diagnostics};

/// HIR の表示と、構文と HIR の診断を位置の順に並べたもの。
pub fn lower_text(text: &str) -> String {
    let lowered = lower_sorted(text);
    with_diagnostics(
        eml_hir::pretty(&lowered.module),
        &short_text(&lowered.files, &lowered.diagnostics),
    )
}

/// 構文と HIR の診断を、位置の順に並べたもの。
pub fn diagnostics(text: &str) -> Vec<String> {
    let lowered = lower_sorted(text);
    short(&lowered.files, &lowered.diagnostics)
}

/// 段階が返した順ではなく位置の順にして、期待値をソースと見比べやすくする。
fn lower_sorted(text: &str) -> Lowered {
    let mut lowered = lower(text);
    lowered.diagnostics.sort_by_key(|d| d.primary.range.start());
    lowered
}
```

- [ ] **Step 2: effects.rs を書き換える**

1. 先頭の `errors` 関数 (ドキュメントコメントを含む) を削除する
2. `use common::lower_text;` と `use eml_test_support::{lower, short};` を次にする

   ```rust
   use common::{diagnostics, lower_text};
   use eml_test_support::lower_clean;
   ```

3. 呼び出しの名前を変える

   ```bash
   sed -i -E 's/\berrors\(/diagnostics(/g' crates/eml_hir/tests/effects.rs
   ```

4. `operations_see_the_type_parameters_of_their_effect_first` の

   ```rust
       let lowered = lower("effect State s where\n  get : Unit -> s\n  never fail : Unit -> a");
       assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
   ```

   を次にする。

   ```rust
       let lowered = lower_clean("effect State s where\n  get : Unit -> s\n  never fail : Unit -> a");
   ```

5. 205〜206行の

   ```rust
       let lowered = lower(text);
       assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
   ```

   を `let lowered = lower_clean(text);` にする

```bash
grep -n '\berrors(\|\blower(' crates/eml_hir/tests/effects.rs; echo "exit=$?"
```

Expected: 何も出ず `exit=1`。

- [ ] **Step 3: structure.rs の module を書き換える**

ドキュメントコメントは変えない。

```rust
fn module(text: &str) -> Module {
    eml_test_support::lower_clean(text).module
}
```

- [ ] **Step 4: 種類3の確かめ**

```bash
git diff -U0 -- crates/eml_hir | grep -E '^[-+].*(@"|assert)'
```

Expected: 次の3行だけが出る (`effects.rs` の2行と `structure.rs` の1行)。

```
-    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
-    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
-    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
```

`errors(` を `diagnostics(` にした行は `assert` を含まないので出ない。`assert_eq!(errors(...), ...)` の形で書いた行があれば、その行の `-` と `+` が出る。そのときは、行の違いが `errors` → `diagnostics` だけであることを目で確かめる。

- [ ] **Step 5: 全体を確かめてコミットする**

```bash
INSTA_UPDATE=no cargo test 2>&1 | grep -E '^test result|FAILED|panicked' | grep -v ' 0 failed' ; echo "failures above (none expected)"
cargo clippy --all-targets 2>&1 | grep -E '^(warning|error)' ; echo "clippy above (none expected)"
cargo fmt --check && echo FMT_OK
find . -path ./target -prune -o \( -name '*.snap.new' -o -name '*.pending-snap' \) -print
git add crates/eml_hir/tests
git commit -F - <<'EOF'
Sort HIR test diagnostics in one place and use the shared helpers

Expected values are unchanged.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```

Expected: Task 1 の Step 6 と同じ。

---

### Task 4: eml_types のテストを共通の部品に移す

**Files:**
- Modify: `crates/eml_types/tests/common/mod.rs`

**Interfaces:**
- Consumes: Task 1 の `with_diagnostics`
- Produces: なし

- [ ] **Step 1: check_text を書き換える**

`crates/eml_types/tests/common/mod.rs` の全体を次にする。

```rust
use eml_test_support::{check, full, with_diagnostics};

/// 推論結果と、構文・HIR・型の診断。診断はラベル、note、help まで表示する。
pub fn check_text(text: &str) -> String {
    let checked = check(text);
    with_diagnostics(
        eml_types::dump(&checked.module, &checked.typed),
        &full(&checked.files, &checked.diagnostics),
    )
}
```

- [ ] **Step 2: 種類3の確かめ**

```bash
git diff -U0 -- crates/eml_types | grep -E '^[-+].*(@"|assert)'; echo "exit=$?"
```

Expected: 何も出ず `exit=1`。

- [ ] **Step 3: 全体を確かめてコミットする**

```bash
INSTA_UPDATE=no cargo test 2>&1 | grep -E '^test result|FAILED|panicked' | grep -v ' 0 failed' ; echo "failures above (none expected)"
cargo clippy --all-targets 2>&1 | grep -E '^(warning|error)' ; echo "clippy above (none expected)"
cargo fmt --check && echo FMT_OK
find . -path ./target -prune -o \( -name '*.snap.new' -o -name '*.pending-snap' \) -print
git add crates/eml_types/tests
git commit -F - <<'EOF'
Build the type test dump from the shared helper

Expected values are unchanged.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```

Expected: Task 1 の Step 6 と同じ。

---

### Task 5: 手書きの Core IR を ir モジュールで組む

**Files:**
- Modify: `crates/eml_core_ir/tests/verify.rs` (`string`、`int`、`var`、`check`、`check_with_effects`、`use`)
- Modify: `crates/eml_interp/tests/closures.rs` (`function` の `vars`、`run_program`、`var`、`use`)
- Modify: `crates/eml_interp/tests/run.rs` (`leaking_program`、`use`)

**Interfaces:**
- Consumes: Task 1 の `ir::{var, boxed, unboxed, program}`
- Produces: なし

- [ ] **Step 1: verify.rs を書き換える**

1. `string`、`int`、`var` の3つの関数を削除する
2. 呼び出しの名前を変える。`\b` で区切るので、`to_string(` や `print_int(` は変わらない

   ```bash
   sed -i -E 's/\bstring\(/boxed(/g; s/\bint\(/unboxed(/g' crates/eml_core_ir/tests/verify.rs
   ```

3. `check` と `check_with_effects` を次にする

   ```rust
   fn check(functions: Vec<CoreFn>) -> Result<(), String> {
       verify(&program(functions, 0, &["s"])).map_err(|error| error.to_string())
   }
   ```

   ```rust
   fn check_with_effects(functions: Vec<CoreFn>, effects: Vec<EffectInfo>) -> Result<(), String> {
       let program = Program {
           effects,
           ..program(functions, 0, &[])
       };
       verify(&program).map_err(|error| error.to_string())
   }
   ```

4. `use` を次にする。`Linearity` は使わなくなるので除く

   ```rust
   use eml_core_ir::{
       Atom, CExpr, CExprId, Call, CoreFn, EffectInfo, FnIdx, JoinId, OperationInfo, PrimOp,
       Program, Rhs, VarId, VarInfo, verify,
   };
   use eml_test_support::ir::{boxed, program, unboxed, var};
   ```

コンパイラが未使用の import を報告したら、その名前だけを `use` から除く。

- [ ] **Step 2: closures.rs を書き換える**

1. `function` の `vars` の組み立てを次にする。引数の名前 `boxed` は `ir::boxed` と重なるので `is_boxed` にする

   ```rust
           vars: vars
               .iter()
               .map(|&(name, is_boxed)| if is_boxed { boxed(name) } else { unboxed(name) })
               .collect(),
   ```

2. `run_program` を次にする

   ```rust
   fn run_program(functions: Vec<CoreFn>, main: u32, strings: &[&str]) -> String {
       let (stdout, result) = eml_test_support::execute(program(functions, main, strings), true);
       result.unwrap();
       stdout
   }
   ```

3. `var` 関数を削除する
4. `use` を次にする

   ```rust
   use eml_core_ir::{Atom, CExpr, CExprId, Call, CoreFn, FnIdx, IoOp, PrimOp, Rhs, VarId};
   use eml_test_support::ir::{boxed, program, unboxed, var};
   ```

`first(boxed: bool)` の引数 `boxed` は関数 `boxed` を隠すが、`first` の中では関数 `boxed` を呼ばないのでそのままにする。

- [ ] **Step 3: run.rs を書き換える**

`leaking_program` を次にする。ドキュメントコメントは変えない。

```rust
fn leaking_program() -> Program {
    let main = CoreFn {
        name: "main".to_string(),
        params: vec![],
        vars: vec![unboxed("p"), boxed("s")],
        body: CExprId(1),
        exprs: vec![
            CExpr::Return(Atom::Unit),
            CExpr::Let {
                var: VarId(1),
                rhs: Rhs::ConstString(0),
                body: CExprId(0),
            },
        ],
        joins: Vec::new(),
    };
    program(vec![main], 0, &["leaked"])
}
```

`use` を次にする。

```rust
use eml_core_ir::{Atom, CExpr, CExprId, CoreFn, Program, Rhs, VarId};
use eml_interp::RuntimeError;
use eml_test_support::ir::{boxed, program, unboxed};
use eml_test_support::{execute, run};
```

- [ ] **Step 4: 種類3の確かめ**

```bash
git diff -U0 -- crates/eml_core_ir crates/eml_interp | grep -E '^[-+].*(@"|assert)'
```

Expected: `verify.rs` の `assert_eq!` の行で、`string(` / `int(` を `boxed(` / `unboxed(` にしただけの `-` と `+` の組が出ることがある。それ以外は出ない。出た組は、違いが名前だけであることを目で確かめる。

- [ ] **Step 5: 全体を確かめてコミットする**

```bash
INSTA_UPDATE=no cargo test 2>&1 | grep -E '^test result|FAILED|panicked' | grep -v ' 0 failed' ; echo "failures above (none expected)"
cargo clippy --all-targets 2>&1 | grep -E '^(warning|error)' ; echo "clippy above (none expected)"
cargo fmt --check && echo FMT_OK
find . -path ./target -prune -o \( -name '*.snap.new' -o -name '*.pending-snap' \) -print
git add crates/eml_core_ir/tests crates/eml_interp/tests
git commit -F - <<'EOF'
Build hand-written Core IR in tests from the shared ir module

Expected values are unchanged.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```

Expected: Task 1 の Step 6 と同じ。

---

### Task 6: 文書を直す

**Files:**
- Modify: `docs/implementation/testing.md` (置き場所の表の3行目、「crate の中の置き方」、地図の `eml_types` と `eml_runtime` の行)
- Modify: `docs/implementation/test-changes.md` (「書き方」)
- Modify: `docs/implementation/status.md` (「テストの整理の残り」)
- Modify: `CLAUDE.md` (Testing の節の `eml_test_support` の行)

**Interfaces:**
- Consumes: Task 1 の関数の名前
- Produces: なし

- [ ] **Step 1: testing.md を直す**

1. 置き場所の表の3行目を次にする

   ```markdown
   | 3 | フロントエンドからは作れない状態 (壊れた IR、`decref` の抜けた IR など) と、テストの中で生成した大きなソース | `eml_core_ir/tests/verify.rs` か `eml_interp/tests/`。Core IR は `eml_test_support::ir` で手書きする |
   ```

2. 「crate の中の置き方」の2つ目の箇条を次にする

   ```markdown
   - crate の結合テストが使う表示の関数は `tests/common/mod.rs` に置く。複数の crate で使う部品は `eml_test_support` に置く
   ```

3. 「crate の中の置き方」の `crates/eml_test_support/` の箇条の1文目を次にする。2文目以降は変えない

   ```markdown
   - `crates/eml_test_support/` は、結合テストのためにパイプラインを組む関数 (`parse`、`lower`、`check`、`core`、`run`、`execute`) と、診断のないことを確かめて組む関数 (`parse_clean`、`lower_clean`)、診断を文字列にする関数 (`short`、`short_text`、`full`)、段階の表示に診断を足す関数 (`with_diagnostics`)、手書きの Core IR の部品 (`ir`) を持つ。
   ```

4. 地図の `eml_types` の行の `effects.rs (エフェクト、handler、継続)` を `effects.rs (エフェクト、handler、継続、線形な継続 (E3001))` にする
5. 地図の `eml_runtime` の行の `heap.rs (確保と解放、世代番号、リーク、フレームと継続の解放)` を `heap.rs (確保と解放、世代番号、リーク、フレームと継続の解放、`take` と `take_or_copy` による複製)` にする

- [ ] **Step 2: test-changes.md の「書き方」に1項目足す**

```markdown
- リファクタリング R1 までの項目は種類の番号を持たないが、履歴なので書き足さない
```

- [ ] **Step 3: status.md の「テストの整理の残り」を直す**

項目の全体を次にする。

```markdown
- テストの整理の残り: テスト本体の整理 (重複したテストと古くなったテストの削除、大きいファイルの分割、UI テストの分類と、それに伴う `ui.rs` の走査のパターンの `**/*.em` への変更と CLAUDE.md の Testing の節の UI テストの説明の更新) が残っている。既存のテストは、[testing.md](testing.md) の「テストの置き場所」の「重複させない」と、「UI テスト」の「分類」に、まだ合っていない
```

- [ ] **Step 4: CLAUDE.md を直す**

Testing の節の `eml_test_support` の行の1文目を次にする。2文目以降は変えない。

```markdown
- `eml_test_support` (dev-only) builds the pipeline for integration tests (`parse` / `lower` / `check` / `core` / `run` / `execute`, and `parse_clean` / `lower_clean` that assert a clean source), formats diagnostics (`short` / `short_text` / `full`, joined to a stage dump with `with_diagnostics`), and builds hand-written Core IR (`ir`).
```

- [ ] **Step 5: 確かめてコミットする**

```bash
python3 - <<'EOF'
import pathlib, re
docs = ["CLAUDE.md", "docs/README.md", "docs/implementation/testing.md",
        "docs/implementation/test-changes.md", "docs/implementation/status.md"]
bad = []
for d in docs:
    p = pathlib.Path(d)
    for target in re.findall(r"\]\(([^)#\s]+)(?:#[^)]*)?\)", p.read_text()):
        if not target.startswith("http") and not (p.parent / target).exists():
            bad.append((d, target))
print("BROKEN:", bad) if bad else print("LINKS OK")
EOF
grep -c 'テスト補助コードの統一' docs/implementation/status.md
python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py docs/implementation/testing.md | grep -E '^L[0-9]+ \[' | grep -v '半角空白\|箇条書きの比率'
git add docs/implementation/testing.md docs/implementation/test-changes.md docs/implementation/status.md CLAUDE.md
git commit -F - <<'EOF'
Document the shared test helpers and fold in the deferred review notes

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```

Expected: `LINKS OK`、`0`。リンターは、すでにある「出す crate ではなく」の INFO 1件のほかは出さない。

---

### Task 7: 作業用の文書を消す

**Files:**
- Delete: `docs/superpowers/specs/2026-10-05-test-helpers-design.md`
- Delete: `docs/superpowers/plans/2026-10-05-test-helpers.md`

**Interfaces:**
- Consumes: Task 1〜6 がすべて終わり、最後のレビューの修正が済んでいること
- Produces: なし

spec の「位置づけ」のとおり、決めた内容がコードと testing.md に入ったら作業用の文書を消す。

- [ ] **Step 1: 消してコミットする**

```bash
git rm docs/superpowers/specs/2026-10-05-test-helpers-design.md docs/superpowers/plans/2026-10-05-test-helpers.md
git commit -F - <<'EOF'
Remove the working documents for the test helpers work

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```
