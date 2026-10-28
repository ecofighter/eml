//! extern の表。標準ライブラリ (`std/`) の extern の宣言ごとに、処理系が知る性質を1行に持つ。HIR、型検査、Core IR、
//! LIR、VM はこの表を引き、名前の文字列を照らし合わせない。ネイティブ化では、この表がランタイムとの ABI の一覧に
//! なる (docs/future/native.md の「IR の層」)。
//!
//! 行の正式な名前は、`std/` の宣言をモジュールの正式な名前で修飾したもの (`Prelude.Int`、`Prelude.++`) である。
//! instance の `extern` で結ぶメソッドの行は、モジュール、クラス、型、メソッドの名前を並べた `Prelude.Eq Int.==` の
//! 形である。どの行も、`std/` の extern の宣言か instance の `extern` のちょうど1つから指されることは、`eml_hir` の
//! 結合テストが確かめる。行の Repr が std の宣言の型の Repr と同じであることは、`eml_core_ir` の結合テストが確かめる。

/// 表を1つの並びから作る。行は宣言の順に並び、`ALL` の添字と `self as usize` と静的な行の表の添字が同じになる。
/// 変種、`ALL`、行を別々に書くと、順と数を手で揃えることになるためである。行の式は定数として評価し、`row` は静的な
/// 行を返す。`use` の木は行の式のためだけに、行の表の中で読む。
macro_rules! table {
    (
        $(#[$attr:meta])*
        pub enum $name:ident: $row:ty $(, use $uses:tt)? {
            $($variant:ident => $value:expr,)*
        }
    ) => {
        $(#[$attr])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $($variant,)*
        }

        impl $name {
            pub const ALL: &[$name] = &[$($name::$variant,)*];

            pub const fn row(self) -> &'static $row {
                $(use $uses;)?
                const ROWS: &[$row] = &[$($value,)*];
                &ROWS[self as usize]
            }

            pub fn from_name(name: &str) -> Option<$name> {
                Self::ALL.iter().copied().find(|item| item.row().name == name)
            }
        }
    };
}

/// 型の Kind。型引数のない extern の型は `Unr` か `Lin` で決まる。型引数を持つ型 (`TypeRow::params > 0`) の `Unr` は
/// 「引数の Kind の join」を表し、`Lin` は引数によらず常に `Lin` である (docs/spec/types.md)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Linearity {
    Unr,
    Lin,
}

/// 値が `Lin` を含みうるかを表す印 (docs/spec/lir.md の「Kind の印」)。extern の行の印の列も同じ3値である
/// (docs/spec/core-ir.md の「表の印の列」)。
/// 順序を持つ型にしないのは、結びの順 (`Lin` > `Poly` > `Unr`) と宣言の順を取り違えないため。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mark {
    /// 共有してよい。
    Unr,
    /// 共有してはならず、ちょうど1回消費する。
    Lin,
    /// 型変数の位置で、`Lin` かもしれない。
    Poly,
}

impl Mark {
    pub fn name(self) -> &'static str {
        match self {
            Mark::Unr => "unr",
            Mark::Lin => "lin",
            Mark::Poly => "poly",
        }
    }

    pub fn join(self, other: Mark) -> Mark {
        match (self, other) {
            (Mark::Lin, _) | (_, Mark::Lin) => Mark::Lin,
            (Mark::Poly, _) | (_, Mark::Poly) => Mark::Poly,
            _ => Mark::Unr,
        }
    }

    /// 送り手の値を受け手に渡せるか。`lin` ビットを落とす向き (`Lin` を `Unr` の受け手へ) だけが渡せない。
    /// `Unr` を `Lin` の受け手へ渡すのは、ビットが立たないだけで安全な側である。`Poly` はどちらの向きも通す。
    pub fn passes_to(sender: Mark, receiver: Mark) -> bool {
        !matches!((sender, receiver), (Mark::Lin, Mark::Unr))
    }

    /// 2つの印の小さいほう。順序は `Unr` < `Poly` < `Lin`。
    pub fn meet(self, other: Mark) -> Mark {
        if self.at_least(other) { other } else { self }
    }

    /// `self` が `other` 以上か (`self >= other`)。順序は `Unr` < `Poly` < `Lin`。
    pub fn at_least(self, other: Mark) -> bool {
        self.rank() >= other.rank()
    }

    fn rank(self) -> u8 {
        match self {
            Mark::Unr => 0,
            Mark::Poly => 1,
            Mark::Lin => 2,
        }
    }
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

    /// 一様な位置で値を受ける Repr (`obj`、`tobj`、`unit`) か (docs/spec/lir.md の「関数の ABI」)。
    pub fn is_uniform(self) -> bool {
        matches!(self, Repr::Obj | Repr::TObj | Repr::Unit)
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

table! {
    /// extern の型。
    pub enum ExternType: TypeRow, use {Linearity::{Lin, Unr}} {
        Int => type_row("Prelude.Int", Unr, Repr::Int),
        Float => type_row("Prelude.Float", Unr, Repr::Float),
        Char => type_row("Prelude.Char", Unr, Repr::Int),
        Byte => type_row("Prelude.Byte", Unr, Repr::Int),
        String => type_row("Prelude.String", Unr, Repr::Obj),
        Bytes => type_row("Prelude.Bytes", Unr, Repr::Obj),
        Unit => type_row("Prelude.Unit", Unr, Repr::Unit),
        FileIn => type_row("Std.FileIn.FileIn", Lin, Repr::Obj),
        FileOut => type_row("Std.FileOut.FileOut", Lin, Repr::Obj),
        Stdin => type_row("Std.Stdin.Stdin", Lin, Repr::Obj),
        Stdout => type_row("Std.Stdout.Stdout", Lin, Repr::Obj),
        Stderr => type_row("Std.Stderr.Stderr", Lin, Repr::Obj),
        Child => type_row("Std.Proc.Child", Lin, Repr::Obj),
        Array => generic_type_row("Prelude.Array", Unr),
        MutArray => generic_type_row("Prelude.MutArray", Lin),
        // 登録表の添字の即値と、タスクのハンドルを中身に持つ `Foreign` (docs/spec/stdlib.md の「`Std.Async`」)
        Cell => type_row("Std.Async.Cell", Unr, Repr::Int),
        Task => generic_type_row("Std.Async.Task", Lin),
        Scope => type_row("Std.Async.Scope", Unr, Repr::Int),
        // driver の操作の記録を指す token の `Foreign` (docs/spec/stdlib.md の「`Std.Async`」)
        OpId => type_row("Std.Async.OpId", Lin, Repr::Obj),
        // 登録表の添字の即値。Kind は中身の型によらない (docs/spec/stdlib.md の「`Std.Chan`」)
        Chan => handle_type_row("Std.Chan.Chan"),
        // 登録表ではなく `Machine` の表の添字の即値 (docs/spec/stdlib.md の「`Std.Atomic`」)
        Counter => type_row("Std.Atomic.Counter", Unr, Repr::Int),
        // driver の `pair` の片方の端を持つ `Foreign` (docs/spec/stdlib.md の「`Std.IoTest`」)
        IoTestStream => type_row("Std.IoTest.Stream", Lin, Repr::Obj),
    }
}

pub struct TypeRow {
    pub name: &'static str,
    pub linearity: Linearity,
    /// 型引数の数。`kind_follows_params` の行では、型引数は共変で、`Unr` の行の Kind は引数の Kind に従う。そうでない行
    /// (ハンドルの型) の型引数は両方向である (docs/spec/types.md の「Kind」)。
    pub params: usize,
    /// 型引数の Kind が値の Kind に効くか。ハンドルの型 (`Chan a`) は効かない。値は登録表の添字の即値で、中身の値の
    /// 持ち主は処理系の表だからである (docs/spec/types.md の「Kind」)。
    pub kind_follows_params: bool,
    /// 値の Repr。translate は extern の型の Repr をここから読む。型検査は `Unit` を要素のないタプルにするので、translate は
    /// `Unit` の行を読まない。この行が要素のないタプルの Repr と同じであることは、結合テストが確かめる。
    pub repr: Repr,
}

const fn type_row(name: &'static str, linearity: Linearity, repr: Repr) -> TypeRow {
    TypeRow {
        name,
        linearity,
        params: 0,
        kind_follows_params: true,
        repr,
    }
}

/// 型引数を1つ持つが、その Kind に引数が効かない `Unr` のハンドル。値は登録表の添字の即値である。
const fn handle_type_row(name: &'static str) -> TypeRow {
    TypeRow {
        params: 1,
        kind_follows_params: false,
        ..type_row(name, Linearity::Unr, Repr::Int)
    }
}

/// 型引数を1つ持つ型。値はいつも物体である。
const fn generic_type_row(name: &'static str, linearity: Linearity) -> TypeRow {
    TypeRow {
        params: 1,
        ..type_row(name, linearity, Repr::Obj)
    }
}

table! {
    /// extern のエフェクト。`IO` は `Effectful` の extern の関数が起こし、`Panic` は `Panics` の行と `Std.Env.arg` が起こす。
    /// どちらも隠れた `never` の操作 (`io_failed`、`panicked`) を Prelude の宣言が持ち、Prelude の `try_io`、`try_panic`
    /// だけが handle する。表の行は名前と多重度を持つ (docs/spec/effects.md の「組み込みの `IO`」「組み込みの `Panic`」)。
    pub enum ExternEffect: EffectRow {
        Io => EffectRow {
            name: "Prelude.IO",
            multiplicity: EffectMultiplicity::Once,
        },
        Panic => EffectRow {
            name: "Prelude.Panic",
            multiplicity: EffectMultiplicity::Never,
        },
    }
}

/// extern のエフェクトの多重度。`Never` は起こすと呼び出し元へ戻らず、外の世界に何も残さないエフェクト、`Once` は
/// その場で1回だけ戻るか、外の世界への書き込みが順序を持つエフェクトである (docs/spec/types.md の「Kind」)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EffectMultiplicity {
    Never,
    Once,
}

