//! extern の実装 (docs/spec/object-model.md の「extern」)。引数と結果は語の ABI に従い、`int` と `enum` のタグは
//! 生の `i64` で渡す。ヒープを使わない算術と比較は `Ctx` を取らない `#[inline]` の関数にし、ほかの crate の命令からも
//! 展開できるようにする。

use std::fmt;

use crate::control::Stop;
use crate::error::HeapError;
#[cfg(verus_only)]
use vstd::arithmetic::div_mod::{rust_div, rust_rem};
use vstd::prelude::*;
#[cfg(verus_only)]
use vstd::std_specs::convert::FromSpecImpl;

verus! {

// `Bool` と `Ordering` のタグ。VM のレジスタは生の 64 ビットの語なので、`i64` で持つと VM の側で型を変えずに済む
pub const FALSE: i64 = 0;

pub const TRUE: i64 = 1;

pub const LT: i64 = 0;

pub const EQ: i64 = 1;

pub const GT: i64 = 2;

/// 組のタグ。`alloc_data` のタグの型に合わせる。
pub const TUPLE: u16 = 0;

/// `IOErrorKind` のタグ。Prelude の宣言の順で、`try_io` の `io_failed` の節が受け取る
/// (docs/spec/stdlib.md の「`try_io`」)。
pub const IO_NOT_FOUND: i64 = 0;

pub const IO_PERMISSION_DENIED: i64 = 1;

pub const IO_NOT_AVAILABLE: i64 = 2;

pub const IO_NOT_UTF8: i64 = 3;

pub const IO_OTHER: i64 = 4;

/// 算術の誤り。`Fault` と分けておくと、`Result<i64, ArithFault>` が2つのレジスタで返る。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithFault {
    DivisionByZero,
    IntegerOverflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoOp {
    Open,
    Read,
    Write,
    /// 子のプロセスの起動。
    Run,
}

/// I/O の誤りの理由。文字列でなく値で分けるのは、S12 の `try_io` が eml の `IOError` の値に変えるためである。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoReason {
    NotFound,
    PermissionDenied,
    /// その実行で使えない入出力 (REPL の標準入力)。
    NotAvailable,
    Other,
}

/// UTF-8 でなかった入力。
#[derive(Debug, PartialEq, Eq)]
pub enum Utf8Source {
    /// `open` に渡されたままのパス。
    File(Box<str>),
    Stdin,
    /// `Env.args ()` のリストの 0 から数えた位置。
    Arg(i64),
    EnvVar(Box<str>),
}

/// 範囲の外の位置。
#[derive(Debug, PartialEq, Eq)]
pub enum IndexFault {
    /// 文字列のバイトの位置 (`byte_at`、`find_from`)。
    String { index: i64, len: i64 },
    /// `slice` の範囲が文字列の外にある。
    Slice { start: i64, end: i64, len: i64 },
    /// `slice` の範囲の端が文字の境界にない。
    Boundary { start: i64, end: i64 },
    /// `Env.arg` の位置。
    Arg { index: i64, count: i64 },
}

/// extern の誤り。位置は VM が付けるので持たない。24 バイトに収めるため (`faults_stay_small`)、まれな誤りの中身は
/// `Box` に入れる。
#[derive(Debug, PartialEq, Eq)]
pub enum Fault {
    Heap(HeapError),
    Arith(ArithFault),
    /// 書き込みの誤りの `io::Error` の文言。
    Output(Box<str>),
    /// `name` は extern に渡されたままのパスか、`Run` ならプログラムの名前である。
    Io { op: IoOp, name: Box<str>, reason: IoReason },
    /// 標準入力の読みの誤り。
    Stdin(IoReason),
    NotUtf8(Box<Utf8Source>),
    Index(Box<IndexFault>),
    /// 読む途中か子を待つ間に、止める語か確保量の上限に当たった。
    Stopped(Stop),
}

impl Fault {
    /// `try_io` が捕まえる誤りなら、`IOErrorKind` のタグを返す。書き出しの誤り (`Output`) は、標準出力をまとめて
    /// 書くので見つかる場所が決まらず、捕まえない (docs/spec/stdlib.md の「`try_io`」)。
    pub fn io_error_kind(&self) -> Option<i64> {
        match self {
            Fault::Io { op: _, name: _, reason } => Some(reason_kind(*reason)),
            Fault::Stdin(reason) => Some(reason_kind(*reason)),
            Fault::NotUtf8(_) => Some(IO_NOT_UTF8),
            Fault::Heap(_)
            | Fault::Arith(_)
            | Fault::Output(_)
            | Fault::Index(_)
            | Fault::Stopped(_) => None,
        }
    }

