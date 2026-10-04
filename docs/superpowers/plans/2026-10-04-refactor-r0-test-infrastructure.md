# リファクタリング R0: テスト基盤と運用 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 結合テストのパイプラインと診断の文字列を開発専用の crate `eml_test_support` にまとめ、テストの変更の運用を文書にする。

**Architecture:** 行と列の計算を `eml_diagnostics` に置き、出力の取り込みと実行の設定に短い API を足す。その上に `eml_test_support` を作り、各 crate の `tests/` のヘルパをその関数に差し替える。`eml_interp` のテストのうち、ソースから実行するものは UI テストに寄せる。

**Tech Stack:** Rust (edition 2024、Cargo workspace)、insta、la-arena

**Spec:** `docs/superpowers/specs/2026-10-04-refactor-r0-test-infrastructure-design.md`

## Global Constraints

- 期待値は、このプランで名前を挙げたテストだけを変える。名前を挙げたのは、Task 6 で削除する `eml_interp/tests/run.rs` の6件と、UI テストに移す2件だけである。期待値を変えない機械的な追随は許す
- 既存のスナップショット (インラインの `@"..."` と `snapshots/` のファイル) は1文字も変えない。増えるスナップショットは `run/strings_freed_in_branches.em` と `run/short_circuit.em` の2件だけである
- `eml_test_support` は `publish = false` にし、各 crate の `tests/` の結合テストからだけ使う。`src/` の `#[cfg(test)]` からは使わない
- テストを変えないことを理由に設計を曲げない。計画どおりに進まないときは、変更が spec の3種類のどれに当たるかを示して相談する
- コードのコメントと `docs/` の文書は日本語で書き、`yomiyasu:yomiyasu` スキルの規則に従う。コメントは「何を」ではなく「なぜ」を書き、規則を指すときは `docs/` のパスを引く。`CLAUDE.md` は英語で書く
- 各タスクの終わりに `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` を通す
- コミットメッセージの末尾に次の2行を付ける

```
Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014mCDZTcwb5EYZfQ1MpvtHn
```

## Review Focus

- ヘルパを差し替えたときに、診断の並び順や行と列の数え方が変わり、スナップショットの期待値を書き換えて通してしまうこと。どのタスクでも、`git diff` でスナップショットの文字列と `assert` の期待値の行が変わっていないことを確かめる (Task 4、5、6 の確認の手順)
- ファイルの先頭の BOM の後の診断の列。spec に従って BOM を数えない (Task 1 のテスト)
- 複数バイトの文字を含む行の列。バイトではなく文字で数える (Task 1 のテスト)
- 移した UI テストの stdout が、元の `assert_eq!` の期待値とずれること (Task 6 で、スナップショットのファイルを期待値から手で書き、テストで突き合わせる)
- dev-dependency の循環 (`eml_syntax` → `eml_test_support` → `eml_syntax`) で、crate ごとのテストが組み立てられないこと (各タスクで `cargo test -p <crate>` を個別に流す)

---

### Task 1: `SourceFiles::line_col`

**Files:**
- Modify: `crates/eml_diagnostics/src/source.rs`
- Modify: `crates/eml_diagnostics/src/lib.rs:6`

**Interfaces:**
- Consumes: なし
- Produces: `eml_diagnostics::LineCol { pub line: u32, pub column: u32 }` (`Debug`、`Clone`、`Copy`、`PartialEq`、`Eq`。`Display` は `"{line}:{column}"`)、`SourceFiles::line_col(&self, file: FileId, offset: TextSize) -> LineCol`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_diagnostics/src/source.rs` の `mod tests` の末尾 (`add_returns_distinct_ids` の後) に足す。

```rust
    fn position(text: &str, offset: u32) -> String {
        let mut files = SourceFiles::new();
        let file = files.add("a.em", text);
        files.line_col(file, offset.into()).to_string()
    }

    #[test]
    fn line_col_is_one_based() {
        assert_eq!(position("abc", 0), "1:1");
        assert_eq!(position("abc", 2), "1:3");
        assert_eq!(position("abc", 3), "1:4");
        assert_eq!(position("ab\ncd", 4), "2:2");
        assert_eq!(position("ab\n\ncd", 4), "3:1");
    }

    #[test]
    fn line_col_counts_characters_not_bytes() {
        // `α` と `β` は2バイトずつなので、`x` はバイト位置 5 にある。
        assert_eq!(position("αβ x", 5), "1:4");
    }

    #[test]
    fn line_col_does_not_count_the_bom() {
        // BOM は列に数えない (docs/spec/lexical.md)。BOM は3バイトなので、`b` はバイト位置 4 にある。
        assert_eq!(position("\u{feff}ab", 4), "1:2");
        assert_eq!(position("\u{feff}a\nb", 5), "2:1");
    }
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_diagnostics line_col`
Expected: コンパイルエラー (`no method named line_col`)

- [ ] **Step 3: 実装する**

`crates/eml_diagnostics/src/source.rs` の先頭に `use std::fmt;` と `use text_size::TextSize;` を足す。`FileId` の定義の後に次を置く。

```rust
/// 1 始まりの行と列。列は文字数で数える。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCol {
    pub line: u32,
    pub column: u32,
}

impl fmt::Display for LineCol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}
```

`impl SourceFiles` の `text` の後に次を置く。

```rust
    /// 位置を行と列にする。ファイルの先頭の BOM は列に数えない (docs/spec/lexical.md)。
    pub fn line_col(&self, file: FileId, offset: TextSize) -> LineCol {
        let before = &self.text(file)[..usize::from(offset)];
        let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
        let mut column_text = &before[line_start..];
        if line_start == 0 {
            column_text = column_text.strip_prefix('\u{feff}').unwrap_or(column_text);
        }
        LineCol {
            line: u32::try_from(before.matches('\n').count() + 1).expect("too many lines"),
            column: u32::try_from(column_text.chars().count() + 1).expect("line too long"),
        }
    }
```

`crates/eml_diagnostics/src/lib.rs` の `pub use source::{FileId, SourceFiles};` を `pub use source::{FileId, LineCol, SourceFiles};` にする。

- [ ] **Step 4: テストが通ることを確かめる**

Run: `cargo test -p eml_diagnostics`
Expected: PASS (既存の4件と新しい3件)

- [ ] **Step 5: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates/eml_diagnostics
git commit -m "Compute line and column in SourceFiles without counting the BOM"
```

(コミットメッセージには Global Constraints の2行を付ける。以下のタスクも同じ。)

---

### Task 2: `Captured` と `RunConfig::with_debug_heap`

