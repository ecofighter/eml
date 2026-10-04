use std::fmt;

use text_size::TextSize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(u32);

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

/// 将来クエリ化するときは salsa の入力に置き換える。
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

    /// 位置を行と列にする。ファイルの先頭の BOM は列に数えない (docs/spec/lexical.md)。
    pub fn line_col(&self, file: FileId, offset: TextSize) -> LineCol {
        let before = &self.text(file)[..usize::from(offset)];
        let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
        let mut column_text = &before[line_start..];
        if line_start == 0 {
            column_text = &column_text[bom_len(column_text)..];
        }
        LineCol {
            line: u32::try_from(before.matches('\n').count() + 1).expect("too many lines"),
            column: u32::try_from(column_text.chars().count() + 1).expect("line too long"),
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.files
            .iter()
            .map(|(path, text)| (path.as_str(), text.as_str()))
    }
}

/// ファイルの先頭の BOM のバイト数。BOM は列に数えないので (docs/spec/lexical.md)、行と列の計算と表示の両方が使う。
pub(crate) fn bom_len(text: &str) -> usize {
    const BOM: &str = "\u{feff}";
    if text.starts_with(BOM) { BOM.len() } else { 0 }
}

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
}
