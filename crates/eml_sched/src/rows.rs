//! extern の行の一覧。どちらのエンジンも行の本体をここの1つのテキストから取るので、行の実装は1つで済む。展開`defs`が
//! 行ごとの関数を、展開`names`が行の名前と置き場所の表`LISTED`を、同じ一覧から作る。

use eml_extern::{Extern, FunctionRow, OpRet, Repr};
use eml_rt::{Ctx, Mode, Stream, Word, externs};

use crate::error::RunFault;

/// 一覧の中の行の置き場所。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// VMの汎用のextern命令が呼ぶ行で、ネイティブでは呼び出しになる。
    Generic,
    /// VMの専用の命令が呼ぶ行。
    Dedicated,
    /// 両方のエンジンが手で書く行。
    Engine,
    /// extern の呼び出しにならない行。
    Lir,
    /// 本体を共有しない、タスクの行。
    Task,
}

/// 一覧の1行が持つ、1つの変種の記録。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Listed {
    pub row: Extern,
    pub section: Section,
    /// ネイティブのエントリの名前。`inline`の行と、本体を共有しない節の行は持たない。
    pub entry: Option<&'static str>,
    /// ネイティブが命令へ展開する行で、エントリを持たない。
    pub inline: bool,
}

const fn repr_eq(a: Repr, b: Repr) -> bool {
    matches!(
        (a, b),
        (Repr::Obj, Repr::Obj)
            | (Repr::TObj, Repr::TObj)
            | (Repr::Int, Repr::Int)
            | (Repr::Float, Repr::Float)
            | (Repr::Enum, Repr::Enum)
            | (Repr::Unit, Repr::Unit)
            | (Repr::Token, Repr::Token)
    )
}

const fn kinds_match(row: &FunctionRow<'_>, params: &[Repr], ret: OpRet) -> bool {
    if row.params.len() != params.len() {
        return false;
    }
    let mut i = 0;
    while i < params.len() {
        if !repr_eq(row.params[i], params[i]) {
            return false;
        }
        i += 1;
    }
    match (row.ret, ret) {
        (OpRet::Never, OpRet::Never) => true,
        (OpRet::Repr(a), OpRet::Repr(b)) => repr_eq(a, b),
        _ => false,
    }
}

// 引数の種類は`entries!`の字句と同じで、対象の物体の語は`Word`のまま渡す。物体の語を整数から作り直さないためである
// (docs/spec/native.md の「エントリのABI」)
macro_rules! arg_ty {
    (obj) => {
        Word
    };
    (tobj) => {
        Word
    };
    (token) => {
        Word
    };
    (unit) => {
        i64
    };
    (int) => {
        i64
    };
    (enum) => {
        i64
    };
    (float) => {
        f64
    };
}

macro_rules! ret_ty {
    (obj) => {
        Word
    };
    (tobj) => {
        Word
    };
    (unit) => {
        ()
    };
    (int) => {
        i64
    };
    (enum) => {
        i64
    };
    (float) => {
        f64
    };
    (never) => {
        ::core::convert::Infallible
    };
}

macro_rules! repr_of {
    (obj) => {
        Repr::Obj
    };
    (tobj) => {
        Repr::TObj
    };
    (token) => {
        Repr::Token
    };
    (unit) => {
        Repr::Unit
    };
    (int) => {
        Repr::Int
    };
    (enum) => {
        Repr::Enum
    };
    (float) => {
        Repr::Float
    };
}

macro_rules! ret_of {
    (never) => {
        OpRet::Never
    };
    ($kind:tt) => {
        OpRet::Repr(repr_of!($kind))
    };
}

macro_rules! section_of {
    (generic) => {
        Section::Generic
    };
    (dedicated) => {
        Section::Dedicated
    };
    (engine) => {
        Section::Engine
    };
    (lir) => {
        Section::Lir
    };
    (task) => {
        Section::Task
    };
}

