//! Core IR (docs/spec/core-ir.md)。型付き HIR から変換する、前向きの辺だけを持つ基本ブロックの列で表す IR で、RC と
//! エフェクトを明示する。

use eml_extern::Extern;

mod contract;
pub mod liveness;
mod perceus;
mod pipeline;
mod pretty;
mod text;
mod translate;
mod verify;

pub use contract::{contract, tail_call};
pub use perceus::perceus;
pub use pipeline::{Pass, lower, lower_until};
pub use pretty::{pretty, pretty_with_positions};
pub use text::{ParseError, parse};
pub use verify::{VerifyError, verify, verify_scopes};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub functions: Vec<CoreFn>,
    /// 実行の入口。`main` を `()` で呼ぶ、引数のない関数 (docs/spec/core-ir.md)。
    pub entry: FnIdx,
    /// 文字列リテラルの定数表。`ConstString` と `CasePattern::String` が添字で引く。インタプリタは項目ごとに不死の
    /// 物体を1つ作り、`ConstString` はその物体の参照を1つ作る (docs/spec/runtime.md の「不死の物体」)。
    pub strings: Vec<String>,
    /// エフェクトの表。添字は `Call::Handle` と `Call::Perform` のエフェクトの番号で、HIR の `EffectId` の添字と同じである。
    pub effects: Vec<EffectInfo>,
    /// 表示用のパスの表。`Loc.file` が添字で引く。
    pub files: Vec<String>,
}

