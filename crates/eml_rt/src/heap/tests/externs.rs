//! extern の単体テスト。結果と誤りの文言が spec のとおりであることを確かめる。

use std::cmp::Ordering as Order;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::ctx::{ctx, ctx_with};
use super::host::{MemoryFile, MemoryHost, Shared, host};
use super::mortal;
use super::string::text;
use super::table::{self, Arg, Case, EDGES, Outcome, Scratch};
use crate::externs::*;
use crate::heap::ctx::positions::{find_byte, find_bytes};
use crate::{Buffering, Control, HeapError, HostRef, Limits, Mode, StdHost, Stop, Word};

fn ordering(o: Order) -> i64 {
    match o {
        Order::Less => LT,
        Order::Equal => EQ,
        Order::Greater => GT,
    }
}

#[test]
fn io_faults_map_to_their_kinds() {
    use crate::externs::{
        ArithFault, Fault, IO_NOT_AVAILABLE, IO_NOT_FOUND, IO_NOT_UTF8, IO_OTHER,
        IO_PERMISSION_DENIED, IoOp, IoReason, Utf8Source,
    };
    let io = |reason| Fault::Io {
        op: IoOp::Open,
        name: "x".into(),
        reason,
    };
    assert_eq!(io(IoReason::NotFound).io_error_kind(), Some(IO_NOT_FOUND));
    assert_eq!(
        io(IoReason::PermissionDenied).io_error_kind(),
        Some(IO_PERMISSION_DENIED)
    );
    assert_eq!(
        io(IoReason::NotAvailable).io_error_kind(),
        Some(IO_NOT_AVAILABLE)
    );
    assert_eq!(io(IoReason::Other).io_error_kind(), Some(IO_OTHER));
    assert_eq!(
        Fault::Stdin(IoReason::NotAvailable).io_error_kind(),
        Some(IO_NOT_AVAILABLE)
    );
    assert_eq!(
        Fault::NotUtf8(Box::new(Utf8Source::Stdin)).io_error_kind(),
        Some(IO_NOT_UTF8)
    );
    assert_eq!(Fault::Output("broken pipe".into()).io_error_kind(), None);
    assert_eq!(
        Fault::Arith(ArithFault::DivisionByZero).io_error_kind(),
        None
    );
}

/// `String.decode` の誤りは値で決まるので `try_panic` が捕まえ、IO の失敗とは重ならない。
#[test]
fn invalid_utf8_faults_panic() {
    let fault = Fault::Utf8 { at: 0 };
    assert!(fault.panics());
    assert_eq!(fault.io_error_kind(), None);
}

/// `try_panic` に渡すのは算術と範囲の外の位置の誤りと、標準のストリームの2つ目の `open` だけで、IO の失敗とは
/// 重ならない。
#[test]
fn only_value_faults_and_second_opens_panic() {
    let index = |fault| Fault::Index(Box::new(fault));
    let panicking = [
        Fault::Arith(ArithFault::DivisionByZero),
        Fault::Arith(ArithFault::IntegerOverflow),
        index(IndexFault::String { index: 3, len: 1 }),
        index(IndexFault::Slice {
            start: 0,
            end: 3,
            len: 1,
        }),
        index(IndexFault::Boundary { start: 0, end: 1 }),
        index(IndexFault::Arg { index: 5, count: 0 }),
        index(IndexFault::Array { index: 3, len: 3 }),
        index(IndexFault::Length { len: -1 }),
        index(IndexFault::BytesIndex { index: 3, len: 3 }),
        index(IndexFault::BytesSlice {
            start: 0,
            end: 3,
            len: 1,
        }),
        index(IndexFault::BytesLength { len: -1 }),
        Fault::InUse(StdStream::Stdin),
        Fault::InUse(StdStream::Stdout),
        Fault::InUse(StdStream::Stderr),
    ];
    for fault in &panicking {
        assert!(fault.panics(), "{fault:?}");
        assert_eq!(fault.io_error_kind(), None, "{fault:?}");
    }
    let others = [
        Fault::Heap(HeapError::WrongKind),
        Fault::Output("broken pipe".into()),
        Fault::Io {
            op: IoOp::Open,
            name: "x".into(),
            reason: IoReason::NotFound,
        },
        Fault::Stdin(IoReason::Other),
        Fault::NotUtf8(Box::new(Utf8Source::Stdin)),
        Fault::Stopped(Stop::Interrupted),
    ];
    for fault in &others {
        assert!(!fault.panics(), "{fault:?}");
    }
}

/// 範囲の中なら `()`、外なら `MutArray.read` と同じ文言の誤りである。長さ 0 の配列ではどの添字も範囲の外になる。
#[test]
fn check_index_accepts_only_positions_inside_the_length() {
    for (index, len) in [(0, 1), (2, 3), (0, i64::MAX)] {
        assert_eq!(check_index(index, len), Ok(()), "{index} {len}");
    }
    for (index, len) in [(3, 3), (-1, 3), (0, 0), (i64::MIN, 0), (i64::MAX, 2)] {
        let fault = check_index(index, len).unwrap_err();
        assert_eq!(
            fault,
            Fault::Index(Box::new(IndexFault::Array { index, len }))
        );
        assert!(fault.panics());
    }
    assert_eq!(
        check_index(0, 0).unwrap_err().to_string(),
        "array index 0 is out of range for length 0"
    );
}

/// 幅 `w` の語のシフトの素直な版。`i128` で計算して `w` ビットに切る。量が負なら逆向きで、符号を反転しない
fn naive_shift(x: i64, n: i64, width: u32, left: bool) -> i64 {
    let (left, amount) = if n < 0 {
        (!left, -(n as i128))
    } else {
        (left, n as i128)
    };
    let x = x as i128;
    let shifted = if amount >= width as i128 {
        if left || x >= 0 { 0 } else { -1 }
    } else if left {
        x << amount
    } else {
        x >> amount
    };
    if width == 64 {
        shifted as i64
    } else {
        (shifted & 0xff) as i64
    }
}

#[test]
fn bits_match_plain_rust() {
    for a in table::BITS_INT {
        assert_eq!(int_complement(a), !a);
        for b in table::BITS_INT {
            assert_eq!(int_and(a, b), a & b);
            assert_eq!(int_or(a, b), a | b);
            assert_eq!(int_xor(a, b), a ^ b);
        }
        for n in table::SHIFT_AMOUNTS {
            assert_eq!(int_shl(a, n), naive_shift(a, n, 64, true), "{a} <<< {n}");
            assert_eq!(int_shr(a, n), naive_shift(a, n, 64, false), "{a} >>> {n}");
        }
    }
    for a in table::BITS_BYTE {
        assert_eq!(byte_complement(a), 255 - a);
        for b in table::BITS_BYTE {
            for r in [int_and(a, b), int_or(a, b), int_xor(a, b)] {
                assert!((0..=255).contains(&r));
            }
        }
        for n in table::SHIFT_AMOUNTS {
            assert_eq!(byte_shl(a, n), naive_shift(a, n, 8, true), "{a} <<< {n}");
            assert_eq!(byte_shr(a, n), naive_shift(a, n, 8, false), "{a} >>> {n}");
        }
    }
}

#[test]
fn arithmetic_matches_checked_rust() {
    for a in EDGES {
        assert_eq!(
            int_negate(a),
            a.checked_neg().ok_or(ArithFault::IntegerOverflow)
        );
        for b in EDGES {
            let over = ArithFault::IntegerOverflow;
            assert_eq!(int_add(a, b), a.checked_add(b).ok_or(over));
            assert_eq!(int_sub(a, b), a.checked_sub(b).ok_or(over));
            assert_eq!(int_mul(a, b), a.checked_mul(b).ok_or(over));
            let zero = Err(ArithFault::DivisionByZero);
            assert_eq!(
                int_div(a, b),
                if b == 0 {
                    zero
                } else {
                    a.checked_div(b).ok_or(over)
                }
            );
            assert_eq!(
                int_rem(a, b),
                if b == 0 {
                    zero
                } else {
                    a.checked_rem(b).ok_or(over)
                }
            );
            assert_eq!(int_eq(a, b), (a == b) as i64);
            assert_eq!(int_ne(a, b), (a != b) as i64);
            assert_eq!(int_lt(a, b), (a < b) as i64);
            assert_eq!(int_le(a, b), (a <= b) as i64);
            assert_eq!(int_gt(a, b), (a > b) as i64);
            assert_eq!(int_ge(a, b), (a >= b) as i64);
            assert_eq!(int_compare(a, b), ordering(a.cmp(&b)));
        }
    }
}

#[test]
fn bools_compare_by_tag() {
    for a in [FALSE, TRUE] {
        for b in [FALSE, TRUE] {
            assert_eq!(bool_eq(a, b), (a == b) as i64);
            assert_eq!(bool_ne(a, b), (a != b) as i64);
        }
    }
}

