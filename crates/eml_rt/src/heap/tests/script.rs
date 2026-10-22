//! スクリプトの extern (引数、環境変数、標準入力、ファイルの書き込み、子のプロセス、標準エラー)。メモリの Host で、
//! 結果と誤りと、引数を手放すことを確かめる (docs/spec/stdlib.md)。

use std::ffi::OsString;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::ctx::{ctx_with, stdin_read};
use super::host::{MemoryFile, MemoryHost};
use super::mortal;
use super::string::text;
use crate::externs::{FALSE, IndexFault, IoOp, IoReason, TRUE, Utf8Source};
use crate::{Control, Ctx, Fault, Limits, Mode, Stop, Stream, Word};

fn s<M: Mode>(ctx: &mut Ctx<M>, x: &str) -> Word {
    unsafe { ctx.heap_mut() }.alloc_string(x)
}

/// UTF-8 でないバイト列の `OsString`。
#[cfg(unix)]
fn not_utf8() -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(vec![b'a', 0xff])
}

fn eprintln_writes_to_the_standard_error<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let a = s(&mut ctx, "oops");
        ctx.write_string(Stream::Err, a).unwrap();
    }
    assert_eq!(memory.err(), "oops\n");
    assert_eq!(memory.out(), "");
    assert_eq!(mortal(ctx.heap()), 0);
}

