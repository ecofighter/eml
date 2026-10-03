# eml マイルストーン1 最初の段階 (スケルトン) 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 8 つの crate からなる workspace を作り、各段階を仮実装でつないで、空のファイルに対する `eml check` が診断 0 件で終了し、UI テストの仕組みと `eml_syntax` の土台 (字句解析とイベント方式のパーサの骨組み) が動く状態にする。

**Architecture:** 各段階は `fn stage(&In) -> (Out, Vec<Diagnostic>)` の形の純粋な関数で、`eml_cli` がそれらを順につなぐ。構文は logos で字句解析し、rust-analyzer と同じイベント方式のパーサでイベント列を作り、別の処理 (sink) で rowan の木に組み立てる。最初の段階の文法はファイル全体の構造とエラー回復だけで、項目の文法は後の段階で TDD で実装する。

**Tech Stack:** Rust (edition 2024)、rowan 0.16、logos 0.16、text-size 1.1、ariadne 0.6、clap 4.6 (derive)、insta 1.49 (glob)、Nix flake

**Spec:** [docs/superpowers/specs/2026-10-03-eml-language-design.md](../specs/2026-10-03-eml-language-design.md) (§3、§6、§7 の字句、§8、§9)。マルチコアのための予防的な決定は [docs/superpowers/specs/2026-10-03-eml-multicore-design.md](../specs/2026-10-03-eml-multicore-design.md) §9

## Global Constraints

- workspace: `members = ["crates/*"]`、`resolver = "3"`、`[workspace.package]` で `edition = "2024"` を共有し、外部 crate のバージョンは `[workspace.dependencies]` でまとめて管理する
- crate は `eml_diagnostics`、`eml_syntax`、`eml_hir`、`eml_types`、`eml_core_ir`、`eml_runtime`、`eml_interp`、`eml_cli` の 8 つ。依存は上の段階から下の段階への一方向だけ (`eml_cli` → `eml_interp` → `eml_core_ir` → `eml_types` → `eml_hir` → `eml_syntax` → `eml_diagnostics`、`eml_interp` → `eml_runtime`)
- 各段階は `fn stage(input: &In) -> (Out, Vec<Diagnostic>)` の形の純粋な関数にする。グローバルな可変状態は持たない
- `eml_diagnostics` は `rowan` に依存しない。`TextRange` は `text-size` crate を直接使う
- エラー番号は段階ごとに範囲を分ける。字句・構文は E0xxx
- CLI の終了コード: 0 = 成功、1 = 診断のエラーあり、または実行時エラー、2 = 使い方の誤り (引数の誤り、ファイルが読めない)
- `eml_cli` の lib は `check(files, file_id) -> Vec<Diagnostic>` と `run(files, file_id, &RunConfig, stdout: OutputSink) -> RunOutcome` を公開する
- `OutputSink` は `Send + Sync` な共有の出力先の型 (`Arc<Mutex<dyn Write + Send>>` を包む型)。`RunConfig` は `Default` を実装し、フィールドを後から足せるようにする
- UI テストは常に `debug_heap` を有効にして実行する
- インタプリタの値とフレームに `Rc` と `RefCell` を使わない (この段階では値はまだないが、仮実装でも使わない)
- 既存のテストは合意済みの仕様なので変更しない。テストが失敗したら実装を直す
- 暫定構文で迷ったときは Haskell の慣習に寄せる
- 外部 crate のバージョン: `rowan = "0.16"`、`logos = "0.16"`、`text-size = "1.1"`、`ariadne = "0.6"`、`clap = { version = "4.6", features = ["derive"] }`、`insta = { version = "1.49", features = ["glob"] }`。spec の外部 crate の一覧にある `la-arena` は、HIR で ID を使い始める後の段階で追加する (この段階では使わないため)

### この計画で決めた、spec にない細部

spec が決めていない細部を、次のように決めた。後の段階で変える場合は spec に書き戻す。

- 字句・構文のエラー番号: E0001 = 認識できない文字、E0002 = 閉じていない文字列、E0003 = 項目が来るべき位置に別のもの、E0004 = まだ対応していない構文
- 連続する認識できない文字は1つの `ERROR_TOKEN` にまとめ、診断は1件だけ出す。パーサは `ERROR_TOKEN` を重ねて報告しない
- 文字列リテラルは行をまたげない。閉じていない文字列は行末までを `STRING` として扱い、E0002 を出す (パーサからは普通の文字列に見える)。エスケープが `\n` `\t` `\\` `\"` のどれかであることの検査は、リテラルの値を作る後の段階で行う
- ファイル先頭などにある BOM (`U+FEFF`) は空白として扱う
- 最初の段階では項目 (`fn` / `type` / `effect`) の文法がないので、項目ごとに E0004 を1件出し、次の項目のキーワードまでを `ERROR` ノードにまとめる。項目の文法を実装する段階でこの診断は消える
- spec §3 の回復の同期点のうち、この段階で使うのはトップレベルのキーワード (`fn` / `type` / `effect`) だけ。セミコロンと波括弧は項目の中の文法で使うので、項目の文法を実装する段階で追加する
- `tests/ui/run/empty.em` は、`main` の検査を実装する段階で `check-fail` に移る (空のファイルには `main` がないため)。これはその段階のタスクとして明示的に行う

## Review Focus

spec が明示していないが、利用者が実際に踏みやすい入力と、そのときに期待される振る舞い。各行のテストは、担当するタスクに入れてある。

1. **CRLF の改行のファイル**: Windows で書いたファイルでも、コメントのトークンに `\r` が入らず、`\r\n` は空白として扱われる (Task 2 の `crlf_line_endings_are_whitespace`)
2. **ファイル先頭の BOM**: エディタが付けた BOM が「認識できない文字」のエラーにならない (Task 2 の `byte_order_mark_is_whitespace`)
3. **非 ASCII 文字より後ろにあるエラーの位置**: 診断の列番号は、バイト数ではなく文字数で数えた値になる (Task 1 の `columns_count_characters_not_bytes`)
4. **UTF-8 でないファイル**: パニックせず、「読めない」として終了コード 2 で終わる (Task 6 の `non_utf8_file_is_a_usage_error`)
5. **改行で終わらないコメント**: ファイルの最後の行がコメントで、末尾に改行がなくても正しくトークンになる (Task 2 の `comment_at_end_of_file_without_newline`)

---

### Task 1: workspace への移行と `eml_diagnostics`

**Files:**
- Modify: `Cargo.toml` (全体を置き換える)
- Delete: `src/main.rs`、`src/lib.rs`
- Create: `crates/eml_diagnostics/Cargo.toml`
- Create: `crates/eml_diagnostics/src/lib.rs`
- Create: `crates/eml_diagnostics/src/source.rs`
- Create: `crates/eml_diagnostics/src/render.rs`

**Interfaces:**
- Consumes: なし
- Produces:
  - `FileId` (`Copy + Eq + Hash + Ord`)、`SourceFiles::new()`、`SourceFiles::add(&mut self, path: impl Into<String>, text: impl Into<String>) -> FileId`、`SourceFiles::path(&self, FileId) -> &str`、`SourceFiles::text(&self, FileId) -> &str`
  - `ErrorCode(pub u16)` (`Display` で `E0001` の形)、`Severity { Error, Warning, Note }`
  - `Label { file: FileId, range: TextRange, message: String }`、`Label::new(file, range, message: impl Into<String>)`
  - `TextEdit { file, range, replacement: String }`
  - `Diagnostic { code, severity, message, primary: Label, secondary: Vec<Label>, notes: Vec<String>, help: Vec<String>, fix: Option<Vec<TextEdit>> }`、`Diagnostic::new(code, severity, message, primary)`、`Diagnostic::error(code, message, primary)`、`with_secondary`、`with_note`、`with_help`、`is_error()`
  - `has_errors(&[Diagnostic]) -> bool`、`render(&[Diagnostic], &SourceFiles) -> String` (色なし)
  - 再公開: `TextRange`、`TextSize`

- [ ] **Step 1: workspace のルートを作り、古い単一パッケージのソースを消す**

`Cargo.toml` を次の内容で置き換える。まだ存在しない crate への path 依存も先に書いておく (使われるまで解決されない)。

```toml
[workspace]
members = ["crates/*"]
resolver = "3"

[workspace.package]
version = "0.1.0"
edition = "2024"

[workspace.dependencies]
eml_diagnostics = { path = "crates/eml_diagnostics" }
eml_syntax = { path = "crates/eml_syntax" }
eml_hir = { path = "crates/eml_hir" }
eml_types = { path = "crates/eml_types" }
eml_core_ir = { path = "crates/eml_core_ir" }
eml_runtime = { path = "crates/eml_runtime" }
eml_interp = { path = "crates/eml_interp" }
ariadne = "0.6"
clap = { version = "4.6", features = ["derive"] }
insta = { version = "1.49", features = ["glob"] }
logos = "0.16"
rowan = "0.16"
text-size = "1.1"
```

```bash
git rm -q src/main.rs src/lib.rs
```

`crates/eml_diagnostics/Cargo.toml`:

```toml
[package]
name = "eml_diagnostics"
version.workspace = true
edition.workspace = true

[dependencies]
ariadne.workspace = true
text-size.workspace = true
```

- [ ] **Step 2: 失敗するテストを書く**

`crates/eml_diagnostics/src/lib.rs` を、まずテストだけの内容で作る:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_is_zero_padded() {
        assert_eq!(ErrorCode(1).to_string(), "E0001");
        assert_eq!(ErrorCode(2105).to_string(), "E2105");
    }

    #[test]
    fn has_errors_ignores_warnings() {
        let mut files = SourceFiles::new();
        let file = files.add("a.em", "x");
        let label = Label::new(file, TextRange::new(0.into(), 1.into()), "");
        let warning = Diagnostic::new(
            ErrorCode(4001),
            Severity::Warning,
            "unreachable",
            label.clone(),
        );
        assert!(!has_errors(std::slice::from_ref(&warning)));
        let error = Diagnostic::error(ErrorCode(1), "bad", label);
        assert!(has_errors(&[warning, error]));
    }
}
```

`crates/eml_diagnostics/src/source.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_returns_distinct_ids() {
        let mut files = SourceFiles::new();
        let a = files.add("a.em", "aaa");
        let b = files.add("b.em", "bbb");
        assert_ne!(a, b);
        assert_eq!(files.path(a), "a.em");
        assert_eq!(files.text(b), "bbb");
    }
}
```

`crates/eml_diagnostics/src/render.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ErrorCode, TextRange};

    #[test]
    fn renders_code_message_label_and_help() {
        let mut files = SourceFiles::new();
        let file = files.add("check-fail/a.em", "let x = $;\n");
        let range = TextRange::new(8.into(), 9.into());
        let diagnostic = Diagnostic::error(
            ErrorCode(1),
            "unexpected character `$`",
            Label::new(file, range, "not valid here"),
        )
        .with_help("remove this character");
        let text = render(&[diagnostic], &files);
        assert!(
            text.contains("[E0001] Error: unexpected character `$`"),
            "{text}"
        );
        assert!(text.contains("check-fail/a.em:1:9"), "{text}");
        assert!(text.contains("not valid here"), "{text}");
        assert!(text.contains("Help: remove this character"), "{text}");
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        let mut files = SourceFiles::new();
        let file = files.add("a.em", "é = $");
        // `$` はバイト位置 5 (`é` が2バイト) だが、文字としては 5 文字目なので列 5 と表示する。
        let range = TextRange::new(5.into(), 6.into());
        let diagnostic = Diagnostic::error(ErrorCode(1), "bad", Label::new(file, range, "here"));
        let text = render(&[diagnostic], &files);
        assert!(text.contains("a.em:1:5"), "{text}");
    }

    #[test]
    fn renders_nothing_for_no_diagnostics() {
        assert_eq!(render(&[], &SourceFiles::new()), "");
    }
}
```

- [ ] **Step 3: テストが失敗することを確認する**

Run: `cargo test -p eml_diagnostics`
Expected: コンパイルエラー (`cannot find type SourceFiles` など)

- [ ] **Step 4: 実装を書く**

各ファイルの先頭 (`#[cfg(test)]` の前) に次を追加する。テストのモジュールとの間は空行1行にする。

