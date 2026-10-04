//! Core IR (docs/spec/core-ir.md)。型付き HIR から変換する ANF 形式の IR で、RC とエフェクトを明示する。

mod liveness;
mod lower;
mod perceus;
mod pretty;

pub use eml_types::Linearity;
pub use lower::lower;
pub use pretty::pretty;

/// 複数のスレッドが同じプログラムを実行できるように、実行時は `Arc<Program>` で読み取り専用で共有する。
#[derive(Debug)]
pub struct Program {
    pub functions: Vec<CoreFn>,
    /// 実行の入口。`main` を `()` で呼ぶ、引数のない関数 (docs/spec/core-ir.md)。
    pub entry: FnIdx,
    /// 文字列リテラルの定数表。`ConstString` が添字で引き、実行のたびに新しい文字列をヒープに作る。
    pub strings: Vec<String>,
}

impl Program {
    pub fn function(&self, idx: FnIdx) -> &CoreFn {
        &self.functions[idx.0 as usize]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FnIdx(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VarId(pub u32);

/// 継続のフレームが「どこから再開するか」を持てるように、式に ID を付ける。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CExprId(pub u32);

/// 関数の中の join point の番号。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JoinId(pub u32);

#[derive(Debug)]
pub struct CoreFn {
    pub name: String,
    pub params: Vec<VarId>,
    pub vars: Vec<VarInfo>,
    pub body: CExprId,
    pub exprs: Vec<CExpr>,
    /// `JoinId` から `Join` の式を引く索引。式のアリーナを作り直すパスは、索引も作り直す。
    pub joins: Vec<CExprId>,
}

impl CoreFn {
    pub fn expr(&self, id: CExprId) -> &CExpr {
        &self.exprs[id.0 as usize]
    }

    /// `Jump` の行き先の、join point の引数と本体。
    pub fn join(&self, join: JoinId) -> (VarId, CExprId) {
        match self.expr(self.joins[join.0 as usize]) {
            CExpr::Join { param, body, .. } => (*param, *body),
            _ => unreachable!("the join index points at join points"),
        }
    }
}

/// 型の情報は消し、Kind とボックス化の有無だけを残す (docs/spec/core-ir.md)。
#[derive(Debug, Clone)]
pub struct VarInfo {
    pub name: String,
    pub linearity: Linearity,
    /// ヒープに置く値。`Unr` でボックス化した変数が RC の対象になる。
    pub boxed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CExpr {
    Let {
        var: VarId,
        rhs: Rhs,
        body: CExprId,
    },
    /// `scope` の中の `Jump` が `body` に入る。`param` は `Jump` が渡す値を受ける。末尾にない `if` の続きを、
    /// ヒープにフレームを積まずに実行するために使う (docs/spec/core-ir.md)。`body` の中からは `Jump` しない。
    Join {
        join: JoinId,
        param: VarId,
        body: CExprId,
        scope: CExprId,
    },
    /// タグで分岐する。`if` もここに変換し、段階4の `match` と同じ命令にする。
    Switch {
        scrutinee: Atom,
        arms: Vec<(u32, CExprId)>,
    },
    Jump {
        join: JoinId,
        arg: Atom,
    },
    Return(Atom),
    /// 関数の末尾の呼び出し。呼び出し元のフレームを積まない。
    TailCall(Call),
    Dup {
        var: VarId,
        body: CExprId,
    },
    Decref {
        var: VarId,
        body: CExprId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rhs {
    Atom(Atom),
    Call(Call),
    /// 関数と先頭の引数の並びからクロージャを作る。並びの値の所有権はクロージャに移る。ラムダの捕獲と部分適用は、
    /// どちらもこの形になる (docs/spec/core-ir.md)。
    MakeClosure(FnIdx, Vec<Atom>),
    Prim(PrimOp, Vec<Atom>),
    ConstString(u32),
    Perform(IoOp, Vec<Atom>),
}

impl Rhs {
    /// 右辺が使う値。関数、プリミティブ、`perform` の引数は、どれも所有権を受け取る (docs/spec/core-ir.md)。
    pub fn atoms(&self) -> Vec<Atom> {
        match self {
            Rhs::Atom(atom) => vec![*atom],
            Rhs::Call(call) => call.atoms(),
            Rhs::MakeClosure(_, args) | Rhs::Prim(_, args) | Rhs::Perform(_, args) => args.clone(),
            Rhs::ConstString(_) => Vec::new(),
        }
    }
}

/// 呼び出し。クロージャと引数の所有権は呼び出しに移る。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    /// 呼ぶ相手が分かっていて、引数の個数が揃っている呼び出し。
    Direct(FnIdx, Vec<Atom>),
    /// 関数値の呼び出し。実行時に引数の個数を比べる (eval/apply)。
    Apply(Atom, Vec<Atom>),
}

impl Call {
    /// 呼び出しが使う値。関数値の呼び出しでは、呼ばれる値が先に来る。
    pub fn atoms(&self) -> Vec<Atom> {
        match self {
            Call::Direct(_, args) => args.clone(),
            Call::Apply(callee, args) => std::iter::once(*callee)
                .chain(args.iter().copied())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Atom {
    Var(VarId),
    Int(i64),
    Unit,
    Tag(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimOp {
    IntAdd,
    IntSub,
    IntMul,
    IntDiv,
    IntMod,
    IntNeg,
    IntEq,
    IntNe,
    IntLt,
    IntLe,
    IntGt,
    IntGe,
    StrConcat,
    ShowInt,
    Not,
}

impl PrimOp {
    pub fn name(self) -> &'static str {
        match self {
            PrimOp::IntAdd => "+",
            PrimOp::IntSub => "-",
            PrimOp::IntMul => "*",
            PrimOp::IntDiv => "/",
            PrimOp::IntMod => "%",
            PrimOp::IntNeg => "negate",
            PrimOp::IntEq => "==",
            PrimOp::IntNe => "!=",
            PrimOp::IntLt => "<",
            PrimOp::IntLe => "<=",
            PrimOp::IntGt => ">",
            PrimOp::IntGe => ">=",
            PrimOp::StrConcat => "++",
            PrimOp::ShowInt => "show_int",
            PrimOp::Not => "not",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoOp {
    Println,
}

/// `Bool` のタグ (docs/spec/core-ir.md)。
pub const FALSE: u32 = 0;
pub const TRUE: u32 = 1;
