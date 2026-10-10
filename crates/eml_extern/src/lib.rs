//! extern の表。標準ライブラリ (`std/`) の extern の宣言ごとに、処理系が知る性質を1行に持つ。HIR、型検査、Core IR、
//! インタプリタはこの表を引き、名前の文字列を照らし合わせない。ネイティブ化では、この表がランタイムとの ABI の一覧に
//! なる (docs/future/roadmap.md)。
//!
//! 行の正式な名前は、`std/` の宣言をモジュールの正式な名前で修飾したもの (`Prelude.Int`、`Prelude.+`) である。
//! instance の `extern` で結ぶメソッドの行は、モジュール、クラス、型、メソッドの名前を並べた `Prelude.Eq Int.==` の
//! 形である。どの行も、`std/` の extern の宣言か instance の `extern` のちょうど1つから指されることは、`eml_hir` の
//! 結合テストが確かめる。行の Repr が std の宣言の型の Repr と同じであることは、`eml_core_ir` の結合テストが確かめる。

/// 型の Kind。extern の型は型引数を持たないので、`Unr` か `Lin` だけで決まる。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Linearity {
    Unr,
    Lin,
}

/// 関数を呼んだときに起こりうること。縮約のパスは `Pure` の呼び出しだけを、使われなければ消す (docs/spec/core-ir.md の「縮約」)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Purity {
    Pure,
    /// 実行時エラーで止まりうる (整数のオーバーフローとゼロ除算)。
    MayFail,
    /// extern のエフェクトを起こす。どのエフェクトかは宣言の型から読む。
    Effectful,
}

