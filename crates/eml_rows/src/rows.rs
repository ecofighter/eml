//! extern の行の一覧。どちらのエンジンも行の本体をここの1つのテキストから取るので、行の実装は1つで済む。展開`defs`が
//! 一覧から、行ごとの関数と、表`SHARED`と、変種で行を流す関数`call`を作る。`eml_nrt`の展開`entries`は、変種ごとに
//! ネイティブのエントリを1つ作る (docs/spec/native.md の「externの失敗」)。

use eml_extern::{Extern, FunctionRow, OpRet, Repr};
use eml_rt::externs::{Fault, Raised};
use eml_rt::{Ctx, Form, Handle, Mode, ReadAmount, Stream, Word, Write, WriteMode, externs};

use eml_sched::error::RunFault;

const fn repr_eq(a: Repr, b: Repr) -> bool {
    matches!(
        (a, b),
        (Repr::Obj, Repr::Obj)
            | (Repr::TObj, Repr::TObj)
            | (Repr::Int, Repr::Int)
            | (Repr::Float, Repr::Float)
            | (Repr::Enum, Repr::Enum)
            | (Repr::Unit, Repr::Unit)
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

macro_rules! read_arg {
    ($args:ident, $i:expr, obj) => {
        $args.word($i)
    };
    ($args:ident, $i:expr, tobj) => {
        $args.word($i)
    };
    ($args:ident, $i:expr, unit) => {
        $args.int($i)
    };
    ($args:ident, $i:expr, int) => {
        $args.int($i)
    };
    ($args:ident, $i:expr, enum) => {
        $args.int($i)
    };
    ($args:ident, $i:expr, float) => {
        $args.float($i)
    };
}

macro_rules! value_of {
    (obj, $v:ident) => {
        Ok(Value::Word($v))
    };
    (tobj, $v:ident) => {
        Ok(Value::Word($v))
    };
    (unit, $v:ident) => {{
        let () = $v;
        Ok(Value::Word(Word::UNIT))
    }};
    (int, $v:ident) => {
        Ok(Value::Int($v))
    };
    (enum, $v:ident) => {
        Ok(Value::Int($v))
    };
    (float, $v:ident) => {
        Ok(Value::Float($v))
    };
    (never, $v:ident) => {
        match $v {}
    };
}

/// 行の一覧。`$site`は一覧を読む展開である。行は`fn 変種(ctx, 引数: 種類, ..) -> 種類 { 本体 }`の形で、変種を`|`で
/// つなぐと1つの関数を共有する。名前の組は、共有の本体を持たない行を並べる。`engine`は各エンジンが自分で書く行、`lir`は
/// externの呼び出しにならない行、`task`はタスクの行、`pending`は本体がまだVMにある行である。
#[macro_export]
macro_rules! extern_rows {
    ($site:path) => {
        $site! {
            rows {
                fn Println(ctx, s: obj) -> unit {
                    ctx.println(Stream::Out, s)?;
                    Ok(())
                }
                fn Eprintln(ctx, s: obj) -> unit {
                    ctx.println(Stream::Err, s)?;
                    Ok(())
                }
                fn IntShow | ByteDisplay(ctx, n: int) -> obj { Ok(ctx.int_show(n)) }
                // SAFETY: `Char` の語は Unicode のスカラー値である (docs/spec/lir.md の「値の表現」)
                fn CharShow(ctx, c: int) -> obj { Ok(ctx.char_show(c)) }
                fn CharDisplay(ctx, c: int) -> obj { Ok(ctx.char_display(c)) }
                fn StrShow(ctx, s: obj) -> obj { Ok(ctx.string_show(s)?) }
                fn StrNe(ctx, a: obj, b: obj) -> enum { Ok(ctx.string_ne(a, b)?) }
                fn StrCharAt(ctx, i: int, s: obj) -> int { Ok(ctx.string_char_at(i, s)?) }
                fn StrSlice(ctx, start: int, end: int, s: obj) -> obj {
                    Ok(ctx.string_slice(start, end, s)?)
                }
                // SAFETY: `Char` の語は Unicode のスカラー値である (docs/spec/lir.md の「値の表現」)
                fn StrPush(ctx, c: int, s: obj) -> obj { Ok(ctx.string_push_char(c, s)?) }
                fn StrFindFrom(ctx, needle: obj, from: int, s: obj) -> int {
                    Ok(ctx.string_find_from(needle, from, s)?)
                }
                fn StringDecode(ctx, b: obj) -> obj { Ok(ctx.string_decode(b)?) }
                fn BytesEmpty(ctx, _u: unit) -> obj { Ok(ctx.bytes_empty()?) }
                fn BytesAppend(ctx, a: obj, b: obj) -> obj { Ok(ctx.bytes_append(a, b)?) }
                fn BytesAppendSlice(ctx, start: int, end: int, src: obj, dst: obj) -> obj {
                    Ok(ctx.bytes_append_part(start, end, src, dst)?)
                }
                fn BytesSlice(ctx, start: int, end: int, b: obj) -> obj {
                    Ok(ctx.bytes_slice(start, end, b)?)
                }
                fn BytesTruncate(ctx, n: int, b: obj) -> obj { Ok(ctx.bytes_truncate(n, b)?) }
                fn BytesOfString(ctx, s: obj) -> obj { Ok(ctx.bytes_of_string(s)?) }
                fn BytesEq(ctx, a: obj, b: obj) -> enum { Ok(ctx.bytes_eq(a, b)?) }
                fn BytesNe(ctx, a: obj, b: obj) -> enum { Ok(ctx.bytes_ne(a, b)?) }
                fn BytesCompare(ctx, a: obj, b: obj) -> enum { Ok(ctx.bytes_compare(a, b)?) }
                fn BytesFindFrom(ctx, needle: obj, from: int, b: obj) -> int {
                    Ok(ctx.bytes_find_from(needle, from, b)?)
                }
                fn BytesMake(ctx, n: int, byte: int) -> obj { Ok(ctx.bytes_make(n, byte)?) }
                fn BytesWithCapacity(ctx, n: int) -> obj { Ok(ctx.bytes_with_capacity(n)?) }
                fn BytesSet(ctx, i: int, byte: int, b: obj) -> obj {
                    Ok(ctx.bytes_set(i, byte, b)?)
                }
                fn BytesPush(ctx, byte: int, b: obj) -> obj { Ok(ctx.bytes_push(byte, b)?) }
                fn BytesFindByte(ctx, byte: int, from: int, b: obj) -> int {
                    Ok(ctx.bytes_find_byte(byte, from, b)?)
                }
                fn FloatShow(ctx, x: float) -> obj { Ok(ctx.float_show(x)) }
                fn StrConcat(ctx, a: obj, b: obj) -> obj { Ok(ctx.string_append(a, b)?) }
                fn StrEq(ctx, a: obj, b: obj) -> enum { Ok(ctx.string_eq(a, b)?) }
                fn StrCompare(ctx, a: obj, b: obj) -> enum { Ok(ctx.string_compare(a, b)?) }
                fn FloatMod(_, a: float, b: float) -> float { Ok(externs::float_rem(a, b)) }
                fn IntNeg(_, a: int) -> int { Ok(externs::int_negate(a)?) }
                fn CharToInt | ByteToInt | CharOfByte(_, a: int) -> int { Ok(a) }
                fn FloatNeg(_, a: float) -> float { Ok(externs::float_negate(a)) }
                fn IntAdd(_, a: int, b: int) -> int { Ok(externs::int_add(a, b)?) }
                fn IntSub(_, a: int, b: int) -> int { Ok(externs::int_sub(a, b)?) }
                fn IntMul(_, a: int, b: int) -> int { Ok(externs::int_mul(a, b)?) }
                fn IntDiv | ByteDiv(_, a: int, b: int) -> int { Ok(externs::int_div(a, b)?) }
                fn IntMod | ByteMod(_, a: int, b: int) -> int { Ok(externs::int_rem(a, b)?) }
                fn IntEq | CharEq | ByteEq(_, a: int, b: int) -> enum { Ok(externs::int_eq(a, b)) }
                fn IntNe | CharNe | ByteNe(_, a: int, b: int) -> enum { Ok(externs::int_ne(a, b)) }
                fn IntLt | CharLt | ByteLt(_, a: int, b: int) -> enum { Ok(externs::int_lt(a, b)) }
                fn IntLe | CharLe | ByteLe(_, a: int, b: int) -> enum { Ok(externs::int_le(a, b)) }
                fn IntGt | CharGt | ByteGt(_, a: int, b: int) -> enum { Ok(externs::int_gt(a, b)) }
                fn IntGe | CharGe | ByteGe(_, a: int, b: int) -> enum { Ok(externs::int_ge(a, b)) }
                fn IntCompare | CharCompare | ByteCompare(_, a: int, b: int) -> enum {
                    Ok(externs::int_compare(a, b))
                }
                fn FloatAdd(_, a: float, b: float) -> float { Ok(externs::float_add(a, b)) }
                fn FloatSub(_, a: float, b: float) -> float { Ok(externs::float_sub(a, b)) }
                fn FloatMul(_, a: float, b: float) -> float { Ok(externs::float_mul(a, b)) }
                fn FloatDiv(_, a: float, b: float) -> float { Ok(externs::float_div(a, b)) }
                fn FloatEq(_, a: float, b: float) -> enum { Ok(externs::float_eq(a, b)) }
                fn FloatNe(_, a: float, b: float) -> enum { Ok(externs::float_ne(a, b)) }
                fn FloatLt(_, a: float, b: float) -> enum { Ok(externs::float_lt(a, b)) }
                fn FloatLe(_, a: float, b: float) -> enum { Ok(externs::float_le(a, b)) }
                fn FloatGt(_, a: float, b: float) -> enum { Ok(externs::float_gt(a, b)) }
                fn FloatGe(_, a: float, b: float) -> enum { Ok(externs::float_ge(a, b)) }
                fn BoolEq(_, a: enum, b: enum) -> enum { Ok(externs::bool_eq(a, b)) }
                fn BoolNe(_, a: enum, b: enum) -> enum { Ok(externs::bool_ne(a, b)) }
                fn CharOfInt(_, n: int) -> int { Ok(externs::char_of_int(n)?) }
                fn ByteOfInt(_, n: int) -> int { Ok(externs::byte_of_int(n)?) }
                fn ByteNeg(_, a: int) -> int { Ok(externs::byte_negate(a)?) }
                fn ByteAdd(_, a: int, b: int) -> int { Ok(externs::byte_add(a, b)?) }
                fn ByteSub(_, a: int, b: int) -> int { Ok(externs::byte_sub(a, b)?) }
                fn ByteMul(_, a: int, b: int) -> int { Ok(externs::byte_mul(a, b)?) }
                fn ByteWrap(_, n: int) -> int { Ok(externs::byte_wrap(n)) }
                fn IntAnd | ByteAnd(_, a: int, b: int) -> int { Ok(externs::int_and(a, b)) }
                fn IntOr | ByteOr(_, a: int, b: int) -> int { Ok(externs::int_or(a, b)) }
                fn IntXor | ByteXor(_, a: int, b: int) -> int { Ok(externs::int_xor(a, b)) }
                fn IntComplement(_, a: int) -> int { Ok(externs::int_complement(a)) }
                fn ByteComplement(_, a: int) -> int { Ok(externs::byte_complement(a)) }
                fn IntShl(_, a: int, n: int) -> int { Ok(externs::int_shl(a, n)) }
                fn IntShr(_, a: int, n: int) -> int { Ok(externs::int_shr(a, n)) }
                fn ByteShl(_, a: int, n: int) -> int { Ok(externs::byte_shl(a, n)) }
                fn ByteShr(_, a: int, n: int) -> int { Ok(externs::byte_shr(a, n)) }
                fn CheckIndex(_, index: int, len: int) -> unit {
                    externs::check_index(index, len)?;
                    Ok(())
                }
                fn FloatCompare(_, a: float, b: float) -> enum { Ok(externs::float_compare(a, b)) }
                fn FloatShowsMinus(_, x: float) -> enum { Ok(externs::float_shows_minus(x)) }
                fn FloatOfInt(_, n: int) -> float { Ok(externs::float_of_int(n)) }
                fn FloatTruncate(_, x: float) -> int { Ok(externs::float_truncate(x)?) }
                fn FloatFloor(_, x: float) -> int { Ok(externs::float_floor(x)?) }
                fn FloatCeil(_, x: float) -> int { Ok(externs::float_ceil(x)?) }
                fn FloatRound(_, x: float) -> int { Ok(externs::float_round(x)?) }
                fn FloatSqrt(_, x: float) -> float { Ok(externs::float_sqrt(x)) }
                fn FloatAbs(_, x: float) -> float { Ok(externs::float_abs(x)) }
                fn FloatSin(_, x: float) -> float { Ok(externs::float_sin(x)) }
                fn FloatCos(_, x: float) -> float { Ok(externs::float_cos(x)) }
                fn FloatExp(_, x: float) -> float { Ok(externs::float_exp(x)) }
                fn FloatLog(_, x: float) -> float { Ok(externs::float_log(x)) }
                fn FloatPow(_, x: float, y: float) -> float { Ok(externs::float_pow(x, y)) }
                fn FloatInfinity(_, _u: unit) -> float { Ok(externs::float_infinity()) }
                fn FloatNan(_, _u: unit) -> float { Ok(externs::float_nan()) }
                fn FloatToFixed(ctx, digits: int, x: float) -> obj {
                    Ok(ctx.float_to_fixed(digits, x)?)
                }
                fn FloatCanParse(ctx, s: obj) -> enum {
                    Ok(if ctx.string_to_float(s)?.is_some() {
                        externs::TRUE
                    } else {
                        externs::FALSE
                    })
                }
                // `Std.Float.parse` は読める文字列にだけ呼ぶ。読めない文字列には NaN を返す
                fn FloatParseRaw(ctx, s: obj) -> float {
                    Ok(ctx.string_to_float(s)?.unwrap_or(f64::NAN))
                }
                fn ArrayMake(ctx, n: int, x: tobj) -> obj { Ok(ctx.array_make(n, x, false)?) }
                fn MutArrayMake(ctx, n: int, x: tobj) -> obj { Ok(ctx.array_make(n, x, true)?) }
                fn MutArrayEmpty(ctx, _u: unit) -> obj { Ok(ctx.array_empty(true)?) }
                fn ArrayThaw(ctx, a: obj) -> obj { Ok(ctx.array_thaw(a)?) }
                fn MutArrayFreeze(ctx, m: obj) -> obj { Ok(ctx.array_freeze(m)?) }
                // Floatの行とIntの行は、スロットの生のビットで同じ`Ctx`の操作を呼ぶ
                // (docs/spec/vm.mdの「基本操作と extern」)
                fn UnboxedArrayMakeFloat(ctx, n: int, x: float) -> obj {
                    Ok(ctx.unboxed_make(n, x.to_bits(), false)?)
                }
                fn UnboxedArrayMakeInt(ctx, n: int, x: int) -> obj {
                    Ok(ctx.unboxed_make(n, x as u64, false)?)
                }
                fn MutUnboxedArrayMakeFloat(ctx, n: int, x: float) -> obj {
                    Ok(ctx.unboxed_make(n, x.to_bits(), true)?)
                }
                fn MutUnboxedArrayMakeInt(ctx, n: int, x: int) -> obj {
                    Ok(ctx.unboxed_make(n, x as u64, true)?)
                }
                fn MutUnboxedArrayEmpty(ctx, _u: unit) -> obj { Ok(ctx.unboxed_empty(true)?) }
                fn MutUnboxedArrayFillFloat(ctx, a: obj, x: float) -> obj {
                    Ok(ctx.unboxed_fill(a, x.to_bits())?)
                }
                fn MutUnboxedArrayFillInt(ctx, a: obj, x: int) -> obj {
                    Ok(ctx.unboxed_fill(a, x as u64)?)
                }
                fn UnboxedArrayThaw(ctx, a: obj) -> obj { Ok(ctx.unboxed_thaw(a)?) }
                fn MutUnboxedArrayFreeze(ctx, m: obj) -> obj { Ok(ctx.unboxed_freeze(m)?) }
                fn ArrayLength | MutArrayLength(ctx, a: obj) -> int { Ok(ctx.array_length(a)?) }
                fn ArrayGet | MutArrayRead(ctx, a: obj, i: int) -> tobj { Ok(ctx.array_get(a, i)?) }
                fn ArraySet | MutArrayWrite(ctx, a: obj, i: int, x: tobj) -> obj {
                    Ok(ctx.array_set(a, i, x)?)
                }
                fn UnboxedArrayLength | MutUnboxedArrayLength(ctx, a: obj) -> int {
                    Ok(ctx.unboxed_length(a)?)
                }
                // Floatの行は`from_bits` / `to_bits`でビットをそのまま写すので、NaNのペイロードも残る。VMはFloatの
                // getとsetの行の代わりにIntの行を呼べる
                fn UnboxedArrayGetFloat | MutUnboxedArrayReadFloat(ctx, a: obj, i: int) -> float {
                    Ok(f64::from_bits(ctx.unboxed_get(a, i)?))
                }
                fn UnboxedArrayGetInt | MutUnboxedArrayReadInt(ctx, a: obj, i: int) -> int {
                    Ok(ctx.unboxed_get(a, i)? as i64)
                }
                fn UnboxedArraySetFloat | MutUnboxedArrayWriteFloat(
                    ctx, a: obj, i: int, x: float
                ) -> obj {
                    Ok(ctx.unboxed_set(a, i, x.to_bits())?)
                }
                fn UnboxedArraySetInt | MutUnboxedArrayWriteInt(
                    ctx, a: obj, i: int, x: int
                ) -> obj {
                    Ok(ctx.unboxed_set(a, i, x as u64)?)
                }
                fn StrByteLength(ctx, s: obj) -> int { Ok(ctx.string_byte_length(s)?) }
                fn StrByteAt(ctx, i: int, s: obj) -> int { Ok(ctx.string_byte_at(i, s)?) }
                fn BytesLength | ShowBytesLength(ctx, b: obj) -> int { Ok(ctx.bytes_length(b)?) }
                fn BytesGet | ShowBytesGet(ctx, i: int, b: obj) -> int { Ok(ctx.bytes_get(i, b)?) }
                fn Open(ctx, path: obj) -> obj { Ok(ctx.fs_open(path, externs::Open::Read)?) }
                fn FileOutCreate(ctx, path: obj) -> obj {
                    Ok(ctx.fs_open(path, externs::Open::Write(WriteMode::Truncate))?)
                }
                fn FileOutAppend(ctx, path: obj) -> obj {
                    Ok(ctx.fs_open(path, externs::Open::Write(WriteMode::Append))?)
                }
                fn ReadAll(ctx, h: obj) -> obj {
                    Ok(ctx.handle_read(h, Handle::FileIn, ReadAmount::All, Form::Text)?)
                }
                fn ReadLine(ctx, h: obj) -> obj {
                    Ok(ctx.handle_read(h, Handle::FileIn, ReadAmount::Line, Form::Text)?)
                }
                fn ReadAllBytes(ctx, h: obj) -> obj {
                    Ok(ctx.handle_read(h, Handle::FileIn, ReadAmount::All, Form::Bytes)?)
                }
                fn ReadLineBytes(ctx, h: obj) -> obj {
                    Ok(ctx.handle_read(h, Handle::FileIn, ReadAmount::Line, Form::Bytes)?)
                }
                fn StdinReadAll(ctx, h: obj) -> obj {
                    Ok(ctx.handle_read(h, Handle::Stdin, ReadAmount::All, Form::Text)?)
                }
                fn StdinReadLine(ctx, h: obj) -> obj {
                    Ok(ctx.handle_read(h, Handle::Stdin, ReadAmount::Line, Form::Text)?)
                }
                fn StdinReadAllBytes(ctx, h: obj) -> obj {
                    Ok(ctx.handle_read(h, Handle::Stdin, ReadAmount::All, Form::Bytes)?)
                }
                fn StdinReadLineBytes(ctx, h: obj) -> obj {
                    Ok(ctx.handle_read(h, Handle::Stdin, ReadAmount::Line, Form::Bytes)?)
                }
                fn ProcReadAll(ctx, h: obj) -> obj {
                    Ok(ctx.handle_read(h, Handle::Child, ReadAmount::All, Form::Text)?)
                }
                fn ProcReadLine(ctx, h: obj) -> obj {
                    Ok(ctx.handle_read(h, Handle::Child, ReadAmount::Line, Form::Text)?)
                }
                fn ProcReadAllBytes(ctx, h: obj) -> obj {
                    Ok(ctx.handle_read(h, Handle::Child, ReadAmount::All, Form::Bytes)?)
                }
                fn ProcReadLineBytes(ctx, h: obj) -> obj {
                    Ok(ctx.handle_read(h, Handle::Child, ReadAmount::Line, Form::Bytes)?)
                }
                fn ReadChunk(ctx, chunk: obj, h: obj) -> obj {
                    Ok(ctx.handle_read(h, Handle::FileIn, ReadAmount::Chunk(chunk), Form::Bytes)?)
                }
                fn StdinReadChunk(ctx, chunk: obj, h: obj) -> obj {
                    Ok(ctx.handle_read(h, Handle::Stdin, ReadAmount::Chunk(chunk), Form::Bytes)?)
                }
                fn ProcReadChunk(ctx, chunk: obj, h: obj) -> obj {
                    Ok(ctx.handle_read(h, Handle::Child, ReadAmount::Chunk(chunk), Form::Bytes)?)
                }
                fn Close(ctx, h: obj) -> unit {
                    ctx.handle_close(h, Handle::FileIn)?;
                    Ok(())
                }
                fn FileOutCloseRaw(ctx, h: obj) -> unit {
                    ctx.handle_close(h, Handle::FileOut)?;
                    Ok(())
                }
                fn StdinClose(ctx, h: obj) -> unit {
                    ctx.handle_close(h, Handle::Stdin)?;
                    Ok(())
                }
                fn StdoutClose(ctx, h: obj) -> unit {
                    ctx.handle_close(h, Handle::Stdout)?;
                    Ok(())
                }
                fn StderrClose(ctx, h: obj) -> unit {
                    ctx.handle_close(h, Handle::Stderr)?;
                    Ok(())
                }
                fn StdinOpen(ctx, _u: unit) -> obj { Ok(ctx.handle_open(Handle::Stdin)?) }
                fn StdoutOpen(ctx, _u: unit) -> obj { Ok(ctx.handle_open(Handle::Stdout)?) }
                fn StderrOpen(ctx, _u: unit) -> obj { Ok(ctx.handle_open(Handle::Stderr)?) }
                fn FileOutWrite(ctx, text: obj, h: obj) -> obj {
                    Ok(ctx.handle_write(h, Handle::FileOut, text, Write::Text)?)
                }
                fn FileOutWriteBytes(ctx, text: obj, h: obj) -> obj {
                    Ok(ctx.handle_write(h, Handle::FileOut, text, Write::Bytes)?)
                }
                fn FileOutWriteLine(ctx, text: obj, h: obj) -> obj {
                    Ok(ctx.handle_write(h, Handle::FileOut, text, Write::Line)?)
                }
                fn StdoutWrite(ctx, text: obj, h: obj) -> obj {
                    Ok(ctx.handle_write(h, Handle::Stdout, text, Write::Text)?)
                }
                fn StdoutWriteBytes(ctx, text: obj, h: obj) -> obj {
                    Ok(ctx.handle_write(h, Handle::Stdout, text, Write::Bytes)?)
                }
                fn StdoutWriteLine(ctx, text: obj, h: obj) -> obj {
                    Ok(ctx.handle_write(h, Handle::Stdout, text, Write::Line)?)
                }
                fn StderrWrite(ctx, text: obj, h: obj) -> obj {
                    Ok(ctx.handle_write(h, Handle::Stderr, text, Write::Text)?)
                }
                fn StderrWriteBytes(ctx, text: obj, h: obj) -> obj {
                    Ok(ctx.handle_write(h, Handle::Stderr, text, Write::Bytes)?)
                }
                fn StderrWriteLine(ctx, text: obj, h: obj) -> obj {
                    Ok(ctx.handle_write(h, Handle::Stderr, text, Write::Line)?)
                }
                fn ProcWrite(ctx, text: obj, h: obj) -> obj {
                    Ok(ctx.handle_write(h, Handle::Child, text, Write::Text)?)
                }
                fn ProcWriteBytes(ctx, text: obj, h: obj) -> obj {
                    Ok(ctx.handle_write(h, Handle::Child, text, Write::Bytes)?)
                }
                fn ProcWriteLine(ctx, text: obj, h: obj) -> obj {
                    Ok(ctx.handle_write(h, Handle::Child, text, Write::Line)?)
                }
                fn StdoutFlush(ctx, h: obj) -> obj { Ok(ctx.handle_flush(h, Handle::Stdout)?) }
                fn FileOutFlush(ctx, h: obj) -> obj { Ok(ctx.handle_flush(h, Handle::FileOut)?) }
                fn EnvArgCount(ctx, _u: unit) -> int { Ok(ctx.env_arg_count()) }
                fn EnvArg(ctx, i: int) -> obj { Ok(ctx.env_arg(i)?) }
                fn EnvHas(ctx, name: obj) -> enum { Ok(ctx.env_has(name)?) }
                fn EnvLookup(ctx, name: obj) -> obj { Ok(ctx.env_var(name)?) }
                fn ProcSpawn(ctx, stages: obj, stdin: int, payload: obj, stdout: int, path: obj) -> obj {
                    Ok(ctx.proc_spawn(stages, stdin, payload, stdout, path)?)
                }
                fn ProcWait(ctx, p: obj) -> int { Ok(ctx.proc_wait(p)?) }
                fn ProcStageCount(ctx, p: obj) -> int { Ok(ctx.proc_stage_count(p)?) }
                fn ProcWaitStage(ctx, n: int, p: obj) -> int { Ok(ctx.proc_wait_stage(p, n)?) }
                fn ProcCloseStdin(ctx, p: obj) -> obj { Ok(ctx.proc_close_stdin(p)?) }
                // いつも誤りを返すので、結果はない。届け先は`failed`が決める (docs/spec/vm.md の「extern の中断」)
                fn FailIo(ctx, kind: enum, message: obj) -> never {
                    let message = ctx.copy_string(message, usize::MAX)?;
                    Err(RunFault::Extern(Fault::Raised(Box::new(Raised {
                        kind,
                        message: message.into_boxed_str(),
                    }))))
                }
                // channelの即値をそのまま`Cell`の即値にする (docs/spec/vm.md の「命令」)
                fn ChanToCell | AsyncJobCell(_, a: int) -> int { Ok(a) }
            }
            engine { Panic, Exit }
            lir { Par }
            task {
                AsyncStartRaw,
                AsyncStartMainRaw,
                AsyncStartOffMainRaw,
                AsyncRunRaw,
                AsyncPark,
                AsyncParkTime,
                AsyncReady,
                AsyncFinishOk,
                AsyncFinishIo,
                AsyncFinishPanic,
                AsyncTaskCell,
                AsyncTakeResult,
                AsyncRunResult,
                AsyncDetach,
                AsyncCancelRaw,
                AsyncStartInRaw,
                AsyncOpenScope,
                AsyncCloseScope,
                AsyncLeaveScope,
                AsyncCancelScopeRaw,
                AsyncFailScopeIo,
                AsyncFailScopePanic,
                AsyncCancelPoint,
                AsyncSpendBudget,
                AsyncShieldEnter,
                AsyncShieldLeave,
                AsyncClaimScope,
                AsyncParkDrain,
                AsyncCancelMembers,
                AsyncParkSend,
                AsyncParkRecv,
                AsyncParkIo,
                AsyncParkIoUntil,
                AsyncParkSignal,
                AsyncOpReady,
                AsyncForkJob,
                AsyncTakeJob,
                AsyncDropJob,
                ChanNewRaw,
                ChanReserveSend,
                ChanPushRaw,
                ChanReserveRecv,
                ChanPopRaw,
                ChanCloseRaw,
                AtomicNew,
                AtomicAdd,
                AtomicLoad,
                AtomicStore,
                NetSubmitListen,
                NetSubmitAccept,
                NetSubmitConnect,
                NetSubmitRead,
                NetSubmitReadLine,
                NetSubmitWrite,
                NetOpListener,
                NetOpStream,
                NetOpOutcome,
                NetOpDone,
                NetTakeListener,
                NetTakeStream,
                NetTakeBytes,
                NetTakeLine,
                NetFinish,
                NetLocalPort,
                NetPeerAddr,
                NetSubmitShutdownWrite,
                NetSubmitSetNodelay,
                StreamSubmitOpen,
                StreamSubmitRead,
                StreamSubmitReadLine,
                StreamSubmitWrite,
                StreamOpReader,
                StreamOpWriter,
                StreamOpOutcome,
                StreamOpPrefix,
                StreamTakeReader,
                StreamTakeWriter,
                StreamTakeBytes,
                StreamTakeLine,
                StreamTakeLineBytes,
                StreamFinish,
                StreamSource,
                ExecSpawnRaw,
                ExecStdin,
                ExecStdout,
                ExecStderr,
                ExecStageCount,
                ExecSubmitWait,
                ExecOpChild,
                ExecOpOutcome,
                ExecOpPrefix,
                ExecFinish,
                ExecReap,
                TlsSubmitClient,
                TlsSubmitPush,
                TlsTakeConn,
                GuiWantRaw,
                GuiOpenUiRaw,
                GuiPokeRaw,
                GuiAbandonRaw,
                GuiIsGuiTaskRaw,
            }
            pending {
                TlsPushTls,
                TlsPushEof,
                TlsPullTls,
                TlsWritePlain,
                TlsReadPlain,
                TlsStatus,
                TlsErrorMessage,
                TlsSendCloseNotify,
                TlsLineReady,
                TlsTakeLine,
                GuiCloseUi,
                GuiBeginWindowRaw,
                GuiBeginGridRaw,
                GuiEndRaw,
                GuiNextCellRaw,
                GuiCreateRaw,
                GuiAddItemRaw,
                GuiSetTextRaw,
                GuiSetIntRaw,
                GuiCloseWindowRaw,
                GuiOpenWindowsRaw,
                GuiFlushRaw,
                GuiPollRaw,
                GuiEventIntRaw,
                GuiEventTextRaw,
            }
        }
    };
}

/// 行の引数。位置と種類で読む。
pub trait Args {
    fn word(&self, i: usize) -> Word;
    fn int(&self, i: usize) -> i64;
    fn float(&self, i: usize) -> f64;
}

/// 行の結果を種類で分けたもの。`unit`の行はUnitの語を返す。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Word(Word),
    Int(i64),
    Float(f64),
}

/// 行ごとに、一覧の最初の変種の名前の関数を1つ作る。変種ごとに、行の種類が`Extern::<変種>.row()`と等しいことをconstで
/// 確かめる。`call`はワイルドカードの腕を持たないので、一覧にない`Extern`の行があるとコンパイルできず、2度並べた変種は
/// 到達しない形になる。
///
/// 展開は再帰せず、3段で進む。行の引数の組は、まずただのトークンとして取って1つのトークン木で渡し、変種ごとの繰り返しが
/// それぞれの引数を持つところで初めて引数に分ける。変種の繰り返しの中に引数の繰り返しを置くと、2つが歩調を合わせて
/// 進まなければならないためである。
macro_rules! defs {
    (
        rows { $(fn $first:ident $(| $rest:ident)* ( $($sig:tt)* ) -> $ret:tt $body:block)* }
        engine { $($engine:ident),* $(,)? }
        lir { $($lir:ident),* $(,)? }
        task { $($task:ident),* $(,)? }
        pending { $($pending:ident),* $(,)? }
    ) => {
        defs!(@lines [[$($engine)*] [$($lir)*] [$($task)*] [$($pending)*]]
            $($first [$first $($rest)*] ($($sig)*) $ret $body)*);
    };
    (@lines $groups:tt $($first:ident [$($variant:ident)*] $sig:tt $ret:tt $body:block)*) => {
        defs!(@functions $($first $sig $ret $body)*);
        defs!(@checks $($($variant $sig $ret)*)*);
        defs!(@call $groups $([$($variant)*] $first $sig $ret)*);
    };
    (@functions $($first:ident ($ctx:tt $(, $arg:ident : $kind:tt)*) $ret:tt $body:block)*) => {
        $(
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
        )*
    };
    (@checks $($variant:ident ($ctx:tt $(, $arg:ident : $kind:tt)*) $ret:tt)*) => {
        $(
            const _: () = assert!(kinds_match(
                Extern::$variant.row(),
                &[$(repr_of!($kind)),*],
                ret_of!($ret),
            ));
        )*

        /// 共有の本体を持つ行。一覧の`rows`の変種を、一覧の順に並べる。
        pub const SHARED: &[Extern] = &[$(Extern::$variant),*];
    };
    (@call
        [[$($engine:ident)*] [$($lir:ident)*] [$($task:ident)*] [$($pending:ident)*]]
        $([$($variant:ident)*] $first:ident ($ctx:tt $(, $arg:ident : $kind:tt)*) $ret:tt)*
    ) => {
        /// 行`ext`を引数`args`で流す。共有の本体を持たない行は内部の誤りになる。
        ///
        /// # Safety
        /// 引数は行`ext`の種類に合い、その行の関数が求める条件を満たす。
        #[deny(unreachable_patterns)]
        #[allow(unused_mut, unused_variables, unused_assignments)]
        pub unsafe fn call<M: Mode>(
            ext: Extern,
            ctx: &mut Ctx<M>,
            args: &impl Args,
        ) -> Result<Value, RunFault> {
            match ext {
                $(
                    $(Extern::$variant)|* => {
                        let mut at = 0;
                        $(
                            let $arg = read_arg!(args, at, $kind);
                            at += 1;
                        )*
                        // SAFETY: 呼ぶ側の約束
                        let value = unsafe { $first::<M>(ctx $(, $arg)*) }?;
                        value_of!($ret, value)
                    }
                )*
                $(Extern::$engine)|* => Err(RunFault::Internal("Panic and Exit run in the engine")),
                $(Extern::$lir)|* => Err(RunFault::Internal("`Std.Par.par` is not an extern instruction")),
                $(Extern::$task)|* => Err(RunFault::Internal("Async externs run through Op::Async")),
                $(Extern::$pending)|* => Err(RunFault::Internal("the row has no shared body yet")),
            }
        }
    };
}

extern_rows!(defs);

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
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

    fn ctx_on(out: Capture) -> Ctx<Fast> {
        let host = StdHost::new(
            Box::new(out),
            Box::new(io::sink()),
            Default::default(),
            Buffering::Line,
        );
        Ctx::<Fast>::new(
            Arc::new(Control::new(Limits::default())),
            HostRef::new(Arc::new(host)),
        )
    }

    #[test]
    fn println_writes_a_line() {
        let out = Capture::default();
        let mut ctx = ctx_on(out.clone());
        let line = ctx.fresh_string("hello");
        unsafe { Println::<Fast>(&mut ctx, line) }.unwrap();
        assert_eq!(out.0.lock().unwrap().as_slice(), b"hello\n");
    }

    #[test]
    fn shared_lists_each_row_with_a_body_once() {
        let distinct: HashSet<Extern> = SHARED.iter().copied().collect();
        assert_eq!(distinct.len(), SHARED.len());
        for row in SHARED {
            assert!(!row.is_async(), "{row:?}");
            assert!(
                !matches!(row, Extern::Par | Extern::Exit | Extern::Panic),
                "{row:?}"
            );
        }
    }

    /// テストの引数。
    #[derive(Clone, Copy)]
    enum Arg {
        Word(Word),
        Int(i64),
        Float(f64),
    }

    struct Slots<'a>(&'a [Arg]);

    impl Args for Slots<'_> {
        fn word(&self, i: usize) -> Word {
            let Arg::Word(w) = self.0[i] else {
                panic!("argument {i} is not a word")
            };
            w
        }

        fn int(&self, i: usize) -> i64 {
            let Arg::Int(n) = self.0[i] else {
                panic!("argument {i} is not an int")
            };
            n
        }

        fn float(&self, i: usize) -> f64 {
            let Arg::Float(x) = self.0[i] else {
                panic!("argument {i} is not a float")
            };
            x
        }
    }

    #[test]
    fn call_runs_a_row_by_its_variant() {
        let mut ctx = ctx_on(Capture::default());
        // SAFETY: 引数は行の種類に合い、文字列は生きている。`StrConcat`は両辺の参照を受け取る
        unsafe {
            let (a, b) = (ctx.fresh_string("ab"), ctx.fresh_string("cd"));
            let Ok(Value::Word(joined)) = call(
                Extern::StrConcat,
                &mut ctx,
                &Slots(&[Arg::Word(a), Arg::Word(b)]),
            ) else {
                panic!("`StrConcat` gives a word")
            };
            assert_eq!(ctx.copy_string(joined, usize::MAX), Ok("abcd".to_string()));
            assert_eq!(
                call(
                    Extern::IntAdd,
                    &mut ctx,
                    &Slots(&[Arg::Int(1), Arg::Int(2)])
                ),
                Ok(Value::Int(3))
            );
            assert_eq!(
                call(
                    Extern::FloatMod,
                    &mut ctx,
                    &Slots(&[Arg::Float(7.5), Arg::Float(2.0)])
                ),
                Ok(Value::Float(1.5))
            );
            for row in [Extern::Exit, Extern::Par, Extern::AsyncPark] {
                assert!(
                    matches!(call(row, &mut ctx, &Slots(&[])), Err(RunFault::Internal(_))),
                    "{row:?}"
                );
            }
        }
    }

    /// Floatの行は、VMが代わりに呼ぶIntの行と同じビットを読み書きする。
    #[test]
    fn unboxed_float_rows_keep_their_bits() {
        // 正規のNaNではないので、NaNを正規化する変換があればビットが変わる
        const BITS: u64 = 0x7ff8_0000_dead_beef;
        let nan = f64::from_bits(BITS);
        assert!(nan.is_nan() && BITS != f64::NAN.to_bits());
        let mut ctx = ctx_on(Capture::default());
        // SAFETY: 引数は行の種類に合い、配列は生きている。setの行は配列を消費して返し、getの行は借りる
        unsafe {
            let Ok(Value::Word(a)) = call(
                Extern::UnboxedArrayMakeInt,
                &mut ctx,
                &Slots(&[Arg::Int(2), Arg::Int(0)]),
            ) else {
                panic!("`UnboxedArrayMakeInt` gives a word")
            };
            let Ok(Value::Word(a)) = call(
                Extern::UnboxedArraySetFloat,
                &mut ctx,
                &Slots(&[Arg::Word(a), Arg::Int(0), Arg::Float(nan)]),
            ) else {
                panic!("`UnboxedArraySetFloat` gives a word")
            };
            let Ok(Value::Word(a)) = call(
                Extern::UnboxedArraySetInt,
                &mut ctx,
                &Slots(&[Arg::Word(a), Arg::Int(1), Arg::Int(BITS as i64)]),
            ) else {
                panic!("`UnboxedArraySetInt` gives a word")
            };
            for i in 0..2 {
                let Ok(Value::Float(x)) = call(
                    Extern::UnboxedArrayGetFloat,
                    &mut ctx,
                    &Slots(&[Arg::Word(a), Arg::Int(i)]),
                ) else {
                    panic!("`UnboxedArrayGetFloat` gives a float")
                };
                assert_eq!(x.to_bits(), BITS, "element {i}");
                assert_eq!(
                    call(
                        Extern::UnboxedArrayGetInt,
                        &mut ctx,
                        &Slots(&[Arg::Word(a), Arg::Int(i)])
                    ),
                    Ok(Value::Int(BITS as i64)),
                    "element {i}"
                );
            }
        }
    }
}