    /// `try_panic` が捕まえる誤りか。値と入力で決まる誤り (算術と範囲の外の位置) だけを `Panic` にする。ヒープの誤り、
    /// 書き出しの誤り、止めたことは、どの呼び出しでも起きうるか起きた位置が決まらないので、型に出さない停止のままにする
    /// (docs/spec/stdlib.md の「`try_panic`」)。
    pub fn panics(&self) -> bool {
        match self {
            Fault::Arith(_) | Fault::Index(_) => true,
            Fault::Heap(_)
            | Fault::Output(_)
            | Fault::Io { op: _, name: _, reason: _ }
            | Fault::Stdin(_)
            | Fault::NotUtf8(_)
            | Fault::Stopped(_) => false,
        }
    }
}

fn reason_kind(reason: IoReason) -> i64 {
    match reason {
        IoReason::NotFound => IO_NOT_FOUND,
        IoReason::PermissionDenied => IO_PERMISSION_DENIED,
        IoReason::NotAvailable => IO_NOT_AVAILABLE,
        IoReason::Other => IO_OTHER,
    }
}

// `?` で `Fault` に変えた後も、どの誤りだったかを呼ぶ側の証明が知れるようにする
#[cfg(verus_only)]
impl FromSpecImpl<ArithFault> for Fault {
    open spec fn obeys_from_spec() -> bool {
        true
    }

    open spec fn from_spec(fault: ArithFault) -> Fault {
        Fault::Arith(fault)
    }
}

#[cfg(verus_only)]
impl FromSpecImpl<HeapError> for Fault {
    open spec fn obeys_from_spec() -> bool {
        true
    }

    open spec fn from_spec(error: HeapError) -> Fault {
        Fault::Heap(error)
    }
}

impl From<ArithFault> for Fault {
    fn from(fault: ArithFault) -> Fault {
        Fault::Arith(fault)
    }
}

impl From<HeapError> for Fault {
    fn from(error: HeapError) -> Fault {
        Fault::Heap(error)
    }
}

#[inline]
pub fn int_add(a: i64, b: i64) -> (r: Result<i64, ArithFault>)
    ensures
        r is Ok <==> i64::MIN <= a + b <= i64::MAX,
        r matches Ok(v) ==> v == a + b,
        r is Err ==> r == Err::<i64, ArithFault>(ArithFault::IntegerOverflow),
{
    match a.checked_add(b) {
        Some(v) => Ok(v),
        None => Err(ArithFault::IntegerOverflow),
    }
}

#[inline]
pub fn int_sub(a: i64, b: i64) -> (r: Result<i64, ArithFault>)
    ensures
        r is Ok <==> i64::MIN <= a - b <= i64::MAX,
        r matches Ok(v) ==> v == a - b,
        r is Err ==> r == Err::<i64, ArithFault>(ArithFault::IntegerOverflow),
{
    match a.checked_sub(b) {
        Some(v) => Ok(v),
        None => Err(ArithFault::IntegerOverflow),
    }
}

#[inline]
pub fn int_mul(a: i64, b: i64) -> (r: Result<i64, ArithFault>)
    ensures
        r is Ok <==> i64::MIN <= a * b <= i64::MAX,
        r matches Ok(v) ==> v == a * b,
        r is Err ==> r == Err::<i64, ArithFault>(ArithFault::IntegerOverflow),
{
    match a.checked_mul(b) {
        Some(v) => Ok(v),
        None => Err(ArithFault::IntegerOverflow),
    }
}

/// 商は 0 の方へ丸める (Rust の `/` と同じ)。
#[inline]
pub fn int_div(a: i64, b: i64) -> (r: Result<i64, ArithFault>)
    ensures
        b == 0 ==> r == Err::<i64, ArithFault>(ArithFault::DivisionByZero),
        b == -1 && a == i64::MIN ==> r == Err::<i64, ArithFault>(ArithFault::IntegerOverflow),
        b != 0 && !(a == i64::MIN && b == -1) ==> r == Ok::<i64, ArithFault>(
            rust_div(a as int, b as int) as i64,
        ),
{
    if b == 0 {
        return Err(ArithFault::DivisionByZero);
    }
    match a.checked_div(b) {
        Some(v) => Ok(v),
        None => Err(ArithFault::IntegerOverflow),
    }
}

/// 余りの符号は `a` に合わせる (Rust の `%` と同じ)。
#[inline]
pub fn int_rem(a: i64, b: i64) -> (r: Result<i64, ArithFault>)
    ensures
        b == 0 ==> r == Err::<i64, ArithFault>(ArithFault::DivisionByZero),
        b == -1 && a == i64::MIN ==> r == Err::<i64, ArithFault>(ArithFault::IntegerOverflow),
        b != 0 && !(a == i64::MIN && b == -1) ==> r == Ok::<i64, ArithFault>(
            rust_rem(a as int, b as int) as i64,
        ),
{
    if b == 0 {
        return Err(ArithFault::DivisionByZero);
    }
    match a.checked_rem(b) {
        Some(v) => Ok(v),
        None => Err(ArithFault::IntegerOverflow),
    }
}

