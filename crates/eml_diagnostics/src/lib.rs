mod render;
mod source;

use std::fmt;

pub use render::render;
pub use source::{FileId, LineCol, SourceFiles};
pub use text_size::{TextRange, TextSize};

/// 番号の範囲は段階ごとに分けている (docs/spec/diagnostics.md)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ErrorCode(pub u16);

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "E{:04}", self.0)
    }
}

/// E0004 (未対応)。構文、HIR、型検査のどの段階でも「後の段階で実装する」という同じ意味で使うので、段階ごとの
/// `codes` ではなくここに置く (docs/spec/diagnostics.md)。
pub const NOT_YET_SUPPORTED: ErrorCode = ErrorCode(4);

/// E0004 のラベル。どの段階でも同じ文言にする。
pub const NOT_YET_SUPPORTED_LABEL: &str = "this is implemented in a later stage";

/// 補足の情報は独立した診断にせず、`notes` と `help` に入れる (docs/spec/diagnostics.md)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    Error,
    Warning,
}

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

    pub fn not_yet_supported(file: FileId, range: TextRange, message: impl Into<String>) -> Self {
        Self::error(
            NOT_YET_SUPPORTED,
            message,
            Label::new(file, range, NOT_YET_SUPPORTED_LABEL),
        )
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

    pub fn with_fix(mut self, edits: Vec<TextEdit>) -> Self {
        self.fix = Some(edits);
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

pub fn has_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics.iter().any(Diagnostic::is_error)
}

/// 表示する順に並べる。(ファイル、primary の開始位置、番号) の順で、3つとも同じなら段階が出した順を保つ。各段階は順を
/// 約束しないので、表示する側がこれを1回呼ぶ (docs/spec/diagnostics.md の「診断の順」)。
pub fn sort_diagnostics(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by_key(|d| (d.primary.file, d.primary.range.start(), d.code));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_is_zero_padded() {
        assert_eq!(ErrorCode(1).to_string(), "E0001");
        assert_eq!(ErrorCode(2105).to_string(), "E2105");
    }

    #[test]
    fn not_yet_supported_is_e0004_with_the_shared_label() {
        let mut files = SourceFiles::new();
        let file = files.add("a.em", "x");
        let range = TextRange::new(0.into(), 1.into());
        let diagnostic = Diagnostic::not_yet_supported(file, range, "lists are not supported yet");
        assert_eq!(diagnostic.code.to_string(), "E0004");
        assert_eq!(diagnostic.message, "lists are not supported yet");
        assert_eq!(diagnostic.primary.message, NOT_YET_SUPPORTED_LABEL);
        assert_eq!(
            NOT_YET_SUPPORTED_LABEL,
            "this is implemented in a later stage"
        );
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

    #[test]
    fn diagnostics_are_sorted_by_file_position_and_code() {
        let mut files = SourceFiles::new();
        let a = files.add("a.em", "0123456789");
        let b = files.add("b.em", "0123456789");
        let at = |file, start: u32, code: u16, message: &str| {
            Diagnostic::error(
                ErrorCode(code),
                message,
                Label::new(file, TextRange::new(start.into(), (start + 1).into()), ""),
            )
        };
        let mut diagnostics = vec![
            at(b, 0, 1, "b0"),
            at(a, 5, 2, "a5-2"),
            at(a, 5, 1, "a5-1 first"),
            at(a, 0, 9, "a0"),
            at(a, 5, 1, "a5-1 second"),
        ];
        sort_diagnostics(&mut diagnostics);
        let messages: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(messages, ["a0", "a5-1 first", "a5-1 second", "a5-2", "b0"]);
    }
}
