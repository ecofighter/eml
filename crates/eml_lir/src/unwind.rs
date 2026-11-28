//! 巻き戻しの地点 (docs/spec/lir.mdの「巻き戻しの地点」)。`Trusted`が関数ごとの巻き戻しの表をここで作り、
//! エンジンはそれを`Trusted`から読む。所有はverifierの所有の段の走査で求める。生きている変数がすべて所有されているとは
//! 限らず、借りたフィールドが呼び出しをまたぐことがあるためである (docs/spec/lir.mdの「所有を取る時点」)。走査を2つ
//! 持たないように、verifierの走査が地点ごとの所有を書き留める。

use eml_extern::CFunction;

use crate::{BlockId, LirFn, Prim, Program, Rhs, Stmt, VarId, VerifyError, verify};

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

/// 関数ごとの巻き戻しの表。`FnIdx`の番号で引く。地点は、すべての関数の末尾でない呼び出し、末尾でない`apply`と
/// `clause`、`Pure`でないextern (誤りが`try_io`か`try_panic`まで巻き戻す)、`prim yield_abort`である。呼ばれる側が
/// 中断するかは問わない。関数を1つずつ確かめ、プログラム全体の規則は
/// 確かめない。所有の段で拒まれた関数は表の代わりにその誤りを返し、ほかの関数は表を返す。
/// `Trusted::unverified`が使う。プログラム全体を1回で求めるのは、検査の表 (関数ごとの要約) を作り直さないためである。
pub(crate) fn points(program: &Program) -> Vec<Result<Unwinds, VerifyError>> {
    verify::unwind_points(program)
}

/// プログラム全体を所有の段で確かめ、関数ごとの巻き戻しの表を`program.functions`の順に返す。
/// `eml_lir::verify`が拒むプログラムは、表の代わりにその誤りを返す。検査と表の記録は関数ごとに1回の走査で行う
/// (docs/spec/lir.mdの「verifier」)。
pub(crate) fn verified(program: &Program) -> Result<Vec<Unwinds>, VerifyError> {
    verify::verified(program)
}

/// 所有の段の走査が拒む関数の表。エントリの行は空で、巻き戻しの地点ごとに、何も手放さず文の着地の印を持つ行を
/// 持つ。中断がそういう関数のフレームを通っても、フレームは何も手放さない。これはいつも記憶の安全を守り、フレームが
/// 持っていたものは手放されずに残る (docs/spec/lir.mdの「巻き戻しの地点」)。
pub(crate) fn unowned(function: &LirFn, c: &[CFunction]) -> Unwinds {
    let mut points = Vec::new();
    for (index, block) in function.blocks.iter().enumerate() {
        for (at, stmt) in block.stmts.iter().enumerate() {
            if let Some((_, landing)) = point(stmt, c) {
                points.push(UnwindPoint {
                    block: BlockId(index as u32),
                    stmt: at,
                    owned: Vec::new(),
                    tokens: Vec::new(),
                    landing,
                });
            }
        }
    }
    Unwinds {
        entry: Vec::new(),
        points,
    }
}

/// 文が巻き戻しの地点なら、文が束縛する変数と着地の印を返す。複数の値を返す呼び出しは、葉をすべて束縛する
/// (docs/spec/vm.md の「巻き戻しの表」)。
pub(crate) fn point<'a>(stmt: &'a Stmt, c: &[CFunction]) -> Option<(&'a [VarId], Option<VarId>)> {
    match stmt {
        Stmt::Let { var, rhs } => {
            let (point, landing) = match rhs {
                Rhs::Call(crate::Call::Par { .. }) => (false, None),
                Rhs::Call(call) => (true, call.prompt()),
                Rhs::Extern { ext, .. } => (ext.row(c).faults, None),
                Rhs::Prim(Prim::YieldAbort, _) => (true, None),
                _ => (false, None),
            };
            point.then_some((std::slice::from_ref(var), landing))
        }
        Stmt::LetMany { vars, call } => Some((&vars[..], call.prompt())),
        _ => None,
    }
}