#[inline]
pub fn int_negate(a: i64) -> (r: Result<i64, ArithFault>)
    ensures
        r is Ok <==> a != i64::MIN,
        r matches Ok(v) ==> v == -a,
        r is Err ==> r == Err::<i64, ArithFault>(ArithFault::IntegerOverflow),
{
    // vstd には `checked_neg` の spec がない
    match 0i64.checked_sub(a) {
        Some(v) => Ok(v),
        None => Err(ArithFault::IntegerOverflow),
    }
}

#[inline]
pub fn int_eq(a: i64, b: i64) -> (r: i64)
    ensures
        r == if a == b {
            TRUE
        } else {
            FALSE
        },
{
    if a == b {
        TRUE
    } else {
        FALSE
    }
}

#[inline]
pub fn int_ne(a: i64, b: i64) -> (r: i64)
    ensures
        r == if a != b {
            TRUE
        } else {
            FALSE
        },
{
    if a != b {
        TRUE
    } else {
        FALSE
    }
}

#[inline]
pub fn int_lt(a: i64, b: i64) -> (r: i64)
    ensures
        r == if a < b {
            TRUE
        } else {
            FALSE
        },
{
    if a < b {
        TRUE
    } else {
        FALSE
    }
}

#[inline]
pub fn int_le(a: i64, b: i64) -> (r: i64)
    ensures
        r == if a <= b {
            TRUE
        } else {
            FALSE
        },
{
    if a <= b {
        TRUE
    } else {
        FALSE
    }
}

#[inline]
pub fn int_gt(a: i64, b: i64) -> (r: i64)
    ensures
        r == if a > b {
            TRUE
        } else {
            FALSE
        },
{
    if a > b {
        TRUE
    } else {
        FALSE
    }
}

#[inline]
pub fn int_ge(a: i64, b: i64) -> (r: i64)
    ensures
        r == if a >= b {
            TRUE
        } else {
            FALSE
        },
{
    if a >= b {
        TRUE
    } else {
        FALSE
    }
}

#[inline]
pub fn int_compare(a: i64, b: i64) -> (r: i64)
    ensures
        r == if a < b {
            LT
        } else if a == b {
            EQ
        } else {
            GT
        },
{
    if a < b {
        LT
    } else if a == b {
        EQ
    } else {
        GT
    }
}

/// タグだけを比べるので、引数が `FALSE` か `TRUE` であることを前提にしない。
#[inline]
pub fn bool_eq(a: i64, b: i64) -> (r: i64)
    ensures
        r == if a == b {
            TRUE
        } else {
            FALSE
        },
{
    if a == b {
        TRUE
    } else {
        FALSE
    }
}

#[inline]
pub fn bool_ne(a: i64, b: i64) -> (r: i64)
    ensures
        r == if a != b {
            TRUE
        } else {
            FALSE
        },
{
    if a != b {
        TRUE
    } else {
        FALSE
    }
}

} // verus!
// 文言は docs/spec/lir.md の「実行時エラー」の一覧と1文字も違わないようにする
impl fmt::Display for ArithFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ArithFault::DivisionByZero => "division by zero",
            ArithFault::IntegerOverflow => "integer overflow",
        })
    }
}

impl fmt::Display for IoReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            IoReason::NotFound => "not found",
            IoReason::PermissionDenied => "permission denied",
            IoReason::NotAvailable => "not available",
            IoReason::Other => "I/O error",
        })
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fault::Heap(e) => write!(f, "{e}"),
            Fault::Arith(e) => write!(f, "{e}"),
            Fault::Output(e) => write!(f, "cannot write the output: {e}"),
            Fault::Io { op, name, reason } => {
                let verb = match op {
                    IoOp::Open => "open",
                    IoOp::Read => "read",
                    IoOp::Write => "write",
                    IoOp::Run => "run",
                };
                write!(f, "cannot {verb} `{name}`: {reason}")
            }
            Fault::Stdin(reason) => write!(f, "cannot read the standard input: {reason}"),
            Fault::NotUtf8(source) => match &**source {
                Utf8Source::File(path) => write!(f, "`{path}` is not valid UTF-8"),
                Utf8Source::Stdin => f.write_str("the standard input is not valid UTF-8"),
                Utf8Source::Arg(index) => write!(f, "argument {index} is not valid UTF-8"),
                Utf8Source::EnvVar(name) => {
                    write!(f, "environment variable `{name}` is not valid UTF-8")
                }
            },
            Fault::Index(fault) => match **fault {
                IndexFault::String { index, len } => {
                    write!(f, "string index {index} is out of range for length {len}")
                }
                IndexFault::Slice { start, end, len } => {
                    write!(
                        f,
                        "string slice {start}..{end} is out of range for length {len}"
                    )
                }
                IndexFault::Boundary { start, end } => {
                    write!(
                        f,
                        "string slice {start}..{end} is not on a character boundary"
                    )
                }
                IndexFault::Arg { index, count } => {
                    write!(
                        f,
                        "argument index {index} is out of range for {count} arguments"
                    )
                }
            },
            Fault::Stopped(s) => write!(f, "{s}"),
        }
    }
}
