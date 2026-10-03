use std::fmt;
use std::sync::Arc;

use eml_core_ir::Program;
use eml_runtime::OutputSink;

/// 将来 `threads` などを足しても呼び出し側を壊さないように、`non_exhaustive` にして `RunConfig::default()` から作らせる。
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct RunConfig {
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
