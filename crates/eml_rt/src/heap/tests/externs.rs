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

/// 文言は docs/spec/lir.md の「実行時エラー」の一覧のとおりである。
#[test]
fn faults_print_their_listed_texts() {
    let cases: [(Fault, &str); 19] = [
        (ArithFault::DivisionByZero.into(), "division by zero"),
        (ArithFault::IntegerOverflow.into(), "integer overflow"),
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
            Fault::NotUtf8(Box::new(Utf8Source::Arg(2))),
            "argument 2 is not valid UTF-8",
        ),
        (
            Fault::NotUtf8(Box::new(Utf8Source::EnvVar("LANG".into()))),
            "environment variable `LANG` is not valid UTF-8",
        ),
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
                let (a, b) = (s(&mut ctx, x), s(&mut ctx, y));
                assert_eq!(ctx.string_ne(a, b), Ok((x != y) as i64));
                let (a, b) = (s(&mut ctx, x), s(&mut ctx, y));
                assert_eq!(
                    ctx.string_compare(a, b),
                    Ok(ordering(x.as_bytes().cmp(y.as_bytes())))
                );
            }
        }
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 呼ぶ側は同じ文字列の参照を2つ持って渡す。両方を手放しても正しく動く。
fn the_same_string_on_both_sides<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let a = s(&mut ctx, "abc");
        ctx.heap_mut().dup(a).unwrap();
        assert_eq!(ctx.string_eq(a, a), Ok(TRUE));
        let a = s(&mut ctx, "abc");
        ctx.heap_mut().dup(a).unwrap();
        assert_eq!(ctx.string_compare(a, a), Ok(EQ));
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
        }
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 文字列の核はバイトの位置で読み、範囲の外と文字の境界を断る。断っても引数は手放す (docs/spec/stdlib.md)。
fn the_string_core_reads_by_byte_positions<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        let a = s(&mut ctx, "a日b");
        assert_eq!(ctx.string_byte_length(a), Ok(5));
        let a = s(&mut ctx, "a日b");
        assert_eq!(ctx.string_byte_at(1, a), Ok(0xe6));
        let a = s(&mut ctx, "a日b");
        assert_eq!(
            ctx.string_byte_at(5, a),
            Err(Fault::Index(Box::new(IndexFault::String {
                index: 5,
                len: 5
            })))
        );
        let a = s(&mut ctx, "a日b");
        let w = ctx.string_slice(1, 4, a).unwrap();
        assert_eq!(text(ctx.heap(), w), "日");
        ctx.heap_mut().decref(w).unwrap();
        let a = s(&mut ctx, "a日b");
        assert_eq!(
            ctx.string_slice(2, 4, a),
            Err(Fault::Index(Box::new(IndexFault::Boundary {
                start: 2,
                end: 4
            })))
        );
        let a = s(&mut ctx, "a日b");
        assert_eq!(
            ctx.string_slice(0, 6, a),
            Err(Fault::Index(Box::new(IndexFault::Slice {
                start: 0,
                end: 6,
                len: 5
            })))
        );
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

/// 開いて、全部読み、組から取り出す。
unsafe fn open_and_read<M: Mode>(
    ctx: &mut crate::Ctx<M>,
    path: &str,
) -> Result<(Word, String), Fault> {
    unsafe {
        let p = s(ctx, path);
        let f = ctx.fs_open(p)?;
        read_once(ctx, f)
    }
}

/// 全部読み、組から File と読んだ中身を取り出す。組は `lin` ビットを持ち、File の参照を持つ。
unsafe fn read_once<M: Mode>(ctx: &mut crate::Ctx<M>, f: Word) -> Result<(Word, String), Fault> {
    unsafe {
        let t = ctx.fs_read_all(f)?;
        assert!(ctx.heap().meta(t).unwrap().lin());
        let file = ctx.heap().field(t, 0).unwrap();
        let text_word = ctx.heap().field(t, 1).unwrap();
        let read = text(ctx.heap(), text_word);
        ctx.heap_mut()
            .release(t, crate::externs::TUPLE, &[true, true])
            .unwrap();
        ctx.heap_mut().decref(text_word).unwrap();
        Ok((file, read))
    }
}

