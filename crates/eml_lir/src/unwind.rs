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

/// 関数ごとの巻き戻しの表。`FnIdx`の番号で引く。地点は、すべての関数の末尾でない呼び出し、末尾でない`apply`と
/// `clause`と`par`、`Pure`でないextern (誤りが`try_io`か`try_panic`まで巻き戻す)、`prim yield_abort`と
/// `prim child_abort`である。呼ばれる側が中断するかは問わない。関数を1つずつ確かめ、プログラム全体の規則は
/// 確かめない。所有の段で拒まれた関数は表の代わりにその誤りを返し、ほかの関数は表を返す。verifierを通さないIRの
/// 地点を見るテストと、VMの確かめない読み込み (feature `testing`) のためである。`wanted`でない関数は求めずに`None`に
/// する。プログラム全体を1回で求めるのは、検査の表 (関数ごとの要約) を作り直さないためである。
pub fn points(
    program: &Program,
    wanted: impl Fn(FnIdx) -> bool,
) -> Vec<Option<Result<Unwinds, VerifyError>>> {
    verify::unwind_points(program, wanted)
}

/// プログラム全体を所有の段で確かめ、関数ごとの巻き戻しの表を`program.functions`の順に返す。
/// `eml_lir::verify`が拒むプログラムは、表の代わりにその誤りを返す。検査と表の記録は関数ごとに1回の走査で行う
/// (docs/spec/lir.mdの「verifier」)。
pub fn verified(program: &Program) -> Result<Vec<Unwinds>, VerifyError> {
    verify::verified(program, |_| true).map(|unwinds| {
        unwinds
            .into_iter()
            .map(|table| table.expect("every function is walked"))
            .collect()
    })
}

/// `verified`と同じくプログラム全体を所有の段で確かめるが、たどるのは`walked`の関数だけで、その表を
/// `program.functions`の欄に返す (ほかの欄は`None`)。プログラム全体の規則は、関数どうしを結ぶうえに走査を要らない
/// ので、すべての関数に当てる。外した関数は、同じ名前で前の確かめの走査を通っていなければならない。VMの追記は、
/// これですでにある関数の走査を省く (docs/spec/vm.mdの「追記」)。
pub fn verified_for(
    program: &Program,
    walked: impl Fn(FnIdx) -> bool,
) -> Result<Vec<Option<Unwinds>>, VerifyError> {
    verify::verified(program, walked)
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
