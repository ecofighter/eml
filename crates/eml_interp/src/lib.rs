//! Core IR を CEK 機械で実行する。
//!
//! 最初の段階では仮実装で、何も実行せずに正常終了する。中身は後の段階で TDD で実装する。

use std::fmt;
use std::sync::Arc;

use eml_core_ir::Program;
use eml_runtime::OutputSink;

/// 実行の設定。フィールドを後から足せるように `non_exhaustive` にする
/// (`RunConfig::default()` から作り、フィールドを代入して使う)。
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct RunConfig {
    /// RC のリーク検出と解放済みアクセスの検出を有効にする。
    pub debug_heap: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError(pub String);

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RuntimeError {}

pub fn run(
    _program: Arc<Program>,
    _config: &RunConfig,
    _out: &OutputSink,
) -> Result<(), RuntimeError> {
    Ok(())
}
