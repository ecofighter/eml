use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use eml_hir::{ModulePath, ModuleSource, ReadError};

/// 根のディレクトリからモジュールのファイルを読む。IO を持つのはこの実装だけで、読み込みの段は同じ読み方を渡せば
/// 同じ結果を返す。
pub struct FsProvider {
    root: PathBuf,
}

impl FsProvider {
    pub fn new(root: impl Into<PathBuf>) -> FsProvider {
        let root = root.into();
        // `Path::parent` は `main.em` に空のパスを返し、空のパスは `read_dir` で読めない
        let root = if root.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            root
        };
        FsProvider { root }
    }
}

impl ModuleSource for FsProvider {
    fn read(&self, path: &ModulePath) -> Result<String, ReadError> {
        let mut file = self.root.clone();
        for name in path.file_path().split('/') {
            if !has_entry(&file, name)? {
                return Err(ReadError::NotFound);
            }
            file.push(name);
        }
        fs::read_to_string(&file).map_err(|error| ReadError::Unreadable(error.to_string()))
    }
}

/// ディレクトリの一覧の名前と1文字ずつ比べる。macOS のように大文字小文字を区別しないファイルシステムで、
/// `import Main` が `main.em` に当たらないようにするため。
fn has_entry(dir: &Path, name: &str) -> Result<bool, ReadError> {
    match fs::read_dir(dir) {
        Ok(entries) => Ok(entries.flatten().any(|entry| entry.file_name() == name)),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(false)
        }
        Err(error) => Err(ReadError::Unreadable(error.to_string())),
    }
}
