//! extern の表。標準ライブラリ (`std/`) の extern の宣言ごとに、処理系が知る性質を1行に持つ。HIR、型検査、Core IR、
//! LIR、VM はこの表を引き、名前の文字列を照らし合わせない。ネイティブ化では、この表がランタイムとの ABI の一覧に
//! なる (docs/future/roadmap.md)。
//!
//! 行の正式な名前は、`std/` の宣言をモジュールの正式な名前で修飾したもの (`Prelude.Int`、`Prelude.++`) である。
//! instance の `extern` で結ぶメソッドの行は、モジュール、クラス、型、メソッドの名前を並べた `Prelude.Eq Int.==` の
//! 形である。どの行も、`std/` の extern の宣言か instance の `extern` のちょうど1つから指されることは、`eml_hir` の
//! 結合テストが確かめる。行の Repr が std の宣言の型の Repr と同じであることは、`eml_core_ir` の結合テストが確かめる。

/// 型の Kind。型引数のない extern の型は `Unr` か `Lin` で決まる。型引数を持つ型 (`TypeRow::params > 0`) の `Unr` は
/// 「引数の Kind の join」を表し、`Lin` は引数によらず常に `Lin` である (docs/spec/types.md)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Linearity {
    Unr,
    Lin,
}

/// 関数を呼んだときに起こりうること。縮約のパスは `Pure` の呼び出しだけを、使われなければ消す (docs/spec/lir.md の「縮約」)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Purity {
    Pure,
    /// `Panic` を起こしうる (整数の桁あふれとゼロ除算、文字列の範囲外の添字、利用者の `panic`)。
    Panics,
    /// `IO` を起こす。`Std.Env.arg` のように、`Panic` も起こしうる行がある。どのエフェクトかは宣言の型から読む
    /// (docs/spec/effects.md の「組み込みの `Panic`」)。
    Effectful,
}

/// `Float` のリテラルと `float` の定数のビット列。`f64` は `Eq` と `Hash` を持たず、`0.0 == -0.0` で NaN が自分と
/// 等しくないので、定数はビット列で比べる。定数の表で `0.0` と `-0.0` をまとめないためである
/// (docs/spec/lir.md の「値の表現」)。HIR のリテラルも持つので、依存のないこの crate に置き、`eml_lir` が再公開する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FloatBits(pub u64);

impl FloatBits {
    pub fn new(value: f64) -> FloatBits {
        FloatBits(value.to_bits())
    }

    pub fn value(self) -> f64 {
        f64::from_bits(self.0)
    }
}

/// IR のテキストの形。Rust の `{:?}` の形 (`1.5`、`1e16`、`-0.0`、`inf`、`NaN`) で、`parse` が読み戻す
/// (docs/implementation/testing.md の「LIR のテキストの形」)。
impl std::fmt::Display for FloatBits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.value())
    }
}

/// 値の表現。IR の変数と extern の行が持ち、RC の対象かどうかもここから決まる (docs/spec/lir.md)。
/// extern の行が持つので、依存のないこの crate に置き、`eml_lir` が再公開する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Repr {
    /// つねにヒープの物体を指す値。
    Obj,
    /// 即値 (引数のないコンストラクタのタグ、捕まえた変数のない関数) か、ヒープの物体を指す値。
    TObj,
    Int,
    /// IEEE 754 の倍精度の浮動小数。変数、ブロックの引数、`con` のフィールドでは生の 64 ビットで持つ。
    Float,
    /// 引数のないコンストラクタだけの data のタグ。
    Enum,
    Unit,
    /// reuse の token。解放した箱を指すか、空を表す1語で、RC の対象ではない。LIR の reuse のパスだけが作り、extern の
    /// 行、配置、操作の表、Core IR には現れない (docs/spec/object-model.md の「reuse」)。
    Token,
    /// 構造体の値。番号は Program の形の表 (`eml_lir::Structs`) を引く。形を番号で指すのは、`Repr` を `Copy` に保つため
    /// である。extern の行、配置、操作の表には現れない
    /// (docs/spec/lir.md の「構造体の形」)。
    Struct(StructId),
}