impl Program {
    pub fn function(&self, idx: FnIdx) -> &CoreFn {
        &self.functions[idx.0 as usize]
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FnIdx(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VarId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreFn {
    pub name: String,
    pub vars: Vec<VarInfo>,
    /// 直接の呼び出しの結果の Repr。
    pub ret: Repr,
    /// `blocks[0]` が入口で、その引数が関数の引数である。辺はすべて番号の大きいブロックへ向かう。
    pub blocks: Vec<Block>,
}

impl CoreFn {
    pub fn params(&self) -> &[VarId] {
        &self.blocks[BlockId::ENTRY.0 as usize].params
    }

    pub fn block(&self, id: BlockId) -> &Block {
        &self.blocks[id.0 as usize]
    }

    pub fn repr(&self, var: VarId) -> Repr {
        self.vars[var.0 as usize].repr
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarInfo {
    pub name: String,
    pub repr: Repr,
}

/// 値の表現。型から決まり、RC の対象かどうかもここから決まる (docs/spec/core-ir.md)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Repr {
    /// つねにヒープの物体を指す値。
    Obj,
    /// 即値 (引数のないコンストラクタのタグ、捕まえた変数のない関数) か、ヒープの物体を指す値。
    TObj,
    Int,
    /// 引数のないコンストラクタだけの data のタグ。
    Enum,
    Unit,
}

impl Repr {
    pub fn is_rc(self) -> bool {
        match self {
            Repr::Obj | Repr::TObj => true,
            Repr::Int | Repr::Enum | Repr::Unit => false,
        }
    }

    /// テキストの形での名前 (docs/implementation/testing.md の「Core IR のテキストの形」)。
    pub fn name(self) -> &'static str {
        match self {
            Repr::Obj => "obj",
            Repr::TObj => "tobj",
            Repr::Int => "int",
            Repr::Enum => "enum",
            Repr::Unit => "unit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub u32);

impl BlockId {
    pub const ENTRY: BlockId = BlockId(0);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub params: Vec<VarId>,
    pub stmts: Vec<Stmt>,
    pub term: Term,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    Let {
        var: VarId,
        rhs: Rhs,
    },
    /// コンストラクタが1つの型の値を分解する。行き先が「ブロックの残り」である1つの case の `switch` と同じ意味で、
    /// フィールドは `Case` のフィールドと同じ規則で束縛する (docs/spec/core-ir.md)。
    Unpack {
        value: VarId,
        tag: u32,
        fields: Vec<VarId>,
    },
    Dup(VarId),
    Decref(VarId),
    /// `release x #t(p1, .., pn)`。Perceus だけが入れる RC の命令である。分解した値 `x` の参照を1つ手放し、名前を
    /// 書いた位置の変数が、そのフィールドの参照を1つずつ受け取る。`None` は残さない位置である
    /// (docs/spec/core-ir.md)。
    Release {
        value: VarId,
        tag: u32,
        fields: Vec<Option<VarId>>,
    },
}

impl Stmt {
    /// 文が定義する変数。
    pub fn defs(&self) -> &[VarId] {
        match self {
            Stmt::Let { var, rhs: _ } => std::slice::from_ref(var),
            Stmt::Unpack {
                value: _,
                tag: _,
                fields,
            } => fields,
            Stmt::Dup(_) | Stmt::Decref(_) | Stmt::Release { .. } => &[],
        }
    }

    /// 文が値として使う変数と定数。`Dup`、`Decref`、`Release` は Perceus の命令なので、値の使用に数えない。
    pub fn for_each_atom(&self, mut f: impl FnMut(Atom)) {
        match self {
            Stmt::Let { var: _, rhs } => rhs.for_each_atom(f),
            Stmt::Unpack {
                value,
                tag: _,
                fields: _,
            } => f(Atom::Var(*value)),
            Stmt::Dup(_) | Stmt::Decref(_) | Stmt::Release { .. } => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Term {
    Return(Atom),
    /// translate が出す、末尾の呼び出しの要求。`mask` は `Rhs::Call` と同じである (docs/spec/core-ir.md)。
    TailCall {
        call: Call,
        mask: Vec<u32>,
    },
    /// 並列な代入。実引数をすべて読んでから、行き先の引数に書く。
    Jump {
        target: BlockId,
        args: Vec<Atom>,
    },
    /// 合う case がなければ `default` に進む。行き先は引数を持たない (docs/spec/core-ir.md)。
    Switch {
        scrutinee: Atom,
        cases: Vec<Case>,
        default: Option<BlockId>,
    },
}

impl Term {
    /// 行き先のブロック。`Switch` は case の順に、続けて `default` を返す。
    pub fn successors(&self) -> impl Iterator<Item = BlockId> + '_ {
        let (jump, cases, default) = match self {
            Term::Jump { target, args: _ } => (Some(*target), &[][..], None),
            Term::Switch {
                scrutinee: _,
                cases,
                default,
            } => (None, &cases[..], *default),
            Term::Return(_) | Term::TailCall { call: _, mask: _ } => (None, &[][..], None),
        };
        jump.into_iter()
            .chain(cases.iter().map(|case| case.target))
            .chain(default)
    }

    pub fn for_each_successor_mut(&mut self, mut f: impl FnMut(&mut BlockId)) {
        match self {
            Term::Jump { target, args: _ } => f(target),
            Term::Switch {
                scrutinee: _,
                cases,
                default,
            } => {
                cases.iter_mut().for_each(|case| f(&mut case.target));
                if let Some(default) = default {
                    f(default);
                }
            }
            Term::Return(_) | Term::TailCall { call: _, mask: _ } => {}
        }
    }

    pub fn for_each_atom(&self, mut f: impl FnMut(Atom)) {
        match self {
            Term::Return(atom) => f(*atom),
            Term::TailCall { call, mask: _ } => call.for_each_atom(f),
            Term::Jump { target: _, args } => args.iter().for_each(|&atom| f(atom)),
            Term::Switch {
                scrutinee,
                cases: _,
                default: _,
            } => f(*scrutinee),
        }
    }

    pub fn for_each_atom_mut(&mut self, mut f: impl FnMut(&mut Atom)) {
        match self {
            Term::Return(atom) => f(atom),
            Term::TailCall { call, mask: _ } => call.for_each_atom_mut(f),
            Term::Jump { target: _, args } => args.iter_mut().for_each(f),
            Term::Switch {
                scrutinee,
                cases: _,
                default: _,
            } => f(scrutinee),
        }
    }
}

/// `switch` の case。引数を持つコンストラクタの case は、すべてのフィールドを順に束縛する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    pub pattern: CasePattern,
    pub fields: Vec<VarId>,
    pub target: BlockId,
}

/// case が比べる値。`String` は文字列定数の表 (`Program::strings`) の番号である。リテラルを1つの `switch` に並べ、
/// 比べる命令の連なりで深くしないため (docs/spec/core-ir.md)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CasePattern {
    Tag(u32),
    Int(i64),
    String(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rhs {
    /// `saved` は、呼び出しの後で生きている変数から結果の変数を除いたもので、Perceus が埋める。`mask` は呼び出しの間に
    /// 飛ばすエフェクトの多重集合で、番号の昇順に並ぶ (docs/spec/core-ir.md)。
    Call {
        call: Call,
        mask: Vec<u32>,
        saved: Vec<VarId>,
    },
    /// 関数と先頭の引数の並びからクロージャを作る。ラムダの捕獲と部分適用は、どちらもこの形になる。
    MakeClosure(FnIdx, Vec<Atom>),
    /// extern の関数をその場で実行する。eml のコードを呼び返さず、継続のフレームも積まない。`at` を持つ呼び出しが
    /// 起こした実行時エラーには、その位置を付ける (docs/spec/core-ir.md)。
    Extern {
        ext: Extern,
        args: Vec<Atom>,
        at: Option<Loc>,
    },
    /// 文字列リテラルの参照を1つ作る。リテラルは不死の物体だが、`dup` と `decref` の釣り合いは保つ
    /// (docs/spec/runtime.md)。
    ConstString(u32),
    /// 引数を持つコンストラクタの値を作る。`args` の所有権は値に移る。引数のないコンストラクタの値は `Atom::Tag` で
    /// ある (docs/spec/core-ir.md)。
    Con { tag: u32, args: Vec<Atom> },
    /// 値の所有権を受け取って捨てる。値は `()` である。
    Drop(Atom),
}

impl Rhs {
    /// 右辺が使う値。関数、extern、`perform` の引数は、どれも所有権を受け取る。
    pub fn for_each_atom(&self, mut f: impl FnMut(Atom)) {
        match self {
            Rhs::Call {
                call,
                mask: _,
                saved: _,
            } => call.for_each_atom(f),
            Rhs::MakeClosure(_, args)
            | Rhs::Extern {
                ext: _,
                args,
                at: _,
            }
            | Rhs::Con { tag: _, args } => args.iter().for_each(|&atom| f(atom)),
            Rhs::Drop(atom) => f(*atom),
            Rhs::ConstString(_) => {}
        }
    }

    pub fn for_each_atom_mut(&mut self, mut f: impl FnMut(&mut Atom)) {
        match self {
            Rhs::Call {
                call,
                mask: _,
                saved: _,
            } => call.for_each_atom_mut(f),
            Rhs::MakeClosure(_, args)
            | Rhs::Extern {
                ext: _,
                args,
                at: _,
            }
            | Rhs::Con { tag: _, args } => args.iter_mut().for_each(f),
            Rhs::Drop(atom) => f(atom),
            Rhs::ConstString(_) => {}
        }
    }
}

/// ソースの位置。`file` は `Program.files` の添字で、`line` と `column` は1から数える (`column` は文字の位置)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Loc {
    pub file: u32,
    pub line: u32,
    pub column: u32,
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
    pub fn for_each_atom(&self, mut f: impl FnMut(Atom)) {
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

    pub fn for_each_atom_mut(&mut self, mut f: impl FnMut(&mut Atom)) {
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