**Files:**
- Modify: `crates/eml_runtime/src/output.rs`
- Modify: `crates/eml_runtime/src/lib.rs:7`
- Modify: `crates/eml_interp/src/lib.rs` (`RunConfig` と、ファイル末尾の単体テスト)
- Modify: `crates/eml_cli/src/lib.rs:8-9` (再公開)
- Modify: `crates/eml_cli/src/main.rs` (`Command::Run` の分岐)
- Modify: `crates/eml_cli/tests/ui.rs`
- Modify: `crates/eml_interp/tests/run.rs:21-28`
- Modify: `crates/eml_interp/tests/closures.rs:3-9`、`:49-60`

**Interfaces:**
- Consumes: なし
- Produces: `eml_runtime::Captured` (`Clone`。`pub fn contents(&self) -> String`)、`OutputSink::capture() -> (OutputSink, Captured)`、`RunConfig::with_debug_heap(self, debug_heap: bool) -> RunConfig`、`eml_cli::Captured` (再公開)

このタスクで書き換える既存のテストは、どれも種類3 (期待値を変えない機械的な追随) である。

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_runtime/src/output.rs` の単体テスト `capture_collects_writes_from_clones` を次に置き換える。期待値の文字列は変えない。

```rust
    #[test]
    fn capture_collects_writes_from_clones() {
        let (sink, captured) = OutputSink::capture();
        let clone = sink.clone();
        sink.write_str("hello ").unwrap();
        clone.write_str("world\n").unwrap();
        assert_eq!(captured.contents(), "hello world\n");
    }
```

`crates/eml_interp/src/lib.rs` の末尾に単体テストを足す。

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_debug_heap_sets_the_flag() {
        assert!(RunConfig::default().with_debug_heap(true).debug_heap);
        assert!(!RunConfig::default().with_debug_heap(false).debug_heap);
    }
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_runtime -p eml_interp --lib`
Expected: コンパイルエラー (`no method named contents`、`no method named with_debug_heap`)

- [ ] **Step 3: 実装する**

`crates/eml_runtime/src/output.rs` の `capture` を次に置き換え、`impl OutputSink` の後に `Captured` を置く。

```rust
    /// テストでプログラムの出力を捕まえるためのもの。
    pub fn capture() -> (Self, Captured) {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        (OutputSink(buffer.clone()), Captured(buffer))
    }
```

```rust
/// `OutputSink::capture` が捕まえた出力。
#[derive(Clone)]
pub struct Captured(Arc<Mutex<Vec<u8>>>);

impl Captured {
    /// プログラムは文字列だけを書くので、出力は UTF-8 である。
    pub fn contents(&self) -> String {
        let buffer = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        String::from_utf8(buffer.clone()).expect("the program writes UTF-8")
    }
}
```

`crates/eml_runtime/src/lib.rs` の `pub use output::OutputSink;` を `pub use output::{Captured, OutputSink};` にする。

`crates/eml_interp/src/lib.rs` の `pub struct RunConfig { ... }` の後に次を置く。

```rust
impl RunConfig {
    pub fn with_debug_heap(mut self, debug_heap: bool) -> Self {
        self.debug_heap = debug_heap;
        self
    }
}
```

`crates/eml_cli/src/lib.rs` の `pub use eml_runtime::OutputSink;` を `pub use eml_runtime::{Captured, OutputSink};` にする。

`crates/eml_cli/src/main.rs` の `Command::Run` の分岐で、次の2行を

```rust
            let mut config = RunConfig::default();
            config.debug_heap = debug_heap;
```

次の1行にする。

```rust
            let config = RunConfig::default().with_debug_heap(debug_heap);
```

- [ ] **Step 4: 呼び出し側を追随させる**

`crates/eml_interp/tests/run.rs` の `execute` を次に置き換える。

```rust
fn execute(program: Program, debug_heap: bool) -> (String, Result<(), RuntimeError>) {
    let (sink, captured) = OutputSink::capture();
    let config = RunConfig::default().with_debug_heap(debug_heap);
    let result = eml_interp::run(Arc::new(program), &config, &sink);
    (captured.contents(), result)
}
```

`crates/eml_interp/tests/closures.rs` の `run_program` の後半 (`let mut config` から関数の終わりまで) を次に置き換える。

```rust
    let config = RunConfig::default().with_debug_heap(true);
    let (sink, captured) = OutputSink::capture();
    run(Arc::new(program), &config, &sink).unwrap();
    captured.contents()
}
```

`crates/eml_cli/tests/ui.rs` の `run` と `run_fail` を、コンパイルと実行を1つにまとめた関数で書き直す。`load` と `check_fail` と冒頭の doc コメントはそのまま残す。`insta::assert_snapshot!` に渡す式は、スナップショットのファイルの `expression:` と一致させるため、今と1文字も変えない。

```rust
use eml_cli::{OutputSink, RunConfig, RunResult};
use eml_diagnostics::{SourceFiles, has_errors, render};
```

```rust
/// 診断のエラーなしでコンパイルし、`debug_heap` を有効にして実行する。stdout、診断の表示、実行の結果を返す。
fn compile_and_execute(path: &Path) -> (String, String, RunResult) {
    let (files, id) = load(path);
    let compiled = eml_cli::compile(&files, id);
    let stderr = render(&compiled.diagnostics, &files);
    let program = compiled
        .program
        .unwrap_or_else(|| panic!("unexpected errors:\n{stderr}"));
    let (sink, captured) = OutputSink::capture();
    let config = RunConfig::default().with_debug_heap(true);
    let result = eml_cli::execute(program, &config, sink);
    (captured.contents(), stderr, result)
}

#[test]
fn run() {
    insta::glob!("../../../tests/ui", "run/*.em", |path| {
        let (stdout, stderr, result) = compile_and_execute(path);
        assert_eq!(result, RunResult::Completed, "{stderr}");
        insta::assert_snapshot!(format!("--- stdout ---\n{stdout}--- stderr ---\n{stderr}"));
    });
}

#[test]
fn run_fail() {
    insta::glob!("../../../tests/ui", "run-fail/*.em", |path| {
        let (stdout, _, result) = compile_and_execute(path);
        let RunResult::RuntimeError(message) = result else {
            panic!("expected a runtime error");
        };
        insta::assert_snapshot!(format!(
            "--- stdout ---\n{stdout}--- runtime error ---\n{message}\n"
        ));
    });
}
```

- [ ] **Step 5: テストが通り、期待値が変わっていないことを確かめる**

Run: `cargo test -p eml_runtime -p eml_interp -p eml_cli`
Expected: PASS

Run: `git diff --stat -- '*.snap'`
Expected: 何も出ない

