//! LIR から `CodeBase` を作る (docs/spec/vm.md)。

mod constants;
mod emit;
mod moves;
mod slots;

use std::sync::Arc;

use eml_lir::{Loc, Program};
use eml_rt::{Control, Mode};

use crate::code::{CodeBase, CompileError};
use crate::op::{Op, Pc};
use constants::Constants;

/// 命令とオペランドを書く先。関数をまたいで1つにする。
#[derive(Default)]
pub(crate) struct Out {
    pub(crate) code: Vec<Op>,
    pub(crate) operands: Vec<u32>,
    pub(crate) effect_lists: Vec<u16>,
    pub(crate) locations: Vec<(Pc, Loc)>,
}

impl Out {
    pub(crate) fn pc(&self) -> Pc {
        Pc::try_from(self.code.len()).expect("the code fits in u32 positions")
    }

    pub(crate) fn push(&mut self, op: Op) -> Pc {
        let pc = self.pc();
        self.code.push(op);
        pc
    }

    /// オペランドの並びを足し、その先頭の添字を返す。
    pub(crate) fn operands(&mut self, items: impl IntoIterator<Item = u32>) -> u32 {
        let at = u32::try_from(self.operands.len()).expect("the operands fit in u32 positions");
        self.operands.extend(items);
        at
    }
}

/// プログラム全体を一度に変換する。呼び出しは関数の番号で指し、VM が `FnEntry` から先頭を引くので、関数の先頭の位置が
/// 決まる前に呼び出しを書ける。
pub fn compile<M: Mode>(
    program: &Program,
    control: Arc<Control>,
) -> Result<CodeBase<M>, CompileError> {
    let mut constants = Constants::<M>::new(control);
    let strings: Vec<u32> = program
        .strings
        .iter()
        .map(|text| constants.string(text))
        .collect();
    let mut out = Out::default();
    out.push(Op::Finish);
    out.push(Op::ApplyRest);
    let mut functions = Vec::with_capacity(program.functions.len());
    for function in &program.functions {
        functions.push(emit::function(
            program,
            function,
            &strings,
            &mut constants,
            &mut out,
        )?);
    }
    let (region, constants) = constants.freeze()?;
    Ok(CodeBase {
        functions,
        code: out.code,
        operands: out.operands,
        effect_lists: out.effect_lists,
        constants,
        region,
        locations: out.locations,
        files: program.files.clone(),
        entry: program.entry.0,
        entry_ret_rc: program.function(program.entry).ret.is_rc(),
    })
}