/// 行の一覧。`$site`は一覧を読む展開で、同じ一覧からエンジンごとの表と関数を作る。行は`fn 変種 as エントリ (ctx, 引数: 種類, ..)
/// -> 種類 { 本体 }`の形で、変種を`|`でつなぐと1つの関数を共有する。`inline`の行はエントリを持たない。エントリの名前は
/// `macro_rules!`が識別子を作れないので書き下す。
#[macro_export]
macro_rules! extern_rows {
    ($site:path) => {
        $site! {
            generic {
                fn Println as eml_x_Println(ctx, s: obj) -> unit {
                    ctx.println(Stream::Out, s)?;
                    Ok(())
                }
                fn Eprintln as eml_x_Eprintln(ctx, s: obj) -> unit {
                    ctx.println(Stream::Err, s)?;
                    Ok(())
                }
                fn IntShow as eml_x_IntShow(ctx, n: int) -> obj { Ok(ctx.int_show(n)) }
                fn StrShow as eml_x_StrShow(ctx, s: obj) -> obj { Ok(ctx.string_show(s)?) }
                fn FloatShow as eml_x_FloatShow(ctx, x: float) -> obj { Ok(ctx.float_show(x)) }
                fn StrConcat as eml_x_StrConcat(ctx, a: obj, b: obj) -> obj {
                    Ok(ctx.string_append(a, b)?)
                }
                fn StrEq as eml_x_StrEq(ctx, a: obj, b: obj) -> enum { Ok(ctx.string_eq(a, b)?) }
                fn StrCompare as eml_x_StrCompare(ctx, a: obj, b: obj) -> enum {
                    Ok(ctx.string_compare(a, b)?)
                }
            }
            dedicated {
                fn FloatMod as eml_x_FloatMod(_, a: float, b: float) -> float {
                    Ok(externs::float_rem(a, b))
                }
                inline fn IntNeg(_, a: int) -> int { Ok(externs::int_negate(a)?) }
                inline fn CharToInt | ByteToInt | CharOfByte(_, a: int) -> int { Ok(a) }
                inline fn FloatNeg(_, a: float) -> float { Ok(externs::float_negate(a)) }
                inline fn IntAdd(_, a: int, b: int) -> int { Ok(externs::int_add(a, b)?) }
                inline fn IntSub(_, a: int, b: int) -> int { Ok(externs::int_sub(a, b)?) }
                inline fn IntMul(_, a: int, b: int) -> int { Ok(externs::int_mul(a, b)?) }
                inline fn IntDiv | ByteDiv(_, a: int, b: int) -> int {
                    Ok(externs::int_div(a, b)?)
                }
                inline fn IntMod | ByteMod(_, a: int, b: int) -> int {
                    Ok(externs::int_rem(a, b)?)
                }
                inline fn IntEq | CharEq | ByteEq(_, a: int, b: int) -> enum {
                    Ok(externs::int_eq(a, b))
                }
                inline fn IntNe | CharNe | ByteNe(_, a: int, b: int) -> enum {
                    Ok(externs::int_ne(a, b))
                }
                inline fn IntLt | CharLt | ByteLt(_, a: int, b: int) -> enum {
                    Ok(externs::int_lt(a, b))
                }
                inline fn IntLe | CharLe | ByteLe(_, a: int, b: int) -> enum {
                    Ok(externs::int_le(a, b))
                }
                inline fn IntGt | CharGt | ByteGt(_, a: int, b: int) -> enum {
                    Ok(externs::int_gt(a, b))
                }
                inline fn IntGe | CharGe | ByteGe(_, a: int, b: int) -> enum {
                    Ok(externs::int_ge(a, b))
                }
                inline fn IntCompare | CharCompare | ByteCompare(_, a: int, b: int) -> enum {
                    Ok(externs::int_compare(a, b))
                }
                inline fn FloatAdd(_, a: float, b: float) -> float { Ok(externs::float_add(a, b)) }
                inline fn FloatSub(_, a: float, b: float) -> float { Ok(externs::float_sub(a, b)) }
                inline fn FloatMul(_, a: float, b: float) -> float { Ok(externs::float_mul(a, b)) }
                inline fn FloatDiv(_, a: float, b: float) -> float { Ok(externs::float_div(a, b)) }
                inline fn FloatEq(_, a: float, b: float) -> enum { Ok(externs::float_eq(a, b)) }
                inline fn FloatNe(_, a: float, b: float) -> enum { Ok(externs::float_ne(a, b)) }
                inline fn FloatLt(_, a: float, b: float) -> enum { Ok(externs::float_lt(a, b)) }
                inline fn FloatLe(_, a: float, b: float) -> enum { Ok(externs::float_le(a, b)) }
                inline fn FloatGt(_, a: float, b: float) -> enum { Ok(externs::float_gt(a, b)) }
                inline fn FloatGe(_, a: float, b: float) -> enum { Ok(externs::float_ge(a, b)) }
                inline fn BoolEq(_, a: enum, b: enum) -> enum { Ok(externs::bool_eq(a, b)) }
                inline fn BoolNe(_, a: enum, b: enum) -> enum { Ok(externs::bool_ne(a, b)) }
            }
            engine {}
            lir {}
            task {}
        }
    };
}