- [ ] **Step 6: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates/eml_runtime crates/eml_interp crates/eml_cli
git commit -m "Return captured output as a string and build RunConfig with with_debug_heap"
```

---

### Task 3: `eml_test_support` crate

**Files:**
- Create: `crates/eml_test_support/Cargo.toml`
- Create: `crates/eml_test_support/src/lib.rs`
- Create: `crates/eml_test_support/tests/support.rs`
- Modify: `Cargo.toml` (ルートの `[workspace.dependencies]`)

**Interfaces:**
- Consumes: `SourceFiles::line_col`、`LineCol` (Task 1)、`OutputSink::capture() -> (OutputSink, Captured)`、`Captured::contents`、`RunConfig::with_debug_heap` (Task 2)
- Produces (すべて `eml_test_support` の公開項目):
  - `pub fn source(text: &str) -> (SourceFiles, FileId)`
  - `pub struct Parsed { pub files: SourceFiles, pub file: FileId, pub parse: eml_syntax::Parse, pub diagnostics: Vec<Diagnostic> }`、`pub fn parse(text: &str) -> Parsed`
  - `pub struct Lowered { pub files: SourceFiles, pub file: FileId, pub module: eml_hir::Module, pub diagnostics: Vec<Diagnostic> }`、`pub fn lower(text: &str) -> Lowered`
  - `pub struct Checked { pub files: SourceFiles, pub file: FileId, pub module: eml_hir::Module, pub typed: eml_types::TypedModule, pub diagnostics: Vec<Diagnostic> }`、`pub fn check(text: &str) -> Checked`
  - `pub fn core(text: &str) -> eml_core_ir::Program`
  - `pub fn run(text: &str) -> (String, Result<(), eml_interp::RuntimeError>)`
  - `pub fn execute(program: eml_core_ir::Program, debug_heap: bool) -> (String, Result<(), eml_interp::RuntimeError>)`
  - `pub fn short(files: &SourceFiles, diagnostics: &[Diagnostic]) -> Vec<String>`
  - `pub fn full(files: &SourceFiles, diagnostics: &[Diagnostic]) -> String`

- [ ] **Step 1: crate の骨組みを作る**

`crates/eml_test_support/Cargo.toml`:

```toml
[package]
name = "eml_test_support"
version.workspace = true
edition.workspace = true
publish = false

[dependencies]
eml_core_ir.workspace = true
eml_diagnostics.workspace = true
eml_hir.workspace = true
eml_interp.workspace = true
eml_runtime.workspace = true
eml_syntax.workspace = true
eml_types.workspace = true
```

ルートの `Cargo.toml` の `[workspace.dependencies]` で、`eml_interp = { path = "crates/eml_interp" }` の次の行に足す。

```toml
eml_test_support = { path = "crates/eml_test_support" }
```

`crates/eml_test_support/src/lib.rs` は、Step 3 で中身を書くまで空のファイルにする。

- [ ] **Step 2: 失敗するテストを書く**

`crates/eml_test_support/tests/support.rs`:

```rust
use eml_diagnostics::{Diagnostic, ErrorCode, Label, TextRange};
use eml_test_support::{check, core, full, lower, parse, run, short, source};

fn range(start: u32, end: u32) -> TextRange {
    TextRange::new(start.into(), end.into())
}

#[test]
fn short_puts_code_position_and_message_on_one_line() {
    let (files, file) = source("a\nbcd");
    let diagnostic = Diagnostic::error(ErrorCode(1), "bad", Label::new(file, range(3, 4), "here"));
    assert_eq!(short(&files, &[diagnostic]), ["E0001 2:2 bad"]);
}

#[test]
fn full_adds_labels_notes_and_help() {
    let (files, file) = source("ab\ncd");
    let diagnostic = Diagnostic::error(ErrorCode(2001), "bad", Label::new(file, range(1, 2), "here"))
        .with_secondary(Label::new(file, range(3, 4), "there"))
        .with_note("a note")
        .with_help("a help");
    assert_eq!(
        full(&files, &[diagnostic]),
        "E2001 1:2 bad\n  1:2 here\n  2:1 there\n  note: a note\n  help: a help\n"
    );
}

#[test]
fn stages_collect_the_diagnostics_of_earlier_stages() {
    // `€` は字句の E0001、`g` は HIR の E1001 (未定義の名前) になる。
    let text = "€\nf : Int -> Int\nf x = g x";
    let codes = |diagnostics: &[Diagnostic]| -> Vec<String> {
        diagnostics.iter().map(|d| d.code.to_string()).collect()
    };
    assert_eq!(codes(&parse(text).diagnostics), ["E0001"]);
    assert_eq!(codes(&lower(text).diagnostics), ["E0001", "E1001"]);
    assert_eq!(codes(&check(text).diagnostics), ["E0001", "E1001"]);
}

#[test]
fn run_executes_with_the_heap_checks() {
    let (stdout, result) = run("main : Unit -> <IO> Unit\nmain () = println \"hi\"");
    assert_eq!(stdout, "hi\n");
    assert_eq!(result, Ok(()));
}

#[test]
#[should_panic]
fn core_rejects_programs_with_errors() {
    core("f : Int -> Int\nf x = g x");
}
```

- [ ] **Step 3: 実装する**

`crates/eml_test_support/src/lib.rs`:

```rust
//! テストのためにパイプラインを組む処理と、診断を文字列にする処理 (docs/implementation/testing.md)。
//!
//! この crate は、テストする crate の型をそのまま使う。そのため、使ってよいのは各 crate の `tests/` にある結合テスト
//! からだけである。`src/` の `#[cfg(test)]` から使うと、テストする crate が2つ別々にリンクされて型が合わなくなる。

use std::fmt::Write;
use std::sync::Arc;

use eml_core_ir::Program;
use eml_diagnostics::{Diagnostic, FileId, Label, LineCol, SourceFiles, has_errors};
use eml_interp::{RunConfig, RuntimeError};
use eml_runtime::OutputSink;

pub struct Parsed {
    pub files: SourceFiles,
    pub file: FileId,
    pub parse: eml_syntax::Parse,
    pub diagnostics: Vec<Diagnostic>,
}

pub struct Lowered {
    pub files: SourceFiles,
    pub file: FileId,
    pub module: eml_hir::Module,
    /// 構文と HIR の診断を、各段階が返した順に並べたもの。
    pub diagnostics: Vec<Diagnostic>,
}

pub struct Checked {
    pub files: SourceFiles,
    pub file: FileId,
    pub module: eml_hir::Module,
    pub typed: eml_types::TypedModule,
    /// 構文、HIR、型の診断を、各段階が返した順に並べたもの。
    pub diagnostics: Vec<Diagnostic>,
}

