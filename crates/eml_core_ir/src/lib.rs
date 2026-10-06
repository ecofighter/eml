//! Core IR (docs/spec/core-ir.md)。型付き HIR から変換する ANF 形式の IR で、RC とエフェクトを明示する。

mod builder;
mod compact;
mod liveness;
mod perceus;
mod pipeline;
mod pretty;
mod simplify;
mod text;
mod translate;
mod verify;

pub use pipeline::{Pass, lower, lower_until};
pub use pretty::pretty;
pub use text::{ParseError, parse};
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
    pub fn join(&self, join: JoinId) -> (&[VarId], CExprId) {
        match self.expr(self.joins[join.0 as usize]) {
            CExpr::Join { params, body, .. } => (params, *body),
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

/// 型の情報は消し、ボックス化の有無だけを残す (docs/spec/core-ir.md)。
#[derive(Debug, Clone)]
pub struct VarInfo {
    pub name: String,
    /// ヒープに置く値。ボックス化した変数が RC の対象になる。
    pub boxed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CExpr {
    Let {
        var: VarId,
        rhs: Rhs,
        body: CExprId,
    },
    /// `scope` の中の `Jump` が `body` に入る。`params` は `Jump` が渡す値を順に受ける。末尾にない `if` の続きを、
    /// ヒープにフレームを積まずに実行するために使う (docs/spec/core-ir.md)。`body` の中からは `Jump` しない。
    /// 末尾にない `if` の join point は引数を1つ持つ。`simplify` の B2 が切り出す枝の join point は、その枝のフィールドを引数に取り、枝が値全体も使うときは値も最後の引数に取る。
    Join {
        join: JoinId,
        params: Vec<VarId>,
        /// 本体が使う外側の変数 (`params` を除く)。RC の対象かどうかによらずすべて入れ、`VarId` の昇順に並べる。
        /// パスの中では古くなってよく、パスの間ではパイプラインが埋め直す (docs/spec/core-ir.md のパスの表)。
        captures: Vec<VarId>,
        body: CExprId,
        scope: CExprId,
    },
    /// タグで分岐する。`if` もここに変換する。scrutinee は move で受け取り、引数を持つコンストラクタの値なら分解して
    /// 枝のフィールドに入れる (docs/spec/core-ir.md)。
    Switch {
        scrutinee: Atom,
        arms: Vec<Arm>,
    },
    Jump {
        join: JoinId,
        args: Vec<Atom>,
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
    /// 子の式を、`Join` は本体、`scope` の順に、`Switch` は枝の順に渡す。子をたどる処理はすべてここを通す。
    pub(crate) fn for_each_child(&self, mut f: impl FnMut(CExprId)) {
        match self {
            CExpr::Let {
                var: _,
                rhs: _,
                body,
            }
            | CExpr::Dup { var: _, body }
            | CExpr::Decref { var: _, body } => f(*body),
            CExpr::Join {
                join: _,
                params: _,
                captures: _,
                body,
                scope,
            } => {
                f(*body);
                f(*scope);
            }
            CExpr::Switch { scrutinee: _, arms } => arms.iter().for_each(|arm| f(arm.body)),
            CExpr::Jump { join: _, args: _ } | CExpr::Return(_) | CExpr::TailCall(_) => {}
        }
    }

    pub(crate) fn for_each_child_mut(&mut self, mut f: impl FnMut(&mut CExprId)) {
        match self {
            CExpr::Let {
                var: _,
                rhs: _,
                body,
            }
            | CExpr::Dup { var: _, body }
            | CExpr::Decref { var: _, body } => f(body),
            CExpr::Join {
                join: _,
                params: _,
                captures: _,
                body,
                scope,
            } => {
                f(body);
                f(scope);
            }
            CExpr::Switch { scrutinee: _, arms } => {
                arms.iter_mut().for_each(|arm| f(&mut arm.body))
            }
            CExpr::Jump { join: _, args: _ } | CExpr::Return(_) | CExpr::TailCall(_) => {}
        }
    }

    /// 式が直接使う値。子の式の値は含まない。`Dup` と `Decref` の変数は Perceus の命令であり、値の使用に数えない。
    pub(crate) fn for_each_atom(&self, mut f: impl FnMut(Atom)) {
        match self {
            CExpr::Let {
                var: _,
                rhs,
                body: _,
            } => rhs.for_each_atom(f),
            CExpr::Switch { scrutinee, arms: _ } => f(*scrutinee),
            CExpr::Jump { join: _, args } => args.iter().for_each(|&atom| f(atom)),
            CExpr::Return(atom) => f(*atom),
            CExpr::TailCall(call) => call.for_each_atom(f),
            CExpr::Join {
                join: _,
                params: _,
                captures: _,
                body: _,
                scope: _,
            }
            | CExpr::Dup { var: _, body: _ }
            | CExpr::Decref { var: _, body: _ } => {}
        }
    }

    pub(crate) fn for_each_atom_mut(&mut self, mut f: impl FnMut(&mut Atom)) {
        match self {
            CExpr::Let {
                var: _,
                rhs,
                body: _,
            } => rhs.for_each_atom_mut(f),
            CExpr::Switch { scrutinee, arms: _ } => f(scrutinee),
            CExpr::Jump { join: _, args } => args.iter_mut().for_each(f),
            CExpr::Return(atom) => f(atom),
            CExpr::TailCall(call) => call.for_each_atom_mut(f),
            CExpr::Join {
                join: _,
                params: _,
                captures: _,
                body: _,
                scope: _,
            }
            | CExpr::Dup { var: _, body: _ }
            | CExpr::Decref { var: _, body: _ } => {}
        }
    }
}

/// `Switch` の枝。引数を持つコンストラクタの枝は、すべてのフィールドを順に束縛する。引数のないコンストラクタの枝の
/// `fields` は空である。枝は、フィールドの参照を1つずつ所有して始まる (docs/spec/core-ir.md)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arm {
    pub tag: u32,
    pub fields: Vec<VarId>,
    pub body: CExprId,
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
    /// 引数を持つコンストラクタの値を作る。`args` の所有権は値に移る。引数のないコンストラクタの値は `Atom::Tag` で
    /// ある (docs/spec/core-ir.md)。
    Con {
        tag: u32,
        args: Vec<Atom>,
    },
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
        let mut atoms = Vec::new();
        self.for_each_atom(|atom| atoms.push(atom));
        atoms
    }

    pub(crate) fn for_each_atom(&self, mut f: impl FnMut(Atom)) {
        match self {
            Rhs::Atom(atom) | Rhs::Drop(atom) => f(*atom),
            Rhs::Call { call, saved: _ } => call.for_each_atom(f),
            Rhs::MakeClosure(_, args)
            | Rhs::Prim(_, args)
            | Rhs::Io(_, args)
            | Rhs::Con { tag: _, args } => args.iter().for_each(|&atom| f(atom)),
            Rhs::ConstString(_) => {}
        }
    }

    pub(crate) fn for_each_atom_mut(&mut self, mut f: impl FnMut(&mut Atom)) {
        match self {
            Rhs::Atom(atom) | Rhs::Drop(atom) => f(atom),
            Rhs::Call { call, saved: _ } => call.for_each_atom_mut(f),
            Rhs::MakeClosure(_, args)
            | Rhs::Prim(_, args)
            | Rhs::Io(_, args)
            | Rhs::Con { tag: _, args } => args.iter_mut().for_each(f),
            Rhs::ConstString(_) => {}
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
        let mut atoms = Vec::new();
        self.for_each_atom(|atom| atoms.push(atom));
        atoms
    }

    pub(crate) fn for_each_atom(&self, mut f: impl FnMut(Atom)) {
        match self {
            Call::Direct(_, args)
            | Call::Perform {
                effect: _,
                op: _,
                args,
            } => args.iter().for_each(|&atom| f(atom)),
            Call::Apply(callee, args) => {
                f(*callee);
                args.iter().for_each(|&atom| f(atom));
            }
            Call::Handle {
                effect: _,
                body,
                clauses,
                ret,
            } => {
                f(*body);
                clauses.iter().for_each(|&atom| f(atom));
                ret.iter().for_each(|&atom| f(atom));
            }
            Call::Resume { k, arg } => {
                f(*k);
                f(*arg);
            }
        }
    }

    pub(crate) fn for_each_atom_mut(&mut self, mut f: impl FnMut(&mut Atom)) {
        match self {
            Call::Direct(_, args)
            | Call::Perform {
                effect: _,
                op: _,
                args,
            } => args.iter_mut().for_each(f),
            Call::Apply(callee, args) => {
                f(callee);
                args.iter_mut().for_each(f);
            }
            Call::Handle {
                effect: _,
                body,
                clauses,
                ret,
            } => {
                f(body);
                clauses.iter_mut().for_each(&mut f);
                ret.iter_mut().for_each(f);
            }
            Call::Resume { k, arg } => {
                f(k);
                f(arg);
            }
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

/// 変種と表示の名前の表から、`name` と、その逆の `from_name` を作る。`name` の `match` は網羅を検査されるので、
/// 変種を足して表に書き忘れるとコンパイルエラーになる。`from_name` も同じ表から作るので、読み戻しから漏れない。
macro_rules! named_ops {
    ($ty:ident { $($variant:ident => $name:literal,)* }) => {
        impl $ty {
            /// 表示での名前。`pretty` が書き、`parse` が `from_name` で読み戻す。
            pub fn name(self) -> &'static str {
                match self {
                    $($ty::$variant => $name,)*
                }
            }

            /// `name` の逆。表にない名前は `None` である。
            pub fn from_name(name: &str) -> Option<$ty> {
                match name {
                    $($name => Some($ty::$variant),)*
                    _ => None,
                }
            }

            #[cfg(test)]
            const VARIANTS: &[$ty] = &[$($ty::$variant,)*];
        }
    };
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
    StrEq,
    StrNe,
    BoolEq,
    BoolNe,
    ShowInt,
    Not,
}

impl PrimOp {
    /// 実行時エラーを起こしうるプリミティブ。整数の演算はオーバーフローとゼロ除算で止まる (docs/spec/declarations.md の
    /// 標準の演算子の表)。`simplify` の DCE は、これらを使われなくても消さない。
    pub fn may_fail(self) -> bool {
        matches!(
            self,
            PrimOp::IntAdd
                | PrimOp::IntSub
                | PrimOp::IntMul
                | PrimOp::IntDiv
                | PrimOp::IntMod
                | PrimOp::IntNeg
        )
    }
}

named_ops!(PrimOp {
    IntAdd => "+",
    IntSub => "-",
    IntMul => "*",
    IntDiv => "/",
    IntMod => "%",
    IntNeg => "negate",
    IntEq => "==",
    IntNe => "!=",
    IntLt => "<",
    IntLe => "<=",
    IntGt => ">",
    IntGe => ">=",
    StrConcat => "++",
    StrEq => "string==",
    StrNe => "string!=",
    BoolEq => "bool==",
    BoolNe => "bool!=",
    ShowInt => "show_int",
    Not => "not",
});

/// 表示での名前 (`name`) は Prelude の名前と同じである。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoOp {
    Println,
    Open,
    ReadAll,
    Close,
}

named_ops!(IoOp {
    Println => "println",
    Open => "open",
    ReadAll => "read_all",
    Close => "close",
});

/// `Bool` のタグ (docs/spec/core-ir.md)。
pub const FALSE: u32 = 0;
pub const TRUE: u32 = 1;

/// タプルの値のタグ。タプルは、コンストラクタが1つの `data` と同じオブジェクトで表す (docs/spec/core-ir.md)。
pub const TUPLE: u32 = 0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn children_and_atoms_come_in_a_fixed_order() {
        let join = CExpr::Join {
            join: JoinId(0),
            params: vec![VarId(0)],
            captures: vec![VarId(1)],
            body: CExprId(3),
            scope: CExprId(4),
        };
        let mut children = Vec::new();
        join.for_each_child(|child| children.push(child));
        assert_eq!(children, [CExprId(3), CExprId(4)]);

        let call = Call::Handle {
            effect: 0,
            body: Atom::Var(VarId(1)),
            clauses: vec![Atom::Var(VarId(2)), Atom::Var(VarId(3))],
            ret: Some(Atom::Var(VarId(4))),
        };
        let handle = CExpr::TailCall(call.clone());
        let mut atoms = Vec::new();
        handle.for_each_atom(|atom| atoms.push(atom));
        assert_eq!(atoms, [1, 2, 3, 4].map(|n| Atom::Var(VarId(n))));
        assert_eq!(atoms, call.atoms());

        let mut handle = handle;
        let mut atoms_mut = Vec::new();
        handle.for_each_atom_mut(|atom| atoms_mut.push(*atom));
        assert_eq!(atoms_mut, atoms);

        let apply = CExpr::TailCall(Call::Apply(Atom::Var(VarId(7)), vec![Atom::Int(1)]));
        let mut atoms = Vec::new();
        apply.for_each_atom(|atom| atoms.push(atom));
        assert_eq!(atoms, [Atom::Var(VarId(7)), Atom::Int(1)]);
    }

    #[test]
    fn every_operation_name_reads_back() {
        for &op in PrimOp::VARIANTS {
            assert_eq!(PrimOp::from_name(op.name()), Some(op));
        }
        for &op in IoOp::VARIANTS {
            assert_eq!(IoOp::from_name(op.name()), Some(op));
        }
        assert_eq!(PrimOp::from_name("println"), None);
        assert_eq!(IoOp::from_name("+"), None);
    }
}
