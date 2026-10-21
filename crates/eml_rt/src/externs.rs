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
    /// `Int` に収まらない `Float` を `Int` に変えようとした。値はビット列で持ち、`Eq` を保つ。
    FloatToInt(u64),
    /// `Float.to_fixed` の桁数が 0 から 100 の外にある。
    FixedDigits(i64),
    /// `Char.of_int` の引数が Unicode のスカラー値でない。
    NotScalar(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoOp {
    Open,
    Read,
    Write,
    /// 子のプロセスの起動。
    Run,
}

/// I/O の誤りの理由。文字列でなく値で分けるのは、`try_io` が eml の `IOError` の値に変えるためである。
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
    /// 配列の添字 (`Array.get`、`Array.set`、`MutArray.read`、`MutArray.write`)。
    Array { index: i64, len: i64 },
    /// `make` に渡した負の長さ。
    Length { len: i64 },
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

/// Unicode のスカラー値: サロゲート (0xD800 から 0xDFFF) を除く 0 から 0x10FFFF (docs/spec/lir.md の「値の表現」)。
pub open spec fn is_scalar(n: int) -> bool {
    (0 <= n <= 0xD7FF) || (0xE000 <= n <= 0x10FFFF)
}

// 範囲を比較で書くのは、vstd の `Range::contains` の約束を信頼せずに済むようにするためである
#[allow(clippy::manual_range_contains)]
#[inline]
pub fn char_of_int(n: i64) -> (r: Result<i64, ArithFault>)
    ensures
        r is Ok <==> is_scalar(n as int),
        r matches Ok(v) ==> v == n,
        r is Err ==> r == Err::<i64, ArithFault>(ArithFault::NotScalar(n)),
{
    if (0 <= n && n <= 0xD7FF) || (0xE000 <= n && n <= 0x10FFFF) {
        Ok(n)
    } else {
        Err(ArithFault::NotScalar(n))
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

/// `var` の配列の変数の添字が範囲の中か。範囲の外なら、`MutArray.read` と同じ誤りを返す
/// (docs/spec/object-model.md の「extern」)。
#[inline]
pub fn check_index(index: i64, len: i64) -> (r: Result<(), Fault>)
    ensures
        r is Ok <==> 0 <= index < len,
        r is Err ==> (r matches Err(Fault::Index(fault)) && *fault == (IndexFault::Array {
            index,
            len,
        })),
{
    if 0 <= index && index < len {
        Ok(())
    } else {
        Err(Fault::Index(Box::new(IndexFault::Array { index, len })))
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

// `Float` の算術と `compare`。Verus は `f64` の算術に前提を満たせない仕様しか持たず、`%` と `total_cmp` には仕様が
// ないので、本体を確かめずに信頼する (docs/spec/object-model.md の「`unsafe` の核」の T12)。値は IEEE 754 の倍精度の
// とおりで、どれも誤りを返さない。結果は期待値の表で確かめる。NaN の結果は、どれも正の quiet NaN (`f64::NAN`) に
// そろえる。Rust は演算の結果の NaN の符号を決めず、x86_64 では符号の立った NaN になるので、そろえないと `compare`
// の答えがプラットフォームで変わるためである (docs/spec/declarations.md の「Prelude のクラス」)
#[verifier::external_body]
#[inline]
pub fn float_add(a: f64, b: f64) -> f64 {
    canonical_nan(a + b)
}

#[verifier::external_body]
#[inline]
pub fn float_sub(a: f64, b: f64) -> f64 {
    canonical_nan(a - b)
}

#[verifier::external_body]
#[inline]
pub fn float_mul(a: f64, b: f64) -> f64 {
    canonical_nan(a * b)
}

#[verifier::external_body]
#[inline]
pub fn float_div(a: f64, b: f64) -> f64 {
    canonical_nan(a / b)
}

/// C の `fmod` と同じく、結果は割られる数の符号を持つ (Rust の `%`)。
#[verifier::external_body]
#[inline]
pub fn float_rem(a: f64, b: f64) -> f64 {
    canonical_nan(a % b)
}

/// NaN の符号も反転せず、正の NaN を返す。
#[verifier::external_body]
#[inline]
pub fn float_negate(a: f64) -> f64 {
    canonical_nan(-a)
}

/// NaN を正の quiet NaN (`f64::NAN`) にそろえ、ほかの値はそのまま返す。
#[verifier::external_body]
#[inline]
pub fn canonical_nan(x: f64) -> f64 {
    if x.is_nan() {
        f64::NAN
    } else {
        x
    }
}

/// 全順序 (`f64::total_cmp`)。`-0.0` は `0.0` より小さく、正の NaN はどの値より大きい。演算が作る NaN はどれも正の
/// NaN にそろえてあるので (上の `canonical_nan`)、NaN は `inf` より大きい
/// (docs/spec/declarations.md の「Prelude のクラス」)。
#[verifier::external_body]
#[inline]
pub fn float_compare(a: f64, b: f64) -> (r: i64)
    ensures
        r == LT || r == EQ || r == GT,
{
    match a.total_cmp(&b) {
        std::cmp::Ordering::Less => LT,
        std::cmp::Ordering::Equal => EQ,
        std::cmp::Ordering::Greater => GT,
    }
}

/// `Float.of_int`。最も近い値に丸め、ちょうど中間なら偶数の側を選ぶ (Rust の `as`)。
#[verifier::external_body]
#[inline]
pub fn float_of_int(i: i64) -> f64 {
    i as f64
}

/// 0 の側へ切り捨てる。結果が `Int` に収まらなければ (NaN、±無限大も)、元の値を持つ誤りを返す。
#[verifier::external_body]
#[inline]
pub fn float_truncate(x: f64) -> Result<i64, ArithFault> {
    float_to_int(x.trunc(), x)
}

#[verifier::external_body]
#[inline]
pub fn float_floor(x: f64) -> Result<i64, ArithFault> {
    float_to_int(x.floor(), x)
}

#[verifier::external_body]
#[inline]
pub fn float_ceil(x: f64) -> Result<i64, ArithFault> {
    float_to_int(x.ceil(), x)
}

/// ちょうど中間なら偶数の側を選ぶ (Python の `round` と同じ)。
#[verifier::external_body]
#[inline]
pub fn float_round(x: f64) -> Result<i64, ArithFault> {
    float_to_int(x.round_ties_even(), x)
}

/// 整数の値 `t` を `Int` にする。`Int` の範囲は `[-2^63, 2^63)` で、両端は `f64` でちょうど表せる。NaN はどちらの
/// 比較も偽なので誤りになる。
#[verifier::external_body]
#[inline]
fn float_to_int(t: f64, x: f64) -> Result<i64, ArithFault> {
    if (-9223372036854775808.0..9223372036854775808.0).contains(&t) {
        Ok(t as i64)
    } else {
        Err(ArithFault::FloatToInt(x.to_bits()))
    }
}

// 数学の関数。値は Rust の `f64` のとおりで、定義域の外は NaN である。NaN は算術と同じく正の quiet NaN にそろえる
#[verifier::external_body]
#[inline]
pub fn float_sqrt(x: f64) -> f64 {
    canonical_nan(x.sqrt())
}

#[verifier::external_body]
#[inline]
pub fn float_abs(x: f64) -> f64 {
    canonical_nan(x.abs())
}

#[verifier::external_body]
#[inline]
pub fn float_sin(x: f64) -> f64 {
    canonical_nan(x.sin())
}

#[verifier::external_body]
#[inline]
pub fn float_cos(x: f64) -> f64 {
    canonical_nan(x.cos())
}

#[verifier::external_body]
#[inline]
pub fn float_exp(x: f64) -> f64 {
    canonical_nan(x.exp())
}

/// 自然対数。
#[verifier::external_body]
#[inline]
pub fn float_log(x: f64) -> f64 {
    canonical_nan(x.ln())
}

/// `x` の `y` 乗。
#[verifier::external_body]
#[inline]
pub fn float_pow(x: f64, y: f64) -> f64 {
    canonical_nan(x.powf(y))
}

#[verifier::external_body]
#[inline]
pub fn float_infinity() -> f64 {
    f64::INFINITY
}

#[verifier::external_body]
#[inline]
pub fn float_nan() -> f64 {
    f64::NAN
}

/// `show x` が `-` で始まるか。符号の立った値 (`-0.0` と `-inf` も) なら真で、NaN は `nan` と書くので偽である。
#[verifier::external_body]
#[inline]
pub fn float_shows_minus(x: f64) -> (r: i64)
    ensures
        r == FALSE || r == TRUE,
{
    if x.is_sign_negative() && !x.is_nan() {
        TRUE
    } else {
        FALSE
    }
}

/// `Show Float` の文字列。Python の `repr` と同じ形である (docs/spec/declarations.md の「Prelude のクラス」)。往復
/// できる最短の桁数を `{:e}` で求め、その桁数で正しく丸めた桁を `{:.*e}` で求める。`{:e}` は最短の桁の候補が2つ
/// あると中間から上を選ぶが、Python は値に近い方 (ちょうど中間なら偶数の側) を選ぶためである。正しく丸めた桁が
/// 読み戻せないとき (2 の冪の一部) は、`{:e}` の桁を使う。10進の指数が -4 以上 16 未満なら固定小数点で、ほかは
/// 指数の形で書く。
#[verifier::external_body]
pub fn float_repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 {
            "inf"
        } else {
            "-inf"
        }.to_string();
    }
    let shortest = format!("{x:e}");
    let mantissa_len = shortest.find('e').expect("`{:e}` writes an exponent");
    let digit_count = shortest[..mantissa_len].bytes().filter(|b| b.is_ascii_digit()).count();
    let rounded = format!("{x:.prec$e}", prec = digit_count - 1);
    // 2 の冪では、往復できる区間が下側だけ狭い。正しく丸めた桁がその外に出たら読み戻せないので、`{:e}` の桁を使う。
    // 2つの桁が同じなら `{:e}` が往復を保証しているので、読み戻して確かめるのは違うときだけにする
    let sci = if rounded == shortest || rounded.parse::<f64>() == Ok(x) {
        rounded
    } else {
        shortest
    };
    let (mantissa, exp) = sci.split_once('e').expect("`{:e}` writes an exponent");
    let exp: i32 = exp.parse().expect("`{:e}` writes a decimal exponent");
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(m) => ("-", m),
        None => ("", mantissa),
    };
    let digits: String = mantissa.replace('.', "");
    let mut out = String::from(sign);
    if (-4..16).contains(&exp) {
        if exp >= 0 {
            let int_len = exp as usize + 1;
            if digits.len() <= int_len {
                out.push_str(&digits);
                out.extend(std::iter::repeat_n('0', int_len - digits.len()));
                out.push_str(".0");
            } else {
                out.push_str(&digits[..int_len]);
                out.push('.');
                out.push_str(&digits[int_len..]);
            }
        } else {
            out.push_str("0.");
            out.extend(std::iter::repeat_n('0', (-exp - 1) as usize));
            out.push_str(&digits);
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push_str(&format!("e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.unsigned_abs()));
    }
    out
}

/// 小数点以下 `digits` 桁の文字列。Python の `f"{x:.{digits}f}"` と同じく、10進の値を正しく丸める (ちょうど中間なら
/// 偶数の側)。Rust は NaN を `NaN` と書くので、Python と同じ `nan` にする。
#[verifier::external_body]
pub fn float_fixed(digits: usize, x: f64) -> String {
    if x.is_nan() {
        "nan".to_string()
    } else {
        format!("{x:.digits$}")
    }
}

/// `Float.parse` の読み。前後の Unicode の空白を除き、Rust の `f64::from_str` が受ける形 (符号、10進と指数の形、大文字
/// 小文字を問わない `inf`、`infinity`、`nan`) を読む。`_` は受けない。NaN は正の quiet NaN にそろえる。
#[verifier::external_body]
pub fn float_parse(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().map(canonical_nan)
}

// 比較の演算子は IEEE 754 のとおりで、NaN が絡むと `!=` のほかは偽である。vstd が比較の仕様を持つので、
// `verus!` の中で確かめる。値は約束せず、結果が `Bool` のタグであることだけを約束する
#[inline]
pub fn float_eq(a: f64, b: f64) -> (r: i64)
    ensures
        r == FALSE || r == TRUE,
{
    if a == b {
        TRUE
    } else {
        FALSE
    }
}

#[inline]
pub fn float_ne(a: f64, b: f64) -> (r: i64)
    ensures
        r == FALSE || r == TRUE,
{
    if a != b {
        TRUE
    } else {
        FALSE
    }
}

#[inline]
pub fn float_lt(a: f64, b: f64) -> (r: i64)
    ensures
        r == FALSE || r == TRUE,
{
    if a < b {
        TRUE
    } else {
        FALSE
    }
}

#[inline]
pub fn float_le(a: f64, b: f64) -> (r: i64)
    ensures
        r == FALSE || r == TRUE,
{
    if a <= b {
        TRUE
    } else {
        FALSE
    }
}

#[inline]
pub fn float_gt(a: f64, b: f64) -> (r: i64)
    ensures
        r == FALSE || r == TRUE,
{
    if a > b {
        TRUE
    } else {
        FALSE
    }
}

#[inline]
pub fn float_ge(a: f64, b: f64) -> (r: i64)
    ensures
        r == FALSE || r == TRUE,
{
    if a >= b {
        TRUE
    } else {
        FALSE
    }
}

} // verus!
// 文言は docs/spec/lir.md の「実行時エラー」の一覧と1文字も違わないようにする
impl fmt::Display for ArithFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArithFault::DivisionByZero => f.write_str("division by zero"),
            ArithFault::IntegerOverflow => f.write_str("integer overflow"),
            ArithFault::FloatToInt(bits) => write!(
                f,
                "float `{}` does not fit in `Int`",
                float_repr(f64::from_bits(*bits))
            ),
            ArithFault::FixedDigits(n) => write!(f, "`to_fixed` takes 0 to 100 digits, not {n}"),
            ArithFault::NotScalar(n) => write!(f, "int `{n}` is not a Unicode scalar value"),
        }
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
                IndexFault::Array { index, len } => {
                    write!(f, "array index {index} is out of range for length {len}")
                }
                IndexFault::Length { len } => write!(f, "array length {len} is negative"),
            },
            Fault::Stopped(s) => write!(f, "{s}"),
        }
    }
}