fn arguments_are_read_by_index<M: Mode>() {
    let memory = Arc::new(MemoryHost::default().with_args(vec!["-l".into(), "日本".into()]));
    let mut ctx = ctx_with::<M>(&memory);
    assert_eq!(ctx.env_arg_count(), 2);
    for (i, expected) in [(0, "-l"), (1, "日本")] {
        let w = ctx.env_arg(i).unwrap();
        assert_eq!(text(ctx.heap(), w), expected);
        unsafe { ctx.heap_mut().decref(w) }.unwrap();
    }
    for i in [-1, 2] {
        assert_eq!(
            ctx.env_arg(i),
            Err(Fault::Index(Box::new(IndexFault::Arg {
                index: i,
                count: 2
            })))
        );
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

fn an_argument_that_is_not_utf8_is_a_fault<M: Mode>() {
    #[cfg(unix)]
    {
        let memory = Arc::new(MemoryHost::default().with_args(vec!["a".into(), not_utf8()]));
        let mut ctx = ctx_with::<M>(&memory);
        assert_eq!(
            ctx.env_arg(1),
            Err(Fault::NotUtf8(Box::new(Utf8Source::Arg(1))))
        );
        assert_eq!(mortal(ctx.heap()), 0);
    }
}

fn environment_variables_are_read_by_name<M: Mode>() {
    let mut env = vec![
        ("HOST", OsString::from("example.org")),
        ("EMPTY", OsString::new()),
    ];
    #[cfg(unix)]
    env.push(("BAD", not_utf8()));
    let memory = Arc::new(MemoryHost::default().with_env(env));
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        for (name, set) in [("HOST", TRUE), ("EMPTY", TRUE), ("NONE", FALSE)] {
            let n = s(&mut ctx, name);
            assert_eq!(ctx.env_has(n), Ok(set), "{name}");
        }
        for (name, value) in [("HOST", "example.org"), ("EMPTY", ""), ("NONE", "")] {
            let n = s(&mut ctx, name);
            let w = ctx.env_var(n).unwrap();
            assert_eq!(text(ctx.heap(), w), value, "{name}");
            ctx.heap_mut().decref(w).unwrap();
        }
        #[cfg(unix)]
        {
            let n = s(&mut ctx, "BAD");
            assert_eq!(ctx.env_has(n), Ok(TRUE));
            let n = s(&mut ctx, "BAD");
            assert_eq!(
                ctx.env_var(n),
                Err(Fault::NotUtf8(Box::new(Utf8Source::EnvVar("BAD".into()))))
            );
        }
    }
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 1行ずつ読むと、行の終わりを含めて返し、終わりでは空の文字列になる。最後の行に `\n` がなくても1行である。
fn the_standard_input_is_read_by_lines_or_whole<M: Mode>() {
    let memory = Arc::new(MemoryHost::default().with_stdin(MemoryFile {
        chunk: 3,
        ..MemoryFile::text("one\r\ntwo\nthree")
    }));
    let mut ctx = ctx_with::<M>(&memory);
    for expected in ["one\r\n", "two\n", "three", ""] {
        let w = stdin_read(&mut ctx, true).unwrap();
        assert_eq!(text(ctx.heap(), w), expected);
        unsafe { ctx.heap_mut().decref(w) }.unwrap();
    }
    let memory = Arc::new(MemoryHost::default().with_stdin(MemoryFile::text("a\nb\n")));
    let mut ctx2 = ctx_with::<M>(&memory);
    let w = stdin_read(&mut ctx2, true).unwrap();
    assert_eq!(text(ctx2.heap(), w), "a\n");
    unsafe { ctx2.heap_mut().decref(w) }.unwrap();
    // 行の後の残りをすべて読む
    let w = stdin_read(&mut ctx2, false).unwrap();
    assert_eq!(text(ctx2.heap(), w), "b\n");
    unsafe { ctx2.heap_mut().decref(w) }.unwrap();
    assert_eq!(mortal(ctx.heap()), 0);
    assert_eq!(mortal(ctx2.heap()), 0);
}

/// 64 KiB の区切りより長い1行も、区切りをまたいで1行として読む。
fn a_long_line_is_read_across_chunks<M: Mode>() {
    let line = "x".repeat(100_000);
    let memory =
        Arc::new(MemoryHost::default().with_stdin(MemoryFile::text(&format!("{line}\nrest"))));
    let mut ctx = ctx_with::<M>(&memory);
    let w = stdin_read(&mut ctx, true).unwrap();
    assert_eq!(text(ctx.heap(), w), format!("{line}\n"));
    unsafe { ctx.heap_mut().decref(w) }.unwrap();
    let w = stdin_read(&mut ctx, true).unwrap();
    assert_eq!(text(ctx.heap(), w), "rest");
    unsafe { ctx.heap_mut().decref(w) }.unwrap();
    assert_eq!(mortal(ctx.heap()), 0);
}

fn the_standard_input_can_be_unavailable_or_not_utf8<M: Mode>() {
    let mut ctx = ctx_with::<M>(&Arc::new(MemoryHost::default()));
    assert_eq!(
        stdin_read(&mut ctx, false),
        Err(Fault::Stdin(IoReason::NotAvailable))
    );
    let memory = Arc::new(MemoryHost::default().with_stdin(MemoryFile {
        bytes: vec![b'a', 0xff, b'\n'],
        ..MemoryFile::default()
    }));
    let mut ctx = ctx_with::<M>(&memory);
    assert_eq!(
        stdin_read(&mut ctx, true),
        Err(Fault::NotUtf8(Box::new(Utf8Source::Stdin)))
    );
    let memory = Arc::new(MemoryHost::default().with_stdin(MemoryFile {
        fail_after: Some(0),
        ..MemoryFile::text("x")
    }));
    let mut ctx = ctx_with::<M>(&memory);
    assert_eq!(
        stdin_read(&mut ctx, true),
        Err(Fault::Stdin(IoReason::Other))
    );
}

/// 止める語が立った後の `Interrupted` で、行の読みも止まる。止める語の立っていない `Interrupted` は読み直す。
fn a_read_of_the_standard_input_stops_on_an_interrupt<M: Mode>() {
    let control = Arc::new(Control::new(Limits::default()));
    let memory = Arc::new(MemoryHost::default().with_stdin(MemoryFile {
        interrupt_with: Some(control.clone()),
        ..MemoryFile::text("abc\n")
    }));
    let mut ctx = Ctx::<M>::new(control.clone(), super::host::host(&memory));
    assert_eq!(
        stdin_read(&mut ctx, true),
        Err(Fault::Stopped(Stop::Interrupted))
    );
    let memory = Arc::new(MemoryHost::default().with_stdin(MemoryFile {
        interrupt_once: true,
        ..MemoryFile::text("abc\n")
    }));
    let mut ctx = ctx_with::<M>(&memory);
    let w = stdin_read(&mut ctx, true).unwrap();
    assert_eq!(text(ctx.heap(), w), "abc\n");
    unsafe { ctx.heap_mut().decref(w) }.unwrap();
}

/// 標準入力を読む前に、まとめて持っている出力を書き出す。
fn the_output_is_flushed_before_the_standard_input_is_read<M: Mode>() {
    let memory = Arc::new(MemoryHost::default().with_stdin(MemoryFile::text("")));
    let mut ctx = ctx_with::<M>(&memory);
    let w = stdin_read(&mut ctx, false).unwrap();
    unsafe { ctx.heap_mut().decref(w) }.unwrap();
    assert_eq!(memory.flushes(), 1);
}

/// 1行ずつの読みも、読み手は 64 KiB の buffer で補充する。1 MiB の16バイトの行は、16回の補充と終わりを確かめる
/// 1回の読みで読み切る。
fn a_line_read_refills_once_per_64_kib<M: Mode>() {
    let reads = Arc::new(AtomicUsize::new(0));
    let line = "0123456789abcde\n";
    let count = 1024 * 1024 / line.len();
    let memory = Arc::new(MemoryHost::default().with_stdin(MemoryFile {
        reads: Some(reads.clone()),
        ..MemoryFile::text(&line.repeat(count))
    }));
    let mut ctx = ctx_with::<M>(&memory);
    let mut lines = 0;
    loop {
        let w = stdin_read(&mut ctx, true).unwrap();
        let read = text(ctx.heap(), w).to_string();
        unsafe { ctx.heap_mut().decref(w) }.unwrap();
        if read.is_empty() {
            break;
        }
        assert_eq!(read, line);
        lines += 1;
    }
    assert_eq!(lines, count);
    assert!(reads.load(Ordering::Relaxed) <= 17, "{reads:?}");
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 読みの誤り、止めたこと、UTF-8 でない行の後も、読み手は slot に残り、次の読みは続きから読める。
fn a_failed_stdin_read_leaves_the_reader_in_the_slot<M: Mode>() {
    let memory = Arc::new(MemoryHost::default().with_stdin(MemoryFile {
        fail_once: true,
        ..MemoryFile::text("a\n")
    }));
    let mut ctx = ctx_with::<M>(&memory);
    assert_eq!(
        stdin_read(&mut ctx, true),
        Err(Fault::Stdin(IoReason::Other))
    );
    let w = stdin_read(&mut ctx, true).unwrap();
    assert_eq!(text(ctx.heap(), w), "a\n");
    unsafe { ctx.heap_mut().decref(w) }.unwrap();

    let control = Arc::new(Control::new(Limits::default()));
    let memory = Arc::new(MemoryHost::default().with_stdin(MemoryFile::text("b\n")));
    let mut ctx = Ctx::<M>::new(control.clone(), super::host::host(&memory));
    control.interrupt();
    assert_eq!(
        stdin_read(&mut ctx, true),
        Err(Fault::Stopped(Stop::Interrupted))
    );
    control.clear();
    let w = stdin_read(&mut ctx, true).unwrap();
    assert_eq!(text(ctx.heap(), w), "b\n");
    unsafe { ctx.heap_mut().decref(w) }.unwrap();

    let memory = Arc::new(MemoryHost::default().with_stdin(MemoryFile {
        bytes: b"\xff\nok\n".to_vec(),
        ..MemoryFile::default()
    }));
    let mut ctx = ctx_with::<M>(&memory);
    assert_eq!(
        stdin_read(&mut ctx, true),
        Err(Fault::NotUtf8(Box::new(Utf8Source::Stdin)))
    );
    let w = stdin_read(&mut ctx, true).unwrap();
    assert_eq!(text(ctx.heap(), w), "ok\n");
    unsafe { ctx.heap_mut().decref(w) }.unwrap();
    assert_eq!(mortal(ctx.heap()), 0);
}

fn a_file_is_written_whole<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        let p = s(&mut ctx, "out.txt");
        let t = s(&mut ctx, "日本\n");
        ctx.fs_write_text(p, t).unwrap();
        let p = s(&mut ctx, "");
        let t = s(&mut ctx, "x");
        assert_eq!(
            ctx.fs_write_text(p, t),
            Err(Fault::Io {
                op: IoOp::Write,
                name: "".into(),
                reason: IoReason::PermissionDenied
            })
        );
    }
    assert_eq!(memory.written("out.txt").as_deref(), Some("日本\n"));
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 引数は、数と各引数を NUL で終えた列で受け、空の引数も1つと数える。数が合わない列 (NUL を含む引数) は起動の誤りに
/// する。
fn a_process_gets_its_arguments_split_at_nul<M: Mode>() {
    let memory = Arc::new(MemoryHost::default());
    let mut ctx = ctx_with::<M>(&memory);
    unsafe {
        for (args, code) in [("0\0", 0), ("3\0-a\0\0x y\0", 3)] {
            let p = s(&mut ctx, "rsync");
            let a = s(&mut ctx, args);
            assert_eq!(ctx.proc_status(p, a), Ok(code));
        }
        let p = s(&mut ctx, "rsync");
        let a = s(&mut ctx, "1\0a\0b\0");
        assert_eq!(
            ctx.proc_status(p, a),
            Err(Fault::Io {
                op: IoOp::Run,
                name: "rsync".into(),
                reason: IoReason::Other
            })
        );
        let p = s(&mut ctx, "missing");
        let a = s(&mut ctx, "0\0");
        assert_eq!(
            ctx.proc_status(p, a),
            Err(Fault::Io {
                op: IoOp::Run,
                name: "missing".into(),
                reason: IoReason::NotFound
            })
        );
    }
    assert_eq!(
        memory.runs(),
        [
            ("rsync".to_string(), vec![]),
            (
                "rsync".to_string(),
                vec!["-a".to_string(), String::new(), "x y".to_string()]
            ),
        ]
    );
    assert_eq!(mortal(ctx.heap()), 0);
}

/// 止める語が起動の前に立っていれば起動せず、子を待つ間に立てば子が終わった後で、止めたことにする。
fn a_process_stops_on_an_interrupt<M: Mode>() {
    let control = Arc::new(Control::new(Limits::default()));
    let memory = Arc::new(MemoryHost::default().interrupting(control.clone()));
    let mut ctx = Ctx::<M>::new(control.clone(), super::host::host(&memory));
    unsafe {
        // メモリの Host の `run` は、子が動く間に届いた割り込みとして止める語を立てる
        let p = s(&mut ctx, "sleep");
        let a = s(&mut ctx, "1\u{0}1\u{0}");
        assert_eq!(
            ctx.proc_status(p, a),
            Err(Fault::Stopped(Stop::Interrupted))
        );
        assert_eq!(memory.runs().len(), 1);
        let p = s(&mut ctx, "sleep");
        let a = s(&mut ctx, "1\u{0}1\u{0}");
        assert_eq!(
            ctx.proc_status(p, a),
            Err(Fault::Stopped(Stop::Interrupted))
        );
    }
    assert_eq!(memory.runs().len(), 1);
    assert_eq!(mortal(ctx.heap()), 0);
}

both_modes!(
    eprintln_writes_to_the_standard_error,
    arguments_are_read_by_index,
    an_argument_that_is_not_utf8_is_a_fault,
    environment_variables_are_read_by_name,
    the_standard_input_is_read_by_lines_or_whole,
    a_long_line_is_read_across_chunks,
    the_standard_input_can_be_unavailable_or_not_utf8,
    a_read_of_the_standard_input_stops_on_an_interrupt,
    the_output_is_flushed_before_the_standard_input_is_read,
    a_line_read_refills_once_per_64_kib,
    a_failed_stdin_read_leaves_the_reader_in_the_slot,
    a_file_is_written_whole,
    a_process_gets_its_arguments_split_at_nul,
    a_process_stops_on_an_interrupt,
);