pub struct EffectRow {
    pub name: &'static str,
    pub multiplicity: EffectMultiplicity,
}

table! {
    /// extern の関数。表の行は宣言の順に並び、`Extern::ALL` の添字は `*self as usize` と同じである。VM のコード生成は
    /// これを行の番号に使う。
    pub enum Extern: FunctionRow, use {Mark::{Lin, Poly, Unr}, Repr::{Enum, Float, Int, Obj, TObj, Unit}} {
        Println => row("Prelude.println", &[Obj], Unit, Purity::Effectful),
        Open => kinded(
            "Std.FileIn.open",
            &[Obj],
            &[Unr],
            Obj,
            Lin,
            Purity::Effectful,
        ),
        ReadAll => borrowing(
            &[true],
            FunctionRow {
                with_arg: Some(0),
                ..kinded(
                    "Std.FileIn.read_all",
                    &[Obj],
                    &[Lin],
                    Obj,
                    Unr,
                    Purity::Effectful,
                )
            },
        ),
        ReadLine => borrowing(
            &[true],
            FunctionRow {
                with_arg: Some(0),
                ..kinded(
                    "Std.FileIn.read_line_raw",
                    &[Obj],
                    &[Lin],
                    Obj,
                    Unr,
                    Purity::Effectful,
                )
            },
        ),
        ReadChunk => borrowing(
            &[false, true],
            FunctionRow {
                with_arg: Some(1),
                ..kinded(
                    "Std.FileIn.read_chunk",
                    &[Obj, Obj],
                    &[Unr, Lin],
                    Obj,
                    Unr,
                    Purity::Effectful,
                )
            },
        ),
        ReadAllBytes => borrowing(
            &[true],
            FunctionRow {
                with_arg: Some(0),
                ..kinded(
                    "Std.FileIn.read_all_bytes",
                    &[Obj],
                    &[Lin],
                    Obj,
                    Unr,
                    Purity::Effectful,
                )
            },
        ),
        ReadLineBytes => borrowing(
            &[true],
            FunctionRow {
                with_arg: Some(0),
                ..kinded(
                    "Std.FileIn.read_line_bytes_raw",
                    &[Obj],
                    &[Lin],
                    Obj,
                    Unr,
                    Purity::Effectful,
                )
            },
        ),
        Close => kinded(
            "Std.FileIn.close",
            &[Obj],
            &[Lin],
            Unit,
            Unr,
            Purity::Effectful,
        ),
        IntShow => row("Prelude.Show Int.show", &[Int], Obj, Purity::Pure),
        IntNeg => row("Prelude.Num Int.negate", &[Int], Int, Purity::Panics),
        IntAdd => row("Prelude.Num Int.+", &[Int, Int], Int, Purity::Panics),
        IntSub => row("Prelude.Num Int.-", &[Int, Int], Int, Purity::Panics),
        IntMul => row("Prelude.Num Int.*", &[Int, Int], Int, Purity::Panics),
        IntDiv => row("Prelude.Div Int./", &[Int, Int], Int, Purity::Panics),
        IntMod => row("Prelude.Div Int.%", &[Int, Int], Int, Purity::Panics),
        IntEq => row("Prelude.Eq Int.==", &[Int, Int], Enum, Purity::Pure),
        IntNe => row("Prelude.Eq Int.!=", &[Int, Int], Enum, Purity::Pure),
        IntCompare => row("Prelude.Ord Int.compare", &[Int, Int], Enum, Purity::Pure),
        IntLt => row("Prelude.Ord Int.<", &[Int, Int], Enum, Purity::Pure),
        IntLe => row("Prelude.Ord Int.<=", &[Int, Int], Enum, Purity::Pure),
        IntGt => row("Prelude.Ord Int.>", &[Int, Int], Enum, Purity::Pure),
        IntGe => row("Prelude.Ord Int.>=", &[Int, Int], Enum, Purity::Pure),
        CharEq => row("Prelude.Eq Char.==", &[Int, Int], Enum, Purity::Pure),
        CharNe => row("Prelude.Eq Char.!=", &[Int, Int], Enum, Purity::Pure),
        CharCompare => row("Prelude.Ord Char.compare", &[Int, Int], Enum, Purity::Pure),
        CharLt => row("Prelude.Ord Char.<", &[Int, Int], Enum, Purity::Pure),
        CharLe => row("Prelude.Ord Char.<=", &[Int, Int], Enum, Purity::Pure),
        CharGt => row("Prelude.Ord Char.>", &[Int, Int], Enum, Purity::Pure),
        CharGe => row("Prelude.Ord Char.>=", &[Int, Int], Enum, Purity::Pure),
        CharToInt => row("Std.Char.to_int", &[Int], Int, Purity::Pure),
        // スカラー値でない `Int` は `Panic` である
        CharOfInt => row("Std.Char.of_int", &[Int], Int, Purity::Panics),
        // Latin-1。0 から 255 はどれもスカラー値なので失敗しない
        CharOfByte => row("Std.Char.of_byte", &[Int], Int, Purity::Pure),
        CharShow => row("Prelude.Show Char.show", &[Int], Obj, Purity::Pure),
        CharDisplay => row("Prelude.Show Char.display", &[Int], Obj, Purity::Pure),
        ByteEq => row("Prelude.Eq Byte.==", &[Int, Int], Enum, Purity::Pure),
        ByteNe => row("Prelude.Eq Byte.!=", &[Int, Int], Enum, Purity::Pure),
        ByteCompare => row("Prelude.Ord Byte.compare", &[Int, Int], Enum, Purity::Pure),
        ByteLt => row("Prelude.Ord Byte.<", &[Int, Int], Enum, Purity::Pure),
        ByteLe => row("Prelude.Ord Byte.<=", &[Int, Int], Enum, Purity::Pure),
        ByteGt => row("Prelude.Ord Byte.>", &[Int, Int], Enum, Purity::Pure),
        ByteGe => row("Prelude.Ord Byte.>=", &[Int, Int], Enum, Purity::Pure),
        ByteNeg => row("Prelude.Num Byte.negate", &[Int], Int, Purity::Panics),
        ByteAdd => row("Prelude.Num Byte.+", &[Int, Int], Int, Purity::Panics),
        ByteSub => row("Prelude.Num Byte.-", &[Int, Int], Int, Purity::Panics),
        ByteMul => row("Prelude.Num Byte.*", &[Int, Int], Int, Purity::Panics),
        ByteDiv => row("Prelude.Div Byte./", &[Int, Int], Int, Purity::Panics),
        ByteMod => row("Prelude.Div Byte.%", &[Int, Int], Int, Purity::Panics),
        ByteDisplay => row("Prelude.Show Byte.display", &[Int], Obj, Purity::Pure),
        ByteToInt => row("Std.Byte.to_int", &[Int], Int, Purity::Pure),
        // 0..=255 の外の `Int` は `Panic` である
        ByteOfInt => row("Std.Byte.of_int", &[Int], Int, Purity::Panics),
        ByteWrap => row("Std.Byte.wrap", &[Int], Int, Purity::Pure),
        IntAnd => row("Prelude.Bits Int.&&&", &[Int, Int], Int, Purity::Pure),
        IntOr => row("Prelude.Bits Int.|||", &[Int, Int], Int, Purity::Pure),
        IntXor => row("Prelude.Bits Int.^^^", &[Int, Int], Int, Purity::Pure),
        IntComplement => row("Prelude.Bits Int.complement", &[Int], Int, Purity::Pure),
        IntShl => row("Prelude.Bits Int.<<<", &[Int, Int], Int, Purity::Pure),
        IntShr => row("Prelude.Bits Int.>>>", &[Int, Int], Int, Purity::Pure),
        ByteAnd => row("Prelude.Bits Byte.&&&", &[Int, Int], Int, Purity::Pure),
        ByteOr => row("Prelude.Bits Byte.|||", &[Int, Int], Int, Purity::Pure),
        ByteXor => row("Prelude.Bits Byte.^^^", &[Int, Int], Int, Purity::Pure),
        ByteComplement => {
            row("Prelude.Bits Byte.complement", &[Int], Int, Purity::Pure)
        },
        ByteShl => row("Prelude.Bits Byte.<<<", &[Int, Int], Int, Purity::Pure),
        ByteShr => row("Prelude.Bits Byte.>>>", &[Int, Int], Int, Purity::Pure),
        // `Float` の算術は `Panic` を起こさない。メソッドの型はクラスで1つなので `<Panic>` を持つが、行は `Pure` である
        // (docs/spec/declarations.md の「Prelude のクラス」)
        FloatNeg => row("Prelude.Num Float.negate", &[Float], Float, Purity::Pure),
        FloatAdd => row("Prelude.Num Float.+", &[Float, Float], Float, Purity::Pure),
        FloatSub => row("Prelude.Num Float.-", &[Float, Float], Float, Purity::Pure),
        FloatMul => row("Prelude.Num Float.*", &[Float, Float], Float, Purity::Pure),
        FloatDiv => row("Prelude.Div Float./", &[Float, Float], Float, Purity::Pure),
        FloatMod => row("Prelude.Div Float.%", &[Float, Float], Float, Purity::Pure),
        FloatEq => row("Prelude.Eq Float.==", &[Float, Float], Enum, Purity::Pure),
        FloatNe => row("Prelude.Eq Float.!=", &[Float, Float], Enum, Purity::Pure),
        FloatCompare => row(
            "Prelude.Ord Float.compare",
            &[Float, Float],
            Enum,
            Purity::Pure,
        ),
        FloatLt => row("Prelude.Ord Float.<", &[Float, Float], Enum, Purity::Pure),
        FloatLe => row("Prelude.Ord Float.<=", &[Float, Float], Enum, Purity::Pure),
        FloatGt => row("Prelude.Ord Float.>", &[Float, Float], Enum, Purity::Pure),
        FloatGe => row("Prelude.Ord Float.>=", &[Float, Float], Enum, Purity::Pure),
        FloatShow => row("Prelude.Show Float.show", &[Float], Obj, Purity::Pure),
        FloatShowsMinus => row("Prelude.shows_minus", &[Float], Enum, Purity::Pure),
        FloatOfInt => row("Std.Float.of_int", &[Int], Float, Purity::Pure),
        // `Int` に収まらない値は `Panic` である
        FloatTruncate => row("Std.Float.truncate", &[Float], Int, Purity::Panics),
        FloatFloor => row("Std.Float.floor", &[Float], Int, Purity::Panics),
        FloatCeil => row("Std.Float.ceil", &[Float], Int, Purity::Panics),
        FloatRound => row("Std.Float.round", &[Float], Int, Purity::Panics),
        FloatSqrt => row("Std.Float.sqrt", &[Float], Float, Purity::Pure),
        FloatAbs => row("Std.Float.abs", &[Float], Float, Purity::Pure),
        FloatSin => row("Std.Float.sin", &[Float], Float, Purity::Pure),
        FloatCos => row("Std.Float.cos", &[Float], Float, Purity::Pure),
        FloatExp => row("Std.Float.exp", &[Float], Float, Purity::Pure),
        FloatLog => row("Std.Float.log", &[Float], Float, Purity::Pure),
        FloatPow => row("Std.Float.pow", &[Float, Float], Float, Purity::Pure),
        FloatInfinity => row("Std.Float.infinity_value", &[Unit], Float, Purity::Pure),
        FloatNan => row("Std.Float.nan_value", &[Unit], Float, Purity::Pure),
        FloatToFixed => row("Std.Float.to_fixed", &[Int, Float], Obj, Purity::Panics),
        FloatCanParse => borrowing(
            &[true],
            row("Std.Float.can_parse", &[Obj], Enum, Purity::Pure),
        ),
        FloatParseRaw => borrowing(
            &[true],
            row("Std.Float.parse_raw", &[Obj], Float, Purity::Pure),
        ),
        StrConcat => row("Prelude.++", &[Obj, Obj], Obj, Purity::Pure),
        StrEq => borrowing(
            &[true, true],
            row("Prelude.Eq String.==", &[Obj, Obj], Enum, Purity::Pure),
        ),
        StrNe => borrowing(
            &[true, true],
            row("Prelude.Eq String.!=", &[Obj, Obj], Enum, Purity::Pure),
        ),
        StrCompare => borrowing(
            &[true, true],
            row(
                "Prelude.Ord String.compare",
                &[Obj, Obj],
                Enum,
                Purity::Pure,
            ),
        ),
        StrShow => borrowing(
            &[true],
            row("Prelude.Show String.show", &[Obj], Obj, Purity::Pure),
        ),
        BoolEq => row("Prelude.Eq Bool.==", &[Enum, Enum], Enum, Purity::Pure),
        BoolNe => row("Prelude.Eq Bool.!=", &[Enum, Enum], Enum, Purity::Pure),
        Eprintln => row("Prelude.eprintln", &[Obj], Unit, Purity::Effectful),
        StrByteLength => borrowing(
            &[true],
            row("Std.String.byte_length", &[Obj], Int, Purity::Pure),
        ),
        StrByteAt => borrowing(
            &[false, true],
            row("Std.String.byte_at", &[Int, Obj], Int, Purity::Panics),
        ),
        StrCharAt => borrowing(
            &[false, true],
            row("Std.String.char_at", &[Int, Obj], Int, Purity::Panics),
        ),
        StrSlice => borrowing(
            &[false, false, true],
            row("Std.String.slice", &[Int, Int, Obj], Obj, Purity::Panics),
        ),
        StrPush => row("Std.String.push", &[Int, Obj], Obj, Purity::Pure),
        StrFindFrom => borrowing(
            &[true, false, true],
            row(
                "Std.String.find_from",
                &[Obj, Int, Obj],
                Int,
                Purity::Panics,
            ),
        ),
        BytesEmpty => row("Std.Bytes.empty", &[Unit], Obj, Purity::Pure),
        BytesLength => {
            borrowing(&[true], row("Std.Bytes.length", &[Obj], Int, Purity::Pure))
        },
        BytesAppend => row("Std.Bytes.append", &[Obj, Obj], Obj, Purity::Pure),
        // `src` を借りて `dst` を消費する。同じ変数なら Perceus が `dup` を入れる
        // (docs/spec/lir.md の「extern の借用」)
        BytesAppendSlice => borrowing(
            &[false, false, true, false],
            row(
                "Std.Bytes.append_slice",
                &[Int, Int, Obj, Obj],
                Obj,
                Purity::Panics,
            ),
        ),
        BytesSlice => borrowing(
            &[false, false, true],
            row("Std.Bytes.slice", &[Int, Int, Obj], Obj, Purity::Panics),
        ),
        BytesTruncate => row("Std.Bytes.truncate", &[Int, Obj], Obj, Purity::Panics),
        BytesOfString => row("Std.Bytes.of_string", &[Obj], Obj, Purity::Pure),
        BytesEq => borrowing(
            &[true, true],
            row("Prelude.Eq Bytes.==", &[Obj, Obj], Enum, Purity::Pure),
        ),
        BytesNe => borrowing(
            &[true, true],
            row("Prelude.Eq Bytes.!=", &[Obj, Obj], Enum, Purity::Pure),
        ),
        BytesCompare => borrowing(
            &[true, true],
            row("Prelude.Ord Bytes.compare", &[Obj, Obj], Enum, Purity::Pure),
        ),
        BytesFindFrom => borrowing(
            &[true, false, true],
            row("Std.Bytes.find_from", &[Obj, Int, Obj], Int, Purity::Panics),
        ),
        BytesMake => row("Std.Bytes.make", &[Int, Int], Obj, Purity::Panics),
        BytesGet => borrowing(
            &[false, true],
            row("Std.Bytes.get", &[Int, Obj], Int, Purity::Panics),
        ),
        BytesSet => row("Std.Bytes.set", &[Int, Int, Obj], Obj, Purity::Panics),
        BytesPush => row("Std.Bytes.push", &[Int, Obj], Obj, Purity::Pure),
        BytesFindByte => borrowing(
            &[false, false, true],
            row(
                "Std.Bytes.find_byte_from",
                &[Int, Int, Obj],
                Int,
                Purity::Panics,
            ),
        ),
        // `Show Bytes` は Prelude にあり、`Std.Bytes` を import できないので、読む行を Prelude にも持つ
        // (docs/spec/stdlib.md の「`Bytes`」)
        ShowBytesLength => borrowing(
            &[true],
            row("Prelude.bytes_length", &[Obj], Int, Purity::Pure),
        ),
        ShowBytesGet => borrowing(
            &[false, true],
            row("Prelude.bytes_get", &[Int, Obj], Int, Purity::Panics),
        ),
        StringDecode => row("Std.String.decode", &[Obj], Obj, Purity::Panics),
        EnvArgCount => row("Std.Env.arg_count", &[Unit], Int, Purity::Effectful),
        EnvArg => row("Std.Env.arg", &[Int], Obj, Purity::Effectful),
        EnvHas => row("Std.Env.has", &[Obj], Enum, Purity::Effectful),
        EnvLookup => row("Std.Env.lookup", &[Obj], Obj, Purity::Effectful),
        StdinOpen => kinded(
            "Std.Stdin.open",
            &[Unit],
            &[Unr],
            Obj,
            Lin,
            Purity::Effectful,
        ),
        StdinClose => kinded(
            "Std.Stdin.close",
            &[Obj],
            &[Lin],
            Unit,
            Unr,
            Purity::Effectful,
        ),
        StdinReadChunk => borrowing(
            &[false, true],
            FunctionRow {
                with_arg: Some(1),
                ..kinded(
                    "Std.Stdin.read_chunk",
                    &[Obj, Obj],
                    &[Unr, Lin],
                    Obj,
                    Unr,
                    Purity::Effectful,
                )
            },
        ),
        StdinReadAllBytes => borrowing(
            &[true],
            FunctionRow {
                with_arg: Some(0),
                ..kinded(
                    "Std.Stdin.read_all_bytes",
                    &[Obj],
                    &[Lin],
                    Obj,
                    Unr,
                    Purity::Effectful,
                )
            },
        ),
        StdinReadAll => borrowing(
            &[true],
            FunctionRow {
                with_arg: Some(0),
                ..kinded(
                    "Std.Stdin.read_all",
                    &[Obj],
                    &[Lin],
                    Obj,
                    Unr,
                    Purity::Effectful,
                )
            },
        ),
        StdinReadLineBytes => borrowing(
            &[true],
            FunctionRow {
                with_arg: Some(0),
                ..kinded(
                    "Std.Stdin.read_line_bytes_raw",
                    &[Obj],
                    &[Lin],
                    Obj,
                    Unr,
                    Purity::Effectful,
                )
            },
        ),
        StdinReadLine => borrowing(
            &[true],
            FunctionRow {
                with_arg: Some(0),
                ..kinded(
                    "Std.Stdin.read_line_raw",
                    &[Obj],
                    &[Lin],
                    Obj,
                    Unr,
                    Purity::Effectful,
                )
            },
        ),
        StdoutOpen => kinded(
            "Std.Stdout.open",
            &[Unit],
            &[Unr],
            Obj,
            Lin,
            Purity::Effectful,
        ),
        StdoutClose => kinded(
            "Std.Stdout.close",
            &[Obj],
            &[Lin],
            Unit,
            Unr,
            Purity::Effectful,
        ),
        StdoutWriteBytes => writing("Std.Stdout.write_bytes"),
        StdoutWrite => writing("Std.Stdout.write"),
        StdoutWriteLine => writing("Std.Stdout.write_line"),
        StdoutFlush => kinded(
            "Std.Stdout.flush",
            &[Obj],
            &[Lin],
            Obj,
            Lin,
            Purity::Effectful,
        ),
        StderrOpen => kinded(
            "Std.Stderr.open",
            &[Unit],
            &[Unr],
            Obj,
            Lin,
            Purity::Effectful,
        ),
        StderrClose => kinded(
            "Std.Stderr.close",
            &[Obj],
            &[Lin],
            Unit,
            Unr,
            Purity::Effectful,
        ),
        StderrWriteBytes => writing("Std.Stderr.write_bytes"),
        StderrWrite => writing("Std.Stderr.write"),
        StderrWriteLine => writing("Std.Stderr.write_line"),
        FileOutCreate => kinded(
            "Std.FileOut.create",
            &[Obj],
            &[Unr],
            Obj,
            Lin,
            Purity::Effectful,
        ),
        FileOutAppend => kinded(
            "Std.FileOut.append",
            &[Obj],
            &[Unr],
            Obj,
            Lin,
            Purity::Effectful,
        ),
        FileOutWrite => writing("Std.FileOut.write"),
        FileOutWriteBytes => writing("Std.FileOut.write_bytes"),
        FileOutWriteLine => writing("Std.FileOut.write_line"),
        FileOutFlush => kinded(
            "Std.FileOut.flush",
            &[Obj],
            &[Lin],
            Obj,
            Lin,
            Purity::Effectful,
        ),
        FileOutCloseRaw => kinded(
            "Std.FileOut.close_raw",
            &[Obj],
            &[Lin],
            Unit,
            Unr,
            Purity::Effectful,
        ),
        // 引数は、プログラム、引数の列、環境の列、作業ディレクトリ、標準入力、標準出力、標準エラーの向きである
        // (docs/spec/stdlib.md の「`Proc`」)
        ProcSpawn => kinded(
            "Std.Proc.spawn_raw",
            &[Obj, Obj, Obj, Obj, Int, Int, Int],
            &[Unr; 7],
            Obj,
            Lin,
            Purity::Effectful,
        ),
        ProcWait => {
            kinded("Std.Proc.wait", &[Obj], &[Lin], Int, Unr, Purity::Effectful)
        },
        ProcCloseStdin => kinded(
            "Std.Proc.close_stdin",
            &[Obj],
            &[Lin],
            Obj,
            Lin,
            Purity::Effectful,
        ),
        ProcWrite => writing("Std.Proc.write"),
        ProcWriteBytes => writing("Std.Proc.write_bytes"),
        ProcWriteLine => writing("Std.Proc.write_line"),
        ProcReadChunk => borrowing(
            &[false, true],
            FunctionRow {
                with_arg: Some(1),
                ..kinded(
                    "Std.Proc.read_chunk",
                    &[Obj, Obj],
                    &[Unr, Lin],
                    Obj,
                    Unr,
                    Purity::Effectful,
                )
            },
        ),
        ProcReadAllBytes => borrowing(
            &[true],
            FunctionRow {
                with_arg: Some(0),
                ..kinded(
                    "Std.Proc.read_all_bytes",
                    &[Obj],
                    &[Lin],
                    Obj,
                    Unr,
                    Purity::Effectful,
                )
            },
        ),
        ProcReadAll => borrowing(
            &[true],
            FunctionRow {
                with_arg: Some(0),
                ..kinded(
                    "Std.Proc.read_all",
                    &[Obj],
                    &[Lin],
                    Obj,
                    Unr,
                    Purity::Effectful,
                )
            },
        ),
        ProcReadLineBytes => borrowing(
            &[true],
            FunctionRow {
                with_arg: Some(0),
                ..kinded(
                    "Std.Proc.read_line_bytes_raw",
                    &[Obj],
                    &[Lin],
                    Obj,
                    Unr,
                    Purity::Effectful,
                )
            },
        ),
        ProcReadLine => borrowing(
            &[true],
            FunctionRow {
                with_arg: Some(0),
                ..kinded(
                    "Std.Proc.read_line_raw",
                    &[Obj],
                    &[Lin],
                    Obj,
                    Unr,
                    Purity::Effectful,
                )
            },
        ),
        // 戻らない extern。VM が実行の底まで中断する (docs/spec/vm.md の「extern の中断」)
        Exit => FunctionRow {
            name: "Prelude.exit",
            params: &[Repr::Int],
            ret: OpRet::Never,
            param_marks: &[Mark::Unr],
            ret_mark: Mark::Unr,
            purity: Purity::Effectful,
            borrowed: &[false],
            with_arg: None,
            held: &[false],
        },
        // いつも `IO` の失敗で終わる戻らない行。宛先には書かず、届け先は VM の `failed` が決める
        // (docs/spec/vm.md の「extern の中断」)
        FailIo => FunctionRow {
            name: "Prelude.fail_io",
            params: &[Repr::Enum, Repr::Obj],
            ret: OpRet::Never,
            param_marks: &[Mark::Unr, Mark::Unr],
            ret_mark: Mark::Unr,
            purity: Purity::Effectful,
            borrowed: &[false, false],
            with_arg: None,
            held: &[false, false],
        },
        // `exit` と同じく VM が自分で実行し、文字列を `Panic` の `panicked` の節に渡す
        // (docs/spec/vm.md の「extern の中断」)
        Panic => FunctionRow {
            name: "Prelude.panic",
            params: &[Repr::Obj],
            ret: OpRet::Never,
            param_marks: &[Mark::Unr],
            ret_mark: Mark::Unr,
            purity: Purity::Panics,
            borrowed: &[false],
            with_arg: None,
            held: &[false],
        },
        // 配列の要素は型変数の位置なので `tobj` である。型変数は `Unr` に固定するので、`Array` の Kind も `Unr` に
        // なる。`MutArray` はいつも `Lin` である (docs/spec/stdlib.md の「`Std.Array`」)
        ArrayMake => row("Std.Array.make", &[Int, TObj], Obj, Purity::Panics),
        ArrayLength => borrowing(
            &[true],
            kinded("Std.Array.length", &[Obj], &[Unr], Int, Unr, Purity::Pure),
        ),
        ArrayGet => borrowing(
            &[true, false],
            kinded(
                "Std.Array.get",
                &[Obj, Int],
                &[Unr, Unr],
                TObj,
                Unr,
                Purity::Panics,
            ),
        ),
        ArraySet => row("Std.Array.set", &[Obj, Int, TObj], Obj, Purity::Panics),
        ArrayThaw => kinded("Std.Array.thaw", &[Obj], &[Unr], Obj, Lin, Purity::Pure),
        MutArrayMake => kinded(
            "Std.MutArray.make",
            &[Int, TObj],
            &[Unr, Unr],
            Obj,
            Lin,
            Purity::Panics,
        ),
        MutArrayEmpty => kinded(
            "Std.MutArray.empty",
            &[Unit],
            &[Unr],
            Obj,
            Lin,
            Purity::Pure,
        ),
        MutArrayLength => borrowing(
            &[true],
            FunctionRow {
                with_arg: Some(0),
                ..kinded(
                    "Std.MutArray.length",
                    &[Obj],
                    &[Lin],
                    Int,
                    Unr,
                    Purity::Pure,
                )
            },
        ),
        MutArrayRead => borrowing(
            &[true, false],
            FunctionRow {
                with_arg: Some(0),
                ..kinded(
                    "Std.MutArray.read",
                    &[Obj, Int],
                    &[Lin, Unr],
                    TObj,
                    Unr,
                    Purity::Panics,
                )
            },
        ),
        MutArrayWrite => kinded(
            "Std.MutArray.write",
            &[Obj, Int, TObj],
            &[Lin, Unr, Unr],
            Obj,
            Lin,
            Purity::Panics,
        ),
        MutArrayFreeze => kinded(
            "Std.MutArray.freeze",
            &[Obj],
            &[Lin],
            Obj,
            Unr,
            Purity::Pure,
        ),
        // `var` の配列の変数の添字を、使う所で確かめる。節の中で `Panic` を起こさないためである
        // (docs/spec/core-ir.md の「`var` の変換」)
        CheckIndex => row("Prelude.check_index", &[Int, Int], Unit, Purity::Panics),
        // std の宣言を型検査に結ぶためだけの行である。translate は呼び出しを `par` の命令にするので、
        // `Rhs::Extern` にならない (docs/spec/core-ir.md の「`par` の呼び出し」)
        Par => kinded(
            "Std.Par.par",
            &[TObj, TObj],
            &[Poly, Poly],
            Obj,
            Poly,
            Purity::Pure,
        ),
        // `Std.Async` の行は VM の `Async` の命令が実行する (docs/spec/vm.md の「`Async` のタスク」)。預かる
        // 引数の型変数は `Unr` に固定せず、その矢印には閉じた row を書ける (docs/spec/stdlib.md の「`Std.Async`」)
        AsyncStartRaw => holding(
            &[true],
            kinded(
                "Std.Async.start_raw",
                &[TObj],
                &[Poly],
                Obj,
                Lin,
                Purity::Effectful,
            ),
        ),
        AsyncRunRaw => holding(
            &[true],
            kinded(
                "Std.Async.run_raw",
                &[TObj],
                &[Poly],
                Obj,
                Lin,
                Purity::Effectful,
            ),
        ),
        AsyncPark => holding(
            &[false, true],
            kinded(
                "Std.Async.park",
                &[Int, TObj],
                &[Unr, Lin],
                Unit,
                Unr,
                Purity::Effectful,
            ),
        ),
        AsyncParkTime => holding(
            &[false, true],
            kinded(
                "Std.Async.park_time",
                &[Int, TObj],
                &[Unr, Lin],
                Unit,
                Unr,
                Purity::Effectful,
            ),
        ),
        AsyncReady => holding(
            &[true],
            kinded(
                "Std.Async.ready",
                &[TObj],
                &[Lin],
                Unit,
                Unr,
                Purity::Effectful,
            ),
        ),
        AsyncFinishOk => holding(
            &[true],
            kinded(
                "Std.Async.finish_ok",
                &[TObj],
                &[Poly],
                Unit,
                Unr,
                Purity::Effectful,
            ),
        ),
        AsyncFinishIo => kinded(
            "Std.Async.finish_io",
            &[Enum, Obj],
            &[Unr, Unr],
            Unit,
            Unr,
            Purity::Effectful,
        ),
        AsyncFinishPanic => kinded(
            "Std.Async.finish_panic",
            &[Obj],
            &[Unr],
            Unit,
            Unr,
            Purity::Effectful,
        ),
        // ハンドルを借りて預かり、宣言の型ではセルとハンドルの組を返す。借りないと、組を作る `con` とこの行の
        // 両方がハンドルを消費する (docs/spec/core-ir.md の「組を返す extern」)
        AsyncTaskCell => holding(
            &[true],
            borrowing(
                &[true],
                FunctionRow {
                    with_arg: Some(0),
                    ..kinded(
                        "Std.Async.task_cell",
                        &[Obj],
                        &[Lin],
                        Int,
                        Unr,
                        Purity::Pure,
                    )
                },
            ),
        ),
        AsyncTakeResult => holding(
            &[true],
            kinded(
                "Std.Async.take_result",
                &[Obj],
                &[Lin],
                TObj,
                Poly,
                Purity::Effectful,
            ),
        ),
        AsyncRunResult => holding(
            &[true],
            kinded(
                "Std.Async.run_result",
                &[Obj],
                &[Lin],
                TObj,
                Poly,
                Purity::Effectful,
            ),
        ),
        AsyncDetach => holding(
            &[true],
            kinded("Std.Async.detach", &[Obj], &[Lin], Unit, Unr, Purity::Effectful),
        ),
        AsyncCancelRaw => holding(
            &[true],
            kinded("Std.Async.cancel_raw", &[Obj], &[Lin], Unit, Unr, Purity::Effectful),
        ),
        AsyncStartInRaw => holding(
            &[false, true],
            kinded(
                "Std.Async.start_in_raw",
                &[Int, TObj],
                &[Unr, Poly],
                Obj,
                Lin,
                Purity::Effectful,
            ),
        ),
        AsyncOpenScope => row("Std.Async.open_scope", &[Unit], Int, Purity::Effectful),
        AsyncCloseScope => row("Std.Async.close_scope", &[Int], Int, Purity::Effectful),
        AsyncLeaveScope => row("Std.Async.leave_scope", &[Int], Unit, Purity::Effectful),
        AsyncCancelScopeRaw => row(
            "Std.Async.cancel_scope_raw",
            &[Int],
            Unit,
            Purity::Effectful,
        ),
        AsyncFailScopeIo => row(
            "Std.Async.fail_scope_io",
            &[Int, Enum, Obj],
            Unit,
            Purity::Effectful,
        ),
        AsyncFailScopePanic => row(
            "Std.Async.fail_scope_panic",
            &[Int, Obj],
            Unit,
            Purity::Effectful,
        ),
        AsyncCancelPoint => row("Std.Async.cancel_point", &[Unit], Enum, Purity::Effectful),
        AsyncSpendBudget => row("Std.Async.spend_budget", &[Unit], Enum, Purity::Effectful),
        AsyncShieldEnter => row("Std.Async.shield_enter", &[Unit], Unit, Purity::Effectful),
        AsyncShieldLeave => row("Std.Async.shield_leave", &[Unit], Unit, Purity::Effectful),
        AsyncClaimScope => row("Std.Async.claim_scope", &[Int], Enum, Purity::Effectful),
        AsyncParkDrain => holding(
            &[false, true],
            kinded(
                "Std.Async.park_drain",
                &[Int, TObj],
                &[Unr, Lin],
                Unit,
                Unr,
                Purity::Effectful,
            ),
        ),
        AsyncCancelMembers => row("Std.Async.cancel_members", &[Int], Unit, Purity::Effectful),
        AsyncParkSend => holding(
            &[false, true],
            kinded(
                "Std.Async.park_send",
                &[Int, TObj],
                &[Unr, Lin],
                Unit,
                Unr,
                Purity::Effectful,
            ),
        ),
        AsyncParkRecv => holding(
            &[false, true],
            kinded(
                "Std.Async.park_recv",
                &[Int, TObj],
                &[Unr, Lin],
                Unit,
                Unr,
                Purity::Effectful,
            ),
        ),
        // token は待つ間だけ記録が持ち、再開のときに継続へ返す。預かるのは継続だけである
        // (docs/spec/vm.md の「`Async` のタスク」)
        AsyncParkIo => holding(
            &[false, true],
            kinded(
                "Std.Async.park_io",
                &[Obj, TObj],
                &[Lin, Lin],
                Unit,
                Unr,
                Purity::Effectful,
            ),
        ),
        // channel の行 (docs/spec/stdlib.md の「`Std.Chan`」)。channel の引数は即値で、RC を持たない
        ChanNewRaw => row("Std.Chan.new_raw", &[Int], Int, Purity::Effectful),
        ChanToCell => row("Std.Chan.chan_cell", &[Int], Int, Purity::Pure),
        ChanReserveSend => row("Std.Chan.reserve_send", &[Int, Enum], Int, Purity::Effectful),
        ChanPushRaw => holding(
            &[true, false],
            kinded(
                "Std.Chan.push_raw",
                &[TObj, Int],
                &[Poly, Unr],
                Unit,
                Unr,
                Purity::Effectful,
            ),
        ),
        ChanReserveRecv => row("Std.Chan.reserve_recv", &[Int, Enum], Int, Purity::Effectful),
        ChanPopRaw => kinded("Std.Chan.pop_raw", &[Int], &[Unr], TObj, Poly, Purity::Effectful),
        ChanCloseRaw => row("Std.Chan.close_raw", &[Int], Unit, Purity::Effectful),
        AtomicNew => row("Std.Atomic.new", &[Int], Int, Purity::Effectful),
        AtomicAdd => row("Std.Atomic.add", &[Int, Int], Int, Purity::Effectful),
        AtomicLoad => row("Std.Atomic.load", &[Int], Int, Purity::Effectful),
        AtomicStore => row("Std.Atomic.store", &[Int, Int], Unit, Purity::Effectful),
        // 実行系のテストのための内部の口 (docs/spec/stdlib.md の「`Std.IoTest`」)。受け付けの行は buffer とストリームを
        // 消費して操作の記録に移し、取り出しの行が token を消費して記録を放す
        IoTestOpenPair => kinded(
            "Std.IoTest.open_pair",
            &[Unit],
            &[Unr],
            Obj,
            Lin,
            Purity::Effectful,
        ),
        IoTestPeer => borrowing(
            &[true],
            FunctionRow {
                with_arg: Some(0),
                ..kinded(
                    "Std.IoTest.peer",
                    &[Obj],
                    &[Lin],
                    Obj,
                    Lin,
                    Purity::Effectful,
                )
            },
        ),
        IoTestSubmitWrite => kinded(
            "Std.IoTest.submit_write",
            &[Obj, Obj],
            &[Unr, Lin],
            Obj,
            Lin,
            Purity::Effectful,
        ),
        IoTestOpStream => borrowing(
            &[true],
            FunctionRow {
                with_arg: Some(0),
                ..kinded(
                    "Std.IoTest.op_stream",
                    &[Obj],
                    &[Lin],
                    Obj,
                    Lin,
                    Purity::Effectful,
                )
            },
        ),
        IoTestTakeWrite => kinded(
            "Std.IoTest.take_write",
            &[Obj],
            &[Lin],
            Int,
            Unr,
            Purity::Effectful,
        ),
    }
}

