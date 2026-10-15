use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// insta の `glob!` は、失敗の数をすべてのスレッドで共有する1つのスタックの一番上に数え、終わりにも一番上を取り出す
/// (insta 1.49 の `glob.rs` と `runtime.rs` の `GLOB_STACK`)。そのため、glob のテストが並んで走ると、ほかのテストの
/// スナップショットの失敗を自分の失敗として報告する。UI テストとトランスクリプトのテストの glob は、どれもこのロックを
/// 持って1つずつ走らせる。
static GLOB: Mutex<()> = Mutex::new(());

/// 前に失敗したテストが毒したロックも使う。そのテストの失敗は、そのテスト自身が報告している。
pub fn glob_lock() -> MutexGuard<'static, ()> {
    GLOB.lock().unwrap_or_else(PoisonError::into_inner)
}

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
