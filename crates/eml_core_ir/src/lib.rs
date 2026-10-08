//! Core IR (docs/spec/core-ir.md)。型付き HIR から変換する ANF 形式の IR で、RC とエフェクトを明示する。

use eml_extern::Extern;

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

#[derive(Debug)]
pub struct Program {
    pub functions: Vec<CoreFn>,
    /// 実行の入口。`main` を `()` で呼ぶ、引数のない関数 (docs/spec/core-ir.md)。
    pub entry: FnIdx,
    /// 文字列リテラルの定数表。`ConstString` と `CasePattern::String` が添字で引く。インタプリタは項目ごとに不死の
    /// 物体を1つ作り、`ConstString` はその物体の参照を1つ作る (docs/spec/runtime.md)。
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
    /// 操作の引数の数。verifier が節の関数の引数の数を確かめるのに使う (docs/spec/core-ir.md)。
    pub arity: usize,
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
    /// scrutinee の値で分岐する。`if` もここに変換する。scrutinee は move で受け取る。合う case がなければ `default` に
    /// 進む。タグの case は引数を持つコンストラクタの値を分解し、case のフィールドに入れる。リテラルの case の `Switch`
    /// はつねに `default` を持つ (docs/spec/core-ir.md)。
    Switch {
        scrutinee: Atom,
        cases: Vec<Case>,
        default: Option<CExprId>,
    },
    Jump {
        join: JoinId,
        args: Vec<Atom>,
    },
    Return(Atom),
    /// 関数の末尾の呼び出し。呼び出し元のフレームを積まない。`mask` は呼び出しの間に飛ばすエフェクトの多重集合で、
    /// エフェクトの番号の昇順に並び、空なら `mask` なしである。末尾かどうかと `mask` は独立である
    /// (docs/spec/core-ir.md)。
    TailCall {
        call: Call,
        mask: Vec<u32>,
    },
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
    /// 子の式を、`Join` は本体、`scope` の順に、`Switch` は case の順に、続けて `default` を渡す。子をたどる処理は
    /// すべてここを通す。
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
            CExpr::Switch {
                scrutinee: _,
                cases,
                default,
            } => {
                cases.iter().for_each(|case| f(case.body));
                if let Some(default) = default {
                    f(*default);
                }
            }
            CExpr::Jump { join: _, args: _ }
            | CExpr::Return(_)
            | CExpr::TailCall { call: _, mask: _ } => {}
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
            CExpr::Switch {
                scrutinee: _,
                cases,
                default,
            } => {
                cases.iter_mut().for_each(|case| f(&mut case.body));
                if let Some(default) = default {
                    f(default);
                }
            }
            CExpr::Jump { join: _, args: _ }
            | CExpr::Return(_)
            | CExpr::TailCall { call: _, mask: _ } => {}
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
            CExpr::Switch { scrutinee, .. } => f(*scrutinee),
            CExpr::Jump { join: _, args } => args.iter().for_each(|&atom| f(atom)),
            CExpr::Return(atom) => f(*atom),
            CExpr::TailCall { call, mask: _ } => call.for_each_atom(f),
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
            CExpr::Switch { scrutinee, .. } => f(scrutinee),
            CExpr::Jump { join: _, args } => args.iter_mut().for_each(f),
            CExpr::Return(atom) => f(atom),
            CExpr::TailCall { call, mask: _ } => call.for_each_atom_mut(f),
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

/// `Switch` の case。引数を持つコンストラクタの case は、すべてのフィールドを順に束縛する。リテラルの case と、引数の
/// ないコンストラクタの case はフィールドを持たない。case は、フィールドの参照を1つずつ所有して始まる
/// (docs/spec/core-ir.md)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    pub pattern: CasePattern,
    pub fields: Vec<VarId>,
    pub body: CExprId,
}