/// 行ごとに、一覧の最初の変種の名前の関数を1つ作る。変種ごとに、行の種類が`Extern::<変種>.row()`と等しいことをconstで確かめる。
macro_rules! defs {
    (
        generic { $($generic:tt)* }
        dedicated { $($dedicated:tt)* }
        engine { $($engine:tt)* }
        lir { $($lir:tt)* }
        task { $($task:tt)* }
    ) => {
        defs!(@lines $($generic)*);
        defs!(@lines $($dedicated)*);
    };
    (@lines) => {};
    (@lines
        inline fn $first:ident $(| $rest:ident)* ( $ctx:tt $(, $arg:ident : $kind:tt)* ) -> $ret:tt $body:block
        $($tail:tt)*
    ) => {
        defs!(@function $first ($ctx $(, $arg : $kind)*) $ret $body);
        defs!(@check [$($kind)*] $ret $first $($rest)*);
        defs!(@lines $($tail)*);
    };
    (@lines
        fn $first:ident as $entry:ident $(| $rest:ident as $rest_entry:ident)*
        ( $ctx:tt $(, $arg:ident : $kind:tt)* ) -> $ret:tt $body:block
        $($tail:tt)*
    ) => {
        defs!(@function $first ($ctx $(, $arg : $kind)*) $ret $body);
        defs!(@check [$($kind)*] $ret $first $($rest)*);
        defs!(@lines $($tail)*);
    };
    (@function $first:ident ($ctx:tt $(, $arg:ident : $kind:tt)*) $ret:tt $body:block) => {
        /// # Safety
        /// 引数の語は生きている物体で、`Ctx`の対応する操作が求める条件を満たす。借りない引数は行が消費する
        /// (docs/spec/lir.md の「extern の借用」)。
        #[inline]
        #[allow(unused_unsafe, non_snake_case)]
        pub unsafe fn $first<M: Mode>(
            $ctx: &mut Ctx<M>
            $(, $arg: arg_ty!($kind))*
        ) -> Result<ret_ty!($ret), RunFault> {
            unsafe { $body }
        }
    };
    (@check $kinds:tt $ret:tt) => {};
    (@check [$($kind:tt)*] $ret:tt $variant:ident $($rest:ident)*) => {
        const _: () = assert!(kinds_match(
            Extern::$variant.row(),
            &[$(repr_of!($kind)),*],
            ret_of!($ret),
        ));
        defs!(@check [$($kind)*] $ret $($rest)*);
    };
}

/// 一覧の全変種を、置き場所とエントリの名前とともに`LISTED`に並べる。
macro_rules! names {
    (
        generic { $($generic:tt)* }
        dedicated { $($dedicated:tt)* }
        engine { $($engine:ident),* $(,)? }
        lir { $($lir:ident),* $(,)? }
        task { $($task:ident),* $(,)? }
    ) => {
        names!(@go []
            generic lines [$($generic)*]
            dedicated lines [$($dedicated)*]
            engine names [$($engine)*]
            lir names [$($lir)*]
            task names [$($task)*]
        );
    };
    (@go [$($listed:tt)*]) => {
        pub const LISTED: &[Listed] = &[$($listed)*];
    };
    (@go $listed:tt $section:ident $form:ident [] $($next:tt)*) => {
        names!(@go $listed $($next)*);
    };
    (@go [$($listed:tt)*] $section:ident lines [
        inline fn $first:ident $(| $rest:ident)* ( $ctx:tt $(, $arg:ident : $kind:tt)* ) -> $ret:tt $body:block
        $($tail:tt)*
    ] $($next:tt)*) => {
        names!(@go [
            $($listed)*
            Listed { row: Extern::$first, section: section_of!($section), entry: None, inline: true },
            $(Listed { row: Extern::$rest, section: section_of!($section), entry: None, inline: true },)*
        ] $section lines [$($tail)*] $($next)*);
    };
    (@go [$($listed:tt)*] $section:ident lines [
        fn $first:ident as $entry:ident $(| $rest:ident as $rest_entry:ident)*
        ( $ctx:tt $(, $arg:ident : $kind:tt)* ) -> $ret:tt $body:block
        $($tail:tt)*
    ] $($next:tt)*) => {
        names!(@go [
            $($listed)*
            Listed {
                row: Extern::$first,
                section: section_of!($section),
                entry: Some(stringify!($entry)),
                inline: false,
            },
            $(Listed {
                row: Extern::$rest,
                section: section_of!($section),
                entry: Some(stringify!($rest_entry)),
                inline: false,
            },)*
        ] $section lines [$($tail)*] $($next)*);
    };
    (@go [$($listed:tt)*] $section:ident names [$first:ident $($rest:ident)*] $($next:tt)*) => {
        names!(@go [
            $($listed)*
            Listed { row: Extern::$first, section: section_of!($section), entry: None, inline: false },
        ] $section names [$($rest)*] $($next)*);
    };
}

