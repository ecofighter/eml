//! 巻き戻しの地点 (docs/spec/lir.md の「巻き戻しの地点」)。VM のコード生成が、
//! 関数ごとの巻き戻しの表をここから作る。所有は verifier の所有の段の走査で求める。生きている変数がすべて所有されて
//! いるとは限らず、借りたフィールドが呼び出しをまたぐことがあるためである (docs/spec/lir.md の「所有を取る時点」)。
//! 走査を2つ持たないように、verifier の走査が地点ごとの所有を書き留める。

use eml_extern::CFunction;

use crate::{BlockId, FnIdx, Prim, Program, Rhs, Stmt, VarId, VerifyError, verify};

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

/// 関数の巻き戻しの表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unwinds {
    /// 関数のエントリで所有しているRCの対象の引数と、その参照の数 (どれも1)。貸された引数は呼んだ側が持つので
    /// 入らない。エントリのフレームは借りたフィールドもtokenも着地も持たないので、これだけで巻き戻しの行になる。
    /// `VarId`の昇順である。
    pub entry: Vec<(VarId, u32)>,
    /// ブロックの順、文の順に並ぶ地点。
    pub points: Vec<UnwindPoint>,
}

/// 関数ごとの巻き戻しの表。`FnIdx` の番号で引く。地点は、すべての関数の末尾でない呼び出し、末尾でない `apply` と
/// `clause` と `par`、`Pure` でない extern (誤りが `try_io` か `try_panic` まで巻き戻す)、`prim yield_abort` と
/// `prim child_abort` である。呼ばれる側が中断するかは問わない。所有の段の verifier が拒む関数は、表の代わりにその
/// 誤りを返す。VM は verifier を通さない IR も読み込むので (VM 自身の見張りを確かめるテスト)、panic せず、拒まれた
/// 関数を表のない関数として扱えるようにする。`wanted` でない関数は求めずに `None` にする。VM の追記は新しい関数の
/// 表だけを要るので、すでにある関数の走査を省く。プログラム全体を1回で求めるのは、検査の表 (関数ごとの要約) を作り
/// 直さないためである。
pub fn points(
    program: &Program,
    wanted: impl Fn(FnIdx) -> bool,
) -> Vec<Option<Result<Unwinds, VerifyError>>> {
    verify::unwind_points(program, wanted)
}

/// 文が巻き戻しの地点なら、文が束縛する変数と着地の印を返す。複数の値を返す呼び出しは、葉をすべて束縛する
/// (docs/spec/vm.md の「巻き戻しの表」)。
pub(crate) fn point<'a>(stmt: &'a Stmt, c: &[CFunction]) -> Option<(&'a [VarId], Option<VarId>)> {
    match stmt {
        Stmt::Let { var, rhs } => {
            let (point, landing) = match rhs {
                Rhs::Call(call) => (true, call.prompt()),
                Rhs::Extern { ext, .. } => (ext.row(c).faults, None),
                Rhs::Prim(Prim::YieldAbort | Prim::ChildAbort, _) => (true, None),
                _ => (false, None),
            };
            point.then_some((std::slice::from_ref(var), landing))
        }
        Stmt::LetMany { vars, call } => Some((&vars[..], call.prompt())),
        _ => None,
    }
}
