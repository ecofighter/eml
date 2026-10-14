//! extern の単体テスト。結果は spec のとおりで、今の実装と同じ文言で誤りを返すことを確かめる。

use std::cmp::Ordering as Order;

use crate::externs::*;
use crate::{HeapError, Stop};

const EDGES: [i64; 13] = [
    0,
    1,
    -1,
    2,
    -2,
    i64::MIN,
    i64::MIN + 1,
    i64::MAX,
    i64::MAX - 1,
    1 << 31,
    -(1 << 31),
    1 << 62,
    -(1 << 62),
];

fn ordering(o: Order) -> i64 {
    match o {
        Order::Less => LT,
        Order::Equal => EQ,
        Order::Greater => GT,
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

/// 文言は今の `eml_interp::Fault` と同じである (S10 で切り替えても UI テストの出力を変えないため)。
#[test]
fn faults_print_as_the_interpreter_does() {
    let cases: [(Fault, &str); 9] = [
        (ArithFault::DivisionByZero.into(), "division by zero"),
        (ArithFault::IntegerOverflow.into(), "integer overflow"),
        (
            Fault::Output("disk full".into()),
            "cannot write the output: disk full",
        ),
        (
            Fault::Io {
                op: IoOp::Open,
                path: "a.txt".into(),
                reason: IoReason::NotFound,
            },
            "cannot open `a.txt`: not found",
        ),
        (
            Fault::Io {
                op: IoOp::Open,
                path: "p".into(),
                reason: IoReason::PermissionDenied,
            },
            "cannot open `p`: permission denied",
        ),
        (
            Fault::Io {
                op: IoOp::Read,
                path: "d".into(),
                reason: IoReason::Other,
            },
            "cannot read `d`: I/O error",
        ),
        (
            Fault::NotUtf8 {
                path: "b.bin".into(),
            },
            "`b.bin` is not valid UTF-8",
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

/// 算術の誤りは2つのレジスタで返り、`Fault` は 24 バイトに収まる。
#[test]
fn faults_stay_small() {
    assert_eq!(std::mem::size_of::<Result<i64, ArithFault>>(), 16);
    assert_eq!(std::mem::size_of::<Fault>(), 24);
}