`crates/eml_diagnostics/src/lib.rs` の先頭:

```rust
//! 診断の型、ソースファイルの管理、ariadne による表示。

mod render;
mod source;

use std::fmt;

pub use render::render;
pub use source::{FileId, SourceFiles};
pub use text_size::{TextRange, TextSize};

/// `E0001` のような安定したエラー番号。段階ごとに番号の範囲を分ける (spec §6)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ErrorCode(pub u16);

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "E{:04}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    Error,
    Warning,
    Note,
}

/// ソース上の位置と、そこに付けるラベル文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub file: FileId,
    pub range: TextRange,
    pub message: String,
}

impl Label {
    pub fn new(file: FileId, range: TextRange, message: impl Into<String>) -> Self {
        Label {
            file,
            range,
            message: message.into(),
        }
    }
}

/// 自動修正の1つの編集。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub file: FileId,
    pub range: TextRange,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: ErrorCode,
    pub severity: Severity,
    pub message: String,
    pub primary: Label,
    pub secondary: Vec<Label>,
    pub notes: Vec<String>,
    pub help: Vec<String>,
    pub fix: Option<Vec<TextEdit>>,
}

impl Diagnostic {
    pub fn new(
        code: ErrorCode,
        severity: Severity,
        message: impl Into<String>,
        primary: Label,
    ) -> Self {
        Diagnostic {
            code,
            severity,
            message: message.into(),
            primary,
            secondary: Vec::new(),
            notes: Vec::new(),
            help: Vec::new(),
            fix: None,
        }
    }

    pub fn error(code: ErrorCode, message: impl Into<String>, primary: Label) -> Self {
        Self::new(code, Severity::Error, message, primary)
    }

    pub fn with_secondary(mut self, label: Label) -> Self {
        self.secondary.push(label);
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help.push(help.into());
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

pub fn has_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics.iter().any(Diagnostic::is_error)
}
```

`crates/eml_diagnostics/src/source.rs` の先頭:

```rust
/// `SourceFiles` の中のファイルを指す ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(u32);

/// `FileId` → パスとテキスト。将来クエリ化するときは salsa の入力に置き換える。
#[derive(Debug, Default)]
pub struct SourceFiles {
    files: Vec<(String, String)>,
}

impl SourceFiles {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, path: impl Into<String>, text: impl Into<String>) -> FileId {
        let id = FileId(u32::try_from(self.files.len()).expect("too many source files"));
        self.files.push((path.into(), text.into()));
        id
    }

    pub fn path(&self, id: FileId) -> &str {
        &self.files[id.0 as usize].0
    }

    pub fn text(&self, id: FileId) -> &str {
        &self.files[id.0 as usize].1
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.files
            .iter()
            .map(|(path, text)| (path.as_str(), text.as_str()))
    }
}
```

`crates/eml_diagnostics/src/render.rs` の先頭:

```rust
use std::ops::Range;

use ariadne::{Config, IndexType, Label as AriadneLabel, Report, ReportKind};

use crate::{Diagnostic, Label, Severity, SourceFiles};

/// 診断を色なしのテキストに整形する。CLI は stderr に、UI テストはスナップショットに使う。
pub fn render(diagnostics: &[Diagnostic], files: &SourceFiles) -> String {
    let mut out = Vec::new();
    for diagnostic in diagnostics {
        let kind = match diagnostic.severity {
            Severity::Error => ReportKind::Error,
            Severity::Warning => ReportKind::Warning,
            Severity::Note => ReportKind::Advice,
        };
        let mut report = Report::build(kind, span(&diagnostic.primary, files))
            .with_config(
                Config::default()
                    .with_color(false)
                    .with_index_type(IndexType::Byte),
            )
            .with_code(diagnostic.code)
            .with_message(&diagnostic.message)
            .with_label(label(&diagnostic.primary, files));
        for secondary in &diagnostic.secondary {
            report = report.with_label(label(secondary, files));
        }
        for note in &diagnostic.notes {
            report = report.with_note(note);
        }
        for help in &diagnostic.help {
            report = report.with_help(help);
        }
        let sources = ariadne::sources(
            files
                .iter()
                .map(|(path, text)| (path.to_string(), text.to_string())),
        );
        report
            .finish()
            .write(sources, &mut out)
            .expect("writing to a Vec cannot fail");
    }
    String::from_utf8(out).expect("ariadne writes UTF-8")
}

fn span(label: &Label, files: &SourceFiles) -> (String, Range<usize>) {
    (
        files.path(label.file).to_string(),
        label.range.start().into()..label.range.end().into(),
    )
}

fn label(label: &Label, files: &SourceFiles) -> AriadneLabel<(String, Range<usize>)> {
    let result = AriadneLabel::new(span(label, files));
    if label.message.is_empty() {
        result
    } else {
        result.with_message(&label.message)
    }
}
```

- [ ] **Step 5: テストが通ることを確認する**

Run: `cargo test -p eml_diagnostics && cargo clippy -p eml_diagnostics --all-targets -- -D warnings && cargo fmt --all --check`
Expected: 6 tests passed、clippy と fmt の指摘なし

- [ ] **Step 6: コミットする**

```bash
git add Cargo.toml Cargo.lock crates/eml_diagnostics
git commit -m "Convert to a Cargo workspace and add eml_diagnostics"
```

---

### Task 2: `SyntaxKind` と字句解析

**Files:**
- Create: `crates/eml_syntax/Cargo.toml`
- Create: `crates/eml_syntax/src/lib.rs` (この段階の内容。Task 3 で置き換える)
- Create: `crates/eml_syntax/src/syntax_kind.rs`
- Create: `crates/eml_syntax/src/lexer.rs`
- Test: `crates/eml_syntax/tests/lexer.rs`

**Interfaces:**
- Consumes: Task 1 の `FileId`、`SourceFiles`、`Diagnostic`、`Label`、`ErrorCode`、`TextRange`、`TextSize`、`render`
- Produces:
  - `SyntaxKind` (`repr(u16)`、トークンが先で `EOF` まで、続いてノードの `SOURCE_FILE`、`ERROR`)、`SyntaxKind::is_trivia(self) -> bool`
  - `EmlLanguage` (rowan の `Language`)、型別名 `SyntaxNode`、`SyntaxToken`、`SyntaxElement`、`SyntaxNodePtr`
  - `Token { kind: SyntaxKind, range: TextRange }`、`lex(file: FileId, text: &str) -> (Vec<Token>, Vec<Diagnostic>)`
  - `codes::{UNEXPECTED_CHARACTER (E0001), UNTERMINATED_STRING (E0002), EXPECTED_ITEM (E0003), NOT_YET_SUPPORTED (E0004)}`

- [ ] **Step 1: crate の設定と、この段階の `lib.rs` を作る**

`crates/eml_syntax/Cargo.toml`:

```toml
[package]
name = "eml_syntax"
version.workspace = true
edition.workspace = true

[dependencies]
eml_diagnostics.workspace = true
logos.workspace = true
rowan.workspace = true

[dev-dependencies]
insta.workspace = true
```

`crates/eml_syntax/src/lib.rs`:

```rust
//! 字句解析、イベント方式のパーサ、rowan の木、型付き AST ラッパ。

mod lexer;
mod syntax_kind;

pub use lexer::{Token, lex};
pub use syntax_kind::{
    EmlLanguage, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxNodePtr, SyntaxToken,
};

/// 字句・構文の診断の番号 (E0xxx)。
pub mod codes {
    use eml_diagnostics::ErrorCode;

    pub const UNEXPECTED_CHARACTER: ErrorCode = ErrorCode(1);
    pub const UNTERMINATED_STRING: ErrorCode = ErrorCode(2);
    pub const EXPECTED_ITEM: ErrorCode = ErrorCode(3);
    pub const NOT_YET_SUPPORTED: ErrorCode = ErrorCode(4);
}
```

- [ ] **Step 2: 失敗するテストを書く**

`crates/eml_syntax/tests/lexer.rs`:

