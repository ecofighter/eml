//! 巻き戻しの地点 (docs/superpowers/specs/2026-10-17-s13c-unwinding-design.md の「巻き戻しの表」)。VM のコード生成が、
//! 関数ごとの巻き戻しの表をここから作る。所有は verifier の所有の段の走査で求める。生きている変数がすべて所有されて
//! いるとは限らず、借りたフィールドが呼び出しをまたぐことがあるためである (docs/spec/lir.md の「所有を取る時点」)。
//! 走査を2つ持たないように、verifier の走査が地点ごとの所有を書き留める。

use eml_extern::Purity;

use crate::{BlockId, LirFn, Prim, Program, Rhs, Stmt, VarId, verify};

/// 巻き戻しの地点。中断がこの命令の中を通ると、フレームは `owned` の参照を手放すか、`landing` に着地する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnwindPoint {
    pub block: BlockId,
    /// `block.stmts` の添字。
    pub stmt: usize,
    /// 命令の直後に所有している RC の対象の変数と、その参照の数。命令が束縛する変数は、命令が戻らなければまだない
    /// ので含めない。`VarId` の昇順で、数は 1 以上である。
    pub owned: Vec<(VarId, u32)>,
    /// `apply` の着地の印の変数。
    pub landing: Option<VarId>,
}

/// 関数の巻き戻しの地点を、ブロックの順、文の順に返す。地点は末尾でない呼び出し、`Effectful` の extern、
/// `prim yield_abort` である。入力は `verify` を通る Perceus の後の LIR でなければならず、通らなければ panic する。
/// VM はリリースのビルドでも呼ぶので、debug のビルドだけの検査には頼らない。
pub fn points(program: &Program, function: &LirFn) -> Vec<UnwindPoint> {
    verify::unwind_points(program, function).unwrap_or_else(|message| {
        panic!(
            "unwind points need verified LIR: {message} in `{}`",
            function.name
        )
    })
}

/// 文が巻き戻しの地点なら、文が束縛する変数と着地の印を返す。
pub(crate) fn point(stmt: &Stmt) -> Option<(VarId, Option<VarId>)> {
    let Stmt::Let { var, rhs } = stmt else {
        return None;
    };
    let landing = match rhs {
        Rhs::Call(call) => call.prompt(),
        Rhs::Extern { ext, .. } if ext.row().purity == Purity::Effectful => None,
        Rhs::Prim(Prim::YieldAbort, _) => None,
        _ => return None,
    };
    Some((*var, landing))
}
