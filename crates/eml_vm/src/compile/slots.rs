//! スロットの割り当て (docs/spec/vm.md の「スロットの割り当て」)。入口の引数をスロット 0.. に固定する。

use eml_lir::LirFn;

use crate::code::CompileError;
use crate::op::Slot;

pub(crate) struct Slots {
    /// 変数の番号ごとのスロット。
    pub(crate) of: Vec<Slot>,
    /// 変数が使うスロットの数。一時のスロットはこの後に置く。
    pub(crate) colors: u16,
}

/// 入口の引数をスロット 0.. に、ほかの変数をその後に1つずつ置く。
pub(crate) fn allocate(function: &LirFn) -> Result<Slots, CompileError> {
    let mut of: Vec<Option<usize>> = vec![None; function.vars.len()];
    let mut next = 0;
    for param in function.params() {
        of[param.0 as usize] = Some(next);
        next += 1;
    }
    for slot in &mut of {
        if slot.is_none() {
            *slot = Some(next);
            next += 1;
        }
    }
    let too_many = || CompileError::TooManySlots {
        function: function.name.clone(),
    };
    let of = of
        .into_iter()
        .map(|slot| {
            Slot::try_from(slot.expect("every variable has a slot")).map_err(|_| too_many())
        })
        .collect::<Result<_, _>>()?;
    Ok(Slots {
        of,
        colors: u16::try_from(next).map_err(|_| too_many())?,
    })
}
