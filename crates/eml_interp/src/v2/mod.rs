//! 基本ブロックの列の Core IR (`eml_core_ir::v2`) を実行する機械。パイプラインを切り替えるときに、今の機械と
//! 置き換える。

mod machine;

use eml_core_ir::v2::Program;
use eml_runtime::OutputSink;

use crate::{RunConfig, RunStats, RuntimeError, finish};

use machine::Machine;

pub fn run(
    program: &Program,
    config: &RunConfig,
    out: &OutputSink,
) -> Result<RunStats, RuntimeError> {
    let mut machine = Machine::new(program, out, &config.file_root);
    machine.run()?;
    finish(&machine.rt, config)
}
