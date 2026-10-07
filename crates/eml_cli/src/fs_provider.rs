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
            find_entry(&file, name)?;
            file.push(name);
        }
        fs::read_to_string(&file).map_err(|error| ReadError::Unreadable(error.to_string()))
    }
}

/// ディレクトリの一覧の名前と1文字ずつ比べる。macOS のように大文字小文字を区別しないファイルシステムで、
/// `import Report.Csv` が `report/Csv.em` に当たらないようにするため (docs/spec/modules.md の「モジュール」)。
/// 大文字小文字だけが違う名前があれば、ないと言うより実際の名前を示すほうが直しやすいので、読めない理由として返す。
fn find_entry(dir: &Path, name: &str) -> Result<(), ReadError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            return Err(ReadError::NotFound);
        }
        Err(error) => return Err(ReadError::Unreadable(error.to_string())),
    };
    let lowered = name.to_lowercase();
    let mut differently_cased = None;
    for entry in entries {
        let actual = entry
            .map_err(|error| ReadError::Unreadable(error.to_string()))?
            .file_name();
        if actual == name {
            return Ok(());
        }
        if let Some(actual) = actual
            .to_str()
            .filter(|actual| actual.to_lowercase() == lowered)
        {
            differently_cased = Some(actual.to_string());
        }
    }
    match differently_cased {
        Some(actual) => Err(ReadError::Unreadable(format!(
            "the file is named `{actual}`, and module paths match the case of file names exactly"
        ))),
        None => Err(ReadError::NotFound),
    }
}
