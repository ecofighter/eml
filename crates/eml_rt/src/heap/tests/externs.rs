//! extern の単体テスト。結果は spec のとおりで、今の実装と同じ文言で誤りを返すことを確かめる。

use std::cmp::Ordering as Order;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::ctx::{ctx, ctx_with};
use super::host::{MemoryFile, MemoryHost, host};
use super::mortal;
use super::string::text;
use crate::externs::*;
use crate::{Control, HeapError, Limits, Mode, Stop, Word};

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

/// 写すバイトを数える。一意な左なら、その場で足しても作り直しても右の長さを、共有か不死の左なら両辺の長さを数える
/// (今のインタプリタと同じ)。
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
        let y = ctx.string_append(k, d).unwrap();
        assert_eq!(text(ctx.heap(), y), "kz");
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

/// エスケープの規則は今のインタプリタの `show_string` と同じである。C1 の制御文字 (U+0080〜U+009F) はそのまま写す。
fn strings_show_with_the_interpreters_escapes<M: Mode>() {
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
        ctx.println(a).unwrap();
        let e = s(&mut ctx, "");
        ctx.println(e).unwrap();
    }
    ctx.flush().unwrap();
    assert_eq!(memory.out(), "héllo\n\n");
    assert_eq!(memory.err(), "");
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 書けなくても文字列は手放す (今の実装と同じ)。
fn a_failed_write_still_releases_the_text<M: Mode>() {
    let memory = Arc::new(MemoryHost::failing("disk full"));
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let a = s(&mut ctx, "x");
        assert_eq!(ctx.println(a), Err(Fault::Output("disk full".into())));
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
        assert_eq!(ctx.println(d), Err(Fault::Heap(HeapError::WrongKind)));
        assert_eq!(
            ctx.println(Word::imm(0)),
            Err(Fault::Heap(HeapError::WrongKind))
        );
        assert_eq!(memory.out(), "");
        ctx.heap_mut().decref(d).unwrap();
    }
}

/// 開いて、全部読み、組から取り出す。組は `lin` ビットを持ち、File の参照を持つ。
unsafe fn open_and_read<M: Mode>(
    ctx: &mut crate::Ctx<M>,
    path: &str,
) -> Result<(Word, String), Fault> {
    unsafe {
        let p = s(ctx, path);
        let f = ctx.fs_open(p)?;
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
                path: "missing.txt".into(),
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
                path: "broken.txt".into(),
                reason: IoReason::Other
            })
        );
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        let p = s(&mut ctx, "bad.bin");
        let f = ctx.fs_open(p).unwrap();
        assert_eq!(
            ctx.fs_read_all(f),
            Err(Fault::NotUtf8 {
                path: "bad.bin".into()
            })
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

/// ヒープの外の読みの領域も、確保量の上限に数える。
fn a_read_past_the_memory_limit_stops<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    memory.add("big.txt", MemoryFile::text(&"x".repeat(200_000)));
    let control = Arc::new(Control::new(Limits {
        max_bytes: 150_000,
        ..Limits::default()
    }));
    let mut ctx = crate::Ctx::<M>::new(control, host(&memory));
    unsafe {
        let p = s(&mut ctx, "big.txt");
        let f = ctx.fs_open(p).unwrap();
        assert_eq!(ctx.fs_read_all(f), Err(Fault::Stopped(Stop::MemoryLimit)));
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
        assert_eq!(
            ctx.fs_read_all(other),
            Err(Fault::Heap(HeapError::WrongKind))
        );
        assert_eq!(ctx.fs_close(other), Err(Fault::Heap(HeapError::WrongKind)));
        ctx.heap_mut().decref(other).unwrap();
        if M::CHECKED {
            let p = s(&mut ctx, "a.txt");
            let f = ctx.fs_open(p).unwrap();
            super::raw::set_rc(ctx.heap_mut(), f, 2);
            assert_eq!(ctx.fs_read_all(f), Err(Fault::Heap(HeapError::NotUnique)));
            super::raw::set_rc(ctx.heap_mut(), f, 1);
            ctx.fs_close(f).unwrap();
        }
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

both_modes!(
    strings_compare_by_bytes,
    the_same_string_on_both_sides,
    appending_counts_the_bytes_it_copies,
    ints_show_in_decimal,
    shows_count_the_bytes_they_write,
    strings_show_with_the_interpreters_escapes,
    a_string_extern_refuses_a_non_string,
    println_writes_a_line_and_releases_its_text,
    a_failed_write_still_releases_the_text,
    println_refuses_a_non_string,
    a_file_is_read_whole_and_closed,
    an_empty_file_reads_as_an_empty_string,
    a_long_unsized_file_is_read_in_chunks,
    an_interrupted_read_is_retried,
    a_missing_file_is_a_fault_naming_its_path,
    a_failed_read_closes_the_file,
    a_read_stops_on_an_interrupt,
    a_read_past_the_memory_limit_stops,
    read_all_refuses_a_non_file_or_a_shared_file,
);
