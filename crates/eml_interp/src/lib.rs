use std::path::PathBuf;

use eml_core_ir::Program;
use eml_runtime::OutputSink;

mod effects;
mod error;
mod externs;
mod machine;

pub use error::{Fault, RuntimeError};

use machine::Machine;

/// フィールドを足しても呼び出し側を壊さないように、`non_exhaustive` にして `RunConfig::default()` から作らせる。
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct RunConfig {
    pub debug_heap: bool,
    /// `open` の相対パスの基準 (docs/spec/effects.md の「組み込みの `IO`」)。既定の空のパスはカレントディレクトリを指す。
    pub file_root: PathBuf,
}

impl RunConfig {
    pub fn with_debug_heap(mut self, debug_heap: bool) -> Self {
        self.debug_heap = debug_heap;
        self
    }

    pub fn with_file_root(mut self, root: PathBuf) -> Self {
        self.file_root = root;
        self
    }
}

/// 実行の仕事の回数。時間ではなく回数を比べて、実行の費用が入力の大きさに比例して伸びることをテストで確かめるために
/// 数える (docs/spec/runtime.md の「実行の API」)。数えるものを後で足しても呼び出し側を壊さないよう、`non_exhaustive`
/// にする。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RunStats {
    /// `perform` が handler を探すときに調べたフレームの数。
    pub handler_visits: u64,
    /// 文字列の物体の中身に書いたバイト数。`String` の容量の再確保と、出力への書き込みは数えない。
    pub string_bytes_copied: u64,
}

/// 実行時エラーとリークのときは `RunStats` を返さない。回数を比べるテストは、正常に終わった実行だけを見る。
pub fn run(
    program: &Program,
    config: &RunConfig,
    out: &OutputSink,
) -> Result<RunStats, RuntimeError> {
    let mut machine = Machine::new(program, out, &config.file_root);
    machine.run()?;
    // 実行時エラーで止まった場合はリークを数えない。途中のフレームが残っているのは当然だから
    if config.debug_heap {
        machine.check_leaks()?;
    }
    Ok(machine.stats())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_debug_heap_sets_the_flag() {
        assert!(RunConfig::default().with_debug_heap(true).debug_heap);
        assert!(!RunConfig::default().with_debug_heap(false).debug_heap);
    }
}