```rust
use eml_diagnostics::{SourceFiles, render};
use eml_syntax::lex;

/// トークン列を1行1トークンで表示する。
fn dump(text: &str) -> String {
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (tokens, diagnostics) = lex(file, text);
    let mut out = String::new();
    for token in &tokens {
        out.push_str(&format!(
            "{:?}@{:?} {:?}\n",
            token.kind, token.range, &text[token.range]
        ));
    }
    let joined: String = tokens.iter().map(|token| &text[token.range]).collect();
    assert_eq!(joined, text, "tokens must cover the whole text");
    if !diagnostics.is_empty() {
        out.push_str("---\n");
        out.push_str(&render(&diagnostics, &files));
    }
    out
}

#[test]
fn keywords_and_identifiers() {
    insta::assert_snapshot!(dump("fn fnord let _ _x x1 Some IO"), @r#"
    FN_KW@0..2 "fn"
    WHITESPACE@2..3 " "
    LIDENT@3..8 "fnord"
    WHITESPACE@8..9 " "
    LET_KW@9..12 "let"
    WHITESPACE@12..13 " "
    UNDERSCORE@13..14 "_"
    WHITESPACE@14..15 " "
    LIDENT@15..17 "_x"
    WHITESPACE@17..18 " "
    LIDENT@18..20 "x1"
    WHITESPACE@20..21 " "
    UIDENT@21..25 "Some"
    WHITESPACE@25..26 " "
    UIDENT@26..28 "IO"
    "#);
}

#[test]
fn all_keywords() {
    let text = "fn let if then else match type effect handle resume drop return never once multi true false";
    let kinds: Vec<String> = dump(text)
        .lines()
        .filter(|line| !line.starts_with("WHITESPACE"))
        .map(|line| line.split('@').next().unwrap().to_string())
        .collect();
    assert_eq!(
        kinds,
        [
            "FN_KW",
            "LET_KW",
            "IF_KW",
            "THEN_KW",
            "ELSE_KW",
            "MATCH_KW",
            "TYPE_KW",
            "EFFECT_KW",
            "HANDLE_KW",
            "RESUME_KW",
            "DROP_KW",
            "RETURN_KW",
            "NEVER_KW",
            "ONCE_KW",
            "MULTI_KW",
            "TRUE_KW",
            "FALSE_KW",
        ]
    );
}

#[test]
fn operators_use_longest_match() {
    let text = "( ) { } , ; : = -> => | < > + - * / % ++ == != <= >= && || ! <<= --";
    let kinds: Vec<String> = dump(text)
        .lines()
        .filter(|line| !line.starts_with("WHITESPACE"))
        .map(|line| line.split('@').next().unwrap().to_string())
        .collect();
    assert_eq!(
        kinds,
        [
            "L_PAREN",
            "R_PAREN",
            "L_BRACE",
            "R_BRACE",
            "COMMA",
            "SEMICOLON",
            "COLON",
            "EQ",
            "THIN_ARROW",
            "FAT_ARROW",
            "PIPE",
            "LT",
            "GT",
            "PLUS",
            "MINUS",
            "STAR",
            "SLASH",
            "PERCENT",
            "PLUS2",
            "EQ2",
            "NEQ",
            "LTEQ",
            "GTEQ",
            "AMP2",
            "PIPE2",
            "BANG",
            "LT",
            "LTEQ",
            "MINUS",
            "MINUS",
        ]
    );
}

#[test]
fn literals_and_comments() {
    insta::assert_snapshot!(dump("42 \"a\\n\\\"b\" // note\n-1"), @r#"
    INT@0..2 "42"
    WHITESPACE@2..3 " "
    STRING@3..11 "\"a\\n\\\"b\""
    WHITESPACE@11..12 " "
    COMMENT@12..19 "// note"
    WHITESPACE@19..20 "\n"
    MINUS@20..21 "-"
    INT@21..22 "1"
    "#);
}

#[test]
fn unexpected_characters_are_merged_into_one_error() {
    insta::assert_snapshot!(dump("a $@ b é"), @r#"
    LIDENT@0..1 "a"
    WHITESPACE@1..2 " "
    ERROR_TOKEN@2..4 "$@"
    WHITESPACE@4..5 " "
    LIDENT@5..6 "b"
    WHITESPACE@6..7 " "
    ERROR_TOKEN@7..9 "é"
    ---
    [E0001] Error: unexpected character `$@`
       ╭─[ test.em:1:3 ]
       │
     1 │ a $@ b é
       │   ─┬  
       │    ╰── not valid in eml source
    ───╯
    [E0001] Error: unexpected character `é`
       ╭─[ test.em:1:8 ]
       │
     1 │ a $@ b é
       │        ┬  
       │        ╰── not valid in eml source
    ───╯
    "#);
}

#[test]
fn unterminated_string_becomes_string_with_error() {
    insta::assert_snapshot!(dump("\"abc\nx"), @r#"
    STRING@0..4 "\"abc"
    WHITESPACE@4..5 "\n"
    LIDENT@5..6 "x"
    ---
    [E0002] Error: unterminated string literal
       ╭─[ test.em:1:1 ]
       │
     1 │ "abc
       │ ──┬─  
       │   ╰─── missing closing `"`
    ───╯
    "#);
}

#[test]
fn crlf_line_endings_are_whitespace() {
    insta::assert_snapshot!(dump("a\r\nb // c\r\n"), @r#"
    LIDENT@0..1 "a"
    WHITESPACE@1..3 "\r\n"
    LIDENT@3..4 "b"
    WHITESPACE@4..5 " "
    COMMENT@5..9 "// c"
    WHITESPACE@9..11 "\r\n"
    "#);
}

#[test]
fn byte_order_mark_is_whitespace() {
    insta::assert_snapshot!(dump("\u{feff}fn"), @r#"
    WHITESPACE@0..3 "\u{feff}"
    FN_KW@3..5 "fn"
    "#);
}

#[test]
fn comment_at_end_of_file_without_newline() {
    insta::assert_snapshot!(dump("x // end"), @r#"
    LIDENT@0..1 "x"
    WHITESPACE@1..2 " "
    COMMENT@2..8 "// end"
    "#);
}
```

`crates/eml_syntax/src/syntax_kind.rs` を、まずテストだけの内容で作る:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_round_trip() {
        for raw in 0..SyntaxKind::__LAST as u16 {
            assert_eq!(SyntaxKind::from_raw(raw) as u16, raw);
        }
    }

    #[test]
    fn tokens_fit_in_token_set() {
        assert!((SyntaxKind::EOF as u16) < 128);
    }
}
```

`crates/eml_syntax/src/lexer.rs` は空のファイルとして作る。

- [ ] **Step 3: テストが失敗することを確認する**

Run: `cargo test -p eml_syntax`
Expected: コンパイルエラー (`cannot find type SyntaxKind`、`unresolved import lexer::lex` など)

- [ ] **Step 4: 実装を書く**

`crates/eml_syntax/src/syntax_kind.rs` の先頭 (`#[cfg(test)]` の前) に追加する:

```rust
use logos::Logos;

/// トークンと構文ノードの種類。トークン (`EOF` まで) を先に並べ、`TokenSet` が 128 ビットに収まるようにする。
/// 字句の規則は spec §7 の「字句」に従う。
#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
#[allow(non_camel_case_types)]
pub enum SyntaxKind {
    // trivia
    #[regex(r"[ \t\r\n\u{FEFF}]+")]
    WHITESPACE = 0,
    #[regex(r"//[^\r\n]*", allow_greedy = true)]
    COMMENT,

    // リテラルと識別子
    #[regex(r"[0-9]+")]
    INT,
    #[regex(r#""([^"\\\n]|\\[^\n])*""#)]
    STRING,
    /// 閉じていない文字列。字句解析の段階で `STRING` に変換し、診断を出す。木には現れない。
    #[regex(r#""([^"\\\n]|\\[^\n])*"#)]
    UNTERMINATED_STRING,
    #[regex(r"[a-z][A-Za-z0-9_]*|_[A-Za-z0-9_]+")]
    LIDENT,
    #[regex(r"[A-Z][A-Za-z0-9_]*")]
    UIDENT,
    #[token("_")]
    UNDERSCORE,

    // キーワード
    #[token("fn")]
    FN_KW,
    #[token("let")]
    LET_KW,
    #[token("if")]
    IF_KW,
    #[token("then")]
    THEN_KW,
    #[token("else")]
    ELSE_KW,
    #[token("match")]
    MATCH_KW,
    #[token("type")]
    TYPE_KW,
    #[token("effect")]
    EFFECT_KW,
    #[token("handle")]
    HANDLE_KW,
    #[token("resume")]
    RESUME_KW,
    #[token("drop")]
    DROP_KW,
    #[token("return")]
    RETURN_KW,
    #[token("never")]
    NEVER_KW,
    #[token("once")]
    ONCE_KW,
    #[token("multi")]
    MULTI_KW,
    #[token("true")]
    TRUE_KW,
    #[token("false")]
    FALSE_KW,

    // 記号
    #[token("(")]
    L_PAREN,
    #[token(")")]
    R_PAREN,
    #[token("{")]
    L_BRACE,
    #[token("}")]
    R_BRACE,
    #[token(",")]
    COMMA,
    #[token(";")]
    SEMICOLON,
    #[token(":")]
    COLON,
    #[token("=")]
    EQ,
    #[token("->")]
    THIN_ARROW,
    #[token("=>")]
    FAT_ARROW,
    #[token("|")]
    PIPE,
    #[token("<")]
    LT,
    #[token(">")]
    GT,
    #[token("+")]
    PLUS,
    #[token("-")]
    MINUS,
    #[token("*")]
    STAR,
    #[token("/")]
    SLASH,
    #[token("%")]
    PERCENT,
    #[token("++")]
    PLUS2,
    #[token("==")]
    EQ2,
    #[token("!=")]
    NEQ,
    #[token("<=")]
    LTEQ,
    #[token(">=")]
    GTEQ,
    #[token("&&")]
    AMP2,
    #[token("||")]
    PIPE2,
    #[token("!")]
    BANG,

    /// 字句として認識できない文字の並び。
    ERROR_TOKEN,
    /// 入力の終わり。パーサの中でだけ使い、木には現れない。
    EOF,

    // ノード
    SOURCE_FILE,
    ERROR,

    #[doc(hidden)]
    __LAST,
}

impl SyntaxKind {
    pub fn is_trivia(self) -> bool {
        matches!(self, SyntaxKind::WHITESPACE | SyntaxKind::COMMENT)
    }

    fn from_raw(raw: u16) -> SyntaxKind {
        assert!(raw < SyntaxKind::__LAST as u16, "invalid SyntaxKind {raw}");
        // SAFETY: `SyntaxKind` は `repr(u16)` で、0 から `__LAST` まで値が連続している。
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EmlLanguage {}

impl rowan::Language for EmlLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
        SyntaxKind::from_raw(raw.0)
    }

    fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind as u16)
    }
}

pub type SyntaxNode = rowan::SyntaxNode<EmlLanguage>;
pub type SyntaxToken = rowan::SyntaxToken<EmlLanguage>;
pub type SyntaxElement = rowan::SyntaxElement<EmlLanguage>;
pub type SyntaxNodePtr = rowan::ast::SyntaxNodePtr<EmlLanguage>;
```

`crates/eml_syntax/src/lexer.rs`:

```rust
use eml_diagnostics::{Diagnostic, FileId, Label, TextRange, TextSize};
use logos::Logos;

use crate::SyntaxKind;
use crate::codes;

/// 字句解析の結果の1トークン。trivia (空白とコメント) も含む。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: SyntaxKind,
    pub range: TextRange,
}

/// テキストをトークン列に分ける。トークン列をつなげると元のテキストに戻る (lossless)。
/// 認識できない文字の並びは1つの `ERROR_TOKEN` にまとめ、診断を1件出す。
pub fn lex(file: FileId, text: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    let mut tokens: Vec<Token> = Vec::new();
    let mut diagnostics = Vec::new();
    let mut lexer = SyntaxKind::lexer(text);
    while let Some(result) = lexer.next() {
        let span = lexer.span();
        let range = TextRange::new(
            TextSize::new(span.start as u32),
            TextSize::new(span.end as u32),
        );
        let kind = match result {
            Ok(SyntaxKind::UNTERMINATED_STRING) => {
                diagnostics.push(Diagnostic::error(
                    codes::UNTERMINATED_STRING,
                    "unterminated string literal",
                    Label::new(file, range, "missing closing `\"`"),
                ));
                SyntaxKind::STRING
            }
            Ok(kind) => kind,
            Err(()) => SyntaxKind::ERROR_TOKEN,
        };
        match tokens.last_mut() {
            Some(last)
                if kind == SyntaxKind::ERROR_TOKEN && last.kind == SyntaxKind::ERROR_TOKEN =>
            {
                last.range = last.range.cover(range);
            }
            _ => tokens.push(Token { kind, range }),
        }
    }
    for token in tokens
        .iter()
        .filter(|token| token.kind == SyntaxKind::ERROR_TOKEN)
    {
        let snippet = &text[token.range];
        diagnostics.push(Diagnostic::error(
            codes::UNEXPECTED_CHARACTER,
            format!("unexpected character `{snippet}`"),
            Label::new(file, token.range, "not valid in eml source"),
        ));
    }
    diagnostics.sort_by_key(|diagnostic| diagnostic.primary.range.start());
    (tokens, diagnostics)
}
```

注意点:
- logos 0.16 は `[^\n]*` のような貪欲な繰り返しに `allow_greedy = true` を要求する。コメントの規則はこの指定がないとコンパイルエラーになる
- コメントの規則は `[^\r\n]*` にする。`[^\n]*` だと CRLF のファイルでコメントに `\r` が入る (Review Focus 1)
- `SyntaxKind` には logos の属性のないノードの種類も並べてよい (logos は属性のない変種を無視する)

- [ ] **Step 5: テストが通ることを確認する**

Run: `cargo test -p eml_syntax && cargo clippy -p eml_syntax --all-targets -- -D warnings && cargo fmt --all --check`
Expected: lexer の 9 tests と syntax_kind の 2 tests が通る。clippy と fmt の指摘なし

インラインのスナップショット (`@r#"..."#`) が一致しない場合は、`cargo insta review` で差分を確認する。期待値を書き換えるのではなく、実装を直す。

