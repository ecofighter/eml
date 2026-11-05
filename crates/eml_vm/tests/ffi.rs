//! `Op::Ffi` で利用者の C の関数を呼ぶ (docs/spec/vm.md の「C の関数の命令」)。`blocking` の C の関数は
//! `Op::FfiSubmit` と `Op::FfiTake` でテストの driver の仕事にする (docs/spec/vm.md の「`blocking` の呼び出し」)。
//! C の関数は `common::c` の Rust の呼ばれる関数で、`Linked` は手で作る。

use std::io;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eml_lir::Program;
use eml_rt::{
    Checked, Completion, Control, Done, Fast, HandleBin, IoDriver, IoHandle, IoWaker, Limits, Op,
    OpKey, Submit,
};
use eml_vm::{
    CodeBase, CompileError, Finished, Linked, Machine, RunConfig, RuntimeError, capture_host,
    disassemble,
};

use crate::common::c::{ADD_I32_CALLS, calls, dtors, linked};
use crate::common::parse;

fn linked_all(program: &Program) -> Linked {
    linked(program, &calls(), &dtors())
}

/// 1つのモードで1回流し、出力と結果を返す。
fn run_mode(
    program: &Program,
    debug_heap: bool,
    control: Arc<Control>,
) -> (String, Result<Finished, RuntimeError>) {
    let (host, out) = capture_host(Default::default());
    let config = RunConfig::default()
        .with_debug_heap(debug_heap)
        .with_control(control);
    let result = eml_vm::execute(program, &linked_all(program), &config, host);
    (out.contents(), result)
}

/// 検査モード、次に速いモードで流し、どちらも通って同じ出力になることを確かめる。出力と、2つの run の `Finished`
/// を返す。
fn run_both_on(program: &Program, workers: usize) -> (String, Vec<Finished>) {
    let (outputs, finished): (Vec<String>, Vec<Finished>) = [true, false]
        .into_iter()
        .map(|debug_heap| {
            let control = Arc::new(Control::new(Limits {
                workers,
                ..Limits::default()
            }));
            let (out, result) = run_mode(program, debug_heap, control);
            let finished =
                result.unwrap_or_else(|error| panic!("debug_heap {debug_heap}: {error}"));
            (out, finished)
        })
        .unzip();
    assert_eq!(
        outputs[0], outputs[1],
        "the fast mode printed something else"
    );
    (outputs[0].clone(), finished)
}

fn run_both(src: &str) -> String {
    run_both_on(&eml_test_support::lir(src), Limits::default().workers).0
}

const ADD: &str = "\
extern \"C\" pure symbol \"rs_add_i32\" add : Int as int32_t -> Int as int32_t -> <Panic> Int as int32_t
";

const HANDLE: &str = "\
extern \"C\" data Handle drop \"rs_handle_free\"
extern \"C\" symbol \"rs_handle_new\" handle_new : Int -> Int -> <IO, Panic> Handle
extern \"C\" symbol \"rs_handle_get\" handle_get : Handle -> <IO> (Int, Handle)
extern \"C\" symbol \"rs_frees\" frees : Int -> <IO> Int
";

#[test]
fn a_c_function_returns_an_int_in_both_modes() {
    let src = "\
extern \"C\" pure symbol \"rs_abs\" abs : Int -> Int

main : Unit -> <IO, Panic> Unit
main () = println (show (abs (0 - 5)))
";
    assert_eq!(run_both(src), "5\n");
}

#[test]
fn an_i32_argument_is_range_checked_before_the_call() {
    // 範囲の外の引数では C の関数を呼ばない。呼ぶのは、両方のモードで最後の1回ずつだけである。`int32_t` の結果は
    // 符号を広げて読む
    let src = format!(
        "{ADD}
main : Unit -> <IO> Unit
main () =
  println (show (try_panic (fn () -> add 1 2147483648)))
  println (show (try_panic (fn () -> add (0 - 2147483649) 1)))
  println (show (try_panic (fn () -> add 2147483647 1)))
"
    );
    let before = ADD_I32_CALLS.load(Ordering::Relaxed);
    assert_eq!(
        run_both(&src),
        "Err \"argument 2 of `Main.add` does not fit in C `int32_t`: 2147483648\"\n\
         Err \"argument 1 of `Main.add` does not fit in C `int32_t`: -2147483649\"\n\
         Ok (-2147483648)\n"
    );
    assert_eq!(ADD_I32_CALLS.load(Ordering::Relaxed) - before, 2);
}

