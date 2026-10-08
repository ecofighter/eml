//! `File` のオブジェクトの中身 (docs/spec/runtime.md)。破棄処理はオブジェクトの解放そのもので、読み出し口を捨てると
//! OS のファイルが閉じる。読み出し口を trait object にするのは、解放で捨てられることを単体テストで確かめるためである。

use std::fmt;
use std::io::Read;

pub struct FileHandle {
    /// `open` に渡したパス。実行時エラーの文言に使う。
    pub path: String,
    pub reader: Box<dyn Read>,
}

impl FileHandle {
    pub fn new(path: String, reader: Box<dyn Read>) -> FileHandle {
        FileHandle { path, reader }
    }
}

impl fmt::Debug for FileHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileHandle")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// `Payload` の比較はテストで中身を確かめるためにあり、読み出し口は比べられないのでパスだけを比べる。
impl PartialEq for FileHandle {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}
