//! extern の表。標準ライブラリ (`std/`) の extern の宣言ごとに、処理系が知る性質を1行に持つ。HIR、型検査、Core IR、
//! インタプリタはこの表を引き、名前の文字列を照らし合わせない。ネイティブ化では、この表がランタイムとの ABI の一覧に
//! なる (docs/future/roadmap.md)。
//!
//! 行の正式な名前は、`std/` の宣言をモジュールの正式な名前で修飾したもの (`Prelude.Int`、`Prelude.+`) である。
//! どの行も `std/` でちょうど1回宣言されていることは、`eml_hir` の結合テストが確かめる。

/// 型の Kind。extern の型は型引数を持たないので、`Unr` か `Lin` だけで決まる。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Linearity {
    Unr,
    Lin,
}

/// 関数を呼んだときに起こりうること。`simplify` は `Pure` の呼び出しだけを、使われなければ消す。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Purity {
    Pure,
    /// 実行時エラーで止まりうる (整数のオーバーフローとゼロ除算)。
    MayFail,
    /// extern のエフェクトを起こす。どのエフェクトかは宣言の型から読む。
    Effectful,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExternType {
    Int,
    String,
    Unit,
    File,
}

pub struct TypeRow {
    pub name: &'static str,
    pub linearity: Linearity,
    /// 値がヒープのオブジェクトか。translate が箱に入れるかを決めるのに使う。
    pub heap: bool,
    /// 型検査は `Unit` を空のレコード `{}` として扱うので、この行は型の形と表示にだけ使う。
    pub empty_record: bool,
}

impl ExternType {
    pub const ALL: &[ExternType] = &[
        ExternType::Int,
        ExternType::String,
        ExternType::Unit,
        ExternType::File,
    ];

    pub fn row(self) -> TypeRow {
        let row = |name, linearity, heap, empty_record| TypeRow {
            name,
            linearity,
            heap,
            empty_record,
        };
        match self {
            ExternType::Int => row("Prelude.Int", Linearity::Unr, false, false),
            ExternType::String => row("Prelude.String", Linearity::Unr, true, false),
            ExternType::Unit => row("Prelude.Unit", Linearity::Unr, false, true),
            ExternType::File => row("Prelude.File", Linearity::Lin, true, false),
        }
    }

    pub fn from_name(name: &str) -> Option<ExternType> {
        Self::ALL.iter().copied().find(|ty| ty.row().name == name)
    }
}

/// 操作を持たないラベルのエフェクト。`Effectful` の extern の関数が起こす。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExternEffect {
    Io,
}

pub struct EffectRow {
    pub name: &'static str,
}

impl ExternEffect {
    pub const ALL: &[ExternEffect] = &[ExternEffect::Io];

    pub fn row(self) -> EffectRow {
        match self {
            ExternEffect::Io => EffectRow { name: "Prelude.IO" },
        }
    }

    pub fn from_name(name: &str) -> Option<ExternEffect> {
        Self::ALL.iter().copied().find(|e| e.row().name == name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Extern {
    Println,
    Open,
    ReadAll,
    Close,
    ShowInt,
    IntNeg,
    IntAdd,
    IntSub,
    IntMul,
    IntDiv,
    IntMod,
    IntLt,
    IntLe,
    IntGt,
    IntGe,
    StrConcat,
    Eq,
    Ne,
    IntEq,
    IntNe,
    StrEq,
    StrNe,
    BoolEq,
    BoolNe,
}

pub struct FunctionRow {
    pub name: &'static str,
    /// 引数の数。シグネチャの一番外側の `->` の数と等しい。
    pub arity: usize,
    pub purity: Purity,
    /// 型で選ぶ行。translate が型引数から比べ方の行に置き換えるので、Core IR には届かない。
    pub by_type: bool,
}

impl Extern {
    pub const ALL: &[Extern] = &[
        Extern::Println,
        Extern::Open,
        Extern::ReadAll,
        Extern::Close,
        Extern::ShowInt,
        Extern::IntNeg,
        Extern::IntAdd,
        Extern::IntSub,
        Extern::IntMul,
        Extern::IntDiv,
        Extern::IntMod,
        Extern::IntLt,
        Extern::IntLe,
        Extern::IntGt,
        Extern::IntGe,
        Extern::StrConcat,
        Extern::Eq,
        Extern::Ne,
        Extern::IntEq,
        Extern::IntNe,
        Extern::StrEq,
        Extern::StrNe,
        Extern::BoolEq,
        Extern::BoolNe,
    ];

    pub fn row(self) -> FunctionRow {
        let row = |name, arity, purity| FunctionRow {
            name,
            arity,
            purity,
            by_type: false,
        };
        match self {
            Extern::Println => row("Prelude.println", 1, Purity::Effectful),
            Extern::Open => row("Prelude.open", 1, Purity::Effectful),
            Extern::ReadAll => row("Prelude.read_all", 1, Purity::Effectful),
            Extern::Close => row("Prelude.close", 1, Purity::Effectful),
            Extern::ShowInt => row("Prelude.show_int", 1, Purity::Pure),
            Extern::IntNeg => row("Prelude.negate", 1, Purity::MayFail),
            Extern::IntAdd => row("Prelude.+", 2, Purity::MayFail),
            Extern::IntSub => row("Prelude.-", 2, Purity::MayFail),
            Extern::IntMul => row("Prelude.*", 2, Purity::MayFail),
            Extern::IntDiv => row("Prelude./", 2, Purity::MayFail),
            Extern::IntMod => row("Prelude.%", 2, Purity::MayFail),
            Extern::IntLt => row("Prelude.<", 2, Purity::Pure),
            Extern::IntLe => row("Prelude.<=", 2, Purity::Pure),
            Extern::IntGt => row("Prelude.>", 2, Purity::Pure),
            Extern::IntGe => row("Prelude.>=", 2, Purity::Pure),
            Extern::StrConcat => row("Prelude.++", 2, Purity::Pure),
            Extern::Eq => FunctionRow {
                by_type: true,
                ..row("Prelude.==", 2, Purity::Pure)
            },
            Extern::Ne => FunctionRow {
                by_type: true,
                ..row("Prelude.!=", 2, Purity::Pure)
            },
            Extern::IntEq => row("Prelude.int_eq", 2, Purity::Pure),
            Extern::IntNe => row("Prelude.int_ne", 2, Purity::Pure),
            Extern::StrEq => row("Prelude.string_eq", 2, Purity::Pure),
            Extern::StrNe => row("Prelude.string_ne", 2, Purity::Pure),
            Extern::BoolEq => row("Prelude.bool_eq", 2, Purity::Pure),
            Extern::BoolNe => row("Prelude.bool_ne", 2, Purity::Pure),
        }
    }

    pub fn from_name(name: &str) -> Option<Extern> {
        Self::ALL.iter().copied().find(|e| e.row().name == name)
    }
}