/// 文言は docs/spec/object-model.md の「誤り」の表のとおりである。`bytes` で始まる3つのほかは docs/spec/lir.md の
/// 「実行時エラー」の一覧にもある。
#[test]
fn faults_print_their_listed_texts() {
    let cases: [(Fault, &str); 33] = [
        (ArithFault::DivisionByZero.into(), "division by zero"),
        (
            ArithFault::NotScalar(55296).into(),
            "int `55296` is not a Unicode scalar value",
        ),
        (
            ArithFault::NotByte(256).into(),
            "int `256` does not fit in `Byte`",
        ),
        (ArithFault::IntegerOverflow.into(), "integer overflow"),
        (
            ArithFault::FloatToInt(f64::NAN.to_bits()).into(),
            "float `nan` does not fit in `Int`",
        ),
        (
            ArithFault::FloatToInt(1e19f64.to_bits()).into(),
            "float `1e+19` does not fit in `Int`",
        ),
        (
            ArithFault::FixedDigits(101).into(),
            "`to_fixed` takes 0 to 100 digits, not 101",
        ),
        (
            Fault::Output("disk full".into()),
            "cannot write the output: disk full",
        ),
        (
            Fault::Io {
                op: IoOp::Open,
                name: "a.txt".into(),
                reason: IoReason::NotFound,
            },
            "cannot open `a.txt`: not found",
        ),
        (
            Fault::Io {
                op: IoOp::Open,
                name: "p".into(),
                reason: IoReason::PermissionDenied,
            },
            "cannot open `p`: permission denied",
        ),
        (
            Fault::Io {
                op: IoOp::Read,
                name: "d".into(),
                reason: IoReason::Other,
            },
            "cannot read `d`: I/O error",
        ),
        (
            Fault::NotUtf8(Box::new(Utf8Source::File("b.bin".into()))),
            "`b.bin` is not valid UTF-8",
        ),
        (
            Fault::Io {
                op: IoOp::Write,
                name: "out.txt".into(),
                reason: IoReason::PermissionDenied,
            },
            "cannot write `out.txt`: permission denied",
        ),
        (
            Fault::Io {
                op: IoOp::Run,
                name: "rsync".into(),
                reason: IoReason::NotFound,
            },
            "cannot run `rsync`: not found",
        ),
        (
            Fault::Stdin(IoReason::NotAvailable),
            "cannot read the standard input: not available",
        ),
        (
            Fault::NotUtf8(Box::new(Utf8Source::Stdin)),
            "the standard input is not valid UTF-8",
        ),
        (
            Fault::InUse(StdStream::Stdin),
            "the standard input is already open",
        ),
        (
            Fault::InUse(StdStream::Stdout),
            "the standard output is already open",
        ),
        (
            Fault::InUse(StdStream::Stderr),
            "the standard error is already open",
        ),
        (
            Fault::NotUtf8(Box::new(Utf8Source::Arg(2))),
            "argument 2 is not valid UTF-8",
        ),
        (
            Fault::NotUtf8(Box::new(Utf8Source::EnvVar("LANG".into()))),
            "environment variable `LANG` is not valid UTF-8",
        ),
        (Fault::Utf8 { at: 3 }, "invalid UTF-8 at byte 3"),
        (
            Fault::Index(Box::new(IndexFault::String { index: 5, len: 3 })),
            "string index 5 is out of range for length 3",
        ),
        (
            Fault::Index(Box::new(IndexFault::Slice {
                start: 2,
                end: 9,
                len: 3,
            })),
            "string slice 2..9 is out of range for length 3",
        ),
        (
            Fault::Index(Box::new(IndexFault::Boundary { start: 1, end: 3 })),
            "string slice 1..3 is not on a character boundary",
        ),
        (
            Fault::Index(Box::new(IndexFault::Arg { index: 3, count: 2 })),
            "argument index 3 is out of range for 2 arguments",
        ),
        (
            Fault::Index(Box::new(IndexFault::Array { index: -1, len: 4 })),
            "array index -1 is out of range for length 4",
        ),
        (
            Fault::Index(Box::new(IndexFault::Length { len: -2 })),
            "array length -2 is negative",
        ),
        (
            Fault::Index(Box::new(IndexFault::BytesIndex { index: 4, len: 4 })),
            "bytes index 4 is out of range for length 4",
        ),
        (
            Fault::Index(Box::new(IndexFault::BytesSlice {
                start: 0,
                end: 9,
                len: 3,
            })),
            "bytes slice 0..9 is out of range for length 3",
        ),
        (
            Fault::Index(Box::new(IndexFault::BytesLength { len: -2 })),
            "bytes length -2 is negative",
        ),
        (
            HeapError::WrongKind.into(),
            "an object is not of the expected kind",
        ),
        (Fault::Stopped(Stop::Interrupted), "interrupted"),
    ];
    for (fault, text) in cases {
        assert_eq!(fault.to_string(), text);
    }
    assert_eq!(
        Stop::MemoryLimit.to_string(),
        "the memory limit is exceeded"
    );
    assert_eq!(
        Stop::DepthLimit.to_string(),
        "the call depth limit is exceeded"
    );
}

/// `Show Float` の表示は、どの ±2^k (非正規化数も) でも読み戻すと同じビット列になる。2 の冪では往復できる区間が
/// 下側だけ狭く、正しく丸めた最短の桁がその外に出ることがあるためである。2^-1017 と 2^89 は Python の `repr` と同じ
/// 桁になる
#[test]
fn every_power_of_two_shows_and_reads_back() {
    for k in -1074..1024 {
        let bits = if k >= -1022 {
            ((k + 1023) as u64) << 52
        } else {
            1 << (k + 1074)
        };
        for x in [f64::from_bits(bits), -f64::from_bits(bits)] {
            let shown = float_repr(x);
            assert_eq!(
                float_parse(&shown).map(f64::to_bits),
                Some(x.to_bits()),
                "{shown}"
            );
            let mantissa = shown.split('e').next().unwrap();
            assert!(
                !mantissa.contains('.') || !mantissa.ends_with('0') || mantissa.ends_with(".0"),
                "{shown}"
            );
        }
    }
    assert_eq!(float_repr(2f64.powi(-1017)), "7.120236347223045e-307");
    assert_eq!(float_repr(2f64.powi(89)), "6.189700196426902e+26");
}

// 信頼の一覧の T12 と T13 の値の確かめ。本体は安全な Rust で、Miri が見るメモリと借用の規則には足すものがないので、
// `trusted` に置かない (docs/implementation/testing.md の「Miri」)

/// 浮動小数の演算と `compare`。特別な値 (±0、±無限大、NaN、非正規化数) を含む組で、Rust の演算と同じビット列を返す。
/// NaN の結果は、Rust の結果の符号と中身によらず、正の quiet NaN (`f64::NAN`) のビット列でなければならない
#[test]
fn float_operations_match_rust() {
    let values = [
        0.0,
        -0.0,
        1.5,
        -2.5,
        f64::MIN_POSITIVE,
        5e-324,
        f64::MAX,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        -f64::NAN,
    ];
    let same = |got: f64, want: f64| {
        let want = if want.is_nan() { f64::NAN } else { want };
        assert_eq!(got.to_bits(), want.to_bits(), "{got:?} != {want:?}")
    };
    for a in values {
        same(float_negate(a), -a);
        for b in values {
            same(float_add(a, b), a + b);
            same(float_sub(a, b), a - b);
            same(float_mul(a, b), a * b);
            same(float_div(a, b), a / b);
            same(float_rem(a, b), a % b);
            let order = match a.total_cmp(&b) {
                std::cmp::Ordering::Less => LT,
                std::cmp::Ordering::Equal => EQ,
                std::cmp::Ordering::Greater => GT,
            };
            assert_eq!(float_compare(a, b), order, "{a:?} {b:?}");
        }
    }
}

/// 浮動小数の変換、数学の関数、書式、読み。各関数が Rust の `f64` の関数と同じ値 (NaN は `f64::NAN` のビット列) を
/// 返し、書式が Python の `repr` と `f"{x:.{n}f}"` の形で、読みが書式の結果を同じビット列に読み戻すこと
#[test]
fn float_conversions_and_formats_match_rust() {
    let values = [
        0.0,
        -0.0,
        0.1,
        2.5,
        -3.5,
        1e16,
        1e-5,
        f64::MIN_POSITIVE,
        5e-324,
        f64::MAX,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        -f64::NAN,
    ];
    let same = |got: f64, want: f64| {
        let want = if want.is_nan() { f64::NAN } else { want };
        assert_eq!(got.to_bits(), want.to_bits(), "{got:?} != {want:?}")
    };
    let int = |t: f64, x: f64| {
        if (-9223372036854775808.0..9223372036854775808.0).contains(&t) {
            Ok(t as i64)
        } else {
            Err(ArithFault::FloatToInt(x.to_bits()))
        }
    };
    for x in values {
        same(float_sqrt(x), x.sqrt());
        same(float_abs(x), x.abs());
        same(float_sin(x), x.sin());
        same(float_cos(x), x.cos());
        same(float_exp(x), x.exp());
        same(float_log(x), x.ln());
        for y in values {
            same(float_pow(x, y), x.powf(y));
        }
        assert_eq!(float_truncate(x), int(x.trunc(), x));
        assert_eq!(float_floor(x), int(x.floor(), x));
        assert_eq!(float_ceil(x), int(x.ceil(), x));
        assert_eq!(float_round(x), int(x.round_ties_even(), x));
        let minus = x.is_sign_negative() && !x.is_nan();
        assert_eq!(float_shows_minus(x), if minus { TRUE } else { FALSE });
        let shown = float_repr(x);
        assert_eq!(shown.starts_with('-'), minus, "{shown}");
        let back = float_parse(&shown).unwrap();
        same(back, x);
        assert_eq!(
            float_fixed(3, x),
            if x.is_nan() {
                "nan".to_string()
            } else {
                format!("{x:.3}")
            }
        );
    }
    for i in [0, 1, -1, i64::MAX, i64::MIN, (1 << 53) + 1] {
        same(float_of_int(i), i as f64);
    }
    same(float_infinity(), f64::INFINITY);
    same(float_nan(), f64::NAN);
    assert_eq!(float_repr(1e16), "1e+16");
    assert_eq!(float_repr(1e-5), "1e-05");
    assert_eq!(float_repr(123456.0), "123456.0");
    assert_eq!(float_parse(" 1_0 "), None);
    same(float_parse("-nan").unwrap(), f64::NAN);
}

/// `find_byte` と比べる素朴なループ。
fn naive_find_byte(hay: &[u8], from: usize, b: u8) -> Option<usize> {
    (from..hay.len()).find(|&i| hay[i] == b)
}

/// 探すバイトの近くの値を混ぜた乱数の列。語の演算で借りが伝わる並び (`b` の直後の `b ^ 1` など) と、`b` の密な
/// 列と疎な列ができる。
fn bytes_near(rng: &mut table::Rng, b: u8, len: usize) -> Vec<u8> {
    let density = [2, 8, 64][(rng.next() % 3) as usize];
    (0..len)
        .map(|_| {
            let r = rng.next();
            if r.is_multiple_of(density) {
                b
            } else {
                let near = [
                    b ^ 1,
                    b ^ 0x80,
                    b.wrapping_add(1),
                    b.wrapping_sub(1),
                    0,
                    0xff,
                ];
                if r % 4 == 1 {
                    (r >> 8) as u8
                } else {
                    near[((r >> 8) % 6) as usize]
                }
            }
        })
        .collect()
}

/// T13 の `find_byte` は、乱数の入力について素朴なループと同じ位置を返す。列は1つの buffer の途中から切り出し、語に
/// 揃わない先頭からも読む。長さは 8 バイトの塊の数と残りのバイトの数のどの組も作る。
#[test]
fn find_byte_matches_a_naive_loop() {
    let mut rng = table::Rng::new();
    for _ in 0..3000 {
        let b = rng.next() as u8;
        let start = (rng.next() % 8) as usize;
        let len = (rng.next() % 40) as usize;
        let buffer = bytes_near(&mut rng, b, start + len);
        let hay = &buffer[start..];
        let from = (rng.next() % (len as u64 + 1)) as usize;
        assert_eq!(
            find_byte(hay, from, b),
            naive_find_byte(hay, from, b),
            "{hay:?} from {from} for {b}"
        );
    }
}

/// `find_from` の照合 (`find_bytes`) は `find_byte` で先頭のバイトを探す。乱数の入力について、`windows` で1つずつ
/// 比べた位置と同じ位置を返す。
#[test]
fn find_byte_under_find_bytes_matches_windows() {
    let mut rng = table::Rng::new();
    for _ in 0..3000 {
        let b = rng.next() as u8;
        let len = (rng.next() % 40) as usize;
        let text = bytes_near(&mut rng, b, len);
        let needle_len = (rng.next() % 4) as usize;
        let needle = bytes_near(&mut rng, b, needle_len);
        let from = (rng.next() % (len as u64 + 1)) as usize;
        let expected = if needle.is_empty() {
            Some(from)
        } else {
            text[from..]
                .windows(needle.len())
                .position(|w| w == needle.as_slice())
                .map(|i| from + i)
        };
        assert_eq!(
            find_bytes(&text, &needle, from),
            expected.map_or(-1, |i| i as i64),
            "{needle:?} in {text:?} from {from}"
        );
    }
}

