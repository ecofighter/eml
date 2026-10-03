#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(u32);

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

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.files
            .iter()
            .map(|(path, text)| (path.as_str(), text.as_str()))
    }
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
}
