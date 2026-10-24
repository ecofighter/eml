//! 書き換えた後の `yields` の印の付け直し (docs/spec/core-ir.md の「`yields` の印」と
//! docs/spec/core-ir.md の「`yields` の付け直し」)。

use std::collections::{HashMap, HashSet};

use crate::{Call, CoreFn, FnRef, Program, Rhs, Stmt};
use eml_lir::{Atom, FnIdx};

use super::known::{known_closures, wrapper};

/// 印を付け直さない関数。handle の本体、節、`return` の節の関数は、印の規則が本体の中身でなく handle で決まる
/// (docs/spec/core-ir.md の「`yields` の印」)。包む関数の形の関数は、値として使われる間だけ印を保つ。値はどの文脈
/// からも呼ばれ、ABI は名前で決まるためである。handle を特殊化した後の `return` の節のように、直接呼ばれるだけに
/// なった関数は、ほかの関数と同じく固定点で印を決める。
fn kept_marks(program: &Program) -> HashSet<FnIdx> {
    let values = value_references(program);
    let mut keep = HashSet::new();
    for (index, function) in program.functions.iter().enumerate() {
        let idx = FnIdx(index as u32);
        if wrapper(function).is_some() && values.contains(&idx) {
            keep.insert(idx);
        }
        let known = known_closures(function);
        let target = |atom: Atom| match atom {
            Atom::Fn(target) => Some(target),
            Atom::Var(var) => known.get(&var).map(|closure| closure.function),
            _ => None,
        };
        for stmt in function.blocks.iter().flat_map(|block| &block.stmts) {
            if let Stmt::Let {
                var: _,
                rhs:
                    Rhs::Call {
                        call:
                            Call::Handle {
                                effect: _,
                                init: _,
                                body,
                                clauses,
                                ret,
                            },
                        mask: _,
                    },
            } = stmt
            {
                let atoms = std::iter::once(*body).chain(clauses.iter().copied());
                keep.extend(atoms.chain([*ret]).filter_map(target));
            }
        }
    }
    keep
}

/// 値として使われる関数。`closure f(..)` と `&f` の関数で、直接の呼び出しの呼ぶ相手は含めない。
fn value_references(program: &Program) -> HashSet<FnIdx> {
    let mut values = HashSet::new();
    for function in &program.functions {
        function.for_each_fn_ref(|target, kind| {
            if kind == FnRef::Value {
                values.insert(target);
            }
        });
    }
    values
}

/// `yields` を最小の固定点で付け直す。translate の印が立ち、本体に yield の地点 (再開できる `perform`、`yields` の
/// `apply`、`resume`、`handle`、yield しうる関数の直接の呼び出し) が残る関数だけが yield しうる。印を付け直さない
/// 関数は translate の印のままにする (`kept_marks`)。印は立つ方へだけ動くので、呼ばれる側が立つたびに呼ぶ側を見直せば、関数と呼び出しの
/// 数に比例する手間で済む。
pub(crate) fn recompute_yields(program: &mut Program) {
    let keep = kept_marks(program);
    let count = program.functions.len();
    let mut yields = vec![false; count];
    let mut callers: HashMap<FnIdx, Vec<FnIdx>> = HashMap::new();
    let mut pending = Vec::new();
    for (index, function) in program.functions.iter().enumerate() {
        let idx = FnIdx(index as u32);
        let starts = if keep.contains(&idx) {
            function.yields
        } else if function.yields {
            function.for_each_fn_ref(|callee, kind| {
                if kind == FnRef::Direct {
                    callers.entry(callee).or_default().push(idx);
                }
            });
            yields_locally(function)
        } else {
            false
        };
        if starts {
            yields[index] = true;
            pending.push(idx);
        }
    }
    while let Some(callee) = pending.pop() {
        for &caller in callers.get(&callee).into_iter().flatten() {
            if !std::mem::replace(&mut yields[caller.0 as usize], true) {
                pending.push(caller);
            }
        }
    }
    for (function, yields) in program.functions.iter_mut().zip(yields) {
        function.yields = yields;
    }
}

/// 呼ぶ相手を見ずに分かる yield の地点を持つか。
fn yields_locally(function: &CoreFn) -> bool {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.stmts)
        .any(|stmt| match stmt {
            Stmt::Let {
                var: _,
                rhs: Rhs::Call { call, mask: _ },
            } => match call {
                Call::Perform { resumable, .. } => *resumable,
                Call::Apply { yields, .. } => *yields,
                Call::Resume { .. } | Call::Handle { .. } => true,
                Call::Direct(..) | Call::Clause { .. } => false,
            },
            _ => false,
        })
}