impl Extern {
    /// `Std.Async` の行と、`chan_cell` を除く `Std.Chan` の行と、`Std.Atomic` と `Std.IoTest` の行。VM はこれらの行を
    /// 汎用の extern の命令でなく `Async` の命令で実行する。登録表に触れるためである (docs/spec/vm.md の「`Async` のタスク」)。
    pub fn is_async(self) -> bool {
        matches!(
            self,
            Extern::AsyncStartRaw
                | Extern::AsyncRunRaw
                | Extern::AsyncPark
                | Extern::AsyncParkTime
                | Extern::AsyncReady
                | Extern::AsyncFinishOk
                | Extern::AsyncFinishIo
                | Extern::AsyncFinishPanic
                | Extern::AsyncTaskCell
                | Extern::AsyncTakeResult
                | Extern::AsyncRunResult
                | Extern::AsyncDetach
                | Extern::AsyncCancelRaw
                | Extern::AsyncShieldEnter
                | Extern::AsyncShieldLeave
                | Extern::AsyncStartInRaw
                | Extern::AsyncOpenScope
                | Extern::AsyncCloseScope
                | Extern::AsyncLeaveScope
                | Extern::AsyncCancelScopeRaw
                | Extern::AsyncFailScopeIo
                | Extern::AsyncFailScopePanic
                | Extern::AsyncCancelPoint
                | Extern::AsyncSpendBudget
                | Extern::AsyncClaimScope
                | Extern::AsyncParkDrain
                | Extern::AsyncCancelMembers
                | Extern::AsyncParkSend
                | Extern::AsyncParkRecv
                | Extern::AsyncParkIo
                | Extern::ChanNewRaw
                | Extern::ChanReserveSend
                | Extern::ChanPushRaw
                | Extern::ChanReserveRecv
                | Extern::ChanPopRaw
                | Extern::ChanCloseRaw
                | Extern::AtomicNew
                | Extern::AtomicAdd
                | Extern::AtomicLoad
                | Extern::AtomicStore
                | Extern::IoTestOpenPair
                | Extern::IoTestPeer
                | Extern::IoTestSubmitWrite
                | Extern::IoTestOpStream
                | Extern::IoTestTakeWrite
        )
    }
}