pub fn source(text: &str) -> (SourceFiles, FileId) {
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    (files, file)
}

/// どのテストでも lossless を確かめるため、木が元のテキストに戻ることもここで確認する。
pub fn parse(text: &str) -> Parsed {
    let (files, file) = source(text);
    let (parse, diagnostics) = eml_syntax::parse(file, text);
    assert_eq!(
        parse.syntax().text().to_string(),
        text,
        "tree must be lossless"
    );
    Parsed {
        files,
        file,
        parse,
        diagnostics,
    }
}

pub fn lower(text: &str) -> Lowered {
    let Parsed {
        files,
        file,
        parse,
        mut diagnostics,
    } = parse(text);
    let (module, stage) = eml_hir::lower(file, &parse.tree());
    diagnostics.extend(stage);
    Lowered {
        files,
        file,
        module,
        diagnostics,
    }
}

pub fn check(text: &str) -> Checked {
    let Lowered {
        files,
        file,
        module,
        mut diagnostics,
    } = lower(text);
    let (typed, stage) = eml_types::check(&module);
    diagnostics.extend(stage);
    Checked {
        files,
        file,
        module,
        typed,
        diagnostics,
    }
}

/// Core IR は診断のエラーがないプログラムだけを受け取る (docs/implementation/architecture.md)。
pub fn core(text: &str) -> Program {
    let checked = check(text);
    assert!(
        !has_errors(&checked.diagnostics),
        "{:#?}",
        checked.diagnostics
    );
    eml_core_ir::lower(&checked.module, &checked.typed)
}

/// 実行のテストでは、つねに `debug_heap` を有効にする (docs/implementation/testing.md)。
pub fn run(text: &str) -> (String, Result<(), RuntimeError>) {
    execute(core(text), true)
}

pub fn execute(program: Program, debug_heap: bool) -> (String, Result<(), RuntimeError>) {
    let (sink, captured) = OutputSink::capture();
    let config = RunConfig::default().with_debug_heap(debug_heap);
    let result = eml_interp::run(Arc::new(program), &config, &sink);
    (captured.contents(), result)
}

/// 1件を `E0001 1:2 message` の1行にする。期待値を読みやすくするため、位置はバイトではなく行と列にする。
pub fn short(files: &SourceFiles, diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .map(|d| format!("{} {} {}", d.code, position(files, &d.primary), d.message))
        .collect()
}

/// 1件を、先頭の行に続けてラベル、note、help を字下げした行にする。
pub fn full(files: &SourceFiles, diagnostics: &[Diagnostic]) -> String {
    let mut out = String::new();
    for d in diagnostics {
        let at = position(files, &d.primary);
        writeln!(out, "{} {at} {}", d.code, d.message).unwrap();
        writeln!(out, "  {at} {}", d.primary.message).unwrap();
        for label in &d.secondary {
            writeln!(out, "  {} {}", position(files, label), label.message).unwrap();
        }
        for note in &d.notes {
            writeln!(out, "  note: {note}").unwrap();
        }
        for help in &d.help {
            writeln!(out, "  help: {help}").unwrap();
        }
    }
    out
}

fn position(files: &SourceFiles, label: &Label) -> LineCol {
    files.line_col(label.file, label.range.start())
}
```

- [ ] **Step 4: テストが通ることを確かめる**

Run: `cargo test -p eml_test_support`
Expected: PASS (5件)

- [ ] **Step 5: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add Cargo.toml Cargo.lock crates/eml_test_support
git commit -m "Add eml_test_support for building the pipeline and formatting diagnostics in tests"
```

---

### Task 4: `eml_syntax` のテストを `eml_test_support` に寄せる

**Files:**
- Modify: `crates/eml_syntax/Cargo.toml`
- Modify: `crates/eml_syntax/tests/common/mod.rs`
- Modify: `crates/eml_syntax/tests/parser.rs`
- Modify: `crates/eml_syntax/tests/ast.rs:1-14`
- Modify: `crates/eml_syntax/tests/corpus.rs:1-53`
- Modify: `crates/eml_syntax/tests/lexer.rs:1-46`

**Interfaces:**
- Consumes: `eml_test_support::{source, parse, short}` (Task 3)
- Produces: `common::item_kinds(text: &str) -> Vec<String>` (トップレベルの子のノードの種類を `{:?}` で並べたもの)。`common` の既存の `shape`、`diagnostics`、`helps`、`lines` は名前と出力を変えない

このタスクの変更はすべて種類3である。期待値は変えない。

- [ ] **Step 1: dev-dependency を足す**

`crates/eml_syntax/Cargo.toml` の `[dev-dependencies]` に `eml_test_support.workspace = true` を足す。

- [ ] **Step 2: `common` を書き換える**

`crates/eml_syntax/tests/common/mod.rs` を次にする。`write_node` は今のまま残す。

```rust
#![allow(dead_code)]

use eml_syntax::{SyntaxElement, SyntaxNode};
use eml_test_support::{parse, short};

/// 木の形 (trivia を除く) と、構文の診断。lossless の確認は `eml_test_support::parse` が行う。
pub fn shape(text: &str) -> String {
    let parsed = parse(text);
    let mut out = String::new();
    write_node(&mut out, &parsed.parse.syntax(), 0);
    let diagnostics = short(&parsed.files, &parsed.diagnostics);
    if !diagnostics.is_empty() {
        out.push_str("---\n");
        for line in diagnostics {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

/// 期待値を読みやすくするため、位置はバイトではなく 1 始まりの行と列 (文字数) で表示する。
pub fn diagnostics(text: &str) -> Vec<String> {
    let parsed = parse(text);
    short(&parsed.files, &parsed.diagnostics)
}

/// help の文言を確かめるテストのため、すべての診断の help を順に返す。
pub fn helps(text: &str) -> Vec<String> {
    parse(text)
        .diagnostics
        .into_iter()
        .flat_map(|diagnostic| diagnostic.help)
        .collect()
}

/// トップレベルの子のノードの種類。回復が次の項目から再開することを確かめるため。
pub fn item_kinds(text: &str) -> Vec<String> {
    parse(text)
        .parse
        .syntax()
        .children()
        .map(|node| format!("{:?}", node.kind()))
        .collect()
}

pub fn lines(lines: &[&str]) -> String {
    lines.join("\n")
}
```

- [ ] **Step 3: `parser.rs` を書き換える**

冒頭の `use` と `dump` を次にする。

