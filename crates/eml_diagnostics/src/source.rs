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
    files: Vec<SourceFile>,
}

#[derive(Debug)]
struct SourceFile {
    path: String,
    text: String,
    /// 各行の先頭の位置で、最初はいつも 0 である。`line_col` は1つのファイルに何度も呼ばれるので、呼ばれるたびに
    /// ファイルの先頭から行を数えずに済むよう、読み込み時に1回だけ作る。
    line_starts: Vec<TextSize>,
}

impl SourceFiles {
    pub fn new() -> Self {
        Self::default()
    }

    /// 先頭の BOM は読み込み時に除く。以後の位置 (`TextRange`、レイアウトの列、診断の行と列) は、すべて BOM を除いた
    /// テキストで数える (docs/spec/lexical.md)。
    pub fn add(&mut self, path: impl Into<String>, text: impl Into<String>) -> FileId {
        let id = FileId(u32::try_from(self.files.len()).expect("too many source files"));
        let mut text = text.into();
        if text.starts_with('\u{feff}') {
            text.drain(..'\u{feff}'.len_utf8());
        }
        let line_starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(newline, _)| newline + 1))
            .map(|start| TextSize::try_from(start).expect("source file too large"))
            .collect();
        self.files.push(SourceFile {
            path: path.into(),
            text,
            line_starts,
        });
        id
    }

    pub fn path(&self, id: FileId) -> &str {
        &self.files[id.0 as usize].path
    }

    pub fn text(&self, id: FileId) -> &str {
        &self.files[id.0 as usize].text
    }

    /// 位置を行と列にする。`offset` は、このファイルのテキストの中の文字の境界でなければならない。
    pub fn line_col(&self, file: FileId, offset: TextSize) -> LineCol {
        let file = &self.files[file.0 as usize];
        // `line_starts` の最初は 0 なので、`offset` 以下の先頭は少なくとも1つあり、その数が1始まりの行になる。
        let line = file.line_starts.partition_point(|&start| start <= offset);
        let line_start = file.line_starts[line - 1];
        let column = file.text[usize::from(line_start)..usize::from(offset)]
            .chars()
            .count();
        LineCol {
            line: u32::try_from(line).expect("too many lines"),
            column: u32::try_from(column + 1).expect("line too long"),
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.files
            .iter()
            .map(|file| (file.path.as_str(), file.text.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

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
    fn add_strips_only_a_leading_bom() {
        let mut files = SourceFiles::new();
        let file = files.add("a.em", "\u{feff}a\u{feff}");
        assert_eq!(files.text(file), "a\u{feff}");
        // BOM は読み込み時に除くので (docs/spec/lexical.md)、位置は BOM を除いたテキストで数える。
        assert_eq!(position("\u{feff}ab", 1), "1:2");
        assert_eq!(position("\u{feff}a\nb", 2), "2:1");
    }

    #[test]
    fn line_col_finds_every_line_of_a_large_file() {
        // 呼ぶたびにファイルの先頭から数えると、行の数の2乗の時間がかかる。行の先頭の表を二分探索する実装なら
        // debug ビルドでも1秒かからない。2乗の実装でテストが長く止まらないよう、上限を超えたところで失敗にする。
        const LINE: &str = "αβ x\n";
        const LIMIT: Duration = Duration::from_secs(5);
        let lines = 50_000;
        let mut files = SourceFiles::new();
        let file = files.add("a.em", LINE.repeat(lines));
        let at = |offset: usize| files.line_col(file, TextSize::try_from(offset).unwrap());
        let start = Instant::now();
        for index in 0..lines {
            let line = u32::try_from(index + 1).unwrap();
            let line_start = index * LINE.len();
            assert_eq!(at(line_start), LineCol { line, column: 1 });
            // `α` と `β` は2バイトずつなので、`x` は行の先頭からバイト位置 5、4文字目にある。
            assert_eq!(at(line_start + 5), LineCol { line, column: 4 });
            assert_eq!(at(line_start + 6), LineCol { line, column: 5 });
            let elapsed = start.elapsed();
            assert!(elapsed < LIMIT, "took {elapsed:?} up to line {line}");
        }
        assert_eq!(
            at(LINE.len() * lines),
            LineCol {
                line: 50_001,
                column: 1
            }
        );
    }
}
