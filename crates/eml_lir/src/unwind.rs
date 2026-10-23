//! 巻き戻しの地点 (docs/spec/vm.md の「巻き戻しの表」)。VM のコード生成が、
//! 関数ごとの巻き戻しの表をここから作る。所有は verifier の所有の段の走査で求める。生きている変数がすべて所有されて
//! いるとは限らず、借りたフィールドが呼び出しをまたぐことがあるためである (docs/spec/lir.md の「所有を取る時点」)。
//! 走査を2つ持たないように、verifier の走査が地点ごとの所有を書き留める。

use eml_extern::Purity;

use crate::{BlockId, Call, FnIdx, Prim, Program, Rhs, Stmt, Term, VarId, VerifyError, verify};

/// 巻き戻しの地点。中断がこの命令の中を通ると、フレームは `owned` の参照と `tokens` を手放すか、`landing` に着地する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnwindPoint {
    pub block: BlockId,
    /// `block.stmts` の添字。
    pub stmt: usize,
    /// 命令の直後に所有している RC の対象の変数と、その参照の数。命令が束縛する変数は、命令が戻らなければまだない
    /// ので含めない。`VarId` の昇順で、数は 1 以上である。
    pub owned: Vec<(VarId, u32)>,
    /// 命令の直後に生きている token の変数。`VarId` の昇順である。token は RC の対象でないので `owned` に入れず、
    /// 巻き戻しでは `drop_token` で手放す (docs/spec/object-model.md の「reuse」)。
    pub tokens: Vec<VarId>,
    /// `apply` の着地の印の変数。
    pub landing: Option<VarId>,
}

/// 関数ごとの巻き戻しの地点。`FnIdx` の番号で引き、各関数の地点はブロックの順、文の順に並ぶ。地点は、中断しうる
/// 関数 (`may_abort`) の末尾でない呼び出し、末尾でない `apply` と `clause`、`Pure` でない extern (誤りが `try_io` か
/// `try_panic` まで巻き戻す)、`prim yield_abort` である。所有の段の verifier が拒む関数は、地点の代わりにその
/// 誤りを返す。VM は verifier を通さない IR も読み込むので (VM 自身の見張りを確かめるテスト)、panic せず、拒まれた
/// 関数を表のない関数として扱えるようにする。プログラム全体を1回で求めるのは、検査の表 (関数ごとの要約) を作り直さない
/// ためである。
pub fn points_all(program: &Program) -> Vec<Result<Vec<UnwindPoint>, VerifyError>> {
    verify::unwind_points(program)
}

/// 文が巻き戻しの地点なら、文が束縛する変数と着地の印を返す。複数の値を返す呼び出しは、葉をすべて束縛する
/// (docs/spec/vm.md の「巻き戻しの表」)。`aborts` は `may_abort` の
/// 結果である。中断しない関数の呼び出しは、中断が通らないので地点にしない。
pub(crate) fn point<'s>(stmt: &'s Stmt, aborts: &[bool]) -> Option<(&'s [VarId], Option<VarId>)> {
    let (vars, abort, landing) = match stmt {
        Stmt::Let { var, rhs } => {
            let landing = match rhs {
                Rhs::Call(call) => call.prompt(),
                _ => None,
            };
            (std::slice::from_ref(var), rhs_abort(rhs), landing)
        }
        Stmt::LetMany { vars, call } => (&vars[..], call_abort(call), call.prompt()),
        _ => return None,
    };
    let point = match abort {
        Abort::Never => false,
        Abort::Always => true,
        Abort::If(callee) => aborts[callee.0 as usize],
    };
    point.then_some((vars, landing))
}

/// 関数ごとに、中断しうるか (`never` の操作、`IO` の失敗、`exit`、`Panic`) を `FnIdx` の番号で返す。最小の不動点で、
/// 中断する文を持たない相互再帰は中断しない (docs/spec/lir.md の「巻き戻しの地点」)。中断しうる関数を呼ぶ関数を、
/// 呼ぶ側への辺でたどって印を広げる。
pub fn may_abort(program: &Program) -> Vec<bool> {
    let count = program.functions.len();
    let mut aborts = vec![false; count];
    let mut callers: Vec<Vec<usize>> = vec![Vec::new(); count];
    let mut work = Vec::new();
    for (caller, function) in program.functions.iter().enumerate() {
        for block in &function.blocks {
            let rhss = block.stmts.iter().filter_map(|stmt| match stmt {
                Stmt::Let { var: _, rhs } => Some(rhs_abort(rhs)),
                Stmt::LetMany { vars: _, call } => Some(call_abort(call)),
                _ => None,
            });
            let tail = match &block.term {
                Term::TailCall(call) => Some(call_abort(call)),
                _ => None,
            };
            for abort in rhss.chain(tail) {
                match abort {
                    Abort::Never => {}
                    Abort::Always => {
                        if !aborts[caller] {
                            aborts[caller] = true;
                            work.push(caller);
                        }
                    }
                    Abort::If(callee) => callers[callee.0 as usize].push(caller),
                }
            }
        }
    }
    while let Some(callee) = work.pop() {
        for &caller in &callers[callee] {
            if !aborts[caller] {
                aborts[caller] = true;
                work.push(caller);
            }
        }
    }
    aborts
}

/// 命令の中を中断が通りうるか。
enum Abort {
    Never,
    Always,
    /// 呼ぶ関数が中断しうるときだけ通る。
    If(FnIdx),
}

fn rhs_abort(rhs: &Rhs) -> Abort {
    match rhs {
        Rhs::Call(call) => call_abort(call),
        Rhs::Extern { ext, .. } if ext.row().purity != Purity::Pure => Abort::Always,
        Rhs::Prim(Prim::YieldAbort | Prim::ChildAbort, _) => Abort::Always,
        _ => Abort::Never,
    }
}

/// `apply` と `clause` は、呼ぶ相手によらず中断しうる。すぐに再開する節は操作を起こしたフレームの上でその場で動く
/// ので、row が `<State>` のような閉包の呼び出しにも、節の本体の `panic` や IO の失敗が通るためである。`par` は、子の
/// 中断を親で起こし直すので中断しうる。
fn call_abort(call: &Call) -> Abort {
    match call {
        Call::Direct(callee, _) => Abort::If(*callee),
        Call::Apply { .. } | Call::Clause { .. } | Call::Par { .. } => Abort::Always,
    }
}