```rust
mod common;

use common::item_kinds;
use eml_diagnostics::render;
use eml_syntax::debug_tree;
use eml_test_support::parse;

/// lossless の確認は `eml_test_support::parse` が行う。
fn dump(text: &str) -> String {
    let parsed = parse(text);
    let mut out = debug_tree(&parsed.parse.syntax());
    if !parsed.diagnostics.is_empty() {
        out.push_str("---\n");
        out.push_str(&render(&parsed.diagnostics, &parsed.files));
    }
    out
}
```

`lexer_errors_are_not_reported_twice` の本体の前半を次にする (コメントと2つの `assert_eq!` は今のまま)。

```rust
    let diagnostics = parse("€ x").diagnostics;
    let codes: Vec<String> = diagnostics.iter().map(|d| d.code.to_string()).collect();
```

`recovery_resumes_at_the_next_item` の本体を次にする (コメントは今のまま)。

```rust
    let text = "a : Int -> )\nb : Int\nc : Int";
    let diagnostics = parse(text).diagnostics;
    let codes: Vec<String> = diagnostics.iter().map(|d| d.code.to_string()).collect();
    assert_eq!(codes, ["E0011"]);
    assert_eq!(u32::from(diagnostics[0].primary.range.start()), 11);
    assert_eq!(item_kinds(text), ["SIGNATURE", "ERROR", "SIGNATURE", "SIGNATURE"]);
```

`assert_parses_losslessly` を次にする (doc コメントは今のまま)。

```rust
fn assert_parses_losslessly(text: &str) {
    parse(text);
}
```

`recovered_empty_block_stays_on_its_line` の本体の前半を次にする (2つの `assert!` は今のまま)。

```rust
    let tree = debug_tree(&parse("f =\ng = 1").parse.syntax());
```

- [ ] **Step 4: `ast.rs`、`corpus.rs`、`lexer.rs` を書き換える**

`ast.rs` の `use eml_diagnostics::SourceFiles;` と `use eml_syntax::parse;` を消し、`source` を次にする。

```rust
fn source(text: &str) -> SourceFile {
    let parsed = eml_test_support::parse(text);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    parsed.parse.tree()
}
```

`corpus.rs` の `use eml_diagnostics::SourceFiles;` と `use eml_syntax::parse;` を消し、`use common::diagnostics;` を `use common::{diagnostics, item_kinds};` にする。`s1_corpus_items` の本体の前半 (`let mut files` から `.collect();` まで) を消し、`assert_eq!(` の第1引数を `item_kinds(S1)` にする。期待値の配列は変えない。

`lexer.rs` の `dump`、`kinds`、`diags` の先頭にある次の2行を

```rust
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
```

次の1行に替える。`kinds` と `diags` では `files` を使わないので、`let (_, file) = source(text);` にする。

```rust
    let (files, file) = source(text);
```

冒頭の `use eml_diagnostics::{SourceFiles, render};` を次にする。

```rust
use eml_diagnostics::render;
use eml_test_support::source;
```

- [ ] **Step 5: テストが通り、期待値が変わっていないことを確かめる**

Run: `cargo test -p eml_syntax`
Expected: PASS

Run: `git diff --stat -- '*.snap'`
Expected: 何も出ない

Run: `git diff -U0 -- crates/eml_syntax/tests | grep '^[-+]' | grep -v '^[-+][-+]'`
Expected: 出てくる行は、ヘルパの関数、`use`、テストの本体のうちファイルを登録して構文解析する行だけである。`@"`/`@r"` の中の行、`assert_eq!` の期待値の配列や文字列の行は1行も出ない

- [ ] **Step 6: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates/eml_syntax Cargo.lock
git commit -m "Build the syntax tests on eml_test_support"
```

---

### Task 5: `eml_hir`、`eml_types`、`eml_core_ir` のテストを寄せる

**Files:**
- Modify: `crates/eml_hir/Cargo.toml`、`crates/eml_hir/tests/common/mod.rs`
- Modify: `crates/eml_types/Cargo.toml`、`crates/eml_types/tests/common/mod.rs`
- Modify: `crates/eml_core_ir/Cargo.toml`、`crates/eml_core_ir/tests/common/mod.rs`

**Interfaces:**
- Consumes: `eml_test_support::{lower, check, core, short, full}` (Task 3)
- Produces: `lower_text`、`check_text`、`core_text` は名前と出力を変えない

このタスクの変更はすべて種類3である。期待値は変えない。

- [ ] **Step 1: dev-dependency を直す**

`crates/eml_hir/Cargo.toml` の `[dev-dependencies]` に `eml_test_support.workspace = true` を足す。

`crates/eml_types/Cargo.toml` の `[dev-dependencies]` に `eml_test_support.workspace = true` を足す。`eml_syntax` の dev-dependency は、単体テスト `src/scc.rs` が使うので残す。

`crates/eml_core_ir/Cargo.toml` の `[dev-dependencies]` から `eml_diagnostics` と `eml_syntax` を消し、`eml_test_support.workspace = true` を足す。

- [ ] **Step 2: 3つの `common` を書き換える**

`crates/eml_hir/tests/common/mod.rs`:

```rust
use eml_test_support::{lower, short};

