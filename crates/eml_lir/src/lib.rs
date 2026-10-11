//! LIR (docs/spec/core-ir.md)。Core IR の下の層で、VM とネイティブの共通の入力になる。今は Core IR と共有する型、定数、
//! 関数を置く。

use std::fmt;

pub use eml_extern::Repr;

mod prim;
pub mod syntax;

pub use prim::{Immediates, Prim, PrimRow};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayoutId(pub u32);

/// `con`、`unpack`、`release` が指すコンストラクタ。タグは配置の中の添字で、宣言の順である。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ctor {
    pub layout: LayoutId,
    pub tag: u32,
}

/// data の型1つか、タプルの要素の数1つの配置 (docs/spec/core-ir.md の「データの配置」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// テキストの形で引く名前。関数と同じく、入口以外のモジュールの型は修飾する (`Prelude.Bool`)。タプルは `(,)`。
    pub name: String,
    /// タグの順のコンストラクタ。
    pub constructors: Vec<LayoutCtor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutCtor {
    pub name: String,
    /// 宣言したフィールドの型の Repr。型変数は `tobj` で、具体化した型は見ない。
    pub fields: Vec<Repr>,
}

impl Layout {
    /// 値の Repr。型の Repr と同じ規則で、コンストラクタのフィールドの数だけから決まる。
    pub fn repr(&self) -> Repr {
        data_repr(self.constructors.iter().map(|ctor| ctor.fields.len()))
    }
}

/// data の値の Repr。フィールドのないコンストラクタだけなら `enum`、どれもフィールドを持てば `obj`、混ざれば `tobj`
/// である。コンストラクタのない型は `enum` になる。
pub fn data_repr(arities: impl IntoIterator<Item = usize>) -> Repr {
    let (mut count, mut with_fields) = (0, 0);
    for arity in arities {
        count += 1;
        with_fields += usize::from(arity > 0);
    }
    if with_fields == 0 {
        Repr::Enum
    } else if with_fields == count {
        Repr::Obj
    } else {
        Repr::TObj
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
    /// 操作の引数の Repr。型変数は `tobj` にする (docs/spec/core-ir.md の
    /// 「エフェクトの位置」)。verifier は長さで節の関数の引数の数を確かめる (docs/spec/core-ir.md)。
    pub params: Vec<Repr>,
    pub ret: OpRet,
    /// `never` の操作は再開しないので、継続を作らずに捨てる (docs/spec/effects.md)。
    pub resumable: bool,
}

/// エフェクトの表の操作。番号が表にあることを verifier が確かめた後のパスが使う。関数の並びを書き換えながら引けるように、
/// `Program` でなく表だけを借りる。
pub fn operation(effects: &[EffectInfo], effect: u32, op: u32) -> &OperationInfo {
    &effects[effect as usize].operations[op as usize]
}

/// 操作の結果。`never` の操作は戻らないので、結果を呼び出した側の位置と比べない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpRet {
    Repr(Repr),
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FnIdx(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VarId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarInfo {
    pub name: String,
    pub repr: Repr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub u32);

impl BlockId {
    pub const ENTRY: BlockId = BlockId(0);
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

/// ソースの位置。`file` は `Program.files` の添字で、`line` と `column` は1から数える (`column` は文字の位置)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Loc {
    pub file: u32,
    pub line: u32,
    pub column: u32,
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

impl Atom {
    /// 変数は Repr が同じとき、定数はその Repr の値になれるときに、`vars` の関数の中で `expected` の位置に収まる (R8)。
    pub fn fits(self, vars: &[VarInfo], expected: Repr) -> bool {
        match self {
            Atom::Var(var) => vars[var.0 as usize].repr == expected,
            Atom::Int(_) => expected == Repr::Int,
            Atom::Unit => expected == Repr::Unit,
            // 引数のないコンストラクタは、`enum` のタグにも、`tobj` の即値にもなる
            Atom::Tag(_) => matches!(expected, Repr::Enum | Repr::TObj),
            Atom::Fn(_) => expected == Repr::TObj,
        }
    }

    /// 互換の位置 (`jump`、`return`、呼び出しのオペランド、`closure` の引数、`tobj` のフィールド) に、命令なしで
    /// 収まる値。変数は Repr が互換なときに収まる。定数は `fits` に加えて、`()` が `tobj` にも収まる。`Int` の定数は
    /// `tobj` に収まらないので、box の挿入が `box` する (docs/spec/core-ir.md の「値の表現」)。box の挿入と verifier が
    /// 同じ規則で位置を比べるように、ここで1回だけ定義する。
    pub fn fits_compatible(self, vars: &[VarInfo], expected: Repr) -> bool {
        match self {
            Atom::Var(var) => vars[var.0 as usize].repr.compatible(expected),
            Atom::Unit => expected.compatible(Repr::Unit),
            Atom::Int(_) | Atom::Tag(_) | Atom::Fn(_) => self.fits(vars, expected),
        }
    }
}

/// `Bool` のタグ (docs/spec/core-ir.md)。
pub const FALSE: u32 = 0;
pub const TRUE: u32 = 1;

/// `Ordering` のタグ。`compare` の extern が配置の表を見ずに作る
/// (docs/spec/declarations.md の「Prelude のクラス」)。
pub const LT: u32 = 0;
pub const EQ: u32 = 1;
pub const GT: u32 = 2;

/// タプルの値のタグ。タプルは、コンストラクタが1つの `data` と同じオブジェクトで表す (docs/spec/core-ir.md)。
pub const TUPLE: u32 = 0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyError {
    pub function: String,
    pub message: String,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} in `{}`", self.message, self.function)
    }
}

impl std::error::Error for VerifyError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 1 から数える行の番号。
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}