/// ランタイムが名前で呼ぶ std の eml の関数。どれも単相なので、名前がそのまま instance の名前になり、引数の Repr が
/// 名前で決まる (docs/spec/stdlib.md の「`Std.Async`」)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RuntimeEntry {
    /// タスクを始める。引数はタスクの本体の閉包である。
    StartTask,
    /// 待たせたタスクを再開する。引数は預けた継続と、それに渡す `Wake` である。
    ResumeTask,
    /// driver の操作を待たせたタスクを再開する。引数は預けた継続と、それに渡す `Wake` と token である。
    ResumeIo,
}

impl RuntimeEntry {
    pub const ALL: &[RuntimeEntry] = &[
        RuntimeEntry::StartTask,
        RuntimeEntry::ResumeTask,
        RuntimeEntry::ResumeIo,
    ];

    pub fn name(self) -> &'static str {
        match self {
            RuntimeEntry::StartTask => "Std.Async.start_task",
            RuntimeEntry::ResumeTask => "Std.Async.resume_task",
            RuntimeEntry::ResumeIo => "Std.Async.resume_io",
        }
    }

    pub fn arity(self) -> usize {
        self.params().len()
    }

    /// 引数の Repr。結果はどれも `Unit` である。
    pub fn params(self) -> &'static [Repr] {
        match self {
            RuntimeEntry::StartTask => &[Repr::TObj],
            RuntimeEntry::ResumeTask => &[Repr::TObj, Repr::Enum],
            RuntimeEntry::ResumeIo => &[Repr::TObj, Repr::Enum, Repr::Obj],
        }
    }
}