/// HIR の表示と、構文と HIR の診断を位置の順に並べたもの。
pub fn lower_text(text: &str) -> String {
    let mut lowered = lower(text);
    lowered
        .diagnostics
        .sort_by_key(|d| d.primary.range.start());
    let mut out = eml_hir::pretty(&lowered.module);
    let diagnostics = short(&lowered.files, &lowered.diagnostics);
    if !diagnostics.is_empty() {
        out.push_str("---\n");
        for line in diagnostics {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}
```

`crates/eml_types/tests/common/mod.rs`:

```rust
use eml_test_support::{check, full};

/// 推論結果と、構文・HIR・型の診断。診断はラベル、note、help まで表示する。
pub fn check_text(text: &str) -> String {
    let checked = check(text);
    let mut out = eml_types::dump(&checked.module, &checked.typed);
    if !checked.diagnostics.is_empty() {
        out.push_str("---\n");
        out.push_str(&full(&checked.files, &checked.diagnostics));
    }
    out
}
```

`crates/eml_core_ir/tests/common/mod.rs`:

```rust
/// 誤りのないプログラムを Core IR にして表示する。
pub fn core_text(text: &str) -> String {
    eml_core_ir::pretty(&eml_test_support::core(text))
}
```

- [ ] **Step 3: テストが通り、期待値が変わっていないことを確かめる**

Run: `cargo test -p eml_hir -p eml_types -p eml_core_ir`
Expected: PASS

Run: `git diff --stat -- crates/eml_hir/tests crates/eml_types/tests crates/eml_core_ir/tests`
Expected: 変わったのは3つの `tests/common/mod.rs` だけである

- [ ] **Step 4: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add crates/eml_hir crates/eml_types crates/eml_core_ir Cargo.lock
git commit -m "Build the HIR, type, and Core IR tests on eml_test_support"
```

---

### Task 6: `eml_interp` のテストを UI テストに寄せる

**Files:**
- Create: `tests/ui/run/strings_freed_in_branches.em`
- Create: `tests/ui/run/short_circuit.em`
- Create: `crates/eml_cli/tests/snapshots/ui__run@strings_freed_in_branches.em.snap`
- Create: `crates/eml_cli/tests/snapshots/ui__run@short_circuit.em.snap`
- Modify: `crates/eml_interp/Cargo.toml`
- Modify: `crates/eml_interp/tests/run.rs` (ファイル全体)
- Modify: `crates/eml_interp/tests/closures.rs:3-9`、`:49-60`

**Interfaces:**
- Consumes: `eml_test_support::{run, execute}` (Task 3)
- Produces: なし

このタスクは種類1 (テストの削除と移動) を含む。spec の承認を合意とし、Task 7 で `testing.md` に記録する。

- [ ] **Step 1: 移す2件を UI テストのファイルにする**

`tests/ui/run/strings_freed_in_branches.em` (元は `strings_are_freed` のソース):

```
-- Strings chosen by `if`, passed to an unused parameter, and discarded must all be freed.
twice : String -> String
twice s = s ++ s

ignore : String -> Int
ignore s = 1

pick : Bool -> String -> String
pick b s =
  let t = if b then s else "none"
  t ++ s

main : Unit -> <IO> Unit
main () =
  let s = "x"
  let s = s ++ "y"
  let _ = "z"
  println (twice s)
  println (show_int (ignore "w"))
  println (pick True "a")
  println (pick False "b")
```

`tests/ui/run/short_circuit.em` (元は `and_and_or_short_circuit` のソース):

```
-- `&&` and `||` evaluate the right operand only when the left one does not decide the result.
noisy : Bool -> <IO> Bool
noisy b =
  println "evaluated"
  b

show_bool : Bool -> String
show_bool b = if b then "True" else "False"

main : Unit -> <IO> Unit
main () =
  println (show_bool (False && noisy True))
  println (show_bool (True || noisy False))
  println (show_bool (True && noisy False))
```

- [ ] **Step 2: スナップショットを元のテストの期待値から書く**

stdout は、元のテストの `assert_eq!` の期待値 (`"xyxy\n1\naa\nnoneb\n"` と `"False\nTrue\nevaluated\nFalse\n"`) をそのまま写す。stderr は、元のテストが診断のないことを前提にしていたので空である。

`crates/eml_cli/tests/snapshots/ui__run@strings_freed_in_branches.em.snap`:

```
---
source: crates/eml_cli/tests/ui.rs
expression: "format!(\"--- stdout ---\\n{stdout}--- stderr ---\\n{stderr}\")"
input_file: tests/ui/run/strings_freed_in_branches.em
---
--- stdout ---
xyxy
1
aa
noneb
--- stderr ---
```

`crates/eml_cli/tests/snapshots/ui__run@short_circuit.em.snap`:

```
---
source: crates/eml_cli/tests/ui.rs
expression: "format!(\"--- stdout ---\\n{stdout}--- stderr ---\\n{stderr}\")"
input_file: tests/ui/run/short_circuit.em
---
--- stdout ---
False
True
evaluated
False
--- stderr ---
```

ファイルの末尾の改行は、既存の `ui__run@hello.em.snap` と同じにする (`tail -c 20 crates/eml_cli/tests/snapshots/ui__run@hello.em.snap | od -c` で確かめる)。

- [ ] **Step 3: UI テストが新しい2件を通すことを確かめる**

Run: `cargo test -p eml_cli --test ui`
Expected: PASS。`.snap.new` のファイルができていないこと (`git status --short` に出ない) も確かめる。できていたら、手で書いたスナップショットと実際の出力が違う。中身を比べて原因を調べる。期待値のほうを直してはいけない

- [ ] **Step 4: `eml_interp` のテストを書き換える**

`crates/eml_interp/Cargo.toml` の `[dev-dependencies]` を次にする。

```toml
[dev-dependencies]
eml_test_support.workspace = true
```

`crates/eml_interp/tests/run.rs` を次にする。削除するのは `hello_world`、`arithmetic_truncates_toward_zero`、`recursion`、`deep_recursion_does_not_overflow_the_stack`、`integer_overflow_is_a_runtime_error`、`division_by_zero_is_a_runtime_error` の6件、UI テストに移したのは `strings_are_freed` と `and_and_or_short_circuit` の2件である。残す2件の本体と期待値は変えない。

```rust
//! ソースから実行して確かめるテストは UI テスト (`tests/ui/run/`) に置く。ここには、手書きの Core IR や生成した
//! ソースが要るものだけを置く (docs/implementation/testing.md)。

use eml_core_ir::{Atom, CExpr, CExprId, CoreFn, FnIdx, Linearity, Program, Rhs, VarId, VarInfo};
use eml_interp::RuntimeError;
use eml_test_support::{execute, run};

fn main_with(body: &str) -> String {
    format!("main : Unit -> <IO> Unit\nmain () =\n{body}")
}
```

この後に、今のファイルの `leaking_program`、`debug_heap_reports_leaks`、`long_statement_sequence_does_not_overflow_the_stack` を、doc コメントとコメントも含めてそのまま続ける。

`crates/eml_interp/tests/closures.rs` の冒頭の `use` を次にする。

```rust
use eml_core_ir::{
    Atom, CExpr, CExprId, CoreFn, FnIdx, IoOp, Linearity, PrimOp, Program, Rhs, VarId, VarInfo,
};
```

`run_program` を次にする。

```rust
fn run_program(functions: Vec<CoreFn>, main: u32, strings: &[&str]) -> String {
    let program = Program {
        functions,
        main: FnIdx(main),
        strings: strings.iter().map(|s| s.to_string()).collect(),
    };
    let (stdout, result) = eml_test_support::execute(program, true);
    result.unwrap();
    stdout
}
```

- [ ] **Step 5: テストが通ることを確かめる**

Run: `cargo test -p eml_interp -p eml_cli`
Expected: PASS

Run: `git diff --stat -- '*.snap'` と `git status --short`
Expected: スナップショットの変更は、新しく足した2つのファイルだけである

- [ ] **Step 6: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

```bash
git add tests/ui/run crates/eml_cli/tests/snapshots crates/eml_interp Cargo.lock
git commit -m "Move source-driven interpreter tests into UI tests and drop the duplicates"
```

---

### Task 7: 文書

**Files:**
- Modify: `docs/implementation/testing.md`
- Modify: `docs/implementation/architecture.md`
- Modify: `docs/implementation/status.md`
- Modify: `CLAUDE.md`

**Interfaces:**
- Consumes: spec の「テストの変更の運用」「これまでの問題」「R1〜R3 で直す項目」の各節
- Produces: なし

日本語の文書を書く前に `yomiyasu:yomiyasu` スキルを読み込み、その規則に従う。

- [ ] **Step 1: `testing.md` の方針を書き換える**

「## 方針」の2つ目の箇条 (「既存のテストは合意済みの仕様である。…例外は、下の「テストの変更に関する合意済みの例外」だけである」) を次にする。

```markdown
- 既存のテストは合意済みの仕様である。テストが失敗したら実装を直す。テストを変えるときは、下の「テストの変更の運用」に従う
```

「## 方針」の後、「## 層ごとの方法」の前に次の節を置く。

```markdown
## テストの変更の運用

テストの変更を3種類に分け、種類ごとに合意の取り方を決める。このプロジェクトで合意した運用で、「既存のテストを変えない」という原則の例外にあたる。

| 種類 | 何が変わるか | 合意と記録 |
|---|---|---|
| 1. 振る舞いの変更 | 言語として観測できる期待値。UI テストの出力、診断の番号と文言、成功か失敗か。テストの削除と移動もここに入れる | 事前に合意を取り、下の「テストの変更の記録」に理由を書く |
| 2. 内部表現の変更 | 中間表現のダンプなど、内部の設計を写したスナップショットの期待値 | 作業の spec に、変わるテストと理由を列挙する。spec の承認を合意とみなし、下の「テストの変更の記録」に書く |
| 3. 機械的な追随 | テストの組み立てだけが変わる。スナップショットの文字列と `assert` の値は1文字も変えない | 作業の計画で、この種類の変更を許すと宣言する。記録はコミットメッセージで足りる |

- テストを変えないことを理由に設計を曲げない。テストが壊れると分かったら、その変更が1〜3のどれに当たるかを示して、変更を提案する。過去には、テストを守るために別の enum の種類を足したり、spec に例外を足したりしたことがある ([status.md](status.md) の「リファクタリング」)
- 作業の計画の全体制約は「期待値は、このプランで名前を挙げたテストだけを変える。期待値を変えない機械的な追随は許す」と書く
- UI テストは最も強い仕様として扱い、種類1でしか変えない
```

- [ ] **Step 2: `testing.md` のテストの置き場所と記録を書き換える**

「現在あるテストの置き場所は次のとおり。」の箇条の末尾に次を足す。

```markdown
- `crates/eml_test_support/`: 結合テストのためにパイプラインを組む関数 (`parse`、`lower`、`check`、`core`、`run`、`execute`) と、診断を文字列にする関数 (`short`、`full`)。開発専用の crate で、各 crate の `tests/` からだけ使う。`src/` の `#[cfg(test)]` から使うと、テストする crate が2つ別々にリンクされて型が合わなくなる
```

「## テストの変更に関する合意済みの例外」の見出しを「## テストの変更の記録」にし、見出しの直後に「種類1と種類2の変更を、作業ごとに記録する。」と書く。今ある5つの箇条を、文言を変えずに次の見出しの下へ振り分ける。

- `### 構文の段階 S1`: 「暫定構文で書いたテストのソースは…」と「本番の構文では `$` と `@` が…」
- `### 縦の貫通 段階1`: 「`tests/ui/run/empty.em`、`comments_only.em` と…」
- `### 縦の貫通 段階2`: 「縦の貫通の段階2で、段階1が E0004 を期待していた…」(入れ子の箇条ごと) と「`eml_types::Type::Fn` に row の末尾 (`tail`) を足したので…」

最後に次の見出しと箇条を足す。

```markdown
### リファクタリング R0

- `crates/eml_interp/tests/run.rs` のうち、ソースから実行するテストを UI テストに寄せた。UI テストが同じことを確かめていた `hello_world`、`arithmetic_truncates_toward_zero`、`recursion`、`deep_recursion_does_not_overflow_the_stack`、`integer_overflow_is_a_runtime_error`、`division_by_zero_is_a_runtime_error` は削除した。UI テストにない場合を含む `strings_are_freed` と `and_and_or_short_circuit` は、ソースを変えずに `tests/ui/run/strings_freed_in_branches.em` と `tests/ui/run/short_circuit.em` に移した。stdout は元の期待値と同じである。手書きの Core IR や生成したソースが要るテストだけを `eml_interp` に残した
```

- [ ] **Step 3: `architecture.md` を直す**

「## リポジトリ」の図で、`eml_cli/` の行の次に足す。

```
    eml_test_support/   # 開発専用。結合テストのパイプラインと診断の文字列
```

「## crate の依存関係」の最後の箇条の後に足す。

```markdown
- `eml_test_support` は開発専用の crate で、パイプラインに入らない。各 crate の結合テストが dev-dependency として使う ([テスト戦略](testing.md))
```

「`eml_core_ir`、`eml_runtime`、`eml_interp` の内部」の、クロージャと `ApplyFrame` の箇条 (「設計文書ではフレームの種類の enum にする案だったが、既存の `Frame` を変えずに済むので、ペイロードの別の種類にした」で終わるもの) の最後の文を次にする。

```markdown
設計文書ではフレームの種類の enum にする案だったが、`Frame` を変えると既存の heap のテストの書き換えが要るため、ペイロードの別の種類にした。テストを守るために構造を曲げた例で、リファクタリング R3 でフレームの種類の enum に直す ([status.md](status.md) の「リファクタリング」)
```

- [ ] **Step 4: `status.md` に「リファクタリング」の節を置く**

「## 名前解決以降の実装段階」の節の後、「## 各 crate の実装状況」の前に次の節を置く。

```markdown
## リファクタリング

段階3に入る前に、これまでの実装で見つかった構成の不合理を、破壊的な変更も含めて整理する。4つの回に分け、R0 → R1 → R2 → R3 の順に、それぞれ spec、計画、実装の1サイクルで進める。言語の観測できる振る舞い (UI テストの出力) は原則として変えず、spec の変更が要る項目はその回の spec で判断する。段階3〜5に向けた器の形は作り替えるが、機能は実装しない。

| 回 | 名前 | 範囲 | 状態 |
|---|---|---|---|
| R0 | テスト基盤と運用 | `eml_test_support`、`SourceFiles::line_col`、テストの変更の運用 ([testing.md](testing.md)) | 完了 |
| R1 | フロントエンド | `eml_syntax` と、`eml_diagnostics` の一部 | 未着手 |
| R2 | HIR と型 | `eml_hir`、`eml_types`。組み込みとエフェクトの表現は下流の crate も追随させる | 未着手 |
| R3 | Core IR とランタイム | `eml_core_ir`、`eml_runtime`、`eml_interp`、`eml_cli` | 未着手 |
```

続けて、spec の「これまでの問題: テストを変えないために設計を曲げた」の節を、見出しを `### テストを変えないために曲げた箇所` にして写す。本文の最初の段落は「過去の計画には、…構造の側を曲げてきた。」の2文だけを残し、「git の履歴と過去の作業記録から、次の例が見つかった。」を「次の例がある。」にする。表はそのまま写す。表の後の `architecture.md` についての段落は、Step 3 で直したので写さない。

続けて、spec の「R1〜R3 で直す項目」の節を、見出しを `### R1〜R3 で直す項目` にして写す。最初の段落を「各回の spec は、この一覧を出発点にし、項目を足したり外したりする。」にする。`#### R1 フロントエンド`、`#### R2 HIR と型`、`#### R3 Core IR とランタイム` の3つの小見出しと箇条は、文言を変えずに写す。箇条の中の「上の表の1」などは、写した表を指すのでそのままでよい。

「## 完了した作業」の表の末尾に行を足す。

```markdown
| リファクタリング R0 | 結合テストのパイプラインと診断の文字列を `eml_test_support` にまとめ、行と列の計算を `eml_diagnostics` に置いた。テストの変更を3種類に分ける運用を決め、`eml_interp` のソースから実行するテストを UI テストに寄せた |
```

- [ ] **Step 5: `CLAUDE.md` の Testing の節を書き換える**

`CLAUDE.md` の「## Testing」の最後の箇条 (`- Agreed exceptions to the rule against changing existing tests are recorded in ...`) を次の3つの箇条にする。英語で書く。

```markdown
- Test changes come in three kinds (`docs/implementation/testing.md`): (1) behavior changes (UI output, diagnostic codes or wording, pass/fail, deleting or moving tests) need agreement beforehand; (2) changes to internal-representation snapshots (e.g. Core IR dumps) are listed in the work's spec, and approving the spec is the agreement; (3) mechanical follow-ups that keep every expected value byte-identical are allowed when the plan says so. Kinds 1 and 2 are recorded in `testing.md`.
- Never bend the design to keep existing tests unchanged (no parallel enum variants, flags, test-only fields, or spec exceptions for that purpose). Propose the test change instead, stating its kind.
- `eml_test_support` (dev-only) builds the pipeline for integration tests (`parse` / `lower` / `check` / `core` / `run` / `execute`) and formats diagnostics (`short` / `full`). Use it only from `tests/`, never from `#[cfg(test)]` in `src/`: the crate under test would be linked twice.
```

- [ ] **Step 6: 文書を検査してコミットする**

Run: `python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py docs/implementation/testing.md`
Expected: 書き足した部分の指摘を見直す。英単語の前後の半角空白は、このリポジトリの文書の書き方なので直さない

Run: `cargo test`
Expected: PASS (文書の変更だけなので、変わらないことの確認)

```bash
git add docs/implementation CLAUDE.md
git commit -m "Document the test-change policy, eml_test_support, and the refactoring roadmap"
```

---

### Task 8: 仕上げの確認

**Files:**
- なし (確認だけ。直す必要が出たら、該当するタスクの範囲で直してコミットする)

**Interfaces:**
- Consumes: Task 1〜7 のすべて
- Produces: なし

- [ ] **Step 1: すべての検査を通す**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて通る

- [ ] **Step 2: スナップショットが変わっていないことを確かめる**

Run: `git diff main --stat -- '*.snap'`
Expected: 増えたファイル2つ (`ui__run@strings_freed_in_branches.em.snap`、`ui__run@short_circuit.em.snap`) だけが出る。既存のファイルは出ない

Run: `git diff main --stat -- 'crates/*/tests/*.rs' 'crates/*/tests/common/*.rs'`
Expected: 変わったのは次のファイルだけである。`eml_syntax` の `common/mod.rs`、`parser.rs`、`ast.rs`、`corpus.rs`、`lexer.rs`、`eml_hir`・`eml_types`・`eml_core_ir` の `common/mod.rs`、`eml_interp` の `run.rs` と `closures.rs`、`eml_cli` の `ui.rs`

Run: `git diff main -- crates/eml_syntax/tests/parser.rs crates/eml_syntax/tests/ast.rs crates/eml_syntax/tests/corpus.rs crates/eml_syntax/tests/lexer.rs crates/eml_interp/tests/closures.rs crates/eml_cli/tests/ui.rs`
Expected: 差分を読み、消えた行と足した行が、ヘルパの関数、`use`、ファイルを登録して構文解析や実行をする行だけであることを確かめる。`@"`・`@r"`・`@r#"` の中の行と、`assert_eq!` や `assert!` の期待値の行は変わっていない。`eml_interp/tests/run.rs` では、消えたテストが Task 6 で名前を挙げた8件だけである

- [ ] **Step 3: パイプラインを組む処理が残っていないことを確かめる**

Run: `grep -rn "eml_syntax::parse(" crates/*/tests`
Expected: 何も出ない

Run: `grep -rn "SourceFiles::new" crates/*/tests`
Expected: `crates/eml_cli/tests/` (UI テストの `load` と `api.rs`) と `crates/eml_test_support/` だけが出る

Run: `grep -rln "chars().count() + 1" crates`
Expected: `crates/eml_diagnostics/src/source.rs` だけが出る (行と列の計算は `eml_diagnostics` だけにある)

- [ ] **Step 4: crate ごとにテストが組み立てられることを確かめる**

Run: `for c in eml_diagnostics eml_syntax eml_hir eml_types eml_core_ir eml_runtime eml_interp eml_cli eml_test_support; do cargo test -q -p $c || echo "FAILED: $c"; done`
Expected: `FAILED` の行が出ない

## 完了後の後始末

ブランチ全体のレビューが済んだら、spec の「位置づけ」に従って作業用の文書を削除する。残す価値のある内容は Task 7 で `testing.md`、`architecture.md`、`status.md` に移してある。

```bash
git rm docs/superpowers/specs/2026-10-04-refactor-r0-test-infrastructure-design.md docs/superpowers/plans/2026-10-04-refactor-r0-test-infrastructure.md
git commit -m "Remove the work documents of refactor R0"
```