#[test]
fn an_uncaught_range_panic_is_a_runtime_error_at_the_call() {
    let src = format!(
        "{ADD}
main : Unit -> <IO, Panic> Unit
main () = println (show (add 1 2147483648))
"
    );
    let program = eml_test_support::lir(&src);
    for debug_heap in [true, false] {
        let control = RunConfig::default().control;
        let (out, result) = run_mode(&program, debug_heap, control);
        assert_eq!(out, "");
        let error = result.expect_err("the panic is not caught");
        assert_eq!(
            error.to_string(),
            "panicked: argument 2 of `Main.add` does not fit in C `int32_t`: 2147483648\n  at test.em:4:26",
            "debug_heap {debug_heap}"
        );
    }
}

#[test]
fn a_null_handle_raises_panic_and_allocates_nothing() {
    // null を返したハンドルの物体は作らないので、dtor は走らず、検査モードの終わりの検査もリークを見ない
    let src = format!(
        "{HANDLE}
make : Int -> <IO, Panic> Int
make value =
  let h = handle_new 100002 value
  let (v, h) = handle_get h
  drop h
  v

main : Unit -> <IO> Unit
main () =
  println (show (try_panic (fn () -> make (0 - 1))))
  println (show (frees 100002))
"
    );
    assert_eq!(
        run_both(&src),
        "Err \"`Main.handle_new` returned a null handle\"\n0\n"
    );
}

#[test]
fn a_handle_runs_its_dtor_once_when_dropped() {
    let src = format!(
        "{HANDLE}
main : Unit -> <IO, Panic> Unit
main () =
  let h = handle_new 100003 7
  let (v, h) = handle_get h
  let (w, h) = handle_get h
  drop h
  println (show (v + w))
  println (show (frees 100003))
"
    );
    assert_eq!(run_both(&src), "14\n1\n");
}

#[test]
fn a_c_function_reads_and_writes_raw_arrays() {
    // C が書いた配列は、呼び手が組で受け取り直す同じ配列である
    let src = "\
import MutUnboxedArray
import UnboxedArray

extern \"C\" pure symbol \"rs_dot\" dot : UnboxedArray Float -> UnboxedArray Float -> Int -> Float
extern \"C\" pure symbol \"rs_fill\" fill : MutUnboxedArray Float -> Int -> Float -> (Unit, MutUnboxedArray Float)

main : Unit -> <IO, Panic> Unit
main () =
  let a = UnboxedArray.from_list [1.0, 2.0, 3.0]
  let b = UnboxedArray.from_list [4.0, 5.0, 6.0]
  println (show (dot a b 3))
  let m = MutUnboxedArray.make 3 0.0
  let ((), m) = fill m 3 1.5
  let (x, m) = MutUnboxedArray.read m 2
  drop m
  println (show x)
";
    assert_eq!(run_both(src), "32.0\n3.0\n");
}