/// 形の表の番号。表は同じ中身の形に同じ番号を付けるので、番号が等しいことと中身が等しいことは同じである。番号は
/// 1つの Program の中だけで意味を持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StructId(pub u32);

impl Repr {
    pub fn is_rc(self) -> bool {
        match self {
            Repr::Obj | Repr::TObj => true,
            Repr::Int | Repr::Float | Repr::Enum | Repr::Unit | Repr::Token | Repr::Struct(_) => {
                false
            }
        }
    }

    /// 箱を要する Repr。一様な位置との間で `box` と `unbox` を要する。`unit` は命令なしで `tobj` と行き来するので
    /// 入らない。box の挿入の規則と verifier はこの述語から作る (docs/spec/lir.md の「値の表現」、
    /// docs/spec/lir.md の「構造体の形」)。
    pub fn needs_box(self) -> bool {
        matches!(self, Repr::Int | Repr::Float | Repr::Enum | Repr::Struct(_))
    }

    /// 互換の位置で、命令なしで値を渡せる2つの Repr。同じ Repr、参照どうし、`unit` と `tobj` である。推移的でない
    /// (`unit` と `obj` は互換でない) ので、どの検査も実際の2つの位置を比べる (docs/spec/lir.md の「値の表現」)。
    /// 構造体どうしは番号が等しいときだけ互換とする。形を見ないので、印だけが違う形は互換にならない。形の表を
    /// 持つ所は `eml_lir::Structs::compatible` で比べる。
    pub fn compatible(self, other: Repr) -> bool {
        self == other
            || (self.is_rc() && other.is_rc())
            || matches!(
                (self, other),
                (Repr::Unit, Repr::TObj) | (Repr::TObj, Repr::Unit)
            )
    }

    /// テキストの形での名前 (docs/implementation/testing.md の「LIR のテキストの形」)。構造体は `struct` を返す。形の
    /// 中身は表を引かないと書けないので、構造体のテキストの形は `eml_lir` が書く。
    pub fn name(self) -> &'static str {
        match self {
            Repr::Obj => "obj",
            Repr::TObj => "tobj",
            Repr::Int => "int",
            Repr::Float => "float",
            Repr::Enum => "enum",
            Repr::Unit => "unit",
            Repr::Token => "token",
            Repr::Struct(_) => "struct",
        }
    }
}

/// 操作と extern の関数の結果。`never` の操作と戻らない extern (`exit`) は戻らないので、結果を呼び出した側の位置と比べない
/// (docs/spec/lir.md の「戻らない extern」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpRet {
    Repr(Repr),
    Never,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExternType {
    Int,
    Float,
    String,
    Unit,
    File,
    Array,
    MutArray,
}

pub struct TypeRow {
    pub name: &'static str,
    pub linearity: Linearity,
    /// 型引数の数。型引数はすべて共変で、`Unr` の行の Kind は引数の Kind に従う。
    pub params: usize,
    /// 値の Repr。translate は extern の型の Repr をここから読む。型検査は `Unit` を要素のないタプルにするので、translate は
    /// `Unit` の行を読まない。この行が要素のないタプルの Repr と同じであることは、結合テストが確かめる。
    pub repr: Repr,
}

impl ExternType {
    pub const ALL: &[ExternType] = &[
        ExternType::Int,
        ExternType::Float,
        ExternType::String,
        ExternType::Unit,
        ExternType::File,
        ExternType::Array,
        ExternType::MutArray,
    ];

    pub fn row(self) -> TypeRow {
        let row = |name, linearity, repr| TypeRow {
            name,
            linearity,
            params: 0,
            repr,
        };
        let generic = |name, linearity| TypeRow {
            params: 1,
            ..row(name, linearity, Repr::Obj)
        };
        match self {
            ExternType::Int => row("Prelude.Int", Linearity::Unr, Repr::Int),
            ExternType::Float => row("Prelude.Float", Linearity::Unr, Repr::Float),
            ExternType::String => row("Prelude.String", Linearity::Unr, Repr::Obj),
            ExternType::Unit => row("Prelude.Unit", Linearity::Unr, Repr::Unit),
            ExternType::File => row("Std.Fs.File", Linearity::Lin, Repr::Obj),
            ExternType::Array => generic("Prelude.Array", Linearity::Unr),
            ExternType::MutArray => generic("Prelude.MutArray", Linearity::Lin),
        }
    }

