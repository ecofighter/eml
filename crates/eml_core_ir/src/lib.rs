//! Core IR (docs/spec/core-ir.md)。型付き HIR から変換する ANF 形式の IR で、RC とエフェクトを明示する。

mod liveness;
mod perceus;
mod pipeline;
mod pretty;
mod simplify;
mod translate;
mod verify;

pub use eml_types::Linearity;
pub use pipeline::lower;
pub use pretty::pretty;
pub use verify::{VerifyError, verify, verify_scopes};

/// 複数のスレッドが同じプログラムを実行できるように、実行時は `Arc<Program>` で読み取り専用で共有する。
#[derive(Debug)]
pub struct Program {
    pub functions: Vec<CoreFn>,
    /// 実行の入口。`main` を `()` で呼ぶ、引数のない関数 (docs/spec/core-ir.md)。
    pub entry: FnIdx,
    /// 文字列リテラルの定数表。`ConstString` が添字で引き、実行のたびに新しい文字列をヒープに作る。
    pub strings: Vec<String>,
    /// エフェクトの表。添字は `Call::Handle` と `Call::Perform` のエフェクトの番号で、HIR の `EffectId` の添字と同じである。
    pub effects: Vec<EffectInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectInfo {
    pub name: String,
    /// 宣言した順の操作。`Call::Handle` の節と `Call::Perform` の操作の番号は、この順の添字である。
    pub operations: Vec<OperationInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationInfo {
    pub name: String,
    /// `never` の操作は再開しないので、継続を作らずに捨てる (docs/spec/effects.md)。
    pub resumable: bool,
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

    /// join point の本体が使う外側の変数。
    pub fn captures(&self, join: JoinId) -> &[VarId] {
        match self.expr(self.joins[join.0 as usize]) {
            CExpr::Join { captures, .. } => captures,
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
        /// 本体が使う外側の変数 (`param` を除く)。RC の対象かどうかによらずすべて入れ、`VarId` の昇順に並べる。
        /// Perceus の最初の解析が埋め直すので、変換や `simplify` は空のままでよい。
        captures: Vec<VarId>,
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

impl CExpr {
    /// 式が直接使う値を書き換える口。子の式の値は含まない。
    pub(crate) fn atoms_mut(&mut self) -> Vec<&mut Atom> {
        match self {
            CExpr::Let { rhs, .. } => rhs.atoms_mut(),
            CExpr::Switch { scrutinee, .. } => vec![scrutinee],
            CExpr::Jump { arg, .. } => vec![arg],
            CExpr::Return(atom) => vec![atom],
            CExpr::TailCall(call) => call.atoms_mut(),
            CExpr::Join { .. } | CExpr::Dup { .. } | CExpr::Decref { .. } => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rhs {
    Atom(Atom),
    /// 呼び出しの後で使う変数 (`saved`) を、呼び出しのフレームに退避する (docs/spec/core-ir.md)。
    Call {
        call: Call,
        saved: Vec<VarId>,
    },
    /// 関数と先頭の引数の並びからクロージャを作る。並びの値の所有権はクロージャに移る。ラムダの捕獲と部分適用は、
    /// どちらもこの形になる (docs/spec/core-ir.md)。
    MakeClosure(FnIdx, Vec<Atom>),
    Prim(PrimOp, Vec<Atom>),
    ConstString(u32),
    /// `IO` の操作。最下部の組み込みの handler が必ずすぐに1回再開するので、継続を遡らずにその場で実行する
    /// (docs/spec/core-ir.md)。
    Io(IoOp, Vec<Atom>),
    /// 値の所有権を受け取って捨てる。値は `()` である (docs/spec/core-ir.md の `drop x`)。
    Drop(Atom),
}

impl Rhs {
    /// 退避する変数をまだ決めていない呼び出し。Perceus が、呼び出しの後で生きている変数で埋める。
    pub fn call(call: Call) -> Rhs {
        Rhs::Call {
            call,
            saved: Vec::new(),
        }
    }

    /// 右辺が使う値。関数、プリミティブ、`perform` の引数は、どれも所有権を受け取る (docs/spec/core-ir.md)。
    pub fn atoms(&self) -> Vec<Atom> {
        match self {
            Rhs::Atom(atom) => vec![*atom],
            Rhs::Call { call, .. } => call.atoms(),
            Rhs::MakeClosure(_, args) | Rhs::Prim(_, args) | Rhs::Io(_, args) => args.clone(),
            Rhs::Drop(atom) => vec![*atom],
            Rhs::ConstString(_) => Vec::new(),
        }
    }

    /// 右辺が使う値を書き換える口。`atoms` と同じ順に並ぶ。
    pub(crate) fn atoms_mut(&mut self) -> Vec<&mut Atom> {
        match self {
            Rhs::Atom(atom) | Rhs::Drop(atom) => vec![atom],
            Rhs::Call { call, .. } => call.atoms_mut(),
            Rhs::MakeClosure(_, args) | Rhs::Prim(_, args) | Rhs::Io(_, args) => {
                args.iter_mut().collect()
            }
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
    /// handler フレームを積み、本体のクロージャに `()` を適用する。本体と節と `return` の節は、捕まえた変数を先頭の
    /// 引数に持つ関数のクロージャである (docs/spec/core-ir.md)。節はエフェクトの操作の順に並ぶ。`ret` が `None` なら、
    /// 本体の値をそのまま返す。
    Handle {
        effect: u32,
        body: Atom,
        clauses: Vec<Atom>,
        ret: Option<Atom>,
    },
    /// ユーザーのエフェクトの操作。継続を遡って handler を探し、その節を呼ぶ。
    Perform {
        effect: u32,
        op: u32,
        args: Vec<Atom>,
    },
    /// 継続を再開する。値は handle 式の値である。
    Resume { k: Atom, arg: Atom },
}

impl Call {
    /// 呼び出しが使う値。関数値の呼び出しでは、呼ばれる値が先に来る。
    pub fn atoms(&self) -> Vec<Atom> {
        match self {
            Call::Direct(_, args) | Call::Perform { args, .. } => args.clone(),
            Call::Apply(callee, args) => std::iter::once(*callee)
                .chain(args.iter().copied())
                .collect(),
            Call::Handle {
                body, clauses, ret, ..
            } => std::iter::once(*body)
                .chain(clauses.iter().copied())
                .chain(*ret)
                .collect(),
            Call::Resume { k, arg } => vec![*k, *arg],
        }
    }

    /// 呼び出しが使う値を書き換える口。`atoms` と同じ順に並ぶ。
    pub(crate) fn atoms_mut(&mut self) -> Vec<&mut Atom> {
        match self {
            Call::Direct(_, args) | Call::Perform { args, .. } => args.iter_mut().collect(),
            Call::Apply(callee, args) => std::iter::once(callee).chain(args.iter_mut()).collect(),
            Call::Handle {
                body, clauses, ret, ..
            } => std::iter::once(body)
                .chain(clauses.iter_mut())
                .chain(ret.iter_mut())
                .collect(),
            Call::Resume { k, arg } => vec![k, arg],
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
