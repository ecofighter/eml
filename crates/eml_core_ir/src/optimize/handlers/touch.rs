//! 状態を渡す形の経路に入れる関数を決める、H に触れるかの判断 (docs/spec/core-ir.md の「handler への特殊化」)。

use std::collections::HashMap;

use crate::{Call, CoreFn, FnRef, Program, Rhs, Stmt};
use eml_lir::FnIdx;

/// 関数ごとの、`effect` の handler H に触れるか。番号は `FnIdx` である。H に触れるとは、その関数か、直接の呼び出しで
/// たどれる関数が、H 宛ての `perform`、`mask` の付いた呼び出し、`handle`、`resume`、`clause`、`yields` の `apply` の
/// どれかを持つことである。触れない関数は H の evidence を引かないので、H の handle を特殊化して evidence がなくなって
/// も、そのまま動く。最小の固定点で、印は立つ方へだけ動くので、呼ばれる側が立つたびに呼ぶ側を見直す
/// (`recompute_yields` と同じ)。
pub(super) fn touching(program: &Program, effect: u32) -> Vec<bool> {
    let mut touches = vec![false; program.functions.len()];
    let mut callers: HashMap<FnIdx, Vec<FnIdx>> = HashMap::new();
    let mut pending = Vec::new();
    for (index, function) in program.functions.iter().enumerate() {
        let idx = FnIdx(index as u32);
        function.for_each_fn_ref(|callee, kind| {
            if kind == FnRef::Direct {
                callers.entry(callee).or_default().push(idx);
            }
        });
        if touches_locally(function, effect) {
            touches[index] = true;
            pending.push(idx);
        }
    }
    while let Some(callee) = pending.pop() {
        for &caller in callers.get(&callee).into_iter().flatten() {
            if !std::mem::replace(&mut touches[caller.0 as usize], true) {
                pending.push(caller);
            }
        }
    }
    touches
}

/// 呼ぶ相手を見ずに分かる、H に触れる文を持つか。
fn touches_locally(function: &CoreFn, effect: u32) -> bool {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.stmts)
        .any(|stmt| match stmt {
            Stmt::Let {
                var: _,
                rhs: Rhs::Call { call, mask },
            } => {
                !mask.is_empty()
                    || match call {
                        Call::Perform { effect: e, .. } => *e == effect,
                        Call::Handle { .. } | Call::Resume { .. } | Call::Clause { .. } => true,
                        Call::Apply { yields, .. } => *yields,
                        Call::Direct(..) => false,
                    }
            }
            _ => false,
        })
}