/// 値の表現。Core IR の変数と extern の行が持ち、RC の対象かどうかもここから決まる (docs/spec/core-ir.md)。
/// extern の行が持つので、依存のないこの crate に置き、`eml_core_ir` が再公開する。
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
    /// 箱を要するスカラー。`tobj` の位置との間で `box` と `unbox` を要する。`unit` は命令なしで `tobj` と行き来する
    /// ので入らない。規則と verifier の文言はこの1つの定義から作るので、後の `Float` の Repr もここに足す
    /// (docs/spec/core-ir.md の「値の表現」)。
    pub const BOXED_SCALARS: [Repr; 2] = [Repr::Int, Repr::Enum];

    pub fn is_rc(self) -> bool {
        match self {
            Repr::Obj | Repr::TObj => true,
            Repr::Int | Repr::Enum | Repr::Unit => false,
        }
    }

    pub fn needs_box(self) -> bool {
        Repr::BOXED_SCALARS.contains(&self)
    }

    /// 互換の位置で、命令なしで値を渡せる2つの Repr。同じ Repr、参照どうし、`unit` と `tobj` である。推移的でない
    /// (`unit` と `obj` は互換でない) ので、どの検査も実際の2つの位置を比べる (docs/spec/core-ir.md の「値の表現」)。
    pub fn compatible(self, other: Repr) -> bool {
        self == other
            || (self.is_rc() && other.is_rc())
            || matches!(
                (self, other),
                (Repr::Unit, Repr::TObj) | (Repr::TObj, Repr::Unit)
            )
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
    /// 値の Repr。translate は extern の型の Repr をここから読む。型検査は `Unit` を要素のないタプルにするので、translate は
    /// `Unit` の行を読まない。この行が要素のないタプルの Repr と同じであることは、結合テストが確かめる。
    pub repr: Repr,
    /// 型検査は `Unit` を要素のないタプルとして扱うので、この行は型の形と表示にだけ使う。
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
        let row = |name, linearity, repr, empty_record| TypeRow {
            name,
            linearity,
            repr,
            empty_record,
        };
        match self {
            ExternType::Int => row("Prelude.Int", Linearity::Unr, Repr::Int, false),
            ExternType::String => row("Prelude.String", Linearity::Unr, Repr::Obj, false),
            ExternType::Unit => row("Prelude.Unit", Linearity::Unr, Repr::Unit, true),
            ExternType::File => row("Std.Fs.File", Linearity::Lin, Repr::Obj, false),
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
    IntShow,
    IntNeg,
    IntAdd,
    IntSub,
    IntMul,
    IntDiv,
    IntMod,
    IntEq,
    IntNe,
    IntCompare,
    IntLt,
    IntLe,
    IntGt,
    IntGe,
    StrConcat,
    StrEq,
    StrNe,
    StrCompare,
    StrShow,
    BoolEq,
    BoolNe,
}

pub struct FunctionRow {
    pub name: &'static str,
    /// 引数の Repr。数はシグネチャの一番外側の `->` の数と等しい。
    pub params: &'static [Repr],
    /// 結果の Repr。データを返す行は、比べる extern の `Bool`、`compare` の `Ordering`、`Std.Fs.read_all` の組である。
    /// `Bool` と `Ordering` のタグは決まっていて、組の配置は大きさだけで決まるので、行は配置を持たない
    /// (docs/spec/core-ir.md の「データの配置」)。
    pub ret: Repr,
    pub purity: Purity,
}

impl Extern {
    pub const ALL: &[Extern] = &[
        Extern::Println,
        Extern::Open,
        Extern::ReadAll,
        Extern::Close,
        Extern::IntShow,
        Extern::IntNeg,
        Extern::IntAdd,
        Extern::IntSub,
        Extern::IntMul,
        Extern::IntDiv,
        Extern::IntMod,
        Extern::IntEq,
        Extern::IntNe,
        Extern::IntCompare,
        Extern::IntLt,
        Extern::IntLe,
        Extern::IntGt,
        Extern::IntGe,
        Extern::StrConcat,
        Extern::StrEq,
        Extern::StrNe,
        Extern::StrCompare,
        Extern::StrShow,
        Extern::BoolEq,
        Extern::BoolNe,
    ];

    pub fn row(self) -> FunctionRow {
        use Repr::{Enum, Int, Obj, Unit};
        let row = |name, params, ret, purity| FunctionRow {
            name,
            params,
            ret,
            purity,
        };
        match self {
            Extern::Println => row("Prelude.println", &[Obj], Unit, Purity::Effectful),
            Extern::Open => row("Std.Fs.open", &[Obj], Obj, Purity::Effectful),
            Extern::ReadAll => row("Std.Fs.read_all", &[Obj], Obj, Purity::Effectful),
            Extern::Close => row("Std.Fs.close", &[Obj], Unit, Purity::Effectful),
            Extern::IntShow => row("Prelude.Show Int.show", &[Int], Obj, Purity::Pure),
            Extern::IntNeg => row("Prelude.negate", &[Int], Int, Purity::MayFail),
            Extern::IntAdd => row("Prelude.+", &[Int, Int], Int, Purity::MayFail),
            Extern::IntSub => row("Prelude.-", &[Int, Int], Int, Purity::MayFail),
            Extern::IntMul => row("Prelude.*", &[Int, Int], Int, Purity::MayFail),
            Extern::IntDiv => row("Prelude./", &[Int, Int], Int, Purity::MayFail),
            Extern::IntMod => row("Prelude.%", &[Int, Int], Int, Purity::MayFail),
            Extern::IntEq => row("Prelude.Eq Int.==", &[Int, Int], Enum, Purity::Pure),
            Extern::IntNe => row("Prelude.Eq Int.!=", &[Int, Int], Enum, Purity::Pure),
            Extern::IntCompare => row("Prelude.Ord Int.compare", &[Int, Int], Enum, Purity::Pure),
            Extern::IntLt => row("Prelude.Ord Int.<", &[Int, Int], Enum, Purity::Pure),
            Extern::IntLe => row("Prelude.Ord Int.<=", &[Int, Int], Enum, Purity::Pure),
            Extern::IntGt => row("Prelude.Ord Int.>", &[Int, Int], Enum, Purity::Pure),
            Extern::IntGe => row("Prelude.Ord Int.>=", &[Int, Int], Enum, Purity::Pure),
            Extern::StrConcat => row("Prelude.++", &[Obj, Obj], Obj, Purity::Pure),
            Extern::StrEq => row("Prelude.Eq String.==", &[Obj, Obj], Enum, Purity::Pure),
            Extern::StrNe => row("Prelude.Eq String.!=", &[Obj, Obj], Enum, Purity::Pure),
            Extern::StrCompare => row(
                "Prelude.Ord String.compare",
                &[Obj, Obj],
                Enum,
                Purity::Pure,
            ),
            Extern::StrShow => row("Prelude.Show String.show", &[Obj], Obj, Purity::Pure),
            Extern::BoolEq => row("Prelude.Eq Bool.==", &[Enum, Enum], Enum, Purity::Pure),
            Extern::BoolNe => row("Prelude.Eq Bool.!=", &[Enum, Enum], Enum, Purity::Pure),
        }
    }

    pub fn from_name(name: &str) -> Option<Extern> {
        Self::ALL.iter().copied().find(|e| e.row().name == name)
    }
}