- [ ] **Step 6: コミットする**

```bash
git add Cargo.lock crates/eml_syntax
git commit -m "Add SyntaxKind and the logos-based lexer"
```

---

### Task 3: イベント方式のパーサの骨組み、トップレベルの文法、`parse`

**Files:**
- Create: `crates/eml_syntax/src/token_set.rs`
- Create: `crates/eml_syntax/src/parser.rs`
- Create: `crates/eml_syntax/src/sink.rs`
- Create: `crates/eml_syntax/src/debug_dump.rs`
- Create: `crates/eml_syntax/src/grammar/mod.rs`
- Create: `crates/eml_syntax/src/ast.rs`
- Modify: `crates/eml_syntax/src/lib.rs` (全体を置き換える)
- Test: `crates/eml_syntax/tests/parser.rs`

**Interfaces:**
- Consumes: Task 2 の `SyntaxKind`、`EmlLanguage`、`SyntaxNode`、`SyntaxElement`、`Token`、`lex`、`codes`
- Produces:
  - `parse(file: FileId, text: &str) -> (Parse, Vec<Diagnostic>)`。診断は位置の順に並ぶ
  - `Parse::syntax(&self) -> SyntaxNode`、`Parse::tree(&self) -> ast::SourceFile`
  - `ast::SourceFile` (`rowan::ast::AstNode` を実装)
  - `debug_tree(&SyntaxNode) -> String` (CST のスナップショット用)
  - crate 内部: `Parser` (`new`、`finish`、`nth`、`current`、`at`、`at_ts`、`at_eof`、`current_range`、`bump_any`、`bump`、`eat`、`start`、`error(code, message, label)`)、`Marker::{complete, abandon}`、`CompletedMarker::precede`、`TokenSet::{new, contains}`、`sink::build_tree(text, &[Token], Vec<Event>) -> GreenNode`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_syntax/tests/parser.rs`:

```rust
use eml_diagnostics::{SourceFiles, render};
use eml_syntax::{debug_tree, parse};

/// パースした木と診断を表示する。木は必ず元のテキストに戻ることも確認する。
fn dump(text: &str) -> String {
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (parse, diagnostics) = parse(file, text);
    assert_eq!(
        parse.syntax().text().to_string(),
        text,
        "tree must be lossless"
    );
    let mut out = debug_tree(&parse.syntax());
    if !diagnostics.is_empty() {
        out.push_str("---\n");
        out.push_str(&render(&diagnostics, &files));
    }
    out
}

#[test]
fn empty_file() {
    insta::assert_snapshot!(dump(""), @"SOURCE_FILE@0..0");
}