#[test]
fn strings_and_bytes_pass_their_payload_pointer() {
    // 定数の文字列は凍結した塊にあり、連結した文字列とバイト列はヒープにある。長さは引数で別に渡す
    let src = "\
import Bytes

extern \"C\" pure symbol \"rs_bytes_sum\" string_sum : String -> Int -> Int
extern \"C\" pure symbol \"rs_bytes_sum\" bytes_sum : Bytes -> Int -> Int

main : Unit -> <IO> Unit
main () =
  let s = \"a\" ++ \"bc\"
  println (show (string_sum s 3))
  println (show (string_sum \"é\" 2))
  println (show (bytes_sum (Bytes.of_string \"xyz\") 2))
";
    assert_eq!(run_both(src), "294\n364\n241\n");
}

const ABS_FIRST: &str = "\
cfn \"Main.abs\" symbol \"rs_abs\" pure link [] (i64) -> i64
fn first() -> unit {
  let n.0: int = extern \"Prelude.Num Int.-\"(0, 5)
  let a.1: int = extern Main.abs(n.0)
  let s.2: obj = extern \"Prelude.Show Int.show\"(a.1)
  let u.3: unit = extern Prelude.println(s.2)
  return u.3
}
";

const ABS_SECOND: &str = "\
cfn \"Main.abs\" symbol \"rs_abs\" pure link [] (i64) -> i64
fn second() -> unit {
  let n.0: int = extern \"Prelude.Num Int.-\"(0, 7)
  let a.1: int = extern Main.abs(n.0)
  let s.2: obj = extern \"Prelude.Show Int.show\"(a.1)
  let u.3: unit = extern Prelude.println(s.2)
  return u.3
}
";

#[test]
fn loading_the_same_c_function_twice_keeps_one_entry() {
    // 3回目は1回目と同じプログラムで、debug ビルドは `first` を変換し直して名前で書いた命令を比べる
    let (host, out) = capture_host(Default::default());
    let mut machine = Machine::<Checked>::new(RunConfig::default().control, host);
    let mut entries = Vec::new();
    for text in [ABS_FIRST, ABS_SECOND, ABS_FIRST] {
        let program = parse(text);
        let loaded = machine
            .load(&program, &linked_all(&program))
            .unwrap_or_else(|error| panic!("{error:?}"));
        entries.push(loaded.entry);
    }
    let text = disassemble(machine.code());
    assert_eq!(text.matches("cfn ").count(), 1, "{text}");
    assert_eq!(text.matches("= Main.abs(").count(), 2, "{text}");
    for entry in entries {
        machine.run(entry).unwrap_or_else(|error| panic!("{error}"));
    }
    machine.close().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out.contents(), "5\n7\n5\n");
}

#[test]
fn a_redeclared_c_function_with_another_signature_is_a_mismatch() {
    let other = "\
cfn \"Main.abs\" symbol \"rs_abs\" pure link [] (i32) -> i32
fn second() -> unit {
  let a.0: int = extern Main.abs(5)
  let s.1: obj = extern \"Prelude.Show Int.show\"(a.0)
  let u.2: unit = extern Prelude.println(s.1)
  return u.2
}
";
    // ハンドルの型も名前で照らす。2つ目の宣言は dtor を持たない
    let handle = |drop: &str, f: &str| {
        format!(
            "\
chandle \"Main.Handle\"{drop} link []
cfn \"Main.new\" symbol \"rs_handle_new\" link [] (i64, i64) -> handle \"Main.Handle\"
fn {f}(n.0: int) -> int {{
  let h.1: obj lin = extern Main.new(100006, n.0)
  let u.2: unit = drop h.1
  return n.0
}}
"
        )
    };
    let (host, out) = capture_host(Default::default());
    let mut machine = Machine::<Fast>::new(RunConfig::default().control, host);
    let first = parse(ABS_FIRST);
    machine.load(&first, &linked_all(&first)).unwrap();
    let with_drop = parse(&handle(" drop \"rs_handle_free\"", "keep"));
    machine.load(&with_drop, &linked_all(&with_drop)).unwrap();
    let second = parse(other);
    assert_eq!(
        machine.load(&second, &linked_all(&second)),
        Err(CompileError::Mismatch {
            function: "Main.abs".into()
        })
    );
    let without_drop = parse(&handle("", "other"));
    assert_eq!(
        machine.load(&without_drop, &linked_all(&without_drop)),
        Err(CompileError::Mismatch {
            function: "Main.Handle".into()
        })
    );
    // `Mismatch` は何も書かないので、同じ `Machine` に前のプログラムをもう一度読み込んで流せる
    let loaded = machine.load(&first, &linked_all(&first)).unwrap();
    machine.run(loaded.entry).unwrap();
    machine.close().unwrap();
    assert_eq!(out.contents(), "5\n");
}

#[test]
#[should_panic(expected = "line up with the C tables")]
fn a_misaligned_linked_table_is_a_programming_error() {
    let load = |program: &Program, linked: &Linked| {
        let mut code = CodeBase::<Fast>::new(Arc::new(Control::new(Limits::default())));
        let _ = code.load(program, linked);
    };
    let rejected = |program: &Program, linked: &Linked, why: &str| {
        let payload =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| load(program, linked)))
                .expect_err(why);
        let message = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .unwrap_or_default();
        assert!(
            message.contains("line up with the C tables"),
            "{why}: {message}"
        );
    };
    let first = parse(ABS_FIRST);
    rejected(&first, &Linked::default(), "a table of the wrong length");
    // 長さが同じでも、別の宣言でつないだ `Linked` は読み込まない。`int64_t` の橋渡しで `int32_t` の関数を呼ばない
    // ためである。橋渡しの語が同じ `Text` と `Int` も見分ける。`Int` の宣言で引いた関数に文字列の位置を渡さない
    // ためである
    let narrower = parse(&ABS_FIRST.replace("(i64) -> i64", "(i32) -> i64"));
    rejected(&narrower, &linked_all(&first), "another word type");
    let text = parse(&ABS_FIRST.replace("(i64) -> i64", "(text) -> i64"));
    rejected(
        &text,
        &linked_all(&first),
        "the same words, another declaration",
    );
    // dtor も、つないだときの宣言の dtor でなければ読み込まない
    let handle = |drop: &str| {
        format!(
            "\
chandle \"Main.Handle\" drop \"{drop}\" link []
cfn \"Main.new\" symbol \"rs_handle_new\" link [] (i64, i64) -> handle \"Main.Handle\"
fn keep(n.0: int) -> int {{
  let h.1: obj lin = extern Main.new(100007, n.0)
  let u.2: unit = drop h.1
  return n.0
}}
"
        )
    };
    let freed = parse(&handle("rs_handle_free"));
    let other = parse(&handle("free"));
    load(&other, &linked_all(&freed));
}

#[test]
fn c_calls_on_two_workers() {
    // `par` の右の子は空いた worker で走り、そこで C の関数を呼び、ハンドルを作って捨てる。dtor はハンドルを解放した
    // worker で走る
    let src = "\
import Std.Par

extern \"C\" pure symbol \"rs_abs\" abs : Int -> Int
extern \"C\" data Handle drop \"rs_handle_free\"
extern \"C\" pure symbol \"rs_handle_new\" handle_new : Int -> Int -> <Panic> Handle
extern \"C\" pure symbol \"rs_handle_get\" handle_get : Handle -> (Int, Handle)
extern \"C\" symbol \"rs_frees\" frees : Int -> <IO> Int

work : Int -> <Panic> Int
work n =
  let h = handle_new 100004 n
  let (v, h) = handle_get h
  drop h
  abs (0 - v)

main : Unit -> <IO, Panic> Unit
main () =
  let (a, b) = Par.par (fn () -> work 1) (fn () -> work 2)
  println (show (a + b))
  println (show (frees 100004))
";
    let (out, finished) = run_both_on(&eml_test_support::lir(src), 2);
    assert_eq!(out, "3\n2\n");
    // 最初の fork が worker 1 を作り、空いたその worker に右の子を置く (docs/spec/vm.md の「タスクとスケジュール」)
    for finished in finished {
        assert!(finished.placed >= 1, "{finished:?}");
    }
}

#[test]
fn the_disassembly_names_the_c_function() {
    let program = parse(ABS_FIRST);
    let mut code = CodeBase::<Fast>::new(Arc::new(Control::new(Limits::default())));
    code.load(&program, &linked_all(&program)).unwrap();
    insta::assert_snapshot!(disassemble(&code), @"
    cfn Main.abs
    fn first (arity 0, frame 3)
         3  load_raw s1, 0
         4  load_raw s2, 5
         5  int_sub s0, s1, s2
         6  ffi s0 = Main.abs(s0)
         7  extern s0 = Prelude.Show Int.show(s0)
         8  extern s0 = Prelude.println(s0)
         9  ret s0
    ");
}

/// `blocking` で宣言した `rs_sub_i32` と、ハンドルの関数。`rs_add_i32` は呼んだ回数を数えるテストが使うので、並べて
/// 流しても数が混ざらないように使わない。
const BLOCKING: &str = "\
import Async (Async)

extern \"C\" blocking symbol \"rs_sub_i32\" sub : Int as int32_t -> Int as int32_t -> <IO, Panic, Async> Int as int32_t
extern \"C\" data Handle drop \"rs_handle_free\"
extern \"C\" blocking symbol \"rs_handle_new\" handle_new : Int -> Int -> <IO, Panic, Async> Handle
extern \"C\" symbol \"rs_frees\" frees : Int -> <IO> Int
";

/// テストの driver を包み、受け付けた `Op::Job` と、`Done::Job` で届いた完了を数える driver。`refuse` が立っていれば、
/// 仕事を走らせずに受け付けで誤りを返す。
struct Jobs {
    inner: eml_io::TestDriver,
    refuse: bool,
    log: Arc<Mutex<(usize, usize)>>,
}

impl Jobs {
    fn new(refuse: bool) -> (Jobs, Arc<Mutex<(usize, usize)>>) {
        let log = Arc::new(Mutex::new((0, 0)));
        let jobs = Jobs {
            inner: eml_io::TestDriver::new(None),
            refuse,
            log: Arc::clone(&log),
        };
        (jobs, log)
    }
}

impl IoDriver for Jobs {
    fn now(&self) -> Duration {
        self.inner.now()
    }

    fn wait(&mut self, deadline_ms: Option<u64>) -> io::Result<bool> {
        self.inner.wait(deadline_ms)
    }

    fn submit(&mut self, key: OpKey, op: Op) -> Submit {
        if matches!(op, Op::Job(_)) {
            self.log.lock().unwrap().0 += 1;
            if self.refuse {
                return Submit::Done(Err(io::Error::other("the pool is gone")));
            }
        }
        self.inner.submit(key, op)
    }

    fn cancel(&mut self, key: OpKey) {
        self.inner.cancel(key);
    }

    fn completions(&mut self, out: &mut Vec<Completion>) {
        let start = out.len();
        self.inner.completions(out);
        let jobs = out[start..]
            .iter()
            .filter(|completion| matches!(completion.result, Ok(Done::Job(_))))
            .count();
        self.log.lock().unwrap().1 += jobs;
    }

    fn pending(&self) -> usize {
        self.inner.pending()
    }

    fn waker(&self) -> Arc<dyn IoWaker> {
        self.inner.waker()
    }

    fn bin(&self) -> Arc<HandleBin> {
        self.inner.bin()
    }

    fn close(&mut self, handle: IoHandle) {
        self.inner.close(handle);
    }

    fn adopt(&mut self, res: eml_rt::RawResource) -> io::Result<IoHandle> {
        self.inner.adopt(res)
    }

    fn pipe(&mut self, parent_reads: bool) -> io::Result<eml_rt::PipePair> {
        self.inner.pipe(parent_reads)
    }

    fn quiesce(&mut self, jobs: Duration) {
        self.inner.quiesce(jobs);
    }
}

/// `main` を `Jobs` の driver の上で検査モードで流し、出力と、受け付けた仕事と `Done::Job` の完了の数を返す。
fn run_counting(main: &str, refuse: bool) -> (String, (usize, usize)) {
    let program = eml_test_support::lir(&format!("{BLOCKING}{main}"));
    let (driver, log) = Jobs::new(refuse);
    let (host, out) = capture_host(Default::default());
    let mut machine =
        Machine::<Checked>::new(RunConfig::default().control, host).with_driver(Box::new(driver));
    let loaded = machine.load(&program, &linked_all(&program)).unwrap();
    machine
        .run(loaded.entry)
        .unwrap_or_else(|error| panic!("{error}"));
    machine.close().unwrap_or_else(|error| panic!("{error}"));
    let counts = *log.lock().unwrap();
    (out.contents(), counts)
}

#[test]
fn a_blocking_call_is_submitted_as_a_job() {
    let main = "
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () -> println (show (sub 7 2)))
";
    assert_eq!(run_counting(main, false), ("5\n".to_string(), (1, 1)));
}

/// 取り消しの地点で投げるタスクは仕事を出さず、`io_wait` が `Cancelled` で再開し、取り出しが取り消しの IO の失敗を
/// 起こす (docs/spec/vm.md の「`blocking` の呼び出し」)。
#[test]
fn a_cancelled_task_submits_no_job() {
    let main = "
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let r = try_io (fn () -> Async.scope (fn s ->
    Async.cancel_scope s
    sub 7 2))
  match r with
    | Ok n -> println \"unreachable \\{show n}\"
    | Err e -> println \"\\{show e.kind}: \\{e}\")
";
    assert_eq!(
        run_counting(main, false),
        ("Cancelled: task was cancelled\n".to_string(), (0, 0))
    );
}

/// 仕事を出した後に取り消されたタスクは、仕事が終わってから `Cancelled` で再開する。返ったハンドルは取り出しが
/// その場で dtor に渡す (docs/spec/vm.md の「`blocking` の呼び出し」)。
#[test]
fn a_cancelled_blocking_call_destroys_its_handle_result() {
    let src = format!(
        "{BLOCKING}
body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    let h = handle_new 100011 5
    println \"unreachable\"
    drop h)
  Async.yield_now ()
  Async.cancel t

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  println (show (frees 100011))
"
    );
    assert_eq!(run_both(&src), "1\n");
}

#[test]
fn a_blocking_call_whose_job_cannot_run_fails_with_io() {
    let main = "
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  match try_io (fn () -> sub 7 2) with
    | Ok n -> println \"unreachable \\{show n}\"
    | Err e -> println \"\\{show e.kind}: \\{e}\")
";
    assert_eq!(
        run_counting(main, true),
        (
            "Other: cannot call `Main.sub`: the pool is gone\n".to_string(),
            (1, 0)
        )
    );
}

/// 範囲の外の引数は、記録も仕事も作らずに `Panic` を上げる。
#[test]
fn a_blocking_range_panic_creates_no_record() {
    let main = "
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () -> println (show (try_panic (fn () -> sub 1 2147483648))))
";
    assert_eq!(
        run_counting(main, false),
        (
            "Err \"argument 2 of `Main.sub` does not fit in C `int32_t`: 2147483648\"\n"
                .to_string(),
            (0, 0)
        )
    );
}