pub struct FunctionRow {
    pub name: &'static str,
    /// 引数の Repr。数はシグネチャの一番外側の `->` の数と等しい。
    pub params: &'static [Repr],
    /// 結果の Repr。`Never` は戻らない extern で、`ret_mark` は読まない。データを返す行は、比べる extern、
    /// `Std.Env.has`、`Prelude.shows_minus`、`Std.Float.can_parse` の `Bool` と、`compare` の `Ordering` である。
    /// タグは決まっているので、行は配置を持たない。組は extern が作らず、`with_arg` の行は translate が作る。`List`
    /// や `Option` も extern が作らず、標準ライブラリの eml が包む (docs/spec/core-ir.md の「translate の配置」)。
    pub ret: OpRet,
    /// 引数ごとの印。長さは `params` と同じである。LIR の印と同じ3値で、型変数の位置のうちスキームが `Unr` に固定しない
    /// 位置は `Poly` である (docs/spec/core-ir.md の「表の印の列」)。LIR の verifier が、呼び出しの引数と結果の印をこの列と比べる。
    pub param_marks: &'static [Mark],
    pub ret_mark: Mark,
    pub purity: Purity,
    /// 引数ごとの借用の印。長さは `params` と同じである。印の立つ引数を extern は読むだけで消費せず、失敗したときも
    /// 呼んだ側が持ったままである (docs/spec/lir.md の「extern の借用」)。
    pub borrowed: &'static [bool],
    /// `Some(k)` の行は、宣言の型では結果と引数 `k` の組を返す。行の結果 (`ret`、`ret_mark`) は組の最初の要素で、
    /// translate が呼び出しの後に `con` で組を作る。組を受けてすぐ開く所では、最適化の既知のコンストラクタで組が
    /// 消える (docs/spec/core-ir.md の「組を返す extern」)。
    pub with_arg: Option<usize>,
    /// 引数ごとの預かる印。長さは `params` と同じで、借用の印とは独立である。印の立つ引数の型引数の値を、処理系は
    /// 写さず中身を見ない。消費する引数なら持って後で高々1回返すか落とし、借りる引数なら読むだけで持たない。その型に
    /// 現れる型変数は `Unr` に固定せず、矢印の row も問わない (docs/spec/declarations.md の「`extern`」)。
    pub held: &'static [bool],
}

