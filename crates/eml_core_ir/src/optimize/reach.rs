//! 到達できない関数の除去。後続の書き換えが元の関数を参照されないまま残すので、EP の前に落として、EP と LIR が
//! 使われない関数を運ばないようにする。

use crate::Program;
use eml_lir::FnIdx;

/// `program.entry` から `Call::Direct`、`Rhs::MakeClosure`、`Atom::Fn` で届く関数だけを残し、元の順のまま
/// `FnIdx` を振り直す。
pub(crate) fn retain_reachable(program: &mut Program) {
    let count = program.functions.len();
    let mut reached = vec![false; count];
    let mut pending = vec![program.entry];
    while let Some(function) = pending.pop() {
        if std::mem::replace(&mut reached[function.0 as usize], true) {
            continue;
        }
        program.functions[function.0 as usize].for_each_fn_ref(|target, _| pending.push(target));
    }
    if reached.iter().all(|&kept| kept) {
        return;
    }

    let mut renumber = vec![FnIdx(0); count];
    let mut next = 0;
    for (index, &kept) in reached.iter().enumerate() {
        if kept {
            renumber[index] = FnIdx(next);
            next += 1;
        }
    }
    let mut index = 0;
    program.functions.retain(|_| {
        index += 1;
        reached[index - 1]
    });
    program.entry = renumber[program.entry.0 as usize];
    for function in &mut program.functions {
        function.for_each_fn_ref_mut(|target| *target = renumber[target.0 as usize]);
    }
}