/// null のハンドルは取り出しで `Panic` を上げ、物体を作らないので dtor は走らない。
#[test]
fn a_blocking_null_handle_panics() {
    let src = format!(
        "{BLOCKING}
make : Int -> <IO, Panic, Async> Unit
make value =
  let h = handle_new 100012 value
  drop h

main : Unit -> <IO, Panic> Unit
main () =
  println (show (Async.run (fn () -> try_panic (fn () -> make (0 - 1)))))
  println (show (frees 100012))
"
    );
    assert_eq!(
        run_both(&src),
        "Err \"`Main.handle_new` returned a null handle\"\n0\n"
    );
}

/// `blocking` の行の受け付けと取り出しの見え方は宣言から決まるので、同じプログラムを同じ `Linked` でもう一度読み込んでも
/// 宣言は同じに比べられる。C が書いた配列は、取り出しの後の組で受け取り直す同じ配列である。
#[test]
fn two_loads_of_a_blocking_row_compare_equal() {
    let src = format!(
        "import MutUnboxedArray
{BLOCKING}
extern \"C\" blocking symbol \"rs_fill\" fill : MutUnboxedArray Float -> Int -> Float -> <IO, Async> (Unit, MutUnboxedArray Float)

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let m = MutUnboxedArray.make 3 0.0
  let ((), m) = fill m 3 1.5
  let (x, m) = MutUnboxedArray.read m 2
  drop m
  println (show x)
  println (show (sub 7 2)))
"
    );
    let program = eml_test_support::lir(&src);
    let linked = linked_all(&program);
    let (host, out) = capture_host(Default::default());
    let mut machine = Machine::<Checked>::new(RunConfig::default().control, host);
    let entries: Vec<_> = (0..2)
        .map(|_| {
            machine
                .load(&program, &linked)
                .unwrap_or_else(|error| panic!("{error:?}"))
                .entry
        })
        .collect();
    for entry in entries {
        machine.run(entry).unwrap_or_else(|error| panic!("{error}"));
    }
    machine.close().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(out.contents(), "3.0\n5\n3.0\n5\n");
}
