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
}

/// I/O の誤りの理由。文字列でなく値で分けるのは、S12 の `try_io` が eml の `IOError` の値に変えるためである。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoReason {
    NotFound,
    PermissionDenied,
    Other,
}

/// extern の誤り。位置は VM が付けるので持たない。
#[derive(Debug, PartialEq, Eq)]
pub enum Fault {
    Heap(HeapError),
    Arith(ArithFault),
    /// 書き込みの誤りの `io::Error` の文言。
    Output(Box<str>),
    /// `path` は `open` に渡されたままのパスである。
    Io { op: IoOp, path: Box<str>, reason: IoReason },
    NotUtf8 { path: Box<str> },
    /// `read_all` が読む途中で止める語か確保量の上限に当たった。
    Stopped(Stop),
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
// 文言は今の `eml_interp::Fault` と1文字も違わないようにする。S10 で VM に切り替えても UI テストの出力が変わらない
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
            Fault::Io {
                op: IoOp::Open,
                path,
                reason,
            } => {
                write!(f, "cannot open `{path}`: {reason}")
            }
            Fault::Io {
                op: IoOp::Read,
                path,
                reason,
            } => {
                write!(f, "cannot read `{path}`: {reason}")
            }
            Fault::NotUtf8 { path } => write!(f, "`{path}` is not valid UTF-8"),
            Fault::Stopped(s) => write!(f, "{s}"),
        }
    }
}