impl FunctionRow {
    /// 結果を使わない呼び出しを消してよい行。`Pure` で、借りずに受ける `Lin` の引数を持たない行である。`Lin` の値を
    /// 消費する呼び出しを消すと、その値を手放す命令がなくなり、Perceus が `lin` の変数を `decref` するためである
    /// (docs/spec/lir.md の「縮約」)。
    pub fn removable(&self) -> bool {
        self.purity == Purity::Pure
            && self
                .param_marks
                .iter()
                .zip(self.borrowed)
                .all(|(&mark, &borrowed)| borrowed || mark == Mark::Unr)
    }
}

/// 引数の Repr と印を同じ長さの配列で受け、`params` と `param_marks` の長さを型で揃える。
const fn kinded<const N: usize>(
    name: &'static str,
    params: &'static [Repr; N],
    param_marks: &'static [Mark; N],
    ret: Repr,
    ret_mark: Mark,
    purity: Purity,
) -> FunctionRow {
    FunctionRow {
        name,
        params,
        ret: OpRet::Repr(ret),
        param_marks,
        ret_mark,
        purity,
        borrowed: const { &[false; N] },
        with_arg: None,
        held: const { &[false; N] },
    }
}

/// 印の立つ引数を借りる行。長さは引数の数と同じである (docs/spec/lir.md の「extern の借用」)。
const fn borrowing<const N: usize>(mask: &'static [bool; N], row: FunctionRow) -> FunctionRow {
    assert!(
        row.params.len() == N,
        "a borrow mask has one mark per parameter"
    );
    FunctionRow {
        borrowed: mask,
        ..row
    }
}

/// 印の立つ引数を預かる行。長さは引数の数と同じである (docs/spec/declarations.md の「`extern`」)。
const fn holding<const N: usize>(mask: &'static [bool; N], row: FunctionRow) -> FunctionRow {
    assert!(
        row.params.len() == N,
        "a held mask has one mark per parameter"
    );
    FunctionRow { held: mask, ..row }
}

/// ハンドルに書く行。データを借り、ハンドルを消費して返す (docs/spec/stdlib.md の「ハンドル」)。
const fn writing(name: &'static str) -> FunctionRow {
    borrowing(
        &[true, false],
        kinded(
            name,
            &[Repr::Obj, Repr::Obj],
            &[Mark::Unr, Mark::Lin],
            Repr::Obj,
            Mark::Lin,
            Purity::Effectful,
        ),
    )
}

/// 線形な値を扱わない行は、すべて `Unr` にする。
const fn row<const N: usize>(
    name: &'static str,
    params: &'static [Repr; N],
    ret: Repr,
    purity: Purity,
) -> FunctionRow {
    kinded(
        name,
        params,
        const { &[Mark::Unr; N] },
        ret,
        Mark::Unr,
        purity,
    )
}