/// case が比べる値。`String` は文字列定数の表 (`Program::strings`) の番号である。リテラルを1つの `Switch` に並べ、
/// 比べる命令の連なりで深くしないため (docs/spec/core-ir.md)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CasePattern {
    Tag(u32),
    Int(i64),
    String(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rhs {
    Atom(Atom),
    /// 呼び出しの後で使う変数 (`saved`) を、呼び出しのフレームに退避する (docs/spec/core-ir.md)。`mask` は
    /// `TailCall` と同じである。
    Call {
        call: Call,
        mask: Vec<u32>,
        saved: Vec<VarId>,
    },
    /// 関数と先頭の引数の並びからクロージャを作る。並びの値の所有権はクロージャに移る。ラムダの捕獲と部分適用は、
    /// どちらもこの形になる (docs/spec/core-ir.md)。
    MakeClosure(FnIdx, Vec<Atom>),
    /// extern の関数の呼び出し。その場で実行して値を返す1階の命令で、eml のコードを呼び返さず、継続のフレームも
    /// 積まない。引数の数は表の行と等しく、型で選ぶ行 (`Prelude.==`) は translate が置き換えるので現れない。
    Extern(Extern, Vec<Atom>),
    /// 文字列リテラルの参照を1つ作る。リテラルは不死の物体だが、ほかの文字列と同じく `dup` と `decref` の釣り合いを
    /// 保つ。RC の操作を飛ばしてよいのは、検査付きヒープを持たないバックエンドだけである (docs/spec/runtime.md)。
    ConstString(u32),
    /// 引数を持つコンストラクタの値を作る。`args` の所有権は値に移る。引数のないコンストラクタの値は `Atom::Tag` で
    /// ある (docs/spec/core-ir.md)。
    Con {
        tag: u32,
        args: Vec<Atom>,
    },
    /// 値の所有権を受け取って捨てる。値は `()` である (docs/spec/core-ir.md の `drop x`)。
    Drop(Atom),
}

impl Rhs {
    /// 退避する変数をまだ決めていない呼び出し。Perceus が、呼び出しの後で生きている変数で埋める。
    pub fn call(call: Call) -> Rhs {
        Rhs::masked_call(call, Vec::new())
    }

    /// `mask` 付きの呼び出し。
    pub fn masked_call(call: Call, mask: Vec<u32>) -> Rhs {
        Rhs::Call {
            call,
            mask,
            saved: Vec::new(),
        }
    }

    /// 右辺が使う値。関数、extern、`perform` の引数は、どれも所有権を受け取る (docs/spec/core-ir.md)。
    pub fn atoms(&self) -> Vec<Atom> {
        let mut atoms = Vec::new();
        self.for_each_atom(|atom| atoms.push(atom));
        atoms
    }

    pub(crate) fn for_each_atom(&self, mut f: impl FnMut(Atom)) {
        match self {
            Rhs::Atom(atom) | Rhs::Drop(atom) => f(*atom),
            Rhs::Call {
                call,
                mask: _,
                saved: _,
            } => call.for_each_atom(f),
            Rhs::MakeClosure(_, args) | Rhs::Extern(_, args) | Rhs::Con { tag: _, args } => {
                args.iter().for_each(|&atom| f(atom))
            }
            Rhs::ConstString(_) => {}
        }
    }

    pub(crate) fn for_each_atom_mut(&mut self, mut f: impl FnMut(&mut Atom)) {
        match self {
            Rhs::Atom(atom) | Rhs::Drop(atom) => f(atom),
            Rhs::Call {
                call,
                mask: _,
                saved: _,
            } => call.for_each_atom_mut(f),
            Rhs::MakeClosure(_, args) | Rhs::Extern(_, args) | Rhs::Con { tag: _, args } => {
                args.iter_mut().for_each(f)
            }
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
    /// handler フレームを積み、本体の関数に `()` を適用する。本体と節と `return` の節は、捕まえた変数を先頭の
    /// 引数に持つ関数の値である (docs/spec/core-ir.md)。節はエフェクトの操作の順に並ぶ。節は最後の引数で状態を、
    /// `ret` は本体の値と状態を受ける。状態のない handler では `init` が `()` である (docs/spec/core-ir.md)。
    Handle {
        effect: u32,
        init: Atom,
        body: Atom,
        clauses: Vec<Atom>,
        ret: Atom,
    },
    /// ユーザーのエフェクトの操作。継続を遡って handler を探し、その節を呼ぶ。`resumable` は操作が `never` でない
    /// ことで、インタプリタはエフェクトの表を引かずに、区間を継続にするか解放するかを決める (docs/spec/core-ir.md)。
    Perform {
        effect: u32,
        op: u32,
        resumable: bool,
        args: Vec<Atom>,
    },
    /// 継続を再開する。`state` を区間の handler フレームに戻してからつなぐ。値は handle 式の値である。
    Resume { k: Atom, arg: Atom, state: Atom },
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
                resumable: _,
                args,
            } => args.iter().for_each(|&atom| f(atom)),
            Call::Apply(callee, args) => {
                f(*callee);
                args.iter().for_each(|&atom| f(atom));
            }
            Call::Handle {
                effect: _,
                init,
                body,
                clauses,
                ret,
            } => {
                f(*init);
                f(*body);
                clauses.iter().for_each(|&atom| f(atom));
                f(*ret);
            }
            Call::Resume { k, arg, state } => {
                f(*k);
                f(*arg);
                f(*state);
            }
        }
    }

    pub(crate) fn for_each_atom_mut(&mut self, mut f: impl FnMut(&mut Atom)) {
        match self {
            Call::Direct(_, args)
            | Call::Perform {
                effect: _,
                op: _,
                resumable: _,
                args,
            } => args.iter_mut().for_each(f),
            Call::Apply(callee, args) => {
                f(callee);
                args.iter_mut().for_each(f);
            }
            Call::Handle {
                effect: _,
                init,
                body,
                clauses,
                ret,
            } => {
                f(init);
                f(body);
                clauses.iter_mut().for_each(&mut f);
                f(ret);
            }
            Call::Resume { k, arg, state } => {
                f(k);
                f(arg);
                f(state);
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
    /// 捕まえた変数のない関数の値。クロージャを確保しない。RC の対象ではない (docs/spec/core-ir.md)。
    Fn(FnIdx),
}

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
            init: Atom::Var(VarId(0)),
            body: Atom::Var(VarId(1)),
            clauses: vec![Atom::Var(VarId(2)), Atom::Var(VarId(3))],
            ret: Atom::Var(VarId(4)),
        };
        let handle = CExpr::TailCall {
            call: call.clone(),
            mask: Vec::new(),
        };
        let mut atoms = Vec::new();
        handle.for_each_atom(|atom| atoms.push(atom));
        assert_eq!(atoms, [0, 1, 2, 3, 4].map(|n| Atom::Var(VarId(n))));
        assert_eq!(atoms, call.atoms());

        let mut handle = handle;
        let mut atoms_mut = Vec::new();
        handle.for_each_atom_mut(|atom| atoms_mut.push(*atom));
        assert_eq!(atoms_mut, atoms);

        let apply = CExpr::TailCall {
            call: Call::Apply(Atom::Var(VarId(7)), vec![Atom::Int(1)]),
            mask: Vec::new(),
        };
        let mut atoms = Vec::new();
        apply.for_each_atom(|atom| atoms.push(atom));
        assert_eq!(atoms, [Atom::Var(VarId(7)), Atom::Int(1)]);
    }
}
