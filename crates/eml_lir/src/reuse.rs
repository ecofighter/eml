//! reuse (docs/spec/object-model.md の「reuse」)。Perceus が手放す箱を token にし、同じ大きさの `con` がその箱を使う。

use crate::{Call, LirFn, Program, operation};

/// `function` の中の呼び出し `call` が yield しうるか。token はこの呼び出しをまたいで生きない。継続を2回再開すると、
/// 同じ箱を2つの `con` が使ってしまうためである。どの呼び出しを数えるかは、EP の yield の地点に合わせる
/// (docs/spec/core-ir.md の「yield の地点」)。`never` の操作の `clause` は戻らないので数えない。
pub fn may_yield(program: &Program, function: &LirFn, call: &Call) -> bool {
    function.yields
        && match call {
            Call::Direct(callee, _) => program.function(*callee).yields,
            Call::Apply { yields, .. } => *yields,
            Call::Clause { effect, op, .. } => operation(&program.effects, *effect, *op).resumable,
        }
}
