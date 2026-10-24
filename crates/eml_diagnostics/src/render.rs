use std::collections::HashMap;
use std::fmt;
use std::ops::Range;

use ariadne::{Cache, Config, IndexType, Label as AriadneLabel, Report, ReportKind, Source};

use crate::{Diagnostic, FileId, Label, Severity, SourceFiles};

/// UI テストのスナップショットにも使うので、色を付けない。
pub fn render(diagnostics: &[Diagnostic], files: &SourceFiles) -> String {
    let mut out = Vec::new();
    let mut sources = Sources {
        files,
        loaded: HashMap::new(),
    };
    for diagnostic in diagnostics {
        let kind = match diagnostic.severity {
            Severity::Error => ReportKind::Error,
            Severity::Warning => ReportKind::Warning,
        };
        let mut report = Report::build(kind, span(&diagnostic.primary, files))
            .with_config(
                Config::default()
                    .with_color(false)
                    .with_index_type(IndexType::Char),
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
        report
            .finish()
            .write(&mut sources, &mut out)
            .expect("writing to a Vec cannot fail");
    }
    String::from_utf8(out).expect("ariadne writes UTF-8")
}

/// ariadne の読み口。ariadne はファイルごとに行の表を作るので、診断が指すファイルの表だけを、最初に指したときに作る。
/// 読み込んだファイルをすべて写すと、診断が1件でも標準ライブラリのすべての表を作るためである。
struct Sources<'a> {
    files: &'a SourceFiles,
    loaded: HashMap<FileId, Source<&'a str>>,
}

impl<'a> Cache<FileId> for Sources<'a> {
    type Storage = &'a str;

    fn fetch(&mut self, id: &FileId) -> Result<&Source<&'a str>, impl fmt::Debug> {
        let files = self.files;
        Ok::<_, ()>(
            self.loaded
                .entry(*id)
                .or_insert_with(|| Source::from(files.text(*id))),
        )
    }

    fn display<'b>(&self, id: &'b FileId) -> Option<impl fmt::Display + 'b> {
        Some(self.files.path(*id).to_string())
    }
}

fn span(label: &Label, files: &SourceFiles) -> (FileId, Range<usize>) {
    // ariadne は主でない側のファイルの見出しの位置を、バイト指定でも文字の番号として読むので、文字の番号で渡す
    let chars = |offset| files.char_index(label.file, offset);
    (
        label.file,
        chars(label.range.start())..chars(label.range.end()),
    )
}

fn label(label: &Label, files: &SourceFiles) -> AriadneLabel<(FileId, Range<usize>)> {
    let result = AriadneLabel::new(span(label, files));
    if label.message.is_empty() {
        result
    } else {
        result.with_message(&label.message)
    }
}

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
    fn renders_several_notes_and_a_help() {
        let mut files = SourceFiles::new();
        let file = files.add("a.em", "x = $");
        let range = TextRange::new(4.into(), 5.into());
        let diagnostic = Diagnostic::error(ErrorCode(1), "bad", Label::new(file, range, "here"))
            .with_note("first note")
            .with_note("second note")
            .with_help("a help");
        let text = render(&[diagnostic], &files);
        // E3001 などは note を2つ持ち、help と併せて出す。その番号付けと並びは UI テストに代表がないので、ここで確かめる。
        let at = |needle: &str| {
            text.find(needle)
                .unwrap_or_else(|| panic!("{needle} in {text}"))
        };
        assert!(at("Help: a help") < at("Note 1: first note"), "{text}");
        assert!(
            at("Note 1: first note") < at("Note 2: second note"),
            "{text}"
        );
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
