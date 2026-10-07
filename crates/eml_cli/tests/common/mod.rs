use std::fs;
use std::path::PathBuf;

/// 一時ディレクトリに `(根からの相対パス, 本文)` のファイルを置く。結合テストは1つのプロセスの中で並んで走るので、
/// `name` でディレクトリを分ける。
pub fn temp_project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("eml-cli-{name}-{}", std::process::id()));
    // 前の実行が途中で止まって残したファイルを読まないため
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    for (path, text) in files {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    dir
}