fn a_file_is_read_whole_and_closed<M: Mode>() {
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
        let (file, read) = open_and_read(&mut ctx, "a.txt").unwrap();
        assert_eq!(read, "line 1\nline 2\n");
        // 2回目は終わりから読むので空である
        let again = ctx.fs_read_all(file).unwrap();
        let second = ctx.heap().field(again, 1).unwrap();
        assert_eq!(text(ctx.heap(), second), "");
        let file = ctx.heap().field(again, 0).unwrap();
        ctx.heap_mut()
            .release(again, crate::externs::TUPLE, &[true, true])
            .unwrap();
        ctx.heap_mut().decref(second).unwrap();
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        ctx.fs_close(file).unwrap();
        assert_eq!(drops.load(Ordering::Relaxed), 1);
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

fn an_empty_file_reads_as_an_empty_string<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    memory.add("empty.txt", MemoryFile::text(""));
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let (file, read) = open_and_read(&mut ctx, "empty.txt").unwrap();
        assert_eq!(read, "");
        ctx.fs_close(file).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 長さの見込みがなく、短い読みしか返さない読み手でも、64 KiB の区切りをまたいで全部読む。
fn a_long_unsized_file_is_read_in_chunks<M: Mode>() {
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
        let (file, read) = open_and_read(&mut ctx, "big.txt").unwrap();
        assert_eq!(read, body);
        ctx.fs_close(file).unwrap();
    }
    if M::CHECKED {
        assert_eq!(ctx.stats().string_bytes_copied, body.len() as u64);
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// シグナルで中断された読みは、誤りにせず読み直す。
fn an_interrupted_read_is_retried<M: Mode>() {
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
        let (file, read) = open_and_read(&mut ctx, "a.txt").unwrap();
        assert_eq!(read, "abc");
        ctx.fs_close(file).unwrap();
    }
    assert_eq!(mortal(ctx.heap()), 0);
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

/// 読みの誤りと UTF-8 でない中身は、File を手放して閉じる。作りかけの文字列も残らない。
fn a_failed_read_closes_the_file<M: Mode>() {
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
        let p = s(&mut ctx, "broken.txt");
        let f = ctx.fs_open(p).unwrap();
        assert_eq!(
            ctx.fs_read_all(f),
            Err(Fault::Io {
                op: IoOp::Read,
                name: "broken.txt".into(),
                reason: IoReason::Other
            })
        );
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        let p = s(&mut ctx, "bad.bin");
        let f = ctx.fs_open(p).unwrap();
        assert_eq!(
            ctx.fs_read_all(f),
            Err(Fault::NotUtf8(Box::new(Utf8Source::File("bad.bin".into()))))
        );
        assert_eq!(drops.load(Ordering::Relaxed), 2);
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 読む間に止める語を見る。割り込まれていれば止め、File を手放す。
fn a_read_stops_on_an_interrupt<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    memory.add("a.txt", MemoryFile::text("abc"));
    let control = Arc::new(Control::new(Limits::default()));
    let mut ctx = crate::Ctx::<M>::new(control.clone(), host(&memory));
    unsafe {
        let p = s(&mut ctx, "a.txt");
        let f = ctx.fs_open(p).unwrap();
        control.interrupt();
        assert_eq!(ctx.fs_read_all(f), Err(Fault::Stopped(Stop::Interrupted)));
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 読みが `EINTR` で戻ったとき、止める語が立っていれば読み直さずに止め、File を手放す。REPL の SIGINT のハンドラは
/// `SA_RESTART` を外して入れるので、待っている読みはこの形で戻る (docs/spec/object-model.md の「`File`」)。読み直すと
/// テストの読み手が panic するので、読みをすぐ止めなければテストが落ちる。
fn an_interrupted_read_stops_when_the_stop_word_is_set<M: Mode>() {
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
        let p = s(&mut ctx, "fifo");
        let f = ctx.fs_open(p).unwrap();
        assert_eq!(ctx.fs_read_all(f), Err(Fault::Stopped(Stop::Interrupted)));
    }
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert_eq!(mortal(ctx.heap()), 0);
}

/// ヒープの外の読みの領域も、確保量の上限に数える。長さの見込みがあれば確保する前に比べるので、確保できないほど
/// 大きい見込みも I/O の誤りでなく上限で止まる。見込みがなければ、1回読むごとに比べる。
fn a_read_past_the_memory_limit_stops<M: Mode>() {
    let body = "x".repeat(200_000);
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
        max_bytes: 150_000,
        ..Limits::default()
    }));
    let mut ctx = crate::Ctx::<M>::new(control, host(&memory));
    for path in ["big.txt", "huge.txt", "unsized.txt"] {
        unsafe {
            let p = s(&mut ctx, path);
            let f = ctx.fs_open(p).unwrap();
            assert_eq!(
                ctx.fs_read_all(f),
                Err(Fault::Stopped(Stop::MemoryLimit)),
                "{path}"
            );
        }
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// File でない物体と、参照が2つある File は読めない。断ったときはヒープを変えない。
fn read_all_refuses_a_non_file_or_a_shared_file<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    memory.add("a.txt", MemoryFile::text("abc"));
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let other = ctx.heap_mut().alloc_foreign(0u64, false);
        let objects = mortal(ctx.heap());
        assert_eq!(
            ctx.fs_read_all(other),
            Err(Fault::Heap(HeapError::WrongKind))
        );
        assert_eq!(mortal(ctx.heap()), objects);
        assert_eq!(ctx.fs_close(other), Err(Fault::Heap(HeapError::WrongKind)));
        assert_eq!(mortal(ctx.heap()), objects);
        ctx.heap_mut().decref(other).unwrap();
        if M::CHECKED {
            let p = s(&mut ctx, "a.txt");
            let f = ctx.fs_open(p).unwrap();
            super::raw::set_rc(ctx.heap_mut(), f, 2);
            let objects = mortal(ctx.heap());
            assert_eq!(ctx.fs_read_all(f), Err(Fault::Heap(HeapError::NotUnique)));
            assert_eq!(mortal(ctx.heap()), objects);
            super::raw::set_rc(ctx.heap_mut(), f, 1);
            ctx.fs_close(f).unwrap();
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
    unsafe {
        let one = |ctx: &mut crate::Ctx<M>, args: &mut Args| args.string(ctx, &inputs[0], root);
        let two = |ctx: &mut crate::Ctx<M>, args: &mut Args| {
            let a = args.string(ctx, &inputs[0], root);
            (a, args.string(ctx, &inputs[1], root))
        };
        Ok(match name {
            "Prelude.+" => arith(int_add)?,
            "Prelude.-" => arith(int_sub)?,
            "Prelude.*" => arith(int_mul)?,
            "Prelude./" => arith(int_div)?,
            "Prelude.%" => arith(int_rem)?,
            "Prelude.negate" => Outcome::Int(int_negate(int(0))?),
            "Prelude.Eq Int.==" => compare(int_eq),
            "Prelude.Eq Int.!=" => compare(int_ne),
            "Prelude.Ord Int.<" => compare(int_lt),
            "Prelude.Ord Int.<=" => compare(int_le),
            "Prelude.Ord Int.>" => compare(int_gt),
            "Prelude.Ord Int.>=" => compare(int_ge),
            "Prelude.Ord Int.compare" => tag_order(int_compare(int(0), int(1))),
            "Prelude.Eq Bool.==" => compare(bool_eq),
            "Prelude.Eq Bool.!=" => compare(bool_ne),
            "Prelude.Show Int.show" => {
                let w = ctx.int_show(int(0));
                take_text(ctx, w)
            }
            "Prelude.Eq String.==" => {
                let (a, b) = two(ctx, args);
                tag_bool(ctx.string_eq(a, b)?)
            }
            "Prelude.Eq String.!=" => {
                let (a, b) = two(ctx, args);
                tag_bool(ctx.string_ne(a, b)?)
            }
            "Prelude.Ord String.compare" => {
                let (a, b) = two(ctx, args);
                tag_order(ctx.string_compare(a, b)?)
            }
            "Prelude.++" => {
                let (a, b) = two(ctx, args);
                let w = ctx.string_append(a, b)?;
                take_text(ctx, w)
            }
            "Prelude.Show String.show" => {
                let a = one(ctx, args);
                let w = ctx.string_show(a)?;
                take_text(ctx, w)
            }
            "Prelude.println" => {
                let a = one(ctx, args);
                ctx.write_string(crate::Stream::Out, a)?;
                ctx.flush()?;
                Outcome::Out(out.text())
            }
            "Std.Fs.read_all" | "Std.Fs.read_all2" => {
                let p = one(ctx, args);
                let f = ctx.fs_open(p)?;
                let (mut f, mut read) = read_once(ctx, f)?;
                if name == "Std.Fs.read_all2" {
                    (f, read) = read_once(ctx, f)?;
                }
                ctx.fs_close(f)?;
                Outcome::Str(read)
            }
            "Std.String.byte_length" => {
                let a = one(ctx, args);
                Outcome::Int(ctx.string_byte_length(a)?)
            }
            "Std.String.byte_at" => {
                let a = args.string(ctx, &inputs[1], root);
                Outcome::Int(ctx.string_byte_at(int(0), a)?)
            }
            "Std.String.slice" => {
                let a = args.string(ctx, &inputs[2], root);
                let w = ctx.string_slice(int(0), int(1), a)?;
                take_text(ctx, w)
            }
            "Std.String.find_from" => {
                let n = args.string(ctx, &inputs[0], root);
                let a = args.string(ctx, &inputs[2], root);
                Outcome::Int(ctx.string_find_from(n, int(1), a)?)
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
    a_file_is_read_whole_and_closed,
    an_empty_file_reads_as_an_empty_string,
    a_long_unsized_file_is_read_in_chunks,
    an_interrupted_read_is_retried,
    a_missing_file_is_a_fault_naming_its_path,
    a_failed_read_closes_the_file,
    a_read_stops_on_an_interrupt,
    an_interrupted_read_stops_when_the_stop_word_is_set,
    a_read_past_the_memory_limit_stops,
    read_all_refuses_a_non_file_or_a_shared_file,
    the_externs_match_the_table,
);