    pub fn from_name(name: &str) -> Option<ExternType> {
        Self::ALL.iter().copied().find(|ty| ty.row().name == name)
    }
}

/// extern のエフェクト。`IO` は `Effectful` の extern の関数が起こし、`Panic` は `Panics` の行と `Std.Env.arg` が起こす。
/// どちらも隠れた `never` の操作 (`io_failed`、`panicked`) を Prelude の宣言が持ち、Prelude の `try_io`、`try_panic`
/// だけが handle する。表の行は名前だけを持つ (docs/spec/effects.md の「組み込みの `IO`」「組み込みの `Panic`」)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExternEffect {
    Io,
    Panic,
}

pub struct EffectRow {
    pub name: &'static str,
}

impl ExternEffect {
    pub const ALL: &[ExternEffect] = &[ExternEffect::Io, ExternEffect::Panic];

    pub fn row(self) -> EffectRow {
        match self {
            ExternEffect::Io => EffectRow { name: "Prelude.IO" },
            ExternEffect::Panic => EffectRow {
                name: "Prelude.Panic",
            },
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
    ReadLine,
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
    FloatNeg,
    FloatAdd,
    FloatSub,
    FloatMul,
    FloatDiv,
    FloatMod,
    FloatEq,
    FloatNe,
    FloatCompare,
    FloatLt,
    FloatLe,
    FloatGt,
    FloatGe,
    FloatShow,
    FloatShowsMinus,
    FloatOfInt,
    FloatTruncate,
    FloatFloor,
    FloatCeil,
    FloatRound,
    FloatSqrt,
    FloatAbs,
    FloatSin,
    FloatCos,
    FloatExp,
    FloatLog,
    FloatPow,
    FloatInfinity,
    FloatNan,
    FloatToFixed,
    FloatCanParse,
    FloatParseRaw,
    StrConcat,
    StrEq,
    StrNe,
    StrCompare,
    StrShow,
    BoolEq,
    BoolNe,
    Eprintln,
    StrByteLength,
    StrByteAt,
    StrSlice,
    StrFindFrom,
    EnvArgCount,
    EnvArg,
    EnvHas,
    EnvLookup,
    StdinReadAll,
    StdinReadLine,
    WriteText,
    ProcStatus,
    Exit,
    Panic,
    ArrayMake,
    ArrayLength,
    ArrayGet,
    ArraySet,
    ArrayThaw,
    MutArrayMake,
    MutArrayEmpty,
    MutArrayLength,
    MutArrayRead,
    MutArrayWrite,
    MutArrayFreeze,
    CheckIndex,
}

pub struct FunctionRow {
    pub name: &'static str,
    /// 引数の Repr。数はシグネチャの一番外側の `->` の数と等しい。
    pub params: &'static [Repr],
    /// 結果の Repr。`Never` は戻らない extern で、`ret_kind` は読まない。データを返す行は、比べる extern、
    /// `Std.Env.has`、`Prelude.shows_minus`、`Std.Float.can_parse` の `Bool`、`compare` の `Ordering`、
    /// `Std.Fs.read_all` と `read_line_raw` の組である。`Bool` と `Ordering` のタグは決まっていて、組の配置は大きさ
    /// だけで決まるので、行は配置を持たない。`List` や `Option` は extern が作らず、標準ライブラリの eml が包む
    /// (docs/spec/core-ir.md の「translate の配置」)。
    pub ret: OpRet,
    /// 引数ごとの Kind。長さは `params` と同じである。LIR の verifier が、呼び出しの引数と結果の印をこの列と比べる
    /// (docs/spec/core-ir.md の「表の印の列」)。
    pub param_kinds: &'static [Linearity],
    pub ret_kind: Linearity,
    pub purity: Purity,
    /// 引数ごとの借用の印。長さは `params` と同じである。印の立つ引数を extern は読むだけで消費せず、失敗したときも
    /// 呼んだ側が持ったままである。配列を借りて読む行だけが持つ (docs/spec/lir.md の「extern の借用」)。
    pub borrowed: &'static [bool],
    /// `Some(k)` の行は、宣言の型では結果と引数 `k` の組を返す。行の結果 (`ret`、`ret_kind`) は組の最初の要素で、
    /// translate が呼び出しの後に `con` で組を作る。組を受けてすぐ開く所では、最適化の既知のコンストラクタで組が
    /// 消える (docs/spec/core-ir.md の「組を返す extern」)。
    pub with_arg: Option<usize>,
}

impl FunctionRow {
    /// 結果を使わない呼び出しを消してよい行。`Pure` で、借りずに受ける `Lin` の引数を持たない行である。`Lin` の値を
    /// 消費する呼び出しを消すと、その値を手放す命令がなくなり、Perceus が `lin` の変数を `decref` するためである
    /// (docs/spec/lir.md の「縮約」)。
    pub fn removable(&self) -> bool {
        self.purity == Purity::Pure
            && self
                .param_kinds
                .iter()
                .zip(self.borrowed)
                .all(|(&kind, &borrowed)| borrowed || kind == Linearity::Unr)
    }
}

impl Extern {
    pub const ALL: &[Extern] = &[
        Extern::Println,
        Extern::Open,
        Extern::ReadAll,
        Extern::ReadLine,
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
        Extern::FloatNeg,
        Extern::FloatAdd,
        Extern::FloatSub,
        Extern::FloatMul,
        Extern::FloatDiv,
        Extern::FloatMod,
        Extern::FloatEq,
        Extern::FloatNe,
        Extern::FloatCompare,
        Extern::FloatLt,
        Extern::FloatLe,
        Extern::FloatGt,
        Extern::FloatGe,
        Extern::FloatShow,
        Extern::FloatShowsMinus,
        Extern::FloatOfInt,
        Extern::FloatTruncate,
        Extern::FloatFloor,
        Extern::FloatCeil,
        Extern::FloatRound,
        Extern::FloatSqrt,
        Extern::FloatAbs,
        Extern::FloatSin,
        Extern::FloatCos,
        Extern::FloatExp,
        Extern::FloatLog,
        Extern::FloatPow,
        Extern::FloatInfinity,
        Extern::FloatNan,
        Extern::FloatToFixed,
        Extern::FloatCanParse,
        Extern::FloatParseRaw,
        Extern::StrConcat,
        Extern::StrEq,
        Extern::StrNe,
        Extern::StrCompare,
        Extern::StrShow,
        Extern::BoolEq,
        Extern::BoolNe,
        Extern::Eprintln,
        Extern::StrByteLength,
        Extern::StrByteAt,
        Extern::StrSlice,
        Extern::StrFindFrom,
        Extern::EnvArgCount,
        Extern::EnvArg,
        Extern::EnvHas,
        Extern::EnvLookup,
        Extern::StdinReadAll,
        Extern::StdinReadLine,
        Extern::WriteText,
        Extern::ProcStatus,
        Extern::Exit,
        Extern::Panic,
        Extern::ArrayMake,
        Extern::ArrayLength,
        Extern::ArrayGet,
        Extern::ArraySet,
        Extern::ArrayThaw,
        Extern::MutArrayMake,
        Extern::MutArrayEmpty,
        Extern::MutArrayLength,
        Extern::MutArrayRead,
        Extern::MutArrayWrite,
        Extern::MutArrayFreeze,
        Extern::CheckIndex,
    ];

    pub fn row(self) -> FunctionRow {
        use Linearity::{Lin, Unr};
        use Repr::{Enum, Float, Int, Obj, TObj, Unit};
        // 引数の Repr と Kind を同じ長さの配列で受け、`params` と `param_kinds` の長さを型で揃える
        fn kinded<const N: usize>(
            name: &'static str,
            params: &'static [Repr; N],
            param_kinds: &'static [Linearity; N],
            ret: Repr,
            ret_kind: Linearity,
            purity: Purity,
        ) -> FunctionRow {
            FunctionRow {
                name,
                params,
                ret: OpRet::Repr(ret),
                param_kinds,
                ret_kind,
                purity,
                borrowed: const { &[false; N] },
                with_arg: None,
            }
        }
        // 配列を借りて読む行。結果はどれも `Unr` である (要素の型変数は `Unr` に固定する)
        fn lending<const N: usize>(
            name: &'static str,
            params: &'static [Repr; N],
            param_kinds: &'static [Linearity; N],
            ret: Repr,
            purity: Purity,
            with_arg: Option<usize>,
        ) -> FunctionRow {
            let mut row = kinded(name, params, param_kinds, ret, Unr, purity);
            // 借りるのは先頭の配列だけである
            row.borrowed = const {
                let mut borrowed = [false; N];
                borrowed[0] = true;
                &{ borrowed }
            };
            row.with_arg = with_arg;
            row
        }
        // 線形な値を扱わない行は、すべて `Unr` にする
        fn row<const N: usize>(
            name: &'static str,
            params: &'static [Repr; N],
            ret: Repr,
            purity: Purity,
        ) -> FunctionRow {
            kinded(name, params, const { &[Unr; N] }, ret, Unr, purity)
        }
        match self {
            Extern::Println => row("Prelude.println", &[Obj], Unit, Purity::Effectful),
            Extern::Open => kinded("Std.Fs.open", &[Obj], &[Unr], Obj, Lin, Purity::Effectful),
            Extern::ReadAll => kinded(
                "Std.Fs.read_all",
                &[Obj],
                &[Lin],
                Obj,
                Lin,
                Purity::Effectful,
            ),
            Extern::ReadLine => kinded(
                "Std.Fs.read_line_raw",
                &[Obj],
                &[Lin],
                Obj,
                Lin,
                Purity::Effectful,
            ),
            Extern::Close => kinded("Std.Fs.close", &[Obj], &[Lin], Unit, Unr, Purity::Effectful),
            Extern::IntShow => row("Prelude.Show Int.show", &[Int], Obj, Purity::Pure),
            Extern::IntNeg => row("Prelude.Num Int.negate", &[Int], Int, Purity::Panics),
            Extern::IntAdd => row("Prelude.Num Int.+", &[Int, Int], Int, Purity::Panics),
            Extern::IntSub => row("Prelude.Num Int.-", &[Int, Int], Int, Purity::Panics),
            Extern::IntMul => row("Prelude.Num Int.*", &[Int, Int], Int, Purity::Panics),
            Extern::IntDiv => row("Prelude.Div Int./", &[Int, Int], Int, Purity::Panics),
            Extern::IntMod => row("Prelude.Div Int.%", &[Int, Int], Int, Purity::Panics),
            Extern::IntEq => row("Prelude.Eq Int.==", &[Int, Int], Enum, Purity::Pure),
            Extern::IntNe => row("Prelude.Eq Int.!=", &[Int, Int], Enum, Purity::Pure),
            Extern::IntCompare => row("Prelude.Ord Int.compare", &[Int, Int], Enum, Purity::Pure),
            Extern::IntLt => row("Prelude.Ord Int.<", &[Int, Int], Enum, Purity::Pure),
            Extern::IntLe => row("Prelude.Ord Int.<=", &[Int, Int], Enum, Purity::Pure),
            Extern::IntGt => row("Prelude.Ord Int.>", &[Int, Int], Enum, Purity::Pure),
            Extern::IntGe => row("Prelude.Ord Int.>=", &[Int, Int], Enum, Purity::Pure),
            // `Float` の算術は `Panic` を起こさない。メソッドの型はクラスで1つなので `<Panic>` を持つが、行は `Pure` である
            // (docs/spec/declarations.md の「Prelude のクラス」)
            Extern::FloatNeg => row("Prelude.Num Float.negate", &[Float], Float, Purity::Pure),
            Extern::FloatAdd => row("Prelude.Num Float.+", &[Float, Float], Float, Purity::Pure),
            Extern::FloatSub => row("Prelude.Num Float.-", &[Float, Float], Float, Purity::Pure),
            Extern::FloatMul => row("Prelude.Num Float.*", &[Float, Float], Float, Purity::Pure),
            Extern::FloatDiv => row("Prelude.Div Float./", &[Float, Float], Float, Purity::Pure),
            Extern::FloatMod => row("Prelude.Div Float.%", &[Float, Float], Float, Purity::Pure),
            Extern::FloatEq => row("Prelude.Eq Float.==", &[Float, Float], Enum, Purity::Pure),
            Extern::FloatNe => row("Prelude.Eq Float.!=", &[Float, Float], Enum, Purity::Pure),
            Extern::FloatCompare => row(
                "Prelude.Ord Float.compare",
                &[Float, Float],
                Enum,
                Purity::Pure,
            ),
            Extern::FloatLt => row("Prelude.Ord Float.<", &[Float, Float], Enum, Purity::Pure),
            Extern::FloatLe => row("Prelude.Ord Float.<=", &[Float, Float], Enum, Purity::Pure),
            Extern::FloatGt => row("Prelude.Ord Float.>", &[Float, Float], Enum, Purity::Pure),
            Extern::FloatGe => row("Prelude.Ord Float.>=", &[Float, Float], Enum, Purity::Pure),
            Extern::FloatShow => row("Prelude.Show Float.show", &[Float], Obj, Purity::Pure),
            Extern::FloatShowsMinus => row("Prelude.shows_minus", &[Float], Enum, Purity::Pure),
            Extern::FloatOfInt => row("Std.Float.of_int", &[Int], Float, Purity::Pure),
            // `Int` に収まらない値は `Panic` である
            Extern::FloatTruncate => row("Std.Float.truncate", &[Float], Int, Purity::Panics),
            Extern::FloatFloor => row("Std.Float.floor", &[Float], Int, Purity::Panics),
            Extern::FloatCeil => row("Std.Float.ceil", &[Float], Int, Purity::Panics),
            Extern::FloatRound => row("Std.Float.round", &[Float], Int, Purity::Panics),
            Extern::FloatSqrt => row("Std.Float.sqrt", &[Float], Float, Purity::Pure),
            Extern::FloatAbs => row("Std.Float.abs", &[Float], Float, Purity::Pure),
            Extern::FloatSin => row("Std.Float.sin", &[Float], Float, Purity::Pure),
            Extern::FloatCos => row("Std.Float.cos", &[Float], Float, Purity::Pure),
            Extern::FloatExp => row("Std.Float.exp", &[Float], Float, Purity::Pure),
            Extern::FloatLog => row("Std.Float.log", &[Float], Float, Purity::Pure),
            Extern::FloatPow => row("Std.Float.pow", &[Float, Float], Float, Purity::Pure),
            Extern::FloatInfinity => row("Std.Float.infinity_value", &[Unit], Float, Purity::Pure),
            Extern::FloatNan => row("Std.Float.nan_value", &[Unit], Float, Purity::Pure),
            Extern::FloatToFixed => row("Std.Float.to_fixed", &[Int, Float], Obj, Purity::Panics),
            Extern::FloatCanParse => row("Std.Float.can_parse", &[Obj], Enum, Purity::Pure),
            Extern::FloatParseRaw => row("Std.Float.parse_raw", &[Obj], Float, Purity::Pure),
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
            Extern::Eprintln => row("Prelude.eprintln", &[Obj], Unit, Purity::Effectful),
            Extern::StrByteLength => row("Std.String.byte_length", &[Obj], Int, Purity::Pure),
            Extern::StrByteAt => row("Std.String.byte_at", &[Int, Obj], Int, Purity::Panics),
            Extern::StrSlice => row("Std.String.slice", &[Int, Int, Obj], Obj, Purity::Panics),
            Extern::StrFindFrom => row(
                "Std.String.find_from",
                &[Obj, Int, Obj],
                Int,
                Purity::Panics,
            ),
            Extern::EnvArgCount => row("Std.Env.arg_count", &[Unit], Int, Purity::Effectful),
            Extern::EnvArg => row("Std.Env.arg", &[Int], Obj, Purity::Effectful),
            Extern::EnvHas => row("Std.Env.has", &[Obj], Enum, Purity::Effectful),
            Extern::EnvLookup => row("Std.Env.lookup", &[Obj], Obj, Purity::Effectful),
            Extern::StdinReadAll => row("Std.Stdin.read_all", &[Unit], Obj, Purity::Effectful),
            Extern::StdinReadLine => {
                row("Std.Stdin.read_line_raw", &[Unit], Obj, Purity::Effectful)
            }
            Extern::WriteText => row("Std.Fs.write_text", &[Obj, Obj], Unit, Purity::Effectful),
            Extern::ProcStatus => row("Std.Proc.status", &[Obj, Obj], Int, Purity::Effectful),
            // 戻らない extern。VM が実行の底まで中断する (docs/spec/vm.md の「extern の中断」)
            Extern::Exit => FunctionRow {
                name: "Prelude.exit",
                params: &[Repr::Int],
                ret: OpRet::Never,
                param_kinds: &[Linearity::Unr],
                ret_kind: Linearity::Unr,
                purity: Purity::Effectful,
                borrowed: &[false],
                with_arg: None,
            },
            // `exit` と同じく VM が自分で実行し、文字列を `Panic` の `panicked` の節に渡す
            // (docs/spec/vm.md の「extern の中断」)
            Extern::Panic => FunctionRow {
                name: "Prelude.panic",
                params: &[Repr::Obj],
                ret: OpRet::Never,
                param_kinds: &[Linearity::Unr],
                ret_kind: Linearity::Unr,
                purity: Purity::Panics,
                borrowed: &[false],
                with_arg: None,
            },
            // 配列の要素は型変数の位置なので `tobj` である。型変数は `Unr` に固定するので、`Array` の Kind も `Unr` に
            // なる。`MutArray` はいつも `Lin` である (docs/spec/stdlib.md の「`Std.Array`」)
            Extern::ArrayMake => row("Std.Array.make", &[Int, TObj], Obj, Purity::Panics),
            Extern::ArrayLength => {
                lending("Std.Array.length", &[Obj], &[Unr], Int, Purity::Pure, None)
            }
            Extern::ArrayGet => lending(
                "Std.Array.get",
                &[Obj, Int],
                &[Unr, Unr],
                TObj,
                Purity::Panics,
                None,
            ),
            Extern::ArraySet => row("Std.Array.set", &[Obj, Int, TObj], Obj, Purity::Panics),
            Extern::ArrayThaw => kinded("Std.Array.thaw", &[Obj], &[Unr], Obj, Lin, Purity::Pure),
            Extern::MutArrayMake => kinded(
                "Std.MutArray.make",
                &[Int, TObj],
                &[Unr, Unr],
                Obj,
                Lin,
                Purity::Panics,
            ),
            Extern::MutArrayEmpty => kinded(
                "Std.MutArray.empty",
                &[Unit],
                &[Unr],
                Obj,
                Lin,
                Purity::Pure,
            ),
            Extern::MutArrayLength => lending(
                "Std.MutArray.length",
                &[Obj],
                &[Lin],
                Int,
                Purity::Pure,
                Some(0),
            ),
            Extern::MutArrayRead => lending(
                "Std.MutArray.read",
                &[Obj, Int],
                &[Lin, Unr],
                TObj,
                Purity::Panics,
                Some(0),
            ),
            Extern::MutArrayWrite => kinded(
                "Std.MutArray.write",
                &[Obj, Int, TObj],
                &[Lin, Unr, Unr],
                Obj,
                Lin,
                Purity::Panics,
            ),
            Extern::MutArrayFreeze => kinded(
                "Std.MutArray.freeze",
                &[Obj],
                &[Lin],
                Obj,
                Unr,
                Purity::Pure,
            ),
            // `var` の配列の変数の添字を、使う所で確かめる。節の中で `Panic` を起こさないためである
            // (docs/spec/core-ir.md の「`var` の変換」)
            Extern::CheckIndex => row("Prelude.check_index", &[Int, Int], Unit, Purity::Panics),
        }
    }

    pub fn from_name(name: &str) -> Option<Extern> {
        Self::ALL.iter().copied().find(|e| e.row().name == name)
    }
}