/// 算術の誤りは2つのレジスタで返り、`Fault` は 24 バイトに収まる。
#[test]
fn faults_stay_small() {
    assert_eq!(std::mem::size_of::<Result<i64, ArithFault>>(), 16);
    assert_eq!(std::mem::size_of::<Fault>(), 24);
}

fn s<M: Mode>(ctx: &mut crate::Ctx<M>, x: &str) -> Word {
    unsafe { ctx.heap_mut() }.alloc_string(x)
}

const TEXTS: [&str; 10] = [
    "", "a", "ab", "b", "é", "日本", "a\u{0}", "\u{7f}", "\u{80}", "ab\u{0}",
];

fn strings_compare_by_bytes<M: Mode>() {
    let mut ctx = ctx::<M>();
    for x in TEXTS {
        for y in TEXTS {
            unsafe {
                let (a, b) = (s(&mut ctx, x), s(&mut ctx, y));
                assert_eq!(ctx.string_eq(a, b), Ok((x == y) as i64));
                assert_eq!(ctx.string_ne(a, b), Ok((x != y) as i64));
                assert_eq!(
                    ctx.string_compare(a, b),
                    Ok(ordering(x.as_bytes().cmp(y.as_bytes())))
                );
                ctx.heap_mut().decref(a).unwrap();
                ctx.heap_mut().decref(b).unwrap();
            }
        }
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 借りる行は同じ文字列を両辺に渡されても、参照1つのまま動く。消費する行は参照を2つ持って渡す。
fn the_same_string_on_both_sides<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let a = s(&mut ctx, "abc");
        assert_eq!(ctx.string_eq(a, a), Ok(TRUE));
        assert_eq!(ctx.string_ne(a, a), Ok(FALSE));
        assert_eq!(ctx.string_compare(a, a), Ok(EQ));
        let n = s(&mut ctx, "b");
        assert_eq!(ctx.string_find_from(a, 0, a), Ok(0));
        assert_eq!(ctx.string_find_from(n, 0, a), Ok(1));
        ctx.heap_mut().decref(n).unwrap();
        ctx.heap_mut().decref(a).unwrap();
        let a = s(&mut ctx, "abc");
        ctx.heap_mut().dup(a).unwrap();
        let w = ctx.string_append(a, a).unwrap();
        assert_eq!(text(ctx.heap(), w), "abcabc");
        ctx.heap_mut().decref(w).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 写すバイトを数える。一意な左なら、その場で足しても作り直しても右の長さを、共有か不死の左なら両辺の長さを数える。
fn appending_counts_the_bytes_it_copies<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let a = s(&mut ctx, "ab");
        let b = s(&mut ctx, "cde");
        let before = ctx.stats().string_bytes_copied;
        let w = ctx.string_append(a, b).unwrap();
        assert_eq!(text(ctx.heap(), w), "abcde");
        if M::CHECKED {
            assert_eq!(ctx.stats().string_bytes_copied - before, 3);
        }
        // 共有した左は作り直す
        ctx.heap_mut().dup(w).unwrap();
        let c = s(&mut ctx, "fg");
        let before = ctx.stats().string_bytes_copied;
        let x = ctx.string_append(w, c).unwrap();
        assert_ne!(x, w);
        assert_eq!(text(ctx.heap(), x), "abcdefg");
        if M::CHECKED {
            assert_eq!(ctx.stats().string_bytes_copied - before, 7);
        }
        // 不死の左も作り直す。呼ぶ側は、不死の物体の参照を `acquire_immortal` で作って渡す
        let k = ctx.heap_mut().alloc_string_immortal("k");
        ctx.heap_mut().acquire_immortal(k).unwrap();
        let d = s(&mut ctx, "z");
        let before = ctx.stats().string_bytes_copied;
        let y = ctx.string_append(k, d).unwrap();
        assert_eq!(text(ctx.heap(), y), "kz");
        if M::CHECKED {
            assert_eq!(ctx.stats().string_bytes_copied - before, 2);
        }
        for v in [w, x, y] {
            ctx.heap_mut().decref(v).unwrap();
        }
    }
    // 不死の `k` だけが残る
    assert_eq!(mortal(ctx.heap()), 1);
}

