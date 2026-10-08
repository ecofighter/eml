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

pub fn run(program: &Program, config: &RunConfig, out: &OutputSink) -> Result<(), RuntimeError> {
    let mut machine = Machine::new(program, out, &config.file_root);
    machine.run()?;
    // 実行時エラーで止まった場合はリークを数えない。途中のフレームが残っているのは当然だから
    if config.debug_heap {
        machine.check_leaks()?;
    }
    Ok(())
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