extern_rows!(defs);
extern_rows!(names);

#[cfg(test)]
mod tests {
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};

    use eml_extern::Extern;
    use eml_rt::{Buffering, Control, Ctx, Fast, HostRef, Limits, StdHost};

    use super::*;

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn println_writes_a_line() {
        let out = Capture::default();
        let host = StdHost::new(
            Box::new(out.clone()),
            Box::new(io::sink()),
            Default::default(),
            Buffering::Line,
        );
        let mut ctx = Ctx::<Fast>::new(
            Arc::new(Control::new(Limits::default())),
            HostRef::new(Arc::new(host)),
        );
        let line = ctx.fresh_string("hello");
        unsafe { Println::<Fast>(&mut ctx, line) }.unwrap();
        assert_eq!(out.0.lock().unwrap().as_slice(), b"hello\n");
    }

    #[test]
    fn the_listed_rows_are_the_seed_and_the_inline_rows() {
        use Extern::*;
        let generic = [
            Println, Eprintln, IntShow, StrShow, FloatShow, StrConcat, StrEq, StrCompare,
        ];
        let inline = [
            IntNeg,
            CharToInt,
            ByteToInt,
            CharOfByte,
            FloatNeg,
            IntAdd,
            IntSub,
            IntMul,
            IntDiv,
            ByteDiv,
            IntMod,
            ByteMod,
            IntEq,
            CharEq,
            ByteEq,
            IntNe,
            CharNe,
            ByteNe,
            IntLt,
            CharLt,
            ByteLt,
            IntLe,
            CharLe,
            ByteLe,
            IntGt,
            CharGt,
            ByteGt,
            IntGe,
            CharGe,
            ByteGe,
            IntCompare,
            CharCompare,
            ByteCompare,
            FloatAdd,
            FloatSub,
            FloatMul,
            FloatDiv,
            FloatEq,
            FloatNe,
            FloatLt,
            FloatLe,
            FloatGt,
            FloatGe,
            BoolEq,
            BoolNe,
        ];
        assert_eq!(inline.len(), 45);

        let of_section = |section| -> Vec<Listed> {
            LISTED
                .iter()
                .copied()
                .filter(|l| l.section == section)
                .collect()
        };
        assert_eq!(LISTED.len(), 54);
        assert!(of_section(Section::Engine).is_empty());
        assert!(of_section(Section::Lir).is_empty());
        assert!(of_section(Section::Task).is_empty());

        let generic_listed = of_section(Section::Generic);
        assert_eq!(
            generic_listed.iter().map(|l| l.row).collect::<Vec<_>>(),
            generic
        );
        for l in &generic_listed {
            assert!(!l.inline);
            assert_eq!(l.entry, Some(format!("eml_x_{:?}", l.row).as_str()));
        }

        let dedicated = of_section(Section::Dedicated);
        let (called, inlined): (Vec<Listed>, Vec<Listed>) =
            dedicated.iter().partition(|l| !l.inline);
        assert_eq!(called.len(), 1);
        assert_eq!(called[0].row, FloatMod);
        assert_eq!(called[0].entry, Some("eml_x_FloatMod"));
        let mut listed_inline: Vec<Extern> = inlined.iter().map(|l| l.row).collect();
        for l in &inlined {
            assert_eq!(l.entry, None);
        }
        let mut expected = inline.to_vec();
        listed_inline.sort_by_key(|e| *e as usize);
        expected.sort_by_key(|e| *e as usize);
        assert_eq!(listed_inline, expected);
    }

    #[test]
    fn a_names_row_has_no_entry() {
        mod listed {
            use super::super::*;
            names! {
                generic {}
                dedicated {}
                engine { Panic, Exit }
                lir { Par }
                task {}
            }
        }
        let rows: Vec<_> = listed::LISTED
            .iter()
            .map(|l| (l.row, l.section, l.entry, l.inline))
            .collect();
        assert_eq!(
            rows,
            [
                (Extern::Panic, Section::Engine, None, false),
                (Extern::Exit, Section::Engine, None, false),
                (Extern::Par, Section::Lir, None, false),
            ]
        );
    }
}