/// `Int.show` と `String.show` は、作った文字列の長さを数える。
fn shows_count_the_bytes_they_write<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut ctx = ctx::<M>();
    let before = ctx.stats().string_bytes_copied;
    let w = ctx.int_show(-12);
    assert_eq!(ctx.stats().string_bytes_copied - before, 3);
    unsafe {
        ctx.heap_mut().decref(w).unwrap();
        let a = s(&mut ctx, "a\n日");
        let before = ctx.stats().string_bytes_copied;
        let w = ctx.string_show(a).unwrap();
        assert_eq!(text(ctx.heap(), w), "\"a\\n日\"");
        assert_eq!(ctx.stats().string_bytes_copied - before, 8);
        ctx.heap_mut().decref(w).unwrap();
        ctx.heap_mut().decref(a).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// `char_display` が書くバイトは、Rust の `char::encode_utf8` と同じで、長さは UTF-8 の幅の境目で変わる。
fn chars_display_as_utf8<M: Mode>() {
    let mut ctx = ctx::<M>();
    let cases = [
        (0x0, 1),
        (0x7f, 1),
        (0x80, 2),
        (0x7ff, 2),
        (0x800, 3),
        (0xd7ff, 3),
        (0xe000, 3),
        (0xffff, 3),
        (0x1_0000, 4),
        (0x10_ffff, 4),
    ];
    for (c, len) in cases {
        let w = unsafe { ctx.char_display(c) };
        let expected = char::from_u32(c as u32).unwrap().to_string();
        assert_eq!(expected.len(), len);
        assert_eq!(text(ctx.heap(), w), expected);
        unsafe { ctx.heap_mut().decref(w) }.unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

fn ints_show_in_decimal<M: Mode>() {
    let mut ctx = ctx::<M>();
    for v in [
        0,
        7,
        -7,
        10,
        -10,
        1_234_567_890,
        i64::MAX,
        i64::MIN,
        i64::MIN + 1,
    ] {
        let w = ctx.int_show(v);
        assert_eq!(text(ctx.heap(), w), v.to_string());
        unsafe { ctx.heap_mut().decref(w) }.unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// エスケープの規則は docs/spec/declarations.md の「Prelude のクラス」のとおりである。C1 の制御文字 (U+0080〜U+009F)
/// はそのまま写す。
fn strings_show_with_their_escapes<M: Mode>() {
    let cases = [
        ("", r#""""#),
        ("abc", r#""abc""#),
        ("a\"b", r#""a\"b""#),
        ("a\\b", r#""a\\b""#),
        ("\n\t\r\0", r#""\n\t\r\0""#),
        ("\u{1}\u{1f}\u{7f}", r#""\u{1}\u{1f}\u{7f}""#),
        (
            "\u{80}\u{85}\u{9f}é日\u{2028}",
            "\"\u{80}\u{85}\u{9f}é日\u{2028}\"",
        ),
        ("x\ny", r#""x\ny""#),
    ];
    let mut ctx = ctx::<M>();
    for (input, shown) in cases {
        unsafe {
            let a = s(&mut ctx, input);
            let w = ctx.string_show(a).unwrap();
            assert_eq!(text(ctx.heap(), w), shown);
            ctx.heap_mut().decref(w).unwrap();
            ctx.heap_mut().decref(a).unwrap();
        }
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 文字列の核はバイトの位置で読み、範囲の外と文字の境界を断る。断っても引数は借りたままで、参照の数は変わらない
/// (docs/spec/lir.md の「extern の借用」)。
fn the_string_core_reads_by_byte_positions<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let a = s(&mut ctx, "a日b");
        assert_eq!(ctx.string_byte_length(a), Ok(5));
        assert_eq!(ctx.string_byte_at(1, a), Ok(0xe6));
        assert_eq!(
            ctx.string_byte_at(5, a),
            Err(Fault::Index(Box::new(IndexFault::String {
                index: 5,
                len: 5
            })))
        );
        let w = ctx.string_slice(1, 4, a).unwrap();
        assert_eq!(text(ctx.heap(), w), "日");
        ctx.heap_mut().decref(w).unwrap();
        assert_eq!(
            ctx.string_slice(2, 4, a),
            Err(Fault::Index(Box::new(IndexFault::Boundary {
                start: 2,
                end: 4
            })))
        );
        assert_eq!(
            ctx.string_slice(0, 6, a),
            Err(Fault::Index(Box::new(IndexFault::Slice {
                start: 0,
                end: 6,
                len: 5
            })))
        );
        ctx.heap_mut().decref(a).unwrap();
        // 探す位置は `from` 以上で、空の文字列は `from` で見つかる
        for (needle, from, found) in [
            ("b", 0, 4),
            ("日", 1, 1),
            ("日", 2, -1),
            ("", 3, 3),
            ("", 5, 5),
        ] {
            let n = s(&mut ctx, needle);
            let a = s(&mut ctx, "a日b");
            assert_eq!(
                ctx.string_find_from(n, from, a),
                Ok(found),
                "{needle} {from}"
            );
            ctx.heap_mut().decref(n).unwrap();
            ctx.heap_mut().decref(a).unwrap();
        }
        let n = s(&mut ctx, "b");
        let a = s(&mut ctx, "a日b");
        assert_eq!(
            ctx.string_find_from(n, 6, a),
            Err(Fault::Index(Box::new(IndexFault::String {
                index: 6,
                len: 5
            })))
        );
        ctx.heap_mut().decref(n).unwrap();
        ctx.heap_mut().decref(a).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// `slice` は写したバイトの数を数える。
fn a_slice_counts_the_bytes_it_copies<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut ctx = ctx::<M>();
    unsafe {
        let a = s(&mut ctx, "a日b");
        let before = ctx.stats().string_bytes_copied;
        let w = ctx.string_slice(0, 4, a).unwrap();
        assert_eq!(ctx.stats().string_bytes_copied - before, 4);
        ctx.heap_mut().decref(w).unwrap();
        ctx.heap_mut().decref(a).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 文字列の核も、文字列でない引数を検査モードで断り、ヒープを変えない。
fn the_string_core_refuses_a_non_string<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut ctx = ctx::<M>();
    unsafe {
        let d = ctx.heap_mut().alloc_data(0, 0, 1, false);
        let a = s(&mut ctx, "a");
        let live = mortal(ctx.heap());
        let wrong = Fault::Heap(HeapError::WrongKind);
        assert_eq!(
            ctx.string_byte_length(d),
            Err(Fault::Heap(HeapError::WrongKind))
        );
        assert_eq!(
            ctx.string_byte_at(0, d),
            Err(Fault::Heap(HeapError::WrongKind))
        );
        assert_eq!(
            ctx.string_slice(0, 0, d),
            Err(Fault::Heap(HeapError::WrongKind))
        );
        assert_eq!(ctx.string_find_from(a, 0, d), Err(wrong));
        assert_eq!(mortal(ctx.heap()), live);
        ctx.heap_mut().decref(a).unwrap();
        ctx.heap_mut().decref(d).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 文字列でない引数は検査モードで断る。断ったときはヒープを変えない。
fn a_string_extern_refuses_a_non_string<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut ctx = ctx::<M>();
    unsafe {
        let d = ctx.heap_mut().alloc_data(0, 0, 1, false);
        let a = s(&mut ctx, "a");
        let live = mortal(ctx.heap());
        assert_eq!(ctx.string_eq(a, d), Err(Fault::Heap(HeapError::WrongKind)));
        assert_eq!(ctx.string_show(d), Err(Fault::Heap(HeapError::WrongKind)));
        assert_eq!(mortal(ctx.heap()), live);
        ctx.heap_mut().decref(a).unwrap();
        ctx.heap_mut().decref(d).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

fn println_writes_a_line_and_releases_its_text<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let a = s(&mut ctx, "héllo");
        ctx.write_string(crate::Stream::Out, a).unwrap();
        let e = s(&mut ctx, "");
        ctx.write_string(crate::Stream::Out, e).unwrap();
    }
    ctx.flush().unwrap();
    assert_eq!(memory.out(), "héllo\n\n");
    assert_eq!(memory.err(), "");
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 書けなくても文字列は手放す。
fn a_failed_write_still_releases_the_text<M: Mode>() {
    let memory = Arc::new(MemoryHost::failing("disk full"));
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let a = s(&mut ctx, "x");
        assert_eq!(
            ctx.write_string(crate::Stream::Out, a),
            Err(Fault::Output("disk full".into()))
        );
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

fn println_refuses_a_non_string<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let memory = Arc::new(MemoryHost::default());
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let d = ctx.heap_mut().alloc_data(0, 0, 1, false);
        assert_eq!(
            ctx.write_string(crate::Stream::Out, d),
            Err(Fault::Heap(HeapError::WrongKind))
        );
        assert_eq!(
            ctx.write_string(crate::Stream::Out, Word::imm(0)),
            Err(Fault::Heap(HeapError::WrongKind))
        );
        assert_eq!(memory.out(), "");
        ctx.heap_mut().decref(d).unwrap();
    }
}

/// 中身を `max_chars` 文字まで写し、文字列を手放す。多バイトの文字の途中では切らない。
fn copy_string_copies_up_to_a_number_of_chars<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let e = s(&mut ctx, "");
        assert_eq!(ctx.copy_string(e, 10).unwrap(), "");
        let a = s(&mut ctx, "héllo, 世界");
        assert_eq!(ctx.copy_string(a, 100).unwrap(), "héllo, 世界");
        let b = s(&mut ctx, "héllo, 世界");
        assert_eq!(ctx.copy_string(b, 9).unwrap(), "héllo, 世界");
        let c = s(&mut ctx, "héllo, 世界");
        assert_eq!(ctx.copy_string(c, 8).unwrap(), "héllo, 世");
        let d = s(&mut ctx, "héllo");
        assert_eq!(ctx.copy_string(d, 2).unwrap(), "hé");
        let z = s(&mut ctx, "abc");
        assert_eq!(ctx.copy_string(z, 0).unwrap(), "");
        let long = s(&mut ctx, &"あ".repeat(10_002));
        assert_eq!(ctx.copy_string(long, 10_001).unwrap(), "あ".repeat(10_001));
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 凍結した領域の文字列も写せる。領域の物体は数えないので、手放しても何も変わらない。
fn copy_string_reads_a_region_string<M: Mode>() {
    let (b, constant, _) = super::frozen::builder::<M>();
    let region = Arc::new(b.freeze().unwrap());
    let mut ctx = ctx::<M>();
    unsafe { ctx.heap_mut() }.attach(region);
    assert_eq!(
        unsafe { ctx.copy_string(constant, 100) }.unwrap(),
        "constant"
    );
    assert_eq!(mortal(ctx.heap()), 0);
}

fn copy_string_refuses_a_non_string<M: Mode>() {
    if !M::CHECKED {
        return;
    }
    let mut ctx = ctx::<M>();
    unsafe {
        let d = ctx.heap_mut().alloc_data(0, 0, 1, false);
        assert_eq!(
            ctx.copy_string(d, 10),
            Err(Fault::Heap(HeapError::WrongKind))
        );
        assert_eq!(
            ctx.copy_string(Word::imm(0), 10),
            Err(Fault::Heap(HeapError::WrongKind))
        );
        ctx.heap_mut().decref(d).unwrap();
    }
}

/// 1行を書いて書き出す。まとめて書く Host でも、呼んだ後には書き先に届いている。
fn print_line_writes_and_flushes<M: Mode>() {
    let out = Shared::default();
    let host = StdHost::new(
        Box::new(out.clone()),
        Box::new(io::sink()),
        Default::default(),
        Buffering::Block,
    );
    let mut ctx = crate::Ctx::<M>::new(
        Arc::new(Control::new(Limits::default())),
        HostRef::new(Arc::new(host)),
    );
    ctx.print_line("[ 1\n, 2 ] : List Int").unwrap();
    assert_eq!(out.text(), "[ 1\n, 2 ] : List Int\n");
    let memory = Arc::new(MemoryHost::failing("disk full"));
    let mut ctx = ctx_with::<M>(&memory);
    assert_eq!(ctx.print_line("x"), Err(Fault::Output("disk full".into())));
}

fn a_missing_file_is_a_fault_naming_its_path<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let p = s(&mut ctx, "missing.txt");
        assert_eq!(
            ctx.fs_open(p),
            Err(Fault::Io {
                op: IoOp::Open,
                name: "missing.txt".into(),
                reason: IoReason::NotFound
            })
        );
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// `handle_read` でハンドルを借りて読み、読んだ中身を写して手放す。ハンドルの参照は呼ぶ側に残る。
unsafe fn read_borrowed<M: Mode>(
    ctx: &mut crate::Ctx<M>,
    f: Word,
    read: ReadAmount,
) -> Result<String, Fault> {
    unsafe {
        let w = ctx.handle_read(f, Handle::File, read, Form::Text)?;
        let read = text(ctx.heap(), w);
        ctx.heap_mut().decref(w).unwrap();
        Ok(read)
    }
}

/// パスの文字列を作って開く。
unsafe fn open<M: Mode>(ctx: &mut crate::Ctx<M>, path: &str) -> Word {
    unsafe {
        let p = s(ctx, path);
        ctx.fs_open(p).unwrap()
    }
}

/// 借りて全部読む。2回目は終わりから読むので空である。読んだ後もハンドルの rc は 1 のままで、閉じるまで落ちない。
fn a_borrowed_file_is_read_whole_and_closed<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let drops = Arc::new(AtomicUsize::new(0));
    memory.add(
        "a.txt",
        MemoryFile {
            drops: Some(drops.clone()),
            ..MemoryFile::text("line 1\nline 2\n")
        },
    );
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let f = open(&mut ctx, "a.txt");
        assert_eq!(
            read_borrowed(&mut ctx, f, ReadAmount::All).unwrap(),
            "line 1\nline 2\n"
        );
        assert_eq!(super::raw::rc(f), 1);
        assert_eq!(read_borrowed(&mut ctx, f, ReadAmount::All).unwrap(), "");
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        ctx.handle_close(f, Handle::File).unwrap();
        assert_eq!(drops.load(Ordering::Relaxed), 1);
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

fn an_empty_borrowed_file_reads_as_an_empty_string<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    memory.add("empty.txt", MemoryFile::text(""));
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let f = open(&mut ctx, "empty.txt");
        assert_eq!(read_borrowed(&mut ctx, f, ReadAmount::All).unwrap(), "");
        ctx.handle_close(f, Handle::File).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 長さの見込みがなく、短い読みしか返さない読み手でも、借りたまま 64 KiB の区切りをまたいで全部読む。
fn a_long_unsized_borrowed_file_is_read_in_chunks<M: Mode>() {
    let body: String = "abcdefg\n".repeat(20_000);
    let memory = Arc::new(MemoryHost::default());
    memory.add(
        "big.txt",
        MemoryFile {
            sized: false,
            chunk: 1000,
            ..MemoryFile::text(&body)
        },
    );
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let f = open(&mut ctx, "big.txt");
        assert_eq!(read_borrowed(&mut ctx, f, ReadAmount::All).unwrap(), body);
        ctx.handle_close(f, Handle::File).unwrap();
    }
    if M::CHECKED {
        assert_eq!(ctx.stats().string_bytes_copied, body.len() as u64);
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// シグナルで中断された読みは、誤りにせず読み直す。
fn an_interrupted_borrowed_read_is_retried<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    memory.add(
        "a.txt",
        MemoryFile {
            interrupt_once: true,
            ..MemoryFile::text("abc")
        },
    );
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let f = open(&mut ctx, "a.txt");
        assert_eq!(read_borrowed(&mut ctx, f, ReadAmount::All).unwrap(), "abc");
        ctx.handle_close(f, Handle::File).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 読みの誤りと UTF-8 でない中身は、ハンドルを手放さず、ヒープを変えない。作りかけの文字列も残らない。
fn a_failed_borrowed_read_keeps_the_file<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let drops = Arc::new(AtomicUsize::new(0));
    memory.add(
        "broken.txt",
        MemoryFile {
            fail_after: Some(3),
            drops: Some(drops.clone()),
            ..MemoryFile::text("abcdef")
        },
    );
    memory.add(
        "bad.bin",
        MemoryFile {
            bytes: vec![b'a', 0xff, 0xfe],
            sized: true,
            drops: Some(drops.clone()),
            ..MemoryFile::default()
        },
    );
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let f = open(&mut ctx, "broken.txt");
        let objects = mortal(ctx.heap());
        assert_eq!(
            read_borrowed(&mut ctx, f, ReadAmount::All),
            Err(Fault::Io {
                op: IoOp::Read,
                name: "broken.txt".into(),
                reason: IoReason::Other
            })
        );
        assert_eq!(super::raw::rc(f), 1);
        assert_eq!(mortal(ctx.heap()), objects);
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        ctx.handle_close(f, Handle::File).unwrap();
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        let f = open(&mut ctx, "bad.bin");
        assert_eq!(
            read_borrowed(&mut ctx, f, ReadAmount::All),
            Err(Fault::NotUtf8(Box::new(Utf8Source::File("bad.bin".into()))))
        );
        assert_eq!(super::raw::rc(f), 1);
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        ctx.handle_close(f, Handle::File).unwrap();
        assert_eq!(drops.load(Ordering::Relaxed), 2);
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 読む間に止める語を見る。割り込まれていれば止め、ハンドルは呼ぶ側に残る。止める語を下ろせば、同じハンドルを
/// 続けて読める。
fn a_borrowed_read_stops_on_an_interrupt<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    memory.add("a.txt", MemoryFile::text("abc"));
    let control = Arc::new(Control::new(Limits::default()));
    let mut ctx = crate::Ctx::<M>::new(control.clone(), host(&memory));
    unsafe {
        let f = open(&mut ctx, "a.txt");
        control.interrupt();
        assert_eq!(
            read_borrowed(&mut ctx, f, ReadAmount::All),
            Err(Fault::Stopped(Stop::Interrupted))
        );
        assert_eq!(super::raw::rc(f), 1);
        control.clear();
        assert_eq!(read_borrowed(&mut ctx, f, ReadAmount::All).unwrap(), "abc");
        ctx.handle_close(f, Handle::File).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 読みが `EINTR` で戻ったとき、止める語が立っていれば読み直さずに止める。ハンドルは呼ぶ側に残り、閉じるまで
/// 落ちない (docs/spec/object-model.md の「読み手」)。読み直すとテストの読み手が panic する。
fn an_interrupted_borrowed_read_stops_when_the_stop_word_is_set<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let control = Arc::new(Control::new(Limits::default()));
    let drops = Arc::new(AtomicUsize::new(0));
    memory.add(
        "fifo",
        MemoryFile {
            interrupt_with: Some(control.clone()),
            drops: Some(drops.clone()),
            ..MemoryFile::text("abc")
        },
    );
    let mut ctx = crate::Ctx::<M>::new(control, host(&memory));
    unsafe {
        let f = open(&mut ctx, "fifo");
        assert_eq!(
            read_borrowed(&mut ctx, f, ReadAmount::All),
            Err(Fault::Stopped(Stop::Interrupted))
        );
        assert_eq!(super::raw::rc(f), 1);
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        ctx.handle_close(f, Handle::File).unwrap();
    }
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert_eq!(mortal(ctx.heap()), 0);
}

/// ヒープの外の読みの領域も、確保量の上限に数える。長さの見込みがあれば確保する前に比べるので、確保できないほど
/// 大きい見込みも I/O の誤りでなく上限で止まる。見込みがなければ、1回読むごとに比べる。行の読みも同じ上限で止まる。
/// ただし行の読みは見込みを使わないので、見込みだけが大きい `huge.txt` は読める。上限は、ヒープが初めに取るページ
/// より十分大きくする。ページだけで上限を超えると、どの読みも中身を見る前に止まり、比べ方を確かめられない。止めた
/// 後もハンドルは呼ぶ側に残る。
fn a_borrowed_read_past_the_memory_limit_stops<M: Mode>() {
    let max_bytes = 1 << 20;
    let body = "x".repeat(2 * max_bytes);
    let memory = Arc::new(MemoryHost::default());
    memory.add("big.txt", MemoryFile::text(&body));
    memory.add(
        "huge.txt",
        MemoryFile {
            claimed_size: Some(u64::MAX),
            ..MemoryFile::text("x")
        },
    );
    memory.add(
        "unsized.txt",
        MemoryFile {
            sized: false,
            chunk: 1000,
            ..MemoryFile::text(&body)
        },
    );
    let control = Arc::new(Control::new(Limits {
        max_bytes,
        ..Limits::default()
    }));
    let mut ctx = crate::Ctx::<M>::new(control.clone(), host(&memory));
    for read in [ReadAmount::All, ReadAmount::Line] {
        for path in ["big.txt", "huge.txt", "unsized.txt"] {
            unsafe {
                let f = open(&mut ctx, path);
                assert_eq!(control.stop_bits(), 0);
                assert!(control.bytes() < max_bytes / 2);
                if read == ReadAmount::Line && path == "huge.txt" {
                    assert_eq!(read_borrowed(&mut ctx, f, read).unwrap(), "x");
                } else {
                    assert_eq!(
                        read_borrowed(&mut ctx, f, read),
                        Err(Fault::Stopped(Stop::MemoryLimit)),
                        "{path}, {read:?}"
                    );
                    assert_eq!(super::raw::rc(f), 1);
                }
                ctx.handle_close(f, Handle::File).unwrap();
            }
        }
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// File でない物体、即値、参照が2つある File は、借りて読めない。断ったときはヒープを変えない。
fn a_borrowed_read_refuses_a_non_file_or_a_shared_file<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    memory.add("a.txt", MemoryFile::text("abc"));
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let other = ctx.heap_mut().alloc_foreign(0u64, false);
        let objects = mortal(ctx.heap());
        for read in [ReadAmount::All, ReadAmount::Line] {
            assert_eq!(
                ctx.handle_read(other, Handle::File, read, Form::Text),
                Err(Fault::Heap(HeapError::WrongKind))
            );
            assert_eq!(
                ctx.handle_read(Word::imm(0), Handle::File, read, Form::Text),
                Err(Fault::Heap(HeapError::WrongKind))
            );
        }
        assert_eq!(mortal(ctx.heap()), objects);
        ctx.heap_mut().decref(other).unwrap();
        if M::CHECKED {
            let f = open(&mut ctx, "a.txt");
            super::raw::set_rc(ctx.heap_mut(), f, 2);
            let objects = mortal(ctx.heap());
            for read in [ReadAmount::All, ReadAmount::Line] {
                assert_eq!(
                    ctx.handle_read(f, Handle::File, read, Form::Text),
                    Err(Fault::Heap(HeapError::NotUnique))
                );
            }
            assert_eq!(mortal(ctx.heap()), objects);
            super::raw::set_rc(ctx.heap_mut(), f, 1);
            ctx.handle_close(f, Handle::File).unwrap();
        }
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 借りたまま、行の終わりを含めて1行ずつ読み、終わりでは空の文字列を返す。全部の読みの後も空である。
fn a_borrowed_file_is_read_line_by_line<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    memory.add("a.txt", MemoryFile::text("one\r\n\ntwo\nlast"));
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let f = open(&mut ctx, "a.txt");
        for expected in ["one\r\n", "\n", "two\n"] {
            assert_eq!(
                read_borrowed(&mut ctx, f, ReadAmount::Line).unwrap(),
                expected
            );
        }
        assert_eq!(read_borrowed(&mut ctx, f, ReadAmount::All).unwrap(), "last");
        for _ in 0..2 {
            assert_eq!(read_borrowed(&mut ctx, f, ReadAmount::Line).unwrap(), "");
        }
        assert_eq!(super::raw::rc(f), 1);
        ctx.handle_close(f, Handle::File).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 行の読みの誤りも、ハンドルを手放さない。
fn a_failed_borrowed_line_read_keeps_the_file<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let drops = Arc::new(AtomicUsize::new(0));
    memory.add(
        "broken.txt",
        MemoryFile {
            fail_after: Some(3),
            drops: Some(drops.clone()),
            ..MemoryFile::text("abcdef")
        },
    );
    memory.add(
        "bad.bin",
        MemoryFile {
            bytes: vec![b'a', 0xff, b'\n'],
            sized: true,
            drops: Some(drops.clone()),
            ..MemoryFile::default()
        },
    );
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let f = open(&mut ctx, "broken.txt");
        assert_eq!(
            read_borrowed(&mut ctx, f, ReadAmount::Line),
            Err(Fault::Io {
                op: IoOp::Read,
                name: "broken.txt".into(),
                reason: IoReason::Other
            })
        );
        assert_eq!(super::raw::rc(f), 1);
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        ctx.handle_close(f, Handle::File).unwrap();
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        let f = open(&mut ctx, "bad.bin");
        assert_eq!(
            read_borrowed(&mut ctx, f, ReadAmount::Line),
            Err(Fault::NotUtf8(Box::new(Utf8Source::File("bad.bin".into()))))
        );
        assert_eq!(super::raw::rc(f), 1);
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        ctx.handle_close(f, Handle::File).unwrap();
        assert_eq!(drops.load(Ordering::Relaxed), 2);
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// `handle_read` でハンドルを借りてバイト列の形で読み、読んだ中身を写して手放す。ハンドルの参照は呼ぶ側に残る。
unsafe fn read_bytes_borrowed<M: Mode>(
    ctx: &mut crate::Ctx<M>,
    f: Word,
    read: ReadAmount,
) -> Result<Vec<u8>, Fault> {
    unsafe {
        let w = ctx.handle_read(f, Handle::File, read, Form::Bytes)?;
        let read = (*ctx.heap().bytes(w).unwrap()).to_vec();
        ctx.heap_mut().decref(w).unwrap();
        Ok(read)
    }
}

/// バイト列の全部の読みは UTF-8 を確かめず、読んだ長さを数える。2回目は終わりから読むので空である。
fn a_borrowed_file_is_read_whole_as_bytes<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    memory.add(
        "bad.bin",
        MemoryFile {
            bytes: vec![0xff, 0xfe, 0],
            sized: true,
            ..MemoryFile::default()
        },
    );
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let f = open(&mut ctx, "bad.bin");
        let before = ctx.stats().string_bytes_copied;
        assert_eq!(
            read_bytes_borrowed(&mut ctx, f, ReadAmount::All).unwrap(),
            [0xff, 0xfe, 0]
        );
        if M::CHECKED {
            assert_eq!(ctx.stats().string_bytes_copied - before, 3);
        }
        assert_eq!(
            read_bytes_borrowed(&mut ctx, f, ReadAmount::All).unwrap(),
            []
        );
        assert_eq!(super::raw::rc(f), 1);
        ctx.handle_close(f, Handle::File).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// バイト列の行の読みは、64 KiB の補充をまたぐ行も1つのバイト列にし、行の長さだけを数える。UTF-8 でない行も読み、
/// 終わりでは空のバイト列を返す。
fn a_borrowed_file_is_read_in_byte_lines<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let long_line = vec![b'x'; 70_000];
    let mut bytes = long_line.clone();
    bytes.extend_from_slice(b"\n\xff\r\n\nlast");
    memory.add(
        "lines.bin",
        MemoryFile {
            bytes,
            chunk: 4096,
            ..MemoryFile::default()
        },
    );
    let mut ctx = ctx_with::<M>(&memory);
    let mut first = long_line;
    first.push(b'\n');
    unsafe {
        let f = open(&mut ctx, "lines.bin");
        for expected in [
            first,
            b"\xff\r\n".to_vec(),
            b"\n".to_vec(),
            b"last".to_vec(),
        ] {
            let before = ctx.stats().string_bytes_copied;
            assert_eq!(
                read_bytes_borrowed(&mut ctx, f, ReadAmount::Line).unwrap(),
                expected
            );
            if M::CHECKED {
                assert_eq!(
                    ctx.stats().string_bytes_copied - before,
                    expected.len() as u64
                );
            }
        }
        for _ in 0..2 {
            assert_eq!(
                read_bytes_borrowed(&mut ctx, f, ReadAmount::Line).unwrap(),
                []
            );
        }
        assert_eq!(super::raw::rc(f), 1);
        ctx.handle_close(f, Handle::File).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// バイト列の行の読みが途中で誤るか止まると、途中まで読んだ行を手放し、ハンドルは呼ぶ側に残る。
fn a_failed_byte_line_read_releases_the_partial_line<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let control = Arc::new(Control::new(Limits::default()));
    memory.add(
        "broken.txt",
        MemoryFile {
            fail_after: Some(3),
            chunk: 2,
            ..MemoryFile::text("abcdef")
        },
    );
    memory.add(
        "slow.txt",
        MemoryFile {
            chunk: 2,
            ..MemoryFile::text("abcdef\n")
        },
    );
    let mut ctx = crate::Ctx::<M>::new(control.clone(), host(&memory));
    unsafe {
        let f = open(&mut ctx, "broken.txt");
        assert_eq!(
            read_bytes_borrowed(&mut ctx, f, ReadAmount::Line),
            Err(Fault::Io {
                op: IoOp::Read,
                name: "broken.txt".into(),
                reason: IoReason::Other
            })
        );
        assert_eq!(super::raw::rc(f), 1);
        ctx.handle_close(f, Handle::File).unwrap();
        assert_eq!(mortal(ctx.heap()), 0);
        // 補充の前の確かめで止まり、読み手のバイトは次の読みに残る
        let f = open(&mut ctx, "slow.txt");
        control.interrupt();
        assert_eq!(
            read_bytes_borrowed(&mut ctx, f, ReadAmount::Line),
            Err(Fault::Stopped(Stop::Interrupted))
        );
        control.clear();
        assert_eq!(super::raw::rc(f), 1);
        assert_eq!(
            read_bytes_borrowed(&mut ctx, f, ReadAmount::Line).unwrap(),
            b"abcdef\n"
        );
        ctx.handle_close(f, Handle::File).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 塊の読みは、buffer に残ったバイトを、空なら1回の補充で読めたバイトを、受け取ったバイト列の後ろに足す。一意な
/// バイト列には足した長さだけを、共有のバイト列には写した全体を数える。終わりなら何も足さない。
fn chunks_are_appended_to_the_given_bytes<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    memory.add(
        "a.txt",
        MemoryFile {
            chunk: 4,
            ..MemoryFile::text("abcdef")
        },
    );
    let mut ctx = ctx_with::<M>(&memory);
    let chunk = |ctx: &mut crate::Ctx<M>, f: Word, buf: Word| unsafe {
        ctx.handle_read(f, Handle::File, ReadAmount::Chunk(buf), Form::Bytes)
    };
    let body = |ctx: &crate::Ctx<M>, w: Word| unsafe { (*ctx.heap().bytes(w).unwrap()).to_vec() };
    unsafe {
        let f = open(&mut ctx, "a.txt");
        let buf = ctx.heap_mut().alloc_bytes_room(16);
        let w = ctx.heap_mut().bytes_extend(buf, b">").unwrap();
        let before = ctx.stats().string_bytes_copied;
        let w = chunk(&mut ctx, f, w).unwrap();
        assert_eq!(body(&ctx, w), b">abcd");
        if M::CHECKED {
            assert_eq!(ctx.stats().string_bytes_copied - before, 4);
        }
        // 共有のバイト列は写してから足す
        ctx.heap_mut().dup(w).unwrap();
        let before = ctx.stats().string_bytes_copied;
        let x = chunk(&mut ctx, f, w).unwrap();
        assert_ne!(x, w);
        assert_eq!(body(&ctx, x), b">abcdef");
        assert_eq!(body(&ctx, w), b">abcd");
        if M::CHECKED {
            assert_eq!(ctx.stats().string_bytes_copied - before, 7);
        }
        ctx.heap_mut().decref(w).unwrap();
        let y = chunk(&mut ctx, f, x).unwrap();
        assert_eq!(body(&ctx, y), b">abcdef");
        ctx.heap_mut().decref(y).unwrap();
        assert_eq!(super::raw::rc(f), 1);
        ctx.handle_close(f, Handle::File).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 塊の読みが誤ると、受け取ったバイト列を手放し、ハンドルは呼ぶ側に残る。検査モードは、バイト列でない `buf` を
/// 読み手のバイトを使う前に断る。
fn a_failed_chunk_read_releases_the_bytes<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    memory.add(
        "broken.txt",
        MemoryFile {
            fail_after: Some(0),
            ..MemoryFile::text("ab")
        },
    );
    memory.add("a.txt", MemoryFile::text("ab"));
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let f = open(&mut ctx, "broken.txt");
        let buf = ctx.heap_mut().alloc_bytes(b"x");
        assert_eq!(
            ctx.handle_read(f, Handle::File, ReadAmount::Chunk(buf), Form::Bytes),
            Err(Fault::Io {
                op: IoOp::Read,
                name: "broken.txt".into(),
                reason: IoReason::Other
            })
        );
        assert_eq!(super::raw::rc(f), 1);
        ctx.handle_close(f, Handle::File).unwrap();
        assert_eq!(mortal(ctx.heap()), 0);
        if M::CHECKED {
            let f = open(&mut ctx, "a.txt");
            let not_bytes = s(&mut ctx, "x");
            assert_eq!(
                ctx.handle_read(f, Handle::File, ReadAmount::Chunk(not_bytes), Form::Bytes),
                Err(Fault::Heap(HeapError::WrongKind))
            );
            ctx.heap_mut().decref(not_bytes).unwrap();
            assert_eq!(read_borrowed(&mut ctx, f, ReadAmount::All).unwrap(), "ab");
            ctx.handle_close(f, Handle::File).unwrap();
        }
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 表の場合を `eml_rt` の extern で動かすときの引数。共有した文字列の残った参照と、作った不死の物体の数を持つ
#[derive(Default)]
struct Args {
    release: Vec<Word>,
    immortals: usize,
}

impl Args {
    fn int(arg: &Arg) -> i64 {
        match arg {
            Arg::Int(v) => *v,
            Arg::Bool(true) => TRUE,
            Arg::Bool(false) => FALSE,
            _ => panic!("not a scalar argument: {arg:?}"),
        }
    }

    fn float(arg: &Arg) -> f64 {
        match arg {
            Arg::Float(bits) => f64::from_bits(*bits),
            _ => panic!("not a float argument: {arg:?}"),
        }
    }

    /// 文字列の引数の参照を1つ作る。定数は VM の `const` と同じく不死の物体にする
    unsafe fn string<M: Mode>(&mut self, ctx: &mut crate::Ctx<M>, arg: &Arg, root: &Path) -> Word {
        unsafe {
            match arg {
                Arg::Const(x) | Arg::Rel(x) => {
                    let w = ctx.heap_mut().alloc_string_immortal(x);
                    ctx.heap_mut().acquire_immortal(w).unwrap();
                    self.immortals += 1;
                    w
                }
                Arg::Abs(x) => {
                    let path = Arg::Const(root.join(x).to_str().unwrap().to_string());
                    self.string(ctx, &path, root)
                }
                Arg::Unique(x) => s(ctx, x),
                Arg::Shared(x) => {
                    let w = s(ctx, x);
                    ctx.heap_mut().dup(w).unwrap();
                    self.release.push(w);
                    w
                }
                _ => panic!("not a string argument: {arg:?}"),
            }
        }
    }

    /// バイト列の引数の参照を1つ作る。共有したものは `dup` し、残った参照は結果を読んだ後で手放す
    unsafe fn bytes<M: Mode>(&mut self, ctx: &mut crate::Ctx<M>, arg: &Arg) -> Word {
        unsafe {
            match arg {
                Arg::Bytes(b) => ctx.heap_mut().alloc_bytes(b),
                Arg::SharedBytes(b) => {
                    let w = ctx.heap_mut().alloc_bytes(b);
                    ctx.heap_mut().dup(w).unwrap();
                    self.release.push(w);
                    w
                }
                _ => panic!("not a bytes argument: {arg:?}"),
            }
        }
    }

    /// 借りて渡すバイト列の引数。呼んだ側の参照は結果を読んだ後で手放す
    unsafe fn lent_bytes<M: Mode>(&mut self, ctx: &mut crate::Ctx<M>, arg: &Arg) -> Word {
        let w = unsafe { self.bytes(ctx, arg) };
        self.release.push(w);
        w
    }

    /// 配列の引数の参照を1つ作る。共有した配列は `dup` し、残った参照は結果を読んだ後で手放す
    unsafe fn array<M: Mode>(&mut self, ctx: &mut crate::Ctx<M>, arg: &Arg) -> Word {
        let Arg::Array {
            elements,
            lin,
            shared,
        } = arg
        else {
            panic!("not an array argument: {arg:?}");
        };
        unsafe {
            let mut w = ctx
                .array_make(elements.len() as i64, Word::imm(0), *lin)
                .unwrap();
            for (i, &e) in elements.iter().enumerate() {
                w = ctx.array_set(w, i as i64, Word::imm(e)).unwrap();
            }
            if *shared {
                ctx.heap_mut().dup(w).unwrap();
                self.release.push(w);
            }
            w
        }
    }

    /// 借りて渡す文字列の引数。呼んだ側の参照は結果を読んだ後で手放す
    unsafe fn lent_string<M: Mode>(
        &mut self,
        ctx: &mut crate::Ctx<M>,
        arg: &Arg,
        root: &Path,
    ) -> Word {
        let w = unsafe { self.string(ctx, arg, root) };
        self.release.push(w);
        w
    }

    /// 借りて渡す配列の引数。呼んだ側の参照は結果を読んだ後で手放す
    unsafe fn lent_array<M: Mode>(&mut self, ctx: &mut crate::Ctx<M>, arg: &Arg) -> Word {
        let w = unsafe { self.array(ctx, arg) };
        self.release.push(w);
        w
    }
}

/// 配列の結果を読み、手放す。要素は整数の即値である
unsafe fn take_array<M: Mode>(ctx: &mut crate::Ctx<M>, w: Word) -> Outcome {
    unsafe {
        let lin = ctx.heap().meta(w).unwrap().lin();
        let len = ctx.array_length(w).unwrap();
        let elements = (0..len)
            .map(|i| ctx.array_get(w, i).unwrap().imm_value())
            .collect();
        ctx.heap_mut().decref(w).unwrap();
        Outcome::Array { elements, lin }
    }
}

fn tag_bool(tag: i64) -> Outcome {
    match tag {
        TRUE => Outcome::Bool(true),
        FALSE => Outcome::Bool(false),
        _ => panic!("not a Bool tag: {tag}"),
    }
}

fn tag_order(tag: i64) -> Outcome {
    Outcome::Order(match tag {
        LT => "LT",
        EQ => "EQ",
        GT => "GT",
        _ => panic!("not an Ordering tag: {tag}"),
    })
}

/// バイト列の結果を読み、手放す
unsafe fn take_bytes<M: Mode>(ctx: &mut crate::Ctx<M>, w: Word) -> Outcome {
    let read = unsafe { &*ctx.heap().bytes(w).unwrap() }.to_vec();
    unsafe { ctx.heap_mut().decref(w) }.unwrap();
    Outcome::Bytes(read)
}

/// 文字列の結果を読み、手放す
unsafe fn take_text<M: Mode>(ctx: &mut crate::Ctx<M>, w: Word) -> Outcome {
    let read = text(ctx.heap(), w);
    unsafe { ctx.heap_mut().decref(w) }.unwrap();
    Outcome::Str(read)
}

/// 表の1つの場合の extern を呼び、結果を表の形に戻す
unsafe fn call<M: Mode>(
    ctx: &mut crate::Ctx<M>,
    name: &str,
    inputs: &[Arg],
    args: &mut Args,
    root: &Path,
    out: &Shared,
) -> Result<Outcome, Fault> {
    let int = |i: usize| Args::int(&inputs[i]);
    let arith = |f: fn(i64, i64) -> Result<i64, ArithFault>| -> Result<Outcome, Fault> {
        Ok(Outcome::Int(f(int(0), int(1))?))
    };
    let compare = |f: fn(i64, i64) -> i64| tag_bool(f(int(0), int(1)));
    let float = |i: usize| Args::float(&inputs[i]);
    let float_arith =
        |f: fn(f64, f64) -> f64| Outcome::Float(table::float_outcome(f(float(0), float(1))));
    let float_test = |f: fn(f64, f64) -> i64| tag_bool(f(float(0), float(1)));
    let float_result = |x: f64| Outcome::Float(table::float_outcome(x));
    unsafe {
        let one = |ctx: &mut crate::Ctx<M>, args: &mut Args| args.string(ctx, &inputs[0], root);
        let two = |ctx: &mut crate::Ctx<M>, args: &mut Args| {
            let a = args.string(ctx, &inputs[0], root);
            (a, args.string(ctx, &inputs[1], root))
        };
        let lent_one =
            |ctx: &mut crate::Ctx<M>, args: &mut Args| args.lent_string(ctx, &inputs[0], root);
        let lent_two = |ctx: &mut crate::Ctx<M>, args: &mut Args| {
            let a = args.lent_string(ctx, &inputs[0], root);
            (a, args.lent_string(ctx, &inputs[1], root))
        };
        Ok(match name {
            "Prelude.Num Int.+" => arith(int_add)?,
            "Prelude.Num Int.-" => arith(int_sub)?,
            "Prelude.Num Int.*" => arith(int_mul)?,
            "Prelude.Div Int./" => arith(int_div)?,
            "Prelude.Div Int.%" => arith(int_rem)?,
            "Prelude.Num Byte.+" => arith(byte_add)?,
            "Prelude.Num Byte.-" => arith(byte_sub)?,
            "Prelude.Num Byte.*" => arith(byte_mul)?,
            "Prelude.Div Byte./" => arith(int_div)?,
            "Prelude.Div Byte.%" => arith(int_rem)?,
            "Prelude.Num Byte.negate" => Outcome::Int(byte_negate(int(0))?),
            "Prelude.Bits Int.&&&" | "Prelude.Bits Byte.&&&" => {
                Outcome::Int(int_and(int(0), int(1)))
            }
            "Prelude.Bits Int.|||" | "Prelude.Bits Byte.|||" => {
                Outcome::Int(int_or(int(0), int(1)))
            }
            "Prelude.Bits Int.^^^" | "Prelude.Bits Byte.^^^" => {
                Outcome::Int(int_xor(int(0), int(1)))
            }
            "Prelude.Bits Int.complement" => Outcome::Int(int_complement(int(0))),
            "Prelude.Bits Byte.complement" => Outcome::Int(byte_complement(int(0))),
            "Prelude.Bits Int.<<<" => Outcome::Int(int_shl(int(0), int(1))),
            "Prelude.Bits Int.>>>" => Outcome::Int(int_shr(int(0), int(1))),
            "Prelude.Bits Byte.<<<" => Outcome::Int(byte_shl(int(0), int(1))),
            "Prelude.Bits Byte.>>>" => Outcome::Int(byte_shr(int(0), int(1))),
            "Prelude.Num Int.negate" => Outcome::Int(int_negate(int(0))?),
            "Prelude.check_index" => {
                check_index(int(0), int(1))?;
                Outcome::Unit
            }
            "Prelude.Eq Int.==" => compare(int_eq),
            "Prelude.Eq Int.!=" => compare(int_ne),
            "Prelude.Ord Int.<" => compare(int_lt),
            "Prelude.Ord Int.<=" => compare(int_le),
            "Prelude.Ord Int.>" => compare(int_gt),
            "Prelude.Ord Int.>=" => compare(int_ge),
            "Prelude.Ord Int.compare" => tag_order(int_compare(int(0), int(1))),
            "Prelude.Eq Char.==" => compare(int_eq),
            "Prelude.Eq Char.!=" => compare(int_ne),
            "Prelude.Ord Char.<" => compare(int_lt),
            "Prelude.Ord Char.<=" => compare(int_le),
            "Prelude.Ord Char.>" => compare(int_gt),
            "Prelude.Ord Char.>=" => compare(int_ge),
            "Prelude.Ord Char.compare" => tag_order(int_compare(int(0), int(1))),
            "Prelude.Eq Byte.==" => compare(int_eq),
            "Prelude.Eq Byte.!=" => compare(int_ne),
            "Prelude.Ord Byte.<" => compare(int_lt),
            "Prelude.Ord Byte.<=" => compare(int_le),
            "Prelude.Ord Byte.>" => compare(int_gt),
            "Prelude.Ord Byte.>=" => compare(int_ge),
            "Prelude.Ord Byte.compare" => tag_order(int_compare(int(0), int(1))),
            "Prelude.Num Float.+" => float_arith(float_add),
            "Prelude.Num Float.-" => float_arith(float_sub),
            "Prelude.Num Float.*" => float_arith(float_mul),
            "Prelude.Div Float./" => float_arith(float_div),
            "Prelude.Div Float.%" => float_arith(float_rem),
            "Prelude.Num Float.negate" => {
                Outcome::Float(table::float_outcome(float_negate(float(0))))
            }
            "Prelude.Eq Float.==" => float_test(float_eq),
            "Prelude.Eq Float.!=" => float_test(float_ne),
            "Prelude.Ord Float.<" => float_test(float_lt),
            "Prelude.Ord Float.<=" => float_test(float_le),
            "Prelude.Ord Float.>" => float_test(float_gt),
            "Prelude.Ord Float.>=" => float_test(float_ge),
            "Prelude.Ord Float.compare" => tag_order(float_compare(float(0), float(1))),
            "Prelude.Show Float.show" => {
                let w = ctx.float_show(float(0));
                take_text(ctx, w)
            }
            "Prelude.shows_minus" => tag_bool(float_shows_minus(float(0))),
            "Std.Float.of_int" => float_result(float_of_int(int(0))),
            "Std.Char.to_int" => Outcome::Int(int(0)),
            "Std.Char.of_int" => Outcome::Int(char_of_int(int(0))?),
            "Std.Byte.to_int" => Outcome::Int(int(0)),
            "Std.Byte.of_int" => Outcome::Int(byte_of_int(int(0))?),
            "Std.Byte.wrap" => Outcome::Int(byte_wrap(int(0))),
            "Std.Float.truncate" => Outcome::Int(float_truncate(float(0))?),
            "Std.Float.floor" => Outcome::Int(float_floor(float(0))?),
            "Std.Float.ceil" => Outcome::Int(float_ceil(float(0))?),
            "Std.Float.round" => Outcome::Int(float_round(float(0))?),
            "Std.Float.sqrt" => float_result(float_sqrt(float(0))),
            "Std.Float.abs" => float_result(float_abs(float(0))),
            "Std.Float.sin" => float_result(float_sin(float(0))),
            "Std.Float.cos" => float_result(float_cos(float(0))),
            "Std.Float.exp" => float_result(float_exp(float(0))),
            "Std.Float.log" => float_result(float_log(float(0))),
            "Std.Float.pow" => float_arith(float_pow),
            "Std.Float.infinity_value" => float_result(float_infinity()),
            "Std.Float.nan_value" => float_result(float_nan()),
            "Std.Float.to_fixed" => {
                let w = ctx.float_to_fixed(int(0), float(1))?;
                take_text(ctx, w)
            }
            "Std.Float.can_parse" => {
                let a = lent_one(ctx, args);
                Outcome::Bool(ctx.string_to_float(a)?.is_some())
            }
            "Std.Float.parse_raw" => {
                let a = lent_one(ctx, args);
                float_result(ctx.string_to_float(a)?.unwrap_or(f64::NAN))
            }
            "Prelude.Eq Bool.==" => compare(bool_eq),
            "Prelude.Eq Bool.!=" => compare(bool_ne),
            "Prelude.Show Char.show" => {
                let w = ctx.char_show(int(0));
                take_text(ctx, w)
            }
            "Prelude.Show Char.display" => {
                let w = ctx.char_display(int(0));
                take_text(ctx, w)
            }
            "Prelude.Show Int.show" | "Prelude.Show Byte.display" => {
                let w = ctx.int_show(int(0));
                take_text(ctx, w)
            }
            "Prelude.Eq String.==" => {
                let (a, b) = lent_two(ctx, args);
                tag_bool(ctx.string_eq(a, b)?)
            }
            "Prelude.Eq String.!=" => {
                let (a, b) = lent_two(ctx, args);
                tag_bool(ctx.string_ne(a, b)?)
            }
            "Prelude.Ord String.compare" => {
                let (a, b) = lent_two(ctx, args);
                tag_order(ctx.string_compare(a, b)?)
            }
            "Prelude.++" => {
                let (a, b) = two(ctx, args);
                let w = ctx.string_append(a, b)?;
                take_text(ctx, w)
            }
            "Prelude.Show String.show" => {
                let a = lent_one(ctx, args);
                let w = ctx.string_show(a)?;
                take_text(ctx, w)
            }
            "Prelude.println" => {
                let a = one(ctx, args);
                ctx.write_string(crate::Stream::Out, a)?;
                ctx.flush()?;
                Outcome::Out(out.text())
            }
            "Std.Fs.read_all"
            | "Std.Fs.read_all2"
            | "Std.Fs.read_line_raw"
            | "Std.Fs.read_line_raw2" => {
                let amount = if name.starts_with("Std.Fs.read_all") {
                    ReadAmount::All
                } else {
                    ReadAmount::Line
                };
                let p = one(ctx, args);
                let f = ctx.fs_open(p)?;
                // 読みは File を借りるので、誤りでも閉じてから返す
                let mut read = read_borrowed(ctx, f, amount);
                if name.ends_with('2') && read.is_ok() {
                    read = read_borrowed(ctx, f, amount);
                }
                ctx.handle_close(f, Handle::File)?;
                Outcome::Str(read?)
            }
            "Std.String.byte_length" => {
                let a = lent_one(ctx, args);
                Outcome::Int(ctx.string_byte_length(a)?)
            }
            "Std.String.byte_at" => {
                let a = args.lent_string(ctx, &inputs[1], root);
                Outcome::Int(ctx.string_byte_at(int(0), a)?)
            }
            "Std.String.slice" => {
                let a = args.lent_string(ctx, &inputs[2], root);
                let w = ctx.string_slice(int(0), int(1), a)?;
                take_text(ctx, w)
            }
            "Std.String.find_from" => {
                let n = args.lent_string(ctx, &inputs[0], root);
                let a = args.lent_string(ctx, &inputs[2], root);
                Outcome::Int(ctx.string_find_from(n, int(1), a)?)
            }
            "Std.Bytes.empty" => {
                let w = ctx.bytes_empty()?;
                take_bytes(ctx, w)
            }
            "Std.Bytes.length" => {
                let a = args.lent_bytes(ctx, &inputs[0]);
                Outcome::Int(ctx.bytes_length(a)?)
            }
            "Std.Bytes.append" => {
                let a = args.bytes(ctx, &inputs[0]);
                let b = args.bytes(ctx, &inputs[1]);
                let w = ctx.bytes_append(a, b)?;
                take_bytes(ctx, w)
            }
            "Std.Bytes.append_slice" => {
                let src = args.lent_bytes(ctx, &inputs[2]);
                let dst = args.bytes(ctx, &inputs[3]);
                let w = ctx.bytes_append_part(int(0), int(1), src, dst)?;
                take_bytes(ctx, w)
            }
            "Std.Bytes.slice" => {
                let a = args.lent_bytes(ctx, &inputs[2]);
                let w = ctx.bytes_slice(int(0), int(1), a)?;
                take_bytes(ctx, w)
            }
            "Std.Bytes.truncate" => {
                let a = args.bytes(ctx, &inputs[1]);
                let w = ctx.bytes_truncate(int(0), a)?;
                take_bytes(ctx, w)
            }
            "Std.Bytes.of_string" => {
                let a = one(ctx, args);
                let w = ctx.bytes_of_string(a)?;
                take_bytes(ctx, w)
            }
            "Prelude.Eq Bytes.==" => {
                let a = args.lent_bytes(ctx, &inputs[0]);
                let b = args.lent_bytes(ctx, &inputs[1]);
                tag_bool(ctx.bytes_eq(a, b)?)
            }
            "Prelude.Eq Bytes.!=" => {
                let a = args.lent_bytes(ctx, &inputs[0]);
                let b = args.lent_bytes(ctx, &inputs[1]);
                tag_bool(ctx.bytes_ne(a, b)?)
            }
            "Prelude.Ord Bytes.compare" => {
                let a = args.lent_bytes(ctx, &inputs[0]);
                let b = args.lent_bytes(ctx, &inputs[1]);
                tag_order(ctx.bytes_compare(a, b)?)
            }
            "Std.String.valid_utf8" => {
                let a = args.lent_bytes(ctx, &inputs[0]);
                tag_bool(ctx.bytes_valid_utf8(a)?)
            }
            "Std.String.decode" => {
                let a = args.bytes(ctx, &inputs[0]);
                let w = ctx.string_decode(a)?;
                take_text(ctx, w)
            }
            "Std.Bytes.make" => {
                let w = ctx.bytes_make(int(0), int(1))?;
                take_bytes(ctx, w)
            }
            "Std.Bytes.get" => {
                let a = args.lent_bytes(ctx, &inputs[1]);
                Outcome::Int(ctx.bytes_get(int(0), a)?)
            }
            "Prelude.bytes_length" => {
                let a = args.lent_bytes(ctx, &inputs[0]);
                Outcome::Int(ctx.bytes_length(a)?)
            }
            "Prelude.bytes_get" => {
                let a = args.lent_bytes(ctx, &inputs[1]);
                Outcome::Int(ctx.bytes_get(int(0), a)?)
            }
            "Std.Bytes.set" => {
                let a = args.bytes(ctx, &inputs[2]);
                let w = ctx.bytes_set(int(0), int(1), a)?;
                take_bytes(ctx, w)
            }
            "Std.Bytes.push" => {
                let a = args.bytes(ctx, &inputs[1]);
                let w = ctx.bytes_push(int(0), a)?;
                take_bytes(ctx, w)
            }
            "Std.Bytes.find_byte_from" => {
                let a = args.lent_bytes(ctx, &inputs[2]);
                Outcome::Int(ctx.bytes_find_byte(int(0), int(1), a)?)
            }
            "Std.Bytes.find_from" => {
                let n = args.lent_bytes(ctx, &inputs[0]);
                let a = args.lent_bytes(ctx, &inputs[2]);
                Outcome::Int(ctx.bytes_find_from(n, int(1), a)?)
            }
            "Std.Array.make" | "Std.MutArray.make" => {
                let lin = name == "Std.MutArray.make";
                let w = ctx.array_make(int(0), Word::imm(int(1)), lin)?;
                take_array(ctx, w)
            }
            "Std.MutArray.empty" => {
                let w = ctx.array_empty(true)?;
                take_array(ctx, w)
            }
            "Std.Array.length" | "Std.MutArray.length" => {
                let a = args.lent_array(ctx, &inputs[0]);
                Outcome::Int(ctx.array_length(a)?)
            }
            "Std.Array.get" | "Std.MutArray.read" => {
                let a = args.lent_array(ctx, &inputs[0]);
                Outcome::Int(ctx.array_get(a, int(1))?.imm_value())
            }
            "Std.Array.set" | "Std.MutArray.write" => {
                let a = args.array(ctx, &inputs[0]);
                let w = ctx.array_set(a, int(1), Word::imm(int(2)))?;
                take_array(ctx, w)
            }
            "Std.Array.thaw" => {
                let a = args.array(ctx, &inputs[0]);
                let w = ctx.array_thaw(a)?;
                take_array(ctx, w)
            }
            "Std.MutArray.freeze" => {
                let a = args.array(ctx, &inputs[0]);
                let w = ctx.array_freeze(a)?;
                take_array(ctx, w)
            }
            name => panic!("an extern the table does not know: {name}"),
        })
    }
}

/// 表の1つの場合を新しい `Ctx` で動かし、結果を表の形に戻す。引数の定数の不死の物体のほかに、物体を残さないことも
/// 確かめる
fn outcome<M: Mode>(name: &str, inputs: &[Arg], root: &Path) -> Outcome {
    let out = Shared::default();
    let host = StdHost::new(
        Box::new(out.clone()),
        Box::new(io::sink()),
        root.to_path_buf(),
        Buffering::Line,
    );
    let mut ctx = crate::Ctx::<M>::new(
        Arc::new(Control::new(Limits::default())),
        HostRef::new(Arc::new(host)),
    );
    let mut args = Args::default();
    let got = unsafe { call(&mut ctx, name, inputs, &mut args, root, &out) }
        .unwrap_or_else(|fault| Outcome::Err(fault.to_string()));
    for w in args.release {
        unsafe { ctx.heap_mut().decref(w) }.unwrap();
    }
    assert_eq!(mortal(ctx.heap()), args.immortals, "{name} {inputs:?}");
    got
}

/// `eml_rt` の extern が、期待値の表のどの場合も同じ結果を返し、物体を残さない。表の入力は `table::inputs` と同じで、
/// 入力を変えて表を書き直していなければ落ちる
fn the_externs_match_the_table<M: Mode>() {
    let scratch = Scratch::new(&format!("eml_rt-table-{}", M::CHECKED));
    let root = scratch.path();
    let locked = table::fixtures(root);
    let cases = table::cases();
    let written: Vec<(String, Vec<Arg>)> = cases
        .iter()
        .map(|c| (c.name.clone(), c.args.clone()))
        .collect();
    let inputs = table::inputs();
    assert!(
        written == inputs,
        "expected.txt is not written from the current inputs ({} cases, {} inputs)",
        written.len(),
        inputs.len()
    );
    for case in cases {
        if !locked
            && case
                .args
                .iter()
                .any(|a| matches!(a, Arg::Rel(p) if p == "locked.txt"))
        {
            continue;
        }
        assert_eq!(
            table::digest(outcome::<M>(&case.name, &case.args, root)),
            case.outcome,
            "{}",
            table::line(&case)
        );
    }
}

/// 表を書く。行を足すか入力を変えたときだけ、`cargo test -p eml_rt the_table_can_be_written -- --ignored` で流し、
/// 差分の結果の欄を人が確かめる (docs/implementation/testing.md の「期待値の表」)。結果は検査モードで作る
#[test]
#[ignore = "writes expected.txt"]
fn the_table_can_be_written() {
    let scratch = Scratch::new("eml_rt-table-write");
    let root = scratch.path();
    assert!(
        table::fixtures(root),
        "cannot make an unreadable locked.txt (running as root?), so its row would be wrong"
    );
    let mut text =
        String::from("# eml_rt の extern の期待値の表。eml_rt の検査モードの結果から作った\n");
    for (name, args) in table::inputs() {
        let outcome = outcome::<crate::Checked>(&name, &args, root);
        text.push_str(&table::line(&Case {
            name,
            args,
            outcome,
        }));
        text.push('\n');
    }
    std::fs::write(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/heap/tests/expected.txt"),
        text,
    )
    .unwrap();
}

both_modes!(
    strings_compare_by_bytes,
    the_same_string_on_both_sides,
    appending_counts_the_bytes_it_copies,
    ints_show_in_decimal,
    chars_display_as_utf8,
    shows_count_the_bytes_they_write,
    strings_show_with_their_escapes,
    a_string_extern_refuses_a_non_string,
    the_string_core_reads_by_byte_positions,
    a_slice_counts_the_bytes_it_copies,
    the_string_core_refuses_a_non_string,
    println_writes_a_line_and_releases_its_text,
    a_failed_write_still_releases_the_text,
    println_refuses_a_non_string,
    copy_string_copies_up_to_a_number_of_chars,
    copy_string_reads_a_region_string,
    copy_string_refuses_a_non_string,
    print_line_writes_and_flushes,
    a_missing_file_is_a_fault_naming_its_path,
    a_borrowed_file_is_read_whole_and_closed,
    an_empty_borrowed_file_reads_as_an_empty_string,
    a_long_unsized_borrowed_file_is_read_in_chunks,
    an_interrupted_borrowed_read_is_retried,
    a_failed_borrowed_read_keeps_the_file,
    a_borrowed_read_stops_on_an_interrupt,
    an_interrupted_borrowed_read_stops_when_the_stop_word_is_set,
    a_borrowed_read_past_the_memory_limit_stops,
    a_borrowed_read_refuses_a_non_file_or_a_shared_file,
    a_borrowed_file_is_read_line_by_line,
    a_failed_borrowed_line_read_keeps_the_file,
    a_borrowed_file_is_read_whole_as_bytes,
    a_borrowed_file_is_read_in_byte_lines,
    a_failed_byte_line_read_releases_the_partial_line,
    chunks_are_appended_to_the_given_bytes,
    a_failed_chunk_read_releases_the_bytes,
    the_externs_match_the_table,
);