#[test]
fn trivia_only_file_has_no_errors() {
    insta::assert_snapshot!(dump("// only a comment\n\n"), @r#"
    SOURCE_FILE@0..19
      COMMENT@0..17 "// only a comment"
      WHITESPACE@17..19 "\n\n"
    "#);
}

#[test]
fn stray_tokens_are_one_error_until_the_next_item() {
    insta::assert_snapshot!(dump("1 2 3"), @r#"
    SOURCE_FILE@0..5
      ERROR@0..5
        INT@0..1 "1"
        WHITESPACE@1..2 " "
        INT@2..3 "2"
        WHITESPACE@3..4 " "
        INT@4..5 "3"
    ---
    [E0003] Error: expected an item (`fn`, `type`, or `effect`)
       ╭─[ test.em:1:1 ]
       │
     1 │ 1 2 3
       │ ┬  
       │ ╰── not the start of an item
    ───╯
    "#);
}

#[test]
fn lexer_errors_are_not_reported_twice() {
    let text = "$ x";
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (_, diagnostics) = parse(file, text);
    let codes: Vec<String> = diagnostics.iter().map(|d| d.code.to_string()).collect();
    // `$` は字句解析の E0001 だけ。続く `x` は項目ではないので E0003 を1件出す。
    assert_eq!(codes, ["E0001", "E0003"]);
    assert_eq!(u32::from(diagnostics[1].primary.range.start()), 2);
}

#[test]
fn recovery_resumes_at_item_keywords() {
    // 項目の文法は後の段階で実装する。今は項目ごとに「まだ対応していない」を1件ずつ出し、
    // 項目の間のエラーとは独立に報告できることを確認する。
    let text = "fn main () : Unit { } ? type T { } effect E { }";
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (parse, diagnostics) = parse(file, text);
    let codes: Vec<String> = diagnostics.iter().map(|d| d.code.to_string()).collect();
    assert_eq!(codes, ["E0004", "E0001", "E0004", "E0004"]);
    let kinds: Vec<String> = parse
        .syntax()
        .children()
        .map(|node| format!("{:?}", node.kind()))
        .collect();
    assert_eq!(kinds, ["ERROR", "ERROR", "ERROR"]);
}
```

`crates/eml_syntax/src/token_set.rs` を、まずテストだけの内容で作る:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_only_listed_kinds() {
        let set = TokenSet::new(&[SyntaxKind::FN_KW, SyntaxKind::EOF]);
        assert!(set.contains(SyntaxKind::FN_KW));
        assert!(set.contains(SyntaxKind::EOF));
        assert!(!set.contains(SyntaxKind::TYPE_KW));
    }
}
```

`crates/eml_syntax/src/parser.rs` を、まずテストだけの内容で作る。テストは、仕組みだけを試す小さな文法をその場で渡して、トリビアの付け方、`precede`、`abandon`、先読み、入力の終わりでの診断の位置を確認する:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug_tree;
    use crate::lexer::lex;
    use crate::sink::build_tree;
    use crate::{SyntaxKind::*, SyntaxNode};
    use eml_diagnostics::SourceFiles;

    /// テキストを字句解析し、`grammar` でパースして木の表示を返す。仕組みだけを試すための小さな文法を渡す。
    fn run(text: &str, grammar: impl FnOnce(&mut Parser)) -> (String, Vec<Diagnostic>) {
        let mut files = SourceFiles::new();
        let file = files.add("test.em", text);
        let (tokens, _) = lex(file, text);
        let mut p = Parser::new(file, &tokens, TextSize::of(text));
        grammar(&mut p);
        let (events, diagnostics) = p.finish();
        let tree = SyntaxNode::new_root(build_tree(text, &tokens, events));
        assert_eq!(tree.text().to_string(), text, "tree must be lossless");
        (debug_tree(&tree), diagnostics)
    }

    #[test]
    fn trivia_inside_root_goes_to_the_enclosing_node() {
        let (tree, _) = run(" a // c\n b ", |p| {
            let root = p.start();
            let inner = p.start();
            p.bump_any();
            inner.complete(p, ERROR);
            p.bump_any();
            root.complete(p, SOURCE_FILE);
        });
        insta::assert_snapshot!(tree, @r#"
        SOURCE_FILE@0..11
          WHITESPACE@0..1 " "
          ERROR@1..2
            LIDENT@1..2 "a"
          WHITESPACE@2..3 " "
          COMMENT@3..7 "// c"
          WHITESPACE@7..9 "\n "
          LIDENT@9..10 "b"
          WHITESPACE@10..11 " "
        "#);
    }

    #[test]
    fn precede_wraps_a_completed_node() {
        let (tree, _) = run("a b", |p| {
            let root = p.start();
            let first = p.start();
            p.bump_any();
            let first = first.complete(p, ERROR);
            let outer = first.precede(p);
            p.bump_any();
            outer.complete(p, ERROR);
            root.complete(p, SOURCE_FILE);
        });
        insta::assert_snapshot!(tree, @r#"
        SOURCE_FILE@0..3
          ERROR@0..3
            ERROR@0..1
              LIDENT@0..1 "a"
            WHITESPACE@1..2 " "
            LIDENT@2..3 "b"
        "#);
    }

    #[test]
    fn abandoned_marker_leaves_no_node() {
        let (tree, _) = run("a", |p| {
            let root = p.start();
            let unused = p.start();
            unused.abandon(p);
            p.bump_any();
            root.complete(p, SOURCE_FILE);
        });
        insta::assert_snapshot!(tree, @r#"
        SOURCE_FILE@0..1
          LIDENT@0..1 "a"
        "#);
    }

    #[test]
    fn lookahead_skips_trivia_and_reports_eof() {
        run("a  // c\n b", |p| {
            let root = p.start();
            assert_eq!(p.current(), LIDENT);
            assert_eq!(p.nth(1), LIDENT);
            assert_eq!(p.nth(2), EOF);
            assert!(!p.eat(UIDENT));
            assert!(p.eat(LIDENT));
            p.bump(LIDENT);
            assert!(p.at_eof());
            root.complete(p, SOURCE_FILE);
        });
    }

    #[test]
    fn error_at_eof_points_at_end_of_text() {
        let (_, diagnostics) = run("a ", |p| {
            let root = p.start();
            p.bump_any();
            p.error(ErrorCode(9999), "expected more", "here");
            root.complete(p, SOURCE_FILE);
        });
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].primary.range,
            TextRange::empty(TextSize::new(2))
        );
    }
}
```

`crates/eml_syntax/src/lib.rs` を全体で置き換える (この時点で、まだ存在しないモジュールを参照する):

```rust
//! 字句解析、イベント方式のパーサ、rowan の木、型付き AST ラッパ。

pub mod ast;
mod debug_dump;
mod grammar;
mod lexer;
mod parser;
mod sink;
mod syntax_kind;
mod token_set;

use eml_diagnostics::{Diagnostic, FileId, TextSize};
use rowan::GreenNode;
use rowan::ast::AstNode;

pub use debug_dump::debug_tree;
pub use lexer::{Token, lex};
pub use syntax_kind::{
    EmlLanguage, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxNodePtr, SyntaxToken,
};

/// 字句・構文の診断の番号 (E0xxx)。
pub mod codes {
    use eml_diagnostics::ErrorCode;

    pub const UNEXPECTED_CHARACTER: ErrorCode = ErrorCode(1);
    pub const UNTERMINATED_STRING: ErrorCode = ErrorCode(2);
    pub const EXPECTED_ITEM: ErrorCode = ErrorCode(3);
    pub const NOT_YET_SUPPORTED: ErrorCode = ErrorCode(4);
}

/// パースの結果の木。壊れた入力でも必ず木を作る。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parse {
    green: GreenNode,
}

impl Parse {
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    pub fn tree(&self) -> ast::SourceFile {
        ast::SourceFile::cast(self.syntax()).expect("the root is always SOURCE_FILE")
    }
}

pub fn parse(file: FileId, text: &str) -> (Parse, Vec<Diagnostic>) {
    let (tokens, mut diagnostics) = lex(file, text);
    let mut parser = parser::Parser::new(file, &tokens, TextSize::of(text));
    grammar::source_file(&mut parser);
    let (events, parse_diagnostics) = parser.finish();
    diagnostics.extend(parse_diagnostics);
    diagnostics.sort_by_key(|diagnostic| diagnostic.primary.range.start());
    let green = sink::build_tree(text, &tokens, events);
    (Parse { green }, diagnostics)
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p eml_syntax`
Expected: コンパイルエラー (`file not found for module sink` など)

- [ ] **Step 3: `TokenSet` と `Parser` を実装する**

`crates/eml_syntax/src/token_set.rs` の先頭 (`#[cfg(test)]` の前) に追加する:

```rust
use crate::SyntaxKind;

/// トークンの種類の集合。エラー回復の同期点などに使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TokenSet(u128);

impl TokenSet {
    pub(crate) const fn new(kinds: &[SyntaxKind]) -> TokenSet {
        let mut bits = 0u128;
        let mut i = 0;
        while i < kinds.len() {
            bits |= mask(kinds[i]);
            i += 1;
        }
        TokenSet(bits)
    }

    pub(crate) const fn contains(&self, kind: SyntaxKind) -> bool {
        self.0 & mask(kind) != 0
    }
}

const fn mask(kind: SyntaxKind) -> u128 {
    assert!((kind as u16) < 128, "only token kinds can be in a TokenSet");
    1u128 << (kind as u16)
}
```

`crates/eml_syntax/src/parser.rs` の先頭 (`#[cfg(test)]` の前) に追加する:

```rust
//! rust-analyzer と同じイベント方式のパーサ。文法の規則は `grammar` に置き、ここは仕組みだけを持つ。

use eml_diagnostics::{Diagnostic, ErrorCode, FileId, Label, TextRange, TextSize};

use crate::SyntaxKind;
use crate::lexer::Token;
use crate::token_set::TokenSet;

/// パーサが出すイベント。`sink::build_tree` がこれを rowan の木に組み立てる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Event {
    /// ノードの開始。`forward_parent` は、`precede` で後から作った親ノードの `Start` までの距離。
    Start {
        kind: SyntaxKind,
        forward_parent: Option<u32>,
    },
    /// trivia でないトークンを1つ進める。
    Token {
        kind: SyntaxKind,
    },
    Finish,
    /// まだ種類の決まっていない `Start`、または取り消したノード。
    Tombstone,
}

pub(crate) struct Parser {
    file: FileId,
    /// trivia を除いたトークン列。
    tokens: Vec<Token>,
    pos: usize,
    eof: TextSize,
    events: Vec<Event>,
    diagnostics: Vec<Diagnostic>,
}

impl Parser {
    pub(crate) fn new(file: FileId, tokens: &[Token], eof: TextSize) -> Parser {
        Parser {
            file,
            tokens: tokens
                .iter()
                .copied()
                .filter(|token| !token.kind.is_trivia())
                .collect(),
            pos: 0,
            eof,
            events: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub(crate) fn finish(self) -> (Vec<Event>, Vec<Diagnostic>) {
        (self.events, self.diagnostics)
    }

    /// `n` 個先の trivia でないトークンの種類。入力の終わりを越えたら `EOF`。
    pub(crate) fn nth(&self, n: usize) -> SyntaxKind {
        self.tokens
            .get(self.pos + n)
            .map_or(SyntaxKind::EOF, |token| token.kind)
    }

    pub(crate) fn current(&self) -> SyntaxKind {
        self.nth(0)
    }

    pub(crate) fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    pub(crate) fn at_ts(&self, set: TokenSet) -> bool {
        set.contains(self.current())
    }

    pub(crate) fn at_eof(&self) -> bool {
        self.at(SyntaxKind::EOF)
    }

    /// 今のトークンの位置。入力の終わりでは、テキストの末尾の空の範囲。
    pub(crate) fn current_range(&self) -> TextRange {
        self.tokens
            .get(self.pos)
            .map_or(TextRange::empty(self.eof), |token| token.range)
    }

    pub(crate) fn bump_any(&mut self) {
        let kind = self.current();
        assert_ne!(kind, SyntaxKind::EOF, "cannot bump past the end of input");
        self.events.push(Event::Token { kind });
        self.pos += 1;
    }

    // 後の段階の文法で使う。
    #[allow(dead_code)]
    pub(crate) fn bump(&mut self, kind: SyntaxKind) {
        assert!(
            self.at(kind),
            "expected {kind:?}, found {:?}",
            self.current()
        );
        self.bump_any();
    }

    // 後の段階の文法で使う。
    #[allow(dead_code)]
    pub(crate) fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump_any();
            true
        } else {
            false
        }
    }

    pub(crate) fn start(&mut self) -> Marker {
        let pos = self.events.len() as u32;
        self.events.push(Event::Tombstone);
        Marker { pos }
    }

    /// 今のトークンの位置に診断を出す。トークンは進めない。`label` はその位置に付ける説明。
    pub(crate) fn error(
        &mut self,
        code: ErrorCode,
        message: impl Into<String>,
        label: impl Into<String>,
    ) {
        let range = self.current_range();
        self.diagnostics.push(Diagnostic::error(
            code,
            message,
            Label::new(self.file, range, label),
        ));
    }
}

/// 開始したノード。`complete` か `abandon` で必ず閉じる。
#[must_use]
pub(crate) struct Marker {
    pos: u32,
}

impl Marker {
    pub(crate) fn complete(self, p: &mut Parser, kind: SyntaxKind) -> CompletedMarker {
        match &mut p.events[self.pos as usize] {
            Event::Start { kind: slot, .. } => *slot = kind,
            slot @ Event::Tombstone => {
                *slot = Event::Start {
                    kind,
                    forward_parent: None,
                }
            }
            _ => unreachable!("marker must point at a Start or Tombstone event"),
        }
        p.events.push(Event::Finish);
        CompletedMarker { pos: self.pos }
    }

    // 後の段階の文法で使う。
    #[allow(dead_code)]
    pub(crate) fn abandon(self, p: &mut Parser) {
        if self.pos as usize == p.events.len() - 1 {
            p.events.pop();
        }
    }
}

pub(crate) struct CompletedMarker {
    pos: u32,
}

impl CompletedMarker {
    /// 完了したノードの外側に、新しい親ノードを開始する (二項演算子の左辺などに使う)。
    // 後の段階の文法で使う。
    #[allow(dead_code)]
    pub(crate) fn precede(self, p: &mut Parser) -> Marker {
        let parent = p.start();
        match &mut p.events[self.pos as usize] {
            Event::Start { forward_parent, .. } => *forward_parent = Some(parent.pos - self.pos),
            _ => unreachable!("completed marker must point at a Start event"),
        }
        parent
    }
}
```

`bump`、`eat`、`abandon`、`precede` はこの段階の文法では使わないので `#[allow(dead_code)]` を付けている。後の段階の文法で使い始めたら外す。

- [ ] **Step 4: 木の組み立て (sink) と木の表示を実装する**

`crates/eml_syntax/src/sink.rs`:

```rust
use std::mem;

use rowan::{GreenNode, GreenNodeBuilder, Language};

use crate::lexer::Token;
use crate::parser::Event;
use crate::{EmlLanguage, SyntaxKind};

/// イベント列と、trivia を含むトークン列から、rowan の木を組み立てる。
/// trivia は、それを囲むノードのうち一番内側のもの (直後に始まるノードの親) に付ける。
pub(crate) fn build_tree(text: &str, tokens: &[Token], mut events: Vec<Event>) -> GreenNode {
    let mut builder = Builder {
        text,
        tokens,
        next: 0,
        inner: GreenNodeBuilder::new(),
    };
    let mut depth = 0usize;
    let mut kinds = Vec::new();
    for i in 0..events.len() {
        match mem::replace(&mut events[i], Event::Tombstone) {
            Event::Start {
                kind,
                forward_parent,
            } => {
                // `precede` で作った親ノードを外側から順に開く。
                kinds.push(kind);
                let mut index = i;
                let mut next = forward_parent;
                while let Some(distance) = next {
                    index += distance as usize;
                    next = match mem::replace(&mut events[index], Event::Tombstone) {
                        Event::Start {
                            kind,
                            forward_parent,
                        } => {
                            kinds.push(kind);
                            forward_parent
                        }
                        _ => unreachable!("forward_parent must point at a Start event"),
                    };
                }
                for kind in kinds.drain(..).rev() {
                    if depth > 0 {
                        builder.eat_trivia();
                    }
                    builder.inner.start_node(EmlLanguage::kind_to_raw(kind));
                    depth += 1;
                }
            }
            Event::Token { kind } => {
                builder.eat_trivia();
                builder.token(kind);
            }
            Event::Finish => {
                depth -= 1;
                if depth == 0 {
                    builder.eat_trivia();
                }
                builder.inner.finish_node();
            }
            Event::Tombstone => {}
        }
    }
    builder.inner.finish()
}

struct Builder<'a> {
    text: &'a str,
    tokens: &'a [Token],
    next: usize,
    inner: GreenNodeBuilder<'static>,
}

impl Builder<'_> {
    fn eat_trivia(&mut self) {
        while let Some(token) = self.tokens.get(self.next) {
            if !token.kind.is_trivia() {
                break;
            }
            self.token(token.kind);
        }
    }

    fn token(&mut self, kind: SyntaxKind) {
        let token = self.tokens[self.next];
        self.inner
            .token(EmlLanguage::kind_to_raw(kind), &self.text[token.range]);
        self.next += 1;
    }
}
```

`crates/eml_syntax/src/debug_dump.rs`:

```rust
use std::fmt::Write;

use crate::{SyntaxElement, SyntaxNode};

/// 木を1行1要素で表示する。CST のスナップショットテストに使う。
/// ノードは `KIND@start..end`、トークンは `KIND@start..end "text"` の形で、子は2文字ずつ字下げする。
pub fn debug_tree(node: &SyntaxNode) -> String {
    let mut out = String::new();
    write_element(&mut out, SyntaxElement::Node(node.clone()), 0);
    out
}

fn write_element(out: &mut String, element: SyntaxElement, depth: usize) {
    let indent = "  ".repeat(depth);
    match element {
        SyntaxElement::Node(node) => {
            writeln!(out, "{indent}{:?}@{:?}", node.kind(), node.text_range()).unwrap();
            for child in node.children_with_tokens() {
                write_element(out, child, depth + 1);
            }
        }
        SyntaxElement::Token(token) => {
            writeln!(
                out,
                "{indent}{:?}@{:?} {:?}",
                token.kind(),
                token.text_range(),
                token.text()
            )
            .unwrap();
        }
    }
}
```

- [ ] **Step 5: トップレベルの文法と型付き AST ラッパを実装する**

`crates/eml_syntax/src/grammar/mod.rs`:

```rust
//! 暫定構文の文法の規則 (spec §7)。構文の差し替えは原則としてこのモジュールの中で行う。
//!
//! 最初の段階では、ファイル全体の構造とエラー回復だけを実装する。項目 (`fn` / `type` / `effect`) の
//! 文法は後の段階で TDD で実装し、それまでは「まだ対応していない」という診断を出す。

use crate::SyntaxKind::*;
use crate::codes;
use crate::parser::Parser;
use crate::token_set::TokenSet;

/// トップレベルのエラー回復の同期点。
const ITEM_START: TokenSet = TokenSet::new(&[FN_KW, TYPE_KW, EFFECT_KW]);

pub(crate) fn source_file(p: &mut Parser) {
    let m = p.start();
    while !p.at_eof() {
        if p.at_ts(ITEM_START) {
            item(p);
        } else {
            stray_tokens(p);
        }
    }
    m.complete(p, SOURCE_FILE);
}

fn item(p: &mut Parser) {
    let keyword = match p.current() {
        FN_KW => "fn",
        TYPE_KW => "type",
        EFFECT_KW => "effect",
        kind => unreachable!("item called at {kind:?}"),
    };
    p.error(
        codes::NOT_YET_SUPPORTED,
        format!("`{keyword}` items are not supported yet"),
        "item syntax is implemented in a later stage",
    );
    let m = p.start();
    p.bump_any();
    skip_to_next_item(p);
    m.complete(p, ERROR);
}

/// 項目の外にあるトークンの並びを、次の同期点まで1つの `ERROR` ノードにまとめる。
/// 診断は1件だけ出す。`ERROR_TOKEN` は字句解析で報告済みなので、それ以外のトークンの位置に出す。
fn stray_tokens(p: &mut Parser) {
    let m = p.start();
    let mut reported = false;
    while !p.at_eof() && !p.at_ts(ITEM_START) {
        if !reported && !p.at(ERROR_TOKEN) {
            p.error(
                codes::EXPECTED_ITEM,
                "expected an item (`fn`, `type`, or `effect`)",
                "not the start of an item",
            );
            reported = true;
        }
        p.bump_any();
    }
    m.complete(p, ERROR);
}

fn skip_to_next_item(p: &mut Parser) {
    while !p.at_eof() && !p.at_ts(ITEM_START) {
        p.bump_any();
    }
}
```

`crates/eml_syntax/src/ast.rs`:

```rust
//! 型付き AST ラッパ。rowan の木の上の薄い型付きの見方を提供する。

use rowan::ast::AstNode;

use crate::{EmlLanguage, SyntaxKind, SyntaxNode};

/// ファイル全体。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceFile {
    syntax: SyntaxNode,
}

impl AstNode for SourceFile {
    type Language = EmlLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::SOURCE_FILE
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        Self::can_cast(syntax.kind()).then_some(SourceFile { syntax })
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.syntax
    }
}
```

- [ ] **Step 6: テストが通ることを確認する**

Run: `cargo test -p eml_syntax && cargo clippy -p eml_syntax --all-targets -- -D warnings && cargo fmt --all --check`
Expected: lexer 9、parser 5、crate 内部の unit tests 8 (syntax_kind 2、token_set 1、parser 5) が通る。clippy と fmt の指摘なし

- [ ] **Step 7: コミットする**

```bash
git add crates/eml_syntax
git commit -m "Add the event-based parser skeleton with top-level error recovery"
```

---

### Task 4: 仮実装の段階 (`eml_hir`、`eml_types`、`eml_core_ir`、`eml_interp`) と `eml_runtime` の `OutputSink`

**Files:**
- Create: `crates/eml_runtime/Cargo.toml`、`crates/eml_runtime/src/lib.rs`、`crates/eml_runtime/src/output.rs`
- Create: `crates/eml_hir/Cargo.toml`、`crates/eml_hir/src/lib.rs`
- Create: `crates/eml_types/Cargo.toml`、`crates/eml_types/src/lib.rs`
- Create: `crates/eml_core_ir/Cargo.toml`、`crates/eml_core_ir/src/lib.rs`
- Create: `crates/eml_interp/Cargo.toml`、`crates/eml_interp/src/lib.rs`

**Interfaces:**
- Consumes: Task 1 の `Diagnostic`、`FileId`。Task 3 の `ast::SourceFile`
- Produces:
  - `eml_runtime::OutputSink` (`Clone + Send + Sync`): `new(impl Write + Send + 'static)`、`stdout()`、`capture() -> (OutputSink, Arc<Mutex<Vec<u8>>>)`、`write_str(&self, &str) -> io::Result<()>`
  - `eml_hir::Module`、`eml_hir::lower(FileId, &ast::SourceFile) -> (Module, Vec<Diagnostic>)`
  - `eml_types::TypedModule`、`eml_types::check(&Module) -> (TypedModule, Vec<Diagnostic>)`
  - `eml_core_ir::Program`、`eml_core_ir::lower(&TypedModule) -> (Program, Vec<Diagnostic>)`
  - `eml_interp::RunConfig { pub debug_heap: bool }` (`Default`、`#[non_exhaustive]`)、`eml_interp::RuntimeError(pub String)`、`eml_interp::run(Arc<Program>, &RunConfig, &OutputSink) -> Result<(), RuntimeError>`

- [ ] **Step 1: `OutputSink` の失敗するテストを書く**

`crates/eml_runtime/Cargo.toml`:

```toml
[package]
name = "eml_runtime"
version.workspace = true
edition.workspace = true

[dependencies]
```

`crates/eml_runtime/src/lib.rs`:

```rust
//! オブジェクトのモデル、ヒープ、参照カウント、debug_heap の検査。
//!
//! 最初の段階では、実行結果の出力先 `OutputSink` だけを持つ。ヒープと参照カウントは後の段階で実装する。

mod output;

pub use output::OutputSink;
```

`crates/eml_runtime/src/output.rs` を、まずテストだけの内容で作る:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn output_sink_is_send_and_sync() {
        assert_send_sync::<OutputSink>();
    }

    #[test]
    fn capture_collects_writes_from_clones() {
        let (sink, buffer) = OutputSink::capture();
        let clone = sink.clone();
        sink.write_str("hello ").unwrap();
        clone.write_str("world\n").unwrap();
        assert_eq!(
            String::from_utf8(buffer.lock().unwrap().clone()).unwrap(),
            "hello world\n"
        );
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p eml_runtime`
Expected: コンパイルエラー (`cannot find type OutputSink`)

- [ ] **Step 3: `OutputSink` を実装する**

`crates/eml_runtime/src/output.rs` の先頭 (`#[cfg(test)]` の前) に追加する:

```rust
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

/// 実行結果の出力先。将来、複数のスレッドから `println` するため `Send + Sync` にする
/// (マルチコア対応の設計 spec §8)。
#[derive(Clone)]
pub struct OutputSink(Arc<Mutex<dyn Write + Send>>);

impl OutputSink {
    pub fn new(writer: impl Write + Send + 'static) -> Self {
        OutputSink(Arc::new(Mutex::new(writer)))
    }

    /// プロセスの標準出力に書く出力先。
    pub fn stdout() -> Self {
        Self::new(io::stdout())
    }

    /// 書いた内容をメモリに貯める出力先と、その中身を読むためのバッファを返す。テストで使う。
    pub fn capture() -> (Self, Arc<Mutex<Vec<u8>>>) {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        (OutputSink(buffer.clone()), buffer)
    }

    pub fn write_str(&self, text: &str) -> io::Result<()> {
        let mut writer = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        writer.write_all(text.as_bytes())?;
        writer.flush()
    }
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p eml_runtime`
Expected: 2 tests passed

- [ ] **Step 5: 仮実装の段階を作る**

各段階は、後の段階で TDD で中身を実装するまで、空の出力と空の診断を返す。動作は Task 5 の UI テストで確認する。

`crates/eml_hir/Cargo.toml`:

```toml
[package]
name = "eml_hir"
version.workspace = true
edition.workspace = true

[dependencies]
eml_diagnostics.workspace = true
eml_syntax.workspace = true
```

`crates/eml_hir/src/lib.rs`:

```rust
//! CST → HIR の変換と名前解決。
//!
//! 最初の段階では仮実装で、空の `Module` を返す。中身は後の段階で TDD で実装する。

use eml_diagnostics::{Diagnostic, FileId};
use eml_syntax::ast;

/// 1つのソースファイルの HIR。
#[derive(Debug, Default)]
pub struct Module {}

pub fn lower(_file: FileId, _source: &ast::SourceFile) -> (Module, Vec<Diagnostic>) {
    (Module::default(), Vec::new())
}
```

`crates/eml_types/Cargo.toml`:

```toml
[package]
name = "eml_types"
version.workspace = true
edition.workspace = true

[dependencies]
eml_diagnostics.workspace = true
eml_hir.workspace = true
```

`crates/eml_types/src/lib.rs`:

```rust
//! Kind・型・row の推論、線形性・多重度の検査、match の網羅性検査。
//!
//! 最初の段階では仮実装で、空の `TypedModule` を返す。中身は後の段階で TDD で実装する。

use eml_diagnostics::Diagnostic;
use eml_hir::Module;

/// 型の情報を別テーブルに持つ HIR。
#[derive(Debug, Default)]
pub struct TypedModule {}

pub fn check(_module: &Module) -> (TypedModule, Vec<Diagnostic>) {
    (TypedModule::default(), Vec::new())
}
```

`crates/eml_core_ir/Cargo.toml`:

```toml
[package]
name = "eml_core_ir"
version.workspace = true
edition.workspace = true

[dependencies]
eml_diagnostics.workspace = true
eml_types.workspace = true
```

`crates/eml_core_ir/src/lib.rs`:

```rust
//! 型付き HIR → Core IR の変換と、dup/decref の挿入パス。
//!
//! 最初の段階では仮実装で、空の `Program` を返す。中身は後の段階で TDD で実装する。

use eml_diagnostics::Diagnostic;
use eml_types::TypedModule;

/// Core IR のプログラム全体。実行時は `Arc<Program>` で読み取り専用で共有する。
#[derive(Debug, Default)]
pub struct Program {}

pub fn lower(_module: &TypedModule) -> (Program, Vec<Diagnostic>) {
    (Program::default(), Vec::new())
}
```

`crates/eml_interp/Cargo.toml`:

```toml
[package]
name = "eml_interp"
version.workspace = true
edition.workspace = true

[dependencies]
eml_core_ir.workspace = true
eml_runtime.workspace = true
```

`crates/eml_interp/src/lib.rs`:

```rust
//! Core IR を CEK 機械で実行する。
//!
//! 最初の段階では仮実装で、何も実行せずに正常終了する。中身は後の段階で TDD で実装する。

use std::fmt;
use std::sync::Arc;

use eml_core_ir::Program;
use eml_runtime::OutputSink;

/// 実行の設定。フィールドを後から足せるように `non_exhaustive` にする
/// (`RunConfig::default()` から作り、フィールドを代入して使う)。
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct RunConfig {
    /// RC のリーク検出と解放済みアクセスの検出を有効にする。
    pub debug_heap: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError(pub String);

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RuntimeError {}

pub fn run(
    _program: Arc<Program>,
    _config: &RunConfig,
    _out: &OutputSink,
) -> Result<(), RuntimeError> {
    Ok(())
}
```

- [ ] **Step 6: workspace 全体を確認する**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: すべて通る。clippy と fmt の指摘なし

- [ ] **Step 7: コミットする**

```bash
git add Cargo.lock crates/eml_runtime crates/eml_hir crates/eml_types crates/eml_core_ir crates/eml_interp
git commit -m "Add stub pipeline stages and the thread-safe OutputSink"
```

---

### Task 5: `eml_cli` の lib API と UI テストの仕組み

**Files:**
- Create: `crates/eml_cli/Cargo.toml` (この段階の内容。Task 6 で置き換える)
- Create: `crates/eml_cli/src/lib.rs`
- Test: `crates/eml_cli/tests/ui.rs`
- Create: `tests/ui/run/empty.em`、`tests/ui/run/comments_only.em`
- Create: `tests/ui/check-fail/unexpected_character.em`、`tests/ui/check-fail/stray_tokens.em`、`tests/ui/check-fail/multiple_errors.em`
- Create: `crates/eml_cli/tests/snapshots/*.snap` (5 件。テストが生成し、内容を確認してコミットする)

**Interfaces:**
- Consumes: Task 1 の `SourceFiles`、`FileId`、`Diagnostic`、`has_errors`、`render`。Task 3 の `eml_syntax::parse`。Task 4 の各段階の関数、`RunConfig`、`OutputSink`
- Produces:
  - `eml_cli::check(&SourceFiles, FileId) -> Vec<Diagnostic>`
  - `eml_cli::run(&SourceFiles, FileId, &RunConfig, OutputSink) -> RunOutcome`
  - `RunOutcome { diagnostics: Vec<Diagnostic>, result: RunResult }`、`RunResult { NotRun, Completed, RuntimeError(String) }`
  - 再公開: `eml_cli::RunConfig`、`eml_cli::OutputSink`

- [ ] **Step 1: UI テストのコーパスとテストを書く**

`crates/eml_cli/Cargo.toml`:

```toml
[package]
name = "eml_cli"
version.workspace = true
edition.workspace = true

[dependencies]
eml_core_ir.workspace = true
eml_diagnostics.workspace = true
eml_hir.workspace = true
eml_interp.workspace = true
eml_runtime.workspace = true
eml_syntax.workspace = true
eml_types.workspace = true

[dev-dependencies]
insta.workspace = true
```

`crates/eml_cli/tests/ui.rs`:

```rust
//! UI テスト。`tests/ui/run/*.em` は実行が正常に終わること、`tests/ui/check-fail/*.em` は診断のエラーが
//! 1件以上出ることを確認し、出力をスナップショットにする (spec §8)。

use std::fs;
use std::path::Path;

use eml_cli::{OutputSink, RunConfig, RunResult};
use eml_diagnostics::{SourceFiles, has_errors, render};

/// スナップショットの中のパスを安定させるため、`tests/ui` からの相対パスでファイルを登録する。
fn load(path: &Path) -> (SourceFiles, eml_diagnostics::FileId) {
    let ui_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/ui")
        .canonicalize()
        .unwrap();
    let relative = path
        .strip_prefix(&ui_root)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    let mut files = SourceFiles::new();
    let id = files.add(relative, fs::read_to_string(path).unwrap());
    (files, id)
}

#[test]
fn run() {
    insta::glob!("../../../tests/ui", "run/*.em", |path| {
        let (files, id) = load(path);
        let mut config = RunConfig::default();
        config.debug_heap = true;
        let (sink, buffer) = OutputSink::capture();
        let outcome = eml_cli::run(&files, id, &config, sink);
        let stderr = render(&outcome.diagnostics, &files);
        assert!(
            !has_errors(&outcome.diagnostics),
            "unexpected errors:\n{stderr}"
        );
        assert_eq!(outcome.result, RunResult::Completed, "{stderr}");
        let stdout = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        insta::assert_snapshot!(format!("--- stdout ---\n{stdout}--- stderr ---\n{stderr}"));
    });
}

#[test]
fn check_fail() {
    insta::glob!("../../../tests/ui", "check-fail/*.em", |path| {
        let (files, id) = load(path);
        let diagnostics = eml_cli::check(&files, id);
        let rendered = render(&diagnostics, &files);
        assert!(
            has_errors(&diagnostics),
            "expected at least one error, got:\n{rendered}"
        );
        insta::assert_snapshot!(rendered);
    });
}
```

コーパスのファイルを作る:

```bash
mkdir -p tests/ui/run tests/ui/check-fail
: > tests/ui/run/empty.em
printf '// A file with only comments and blank lines.\n\n// eml has no block comments.\n' > tests/ui/run/comments_only.em
printf '// `$` is not a valid character in eml source.\n$\n' > tests/ui/check-fail/unexpected_character.em
printf '1 2 3\n' > tests/ui/check-fail/stray_tokens.em
printf '// Independent errors are all reported in one run.\n$\nfoo\n@\n' > tests/ui/check-fail/multiple_errors.em
```

`crates/eml_cli/src/lib.rs` は空のファイルとして作る。

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p eml_cli --test ui`
Expected: コンパイルエラー (`unresolved imports eml_cli::OutputSink, eml_cli::RunConfig, eml_cli::RunResult`)

- [ ] **Step 3: lib API を実装する**

`crates/eml_cli/src/lib.rs`:

```rust
//! `eml check` / `eml run` の中身。各段階をつなぐだけで、テストからプロセス内で呼べる API を公開する。

use std::sync::Arc;

use eml_core_ir::Program;
use eml_diagnostics::{Diagnostic, FileId, SourceFiles, has_errors};

pub use eml_interp::RunConfig;
pub use eml_runtime::OutputSink;

/// ファイルを検査し、すべての段階の診断を返す。
pub fn check(files: &SourceFiles, file: FileId) -> Vec<Diagnostic> {
    analyze(files, file).1
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunResult {
    /// 診断のエラーがあったので実行しなかった。
    NotRun,
    Completed,
    RuntimeError(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    /// 検査で出た診断 (警告を含む)。
    pub diagnostics: Vec<Diagnostic>,
    pub result: RunResult,
}

/// ファイルを検査し、エラーがなければ実行する。プログラムの出力は `stdout` に書く。
pub fn run(
    files: &SourceFiles,
    file: FileId,
    config: &RunConfig,
    stdout: OutputSink,
) -> RunOutcome {
    let (program, diagnostics) = analyze(files, file);
    if has_errors(&diagnostics) {
        return RunOutcome {
            diagnostics,
            result: RunResult::NotRun,
        };
    }
    let result = match eml_interp::run(Arc::new(program), config, &stdout) {
        Ok(()) => RunResult::Completed,
        Err(error) => RunResult::RuntimeError(error.to_string()),
    };
    RunOutcome {
        diagnostics,
        result,
    }
}

/// 各段階を順につなぐ。エラーがあっても止めず、すべての段階の診断を集める。
fn analyze(files: &SourceFiles, file: FileId) -> (Program, Vec<Diagnostic>) {
    let (parse, mut diagnostics) = eml_syntax::parse(file, files.text(file));
    let (module, stage) = eml_hir::lower(file, &parse.tree());
    diagnostics.extend(stage);
    let (typed, stage) = eml_types::check(&module);
    diagnostics.extend(stage);
    let (program, stage) = eml_core_ir::lower(&typed);
    diagnostics.extend(stage);
    (program, diagnostics)
}
```

- [ ] **Step 4: スナップショットがまだないので失敗することを確認する**

Run: `cargo test -p eml_cli --test ui`
Expected: `run` と `check_fail` が FAIL (新しいスナップショットとして `.snap.new` が作られる)。成功・失敗の判定 (`assert!`) では落ちていないことを、メッセージが `snapshot assertion` であることで確認する

- [ ] **Step 5: スナップショットを生成し、内容を確認する**

Run: `INSTA_UPDATE=always cargo test -p eml_cli --test ui && rm -f crates/eml_cli/tests/snapshots/*.snap.new`

生成された 5 件のスナップショットが次の内容と一致することを確認する (各ファイルの先頭の `---` で囲まれたヘッダは insta が付けるもの)。行末の空白は ariadne の出力の一部なのでそのまま残す。

`crates/eml_cli/tests/snapshots/ui__run@empty.em.snap`:

```
---
source: crates/eml_cli/tests/ui.rs
expression: "format!(\"--- stdout ---\\n{stdout}--- stderr ---\\n{stderr}\")"
input_file: tests/ui/run/empty.em
---
--- stdout ---
--- stderr ---
```

`crates/eml_cli/tests/snapshots/ui__run@comments_only.em.snap`:

```
---
source: crates/eml_cli/tests/ui.rs
expression: "format!(\"--- stdout ---\\n{stdout}--- stderr ---\\n{stderr}\")"
input_file: tests/ui/run/comments_only.em
---
--- stdout ---
--- stderr ---
```

`crates/eml_cli/tests/snapshots/ui__check_fail@unexpected_character.em.snap`:

```
---
source: crates/eml_cli/tests/ui.rs
expression: rendered
input_file: tests/ui/check-fail/unexpected_character.em
---
[E0001] Error: unexpected character `$`
   ╭─[ check-fail/unexpected_character.em:2:1 ]
   │
 2 │ $
   │ ┬  
   │ ╰── not valid in eml source
───╯
```

`crates/eml_cli/tests/snapshots/ui__check_fail@stray_tokens.em.snap`:

```
---
source: crates/eml_cli/tests/ui.rs
expression: rendered
input_file: tests/ui/check-fail/stray_tokens.em
---
[E0003] Error: expected an item (`fn`, `type`, or `effect`)
   ╭─[ check-fail/stray_tokens.em:1:1 ]
   │
 1 │ 1 2 3
   │ ┬  
   │ ╰── not the start of an item
───╯
```

`crates/eml_cli/tests/snapshots/ui__check_fail@multiple_errors.em.snap`:

```
---
source: crates/eml_cli/tests/ui.rs
expression: rendered
input_file: tests/ui/check-fail/multiple_errors.em
---
[E0001] Error: unexpected character `$`
   ╭─[ check-fail/multiple_errors.em:2:1 ]
   │
 2 │ $
   │ ┬  
   │ ╰── not valid in eml source
───╯
[E0003] Error: expected an item (`fn`, `type`, or `effect`)
   ╭─[ check-fail/multiple_errors.em:3:1 ]
   │
 3 │ foo
   │ ─┬─  
   │  ╰─── not the start of an item
───╯
[E0001] Error: unexpected character `@`
   ╭─[ check-fail/multiple_errors.em:4:1 ]
   │
 4 │ @
   │ ┬  
   │ ╰── not valid in eml source
───╯
```

- [ ] **Step 6: スナップショットありで通ることを確認する**

Run: `cargo test -p eml_cli --test ui && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: 2 tests passed。clippy と fmt の指摘なし

- [ ] **Step 7: コミットする**

```bash
git add Cargo.lock crates/eml_cli tests/ui
git commit -m "Add the eml_cli library API and the UI test harness"
```

---

### Task 6: `eml` バイナリと CLI のテスト

**Files:**
- Modify: `crates/eml_cli/Cargo.toml` (全体を置き換える)
- Create: `crates/eml_cli/src/main.rs`
- Test: `crates/eml_cli/tests/cli.rs`

**Interfaces:**
- Consumes: Task 5 の `eml_cli::{check, run, RunConfig, OutputSink, RunResult}`。Task 1 の `SourceFiles`、`has_errors`、`render`
- Produces: バイナリ `eml` (`eml check <file>`、`eml run [--debug-heap] <file>`)。終了コード 0 / 1 / 2

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_cli/tests/cli.rs`:

```rust
//! `eml` バイナリの終了コードと引数の誤りの確認。

use std::process::{Command, Output};

fn eml(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_eml"))
        .args(args)
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/ui"))
        .output()
        .unwrap()
}

#[test]
fn check_succeeds_on_a_valid_file() {
    let output = eml(&["check", "run/empty.em"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
}

#[test]
fn check_fails_with_diagnostics() {
    let output = eml(&["check", "check-fail/unexpected_character.em"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("[E0001]"));
}

#[test]
fn run_succeeds_on_a_valid_file() {
    let output = eml(&["run", "--debug-heap", "run/empty.em"]);
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn run_does_not_execute_a_file_with_errors() {
    let output = eml(&["run", "check-fail/unexpected_character.em"]);
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn unreadable_file_is_a_usage_error() {
    let output = eml(&["check", "does/not/exist.em"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read `does/not/exist.em`"));
}

#[test]
fn missing_arguments_are_a_usage_error() {
    assert_eq!(eml(&[]).status.code(), Some(2));
    assert_eq!(eml(&["check"]).status.code(), Some(2));
    assert_eq!(eml(&["frobnicate"]).status.code(), Some(2));
}

#[test]
fn non_utf8_file_is_a_usage_error() {
    let path = std::env::temp_dir().join(format!("eml-cli-test-{}.em", std::process::id()));
    std::fs::write(&path, [0x66, 0x6e, 0xff, 0xfe]).unwrap();
    let output = eml(&["check", path.to_str().unwrap()]);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read"));
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p eml_cli --test cli`
Expected: コンパイルエラー (`environment variable CARGO_BIN_EXE_eml not defined`)

- [ ] **Step 3: バイナリを実装する**

`crates/eml_cli/Cargo.toml` を全体で置き換える (バイナリの名前を `eml` にし、clap を追加する):

```toml
[package]
name = "eml_cli"
version.workspace = true
edition.workspace = true

[[bin]]
name = "eml"
path = "src/main.rs"

[dependencies]
clap.workspace = true
eml_core_ir.workspace = true
eml_diagnostics.workspace = true
eml_hir.workspace = true
eml_interp.workspace = true
eml_runtime.workspace = true
eml_syntax.workspace = true
eml_types.workspace = true

[dev-dependencies]
insta.workspace = true
```

`crates/eml_cli/src/main.rs`:

```rust
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use eml_cli::{OutputSink, RunConfig, RunResult};
use eml_diagnostics::{FileId, SourceFiles, has_errors, render};

#[derive(Parser)]
#[command(name = "eml", version, about = "The eml programming language")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check a file and print diagnostics
    Check { file: PathBuf },
    /// Check a file and run it if there are no errors
    Run {
        /// Detect reference-count leaks and use-after-free
        #[arg(long)]
        debug_heap: bool,
        file: PathBuf,
    },
}

/// 終了コード: 0 = 成功、1 = 診断のエラーか実行時エラー、2 = 使い方の誤り (clap が引数の誤りで 2 を返す)。
fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Check { file } => {
            let Some((files, id)) = load(&file) else {
                return ExitCode::from(2);
            };
            let diagnostics = eml_cli::check(&files, id);
            eprint!("{}", render(&diagnostics, &files));
            if has_errors(&diagnostics) {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Command::Run { debug_heap, file } => {
            let Some((files, id)) = load(&file) else {
                return ExitCode::from(2);
            };
            let mut config = RunConfig::default();
            config.debug_heap = debug_heap;
            let outcome = eml_cli::run(&files, id, &config, OutputSink::stdout());
            eprint!("{}", render(&outcome.diagnostics, &files));
            match outcome.result {
                RunResult::Completed => ExitCode::SUCCESS,
                RunResult::NotRun => ExitCode::from(1),
                RunResult::RuntimeError(message) => {
                    eprintln!("runtime error: {message}");
                    ExitCode::from(1)
                }
            }
        }
    }
}

fn load(path: &Path) -> Option<(SourceFiles, FileId)> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let mut files = SourceFiles::new();
            let id = files.add(path.display().to_string(), text);
            Some((files, id))
        }
        Err(error) => {
            eprintln!("error: cannot read `{}`: {error}", path.display());
            None
        }
    }
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p eml_cli && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: cli の 7 tests と ui の 2 tests が通る。clippy と fmt の指摘なし

手でも確認する:

Run: `cargo run -q -p eml_cli -- check tests/ui/check-fail/multiple_errors.em; echo "exit=$?"`
Expected: 3 件の診断 (E0001、E0003、E0001) が stderr に表示され、`exit=1`

- [ ] **Step 5: コミットする**

```bash
git add Cargo.lock crates/eml_cli
git commit -m "Add the eml binary with check and run commands"
```

---

### Task 7: flake の更新と最終確認

**Files:**
- Modify: `flake.nix`

**Interfaces:**
- Consumes: Task 6 のバイナリ `eml` (`eml_cli` パッケージ)
- Produces: `nix build` で `bin/eml` ができる。`nix develop` で `cargo-insta`、`clippy`、`rustfmt` が使える

- [ ] **Step 1: flake を更新する**

`flake.nix` の `buildRustPackage` に `cargoBuildFlags` を足し、devShell の `packages` に `clippy`、`rustfmt`、`cargo-insta` を足す。変更後の全体:

```nix
{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-26.05";
    flake-parts.url = "github:hercules-ci/flake-parts";
  };
  outputs =
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-darwin"
      ];

      perSystem =
        { pkgs, ... }:
        {
          packages = rec {
            eml = pkgs.rustPlatform.buildRustPackage {
              pname = "eml";
              version = "0.0.0";
              src = ./.;
              cargoLock = {
                lockFile = ./Cargo.lock;
              };
              cargoBuildFlags = [
                "-p"
                "eml_cli"
              ];
            };
            default = eml;
          };

          devShells = {
            default = pkgs.mkShell {
              packages = with pkgs; [
                rust-analyzer
                clippy
                rustfmt
                cargo-insta
              ];
              nativeBuildInputs = with pkgs; [
                cargo
                rustc
              ];
              buildInputs = [ ];
              checkInputs = [ ];
              doCheck = false;
            };
          };
        };
    };
}
```

`.gitignore` にはすでに `.direnv/` があるので変更しない。

- [ ] **Step 2: Nix でビルドとテストができることを確認する**

flake は git が追跡しているファイルしか見ないので、先に `git add flake.nix` しておく。

Run: `git add flake.nix && nix build .#eml --print-out-paths && ls result/bin`
Expected: ビルドが成功し (`buildRustPackage` の check フェーズで workspace のテストも走る)、`eml` が表示される

Run: `nix develop -c sh -c 'cargo insta --version && cargo clippy --version && rustfmt --version'`
Expected: 3 つのツールのバージョンが表示される

- [ ] **Step 3: 最終確認**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: すべて通る

Run: `cargo run -q -p eml_cli -- check tests/ui/run/empty.em; echo "exit=$?"`
Expected: 何も表示されず `exit=0` (spec §9 の「空のファイルに対する `eml check` が診断 0 件で終了する」)

- [ ] **Step 4: コミットする**

```bash
rm -f result
git add flake.nix
git commit -m "Build eml_cli from the workspace in the Nix flake"
```
