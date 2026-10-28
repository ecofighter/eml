//! 入力をまたいで `CodeBase` と `Ctx` を持つ `Machine` (docs/spec/vm.md の「追記」)。手書きの LIR のプログラムを1つの
//! `Machine` に続けて読み込み、名前で照らした関数の番号、定数、エフェクト、位置のファイルの引き直しを確かめる。

use std::sync::Arc;

use eml_rt::{ArithFault, Checked, Fast, Fault, HeapError};
use eml_vm::{
    AnyMachine, CompileError, Control, Limits, Machine, RunConfig, RuntimeError, SourceLocation,
    VmFault, capture_host, disassemble,
};

use crate::common::parse;

fn names<M: eml_rt::Mode>(machine: &Machine<M>) -> Vec<&str> {
    machine
        .code()
        .functions()
        .iter()
        .map(|f| f.name.as_str())
        .collect()
}

/// 2つのプログラムを1つの `Machine` に読み込み、それぞれの入口を実行して出力を集める。両方のモードで流し、出力が
/// 同じことを確かめる。
fn run_two(first: &str, second: &str) -> String {
    let mut outputs = Vec::new();
    for debug_heap in [true, false] {
        let (host, out) = capture_host(Default::default());
        let config = RunConfig::default().with_debug_heap(debug_heap);
        let mut machine = AnyMachine::new(&config, host);
        for text in [first, second] {
            let program = parse(text);
            eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
            let loaded = machine
                .load(&program)
                .unwrap_or_else(|error| panic!("{error:?}"));
            machine
                .run(loaded.entry)
                .unwrap_or_else(|error| panic!("{error}"));
        }
        machine.close().unwrap_or_else(|error| panic!("{error}"));
        outputs.push(out.contents());
    }
    assert_eq!(
        outputs[0], outputs[1],
        "the fast mode printed something else"
    );
    outputs.remove(0)
}

const SAY: &str = "\
fn say(s.0: obj) -> unit {
  let u.1: unit = extern Prelude.println(s.0)
  return u.1
}
";

#[test]
fn a_known_name_keeps_its_index_and_a_new_name_is_appended() {
    let first = format!(
        "\
fn main() -> unit {{
  let s.0: obj = const \"one\"
  tail call say(s.0)
}}
{SAY}"
    );
    let second = format!(
        "\
fn second() -> unit {{
  let s.0: obj = const \"two\"
  tail call say(s.0)
}}
{SAY}"
    );
    assert_eq!(run_two(&first, &second), "one\ntwo\n");

    let (host, _) = capture_host(Default::default());
    let mut machine = Machine::<Fast>::new(RunConfig::default().control, host);
    let a = machine.load(&parse(&first)).unwrap();
    let b = machine.load(&parse(&second)).unwrap();
    assert_eq!((a.entry, b.entry), (0, 2));
    assert_eq!(names(&machine), ["main", "say", "second"]);
}

/// 前に読み込んだ文字列は同じ定数を使い、新しい文字列と `i32` に収まらない `Int` の定数は新しい凍結した塊に置く。検査
/// モードは、付けていない塊の物体を指す語を断るので、2つ目の塊を実行のヒープに付け直したことも確かめられる。
#[test]
fn new_constants_go_to_a_new_chunk_and_old_ones_are_reused() {
    let first = format!(
        "\
fn main() -> unit {{
  let s.0: obj = const \"one\"
  tail call say(s.0)
}}
{SAY}"
    );
    let second = format!(
        "\
fn second() -> unit {{
  let s.0: obj = const \"one\"
  let u.1: unit = call say(s.0)
  let t.2: obj = const \"two\"
  let v.3: unit = call say(t.2)
  let b.4: tobj = box 4611686018427387904
  let n.5: int = unbox b.4
  decref b.4
  let w.6: obj = extern \"Prelude.Show Int.show\"(n.5)
  tail call say(w.6)
}}
{SAY}"
    );
    assert_eq!(
        run_two(&first, &second),
        "one\none\ntwo\n4611686018427387904\n"
    );

    let (host, _) = capture_host(Default::default());
    let mut machine = Machine::<Checked>::new(RunConfig::default().control, host);
    machine.load(&parse(&first)).unwrap();
    machine.load(&parse(&second)).unwrap();
    let text = disassemble(machine.code());
    let second_fn = &text[text.find("fn second").unwrap()..];
    assert!(second_fn.contains("load_const s0, c0"), "{second_fn}");
    assert!(second_fn.contains("load_const s0, c1"), "{second_fn}");
    assert!(second_fn.contains("box_const s0, c2"), "{second_fn}");
}

/// エフェクトの番号は名前で引き、`evv_mask` の並びも引き直す。2つ目のプログラムでは `B` が先に並ぶが、実行時の番号は
/// 先に読み込んだ `A` が 0 のままで、`B` が 1 になる。
#[test]
fn effects_are_renumbered_by_name() {
    let first = "\
effect A { a(unit) -> unit }
fn f(p.0: unit) -> unit {
  let ev.1: obj = prim ev_lookup[A]()
  return ()
}
";
    let second = "\
effect B { b(unit) -> unit }
effect A { a(unit) -> unit }
fn g(p.0: unit) -> unit {
  let ev.1: obj = prim ev_lookup[A]()
  let ev.2: obj = prim ev_lookup[B]()
  let w.3: tobj = prim evv_get()
  let w.4: tobj = prim evv_mask[B, A@1](w.3)
  return ()
}
";
    let (host, _) = capture_host(Default::default());
    let mut machine = Machine::<Fast>::new(RunConfig::default().control, host);
    machine.load(&parse(first)).unwrap();
    machine.load(&parse(second)).unwrap();
    let text = disassemble(machine.code());
    let g = &text[text.find("fn g").unwrap()..];
    assert!(g.contains("ev_lookup s0, E0"), "{g}");
    assert!(g.contains("ev_lookup s0, E1"), "{g}");
    assert!(g.contains("evv_mask s0, [0@1, 1]"), "{g}");
}

/// 位置のファイルはパスで引く。2つ目のプログラムの `Program.files` では `b.em` が 0 番だが、誤りの位置は `b.em` を指す。
#[test]
fn files_are_remapped_by_path() {
    let first = "\
fn main() -> unit {
  let n.0: int = extern \"Prelude.Num Int.+\"(1, 2) @\"a.em\":1:1
  return ()
}
";
    let second = "\
fn second() -> unit {
  let n.0: int = extern \"Prelude.Div Int./\"(1, 0) @\"b.em\":2:3
  return ()
}
";
    let (host, _) = capture_host(Default::default());
    let mut machine = Machine::<Checked>::new(RunConfig::default().control, host);
    let a = machine.load(&parse(first)).unwrap();
    machine.run(a.entry).unwrap();
    let b = machine.load(&parse(second)).unwrap();
    assert_eq!(
        machine.run(b.entry).unwrap_err(),
        RuntimeError::Fault {
            fault: VmFault::Extern(Fault::Arith(ArithFault::DivisionByZero)),
            function: "second".to_string(),
            at: Some(SourceLocation {
                path: "b.em".to_string(),
                line: 2,
                column: 3,
            }),
        }
    );
}

/// 名前が同じなのにコードが違う関数は、debug ビルドの一致の確かめが捕まえる。
#[cfg(debug_assertions)]
#[test]
fn a_known_name_with_other_code_is_a_mismatch() {
    let first = "\
fn one() -> int {
  return 1
}
";
    let second = "\
fn one() -> int {
  return 2
}
";
    let (host, _) = capture_host(Default::default());
    let mut machine = Machine::<Fast>::new(RunConfig::default().control, host);
    machine.load(&parse(first)).unwrap();
    assert_eq!(
        machine.load(&parse(second)).unwrap_err(),
        CompileError::Mismatch {
            function: "one".to_string()
        }
    );
}

/// 位置だけが違う同じ関数は一致する。命令の位置は関数の先頭からの位置にそろえて比べるので、別の関数の後ろに並べ直した
/// 写しも同じコードである。
#[test]
fn the_same_function_at_another_position_matches() {
    let first = "\
layout Bool { False, True }
fn pick(b.0: enum) -> int {
  switch b.0 Bool { #0 -> b1, #1 -> b2 }
b1:
  return 10
b2:
  tail call pick(#0)
}
";
    let second = "\
layout Bool { False, True }
fn other() -> int {
  return 3
}
fn pick(b.0: enum) -> int {
  switch b.0 Bool { #0 -> b1, #1 -> b2 }
b1:
  return 10
b2:
  tail call pick(#0)
}
";
    let (host, _) = capture_host(Default::default());
    let mut machine = Machine::<Fast>::new(RunConfig::default().control, host);
    machine.load(&parse(first)).unwrap();
    machine.load(&parse(second)).unwrap();
    assert_eq!(names(&machine), ["pick", "other"]);
}

#[test]
fn a_broken_error_is_a_heap_or_internal_fault() {
    let fault = |fault| RuntimeError::Fault {
        fault,
        function: "f".to_string(),
        at: None,
    };
    assert!(fault(VmFault::Internal("x")).is_broken());
    assert!(fault(VmFault::Driver("x".to_string())).is_broken());
    assert!(
        fault(VmFault::Extern(Fault::Heap(
            eml_rt::HeapError::UseAfterFree
        )))
        .is_broken()
    );
    assert!(!fault(VmFault::Extern(Fault::Arith(ArithFault::DivisionByZero))).is_broken());
    assert!(!RuntimeError::Leak(Vec::new()).is_broken());
}

/// 根に置いた値は実行をまたいで残り、`close` で手放す。検査モードでも、閉じた後にリークは残らない。
#[test]
fn a_root_outlives_its_run_and_is_released_at_close() {
    let first = "\
fn main() -> unit {
  let s.0: obj = const \"kept\"
  let u.1: unit = prim root_set[1](s.0)
  return u.1
}
";
    let second = "\
fn second() -> unit {
  let t.0: obj = prim root_get[1]()
  let u.1: unit = extern Prelude.println(t.0)
  return u.1
}
";
    assert_eq!(run_two(first, second), "kept\n");
}

/// 根を読む入力は fork の関数を持つ (docs/spec/core-ir.md の「`par` の子の中断」)。ロードは呼ばれない `fork$` も
/// 変換するので、`child_abort` を含むプログラムも読み込めて、`par` を走らせなければそのまま動く。
#[test]
fn a_root_reading_input_with_fork_entries_loads_and_runs() {
    let first = "\
fn main() -> unit {
  let s.0: obj = const \"kept\"
  let u.1: unit = prim root_set[1](s.0)
  return u.1
}
";
    let second = "\
effect Fail { never fail(tobj poly) -> never }
fn second() -> unit {
  let t.0: obj = prim root_get[1]()
  let u.1: unit = extern Prelude.println(t.0)
  return u.1
}
clause Fail.fail fn fork$Fail.fail(ev.0: obj, a.1: tobj poly) -> obj poly {
  let c.2: tobj poly = closure reperform$Fail.fail(a.1)
  let u.3: unit = prim child_abort(ev.0, c.2)
  decref ev.0
  let d.4: obj poly = prim dummy()
  return d.4
}
uniform from 1 fn reperform$Fail.fail(a.0: tobj poly, u.1: unit) -> tobj poly {
  let ev.2: obj = prim ev_lookup[Fail]()
  let c.3: tobj = prim ev_clause[Fail.fail](ev.2)
  tail clause Fail.fail c.3(ev.2, a.0)
}
";
    assert!(parse(second).may_run_par());
    assert_eq!(run_two(first, second), "kept\n");
}

/// 続けられる誤りの後は、根から届く物体を新しいヒープに写して古いヒープを捨てる。evv と yield の状態は空に戻るので、
/// 次の実行は終わりの確かめを通り、スタックと evv に残った物体も消えるので、閉じるときのリークの検査も通る
/// (docs/spec/object-model.md の「回復」)。
#[test]
fn a_continuable_error_recovers_the_heap_for_the_next_run() {
    let first = "\
effect A { a(unit) -> unit }
fn main() -> unit {
  let w.0: tobj = prim evv_get()
  dup w.0
  let e.1: obj = prim ev_new[A]((), w.0)
  let w.2: tobj = prim evv_push(w.0, e.1)
  let u.3: unit = prim evv_set(w.2)
  let n.4: int = extern \"Prelude.Div Int.%\"(1, 0)
  return ()
}
";
    let second = "\
fn second() -> unit {
  let s.0: obj = const \"after\"
  let u.1: unit = extern Prelude.println(s.0)
  return u.1
}
";
    for debug_heap in [true, false] {
        let (host, out) = capture_host(Default::default());
        let config = RunConfig::default().with_debug_heap(debug_heap);
        let mut machine = AnyMachine::new(&config, host);
        let first = machine.load(&parse(first)).unwrap();
        let error = machine.run(first.entry).unwrap_err();
        assert_eq!(error.to_string(), "division by zero in `main`");
        assert!(!error.is_broken());
        let second = machine.load(&parse(second)).unwrap();
        machine
            .run(second.entry)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(out.contents(), "after\n");
        machine.close().unwrap_or_else(|error| panic!("{error}"));
    }
}

/// 数え終わってから表示するので、割り込みで止まれば何も表示しない。
const COUNT_THEN_SAY: &str = "\
layout Bool { False, True }
fn main() -> unit {
  tail call count(0)
}
fn count(n.0: int) -> unit {
  let d.1: enum = extern \"Prelude.Eq Int.==\"(n.0, 100000)
  switch d.1 Bool { #0 -> b1, #1 -> b2 }
b1:
  let m.2: int = extern \"Prelude.Num Int.+\"(n.0, 1)
  tail call count(m.2)
b2:
  let s.3: obj = const \"done\"
  let u.4: unit = extern Prelude.println(s.3)
  return u.4
}
";

/// `load` は止める理由のビットを見ないので、`load` の前に立てた割り込みは、実行の最初の安全点まで保留される。`load`
/// の後に立てたときと同じく、実行は何も表示せずに止まる (docs/spec/vm.md の「追記」)。
#[test]
fn an_interrupt_before_load_is_held_until_the_first_safe_point() {
    for debug_heap in [true, false] {
        for before_load in [true, false] {
            let control = Arc::new(Control::new(Limits::default()));
            let config = RunConfig::default()
                .with_debug_heap(debug_heap)
                .with_control(Arc::clone(&control));
            let (host, out) = capture_host(Default::default());
            let mut machine = AnyMachine::new(&config, host);
            if before_load {
                control.interrupt();
            }
            let loaded = machine.load(&parse(COUNT_THEN_SAY)).unwrap();
            if !before_load {
                control.interrupt();
            }
            let error = machine.run(loaded.entry).unwrap_err();
            assert_eq!(error.to_string(), "interrupted in `main`");
            assert_eq!(out.contents(), "");
            machine.close().unwrap_or_else(|error| panic!("{error}"));
        }
    }
}

/// 回復の写しの確保と `dup` は `RunStats` に数えない。1つ目の実行は根に共有のある値を置き (写すと子に `dup` が要る)、
/// 根に置かない物体も同時に作るので、物体の数の最大は回復の後のヒープより大きい。RC の仕事をしない実行で失敗させて
/// 回復し、何もしない3つ目の実行の統計が1つ目と同じことを確かめる (docs/spec/vm.md の「`RunStats`」)。
#[test]
fn recovery_work_is_not_counted_in_run_stats() {
    let first = "\
layout (,) { (,)(tobj, tobj) }
fn main() -> unit {
  let s.0: obj = const \"leaf\"
  dup s.0
  dup s.0
  dup s.0
  let x.1: obj = con (,) #0(s.0, s.0)
  dup x.1
  let y.2: obj = con (,) #0(x.1, x.1)
  let z.3: obj = con (,) #0(s.0, s.0)
  decref z.3
  let u.4: unit = prim root_set[1](y.2)
  return u.4
}
";
    let fail = "\
fn fail() -> unit {
  let n.0: int = extern \"Prelude.Div Int.%\"(1, 0)
  return ()
}
";
    let third = "\
fn third() -> unit {
  return ()
}
";
    let (host, _) = capture_host(Default::default());
    let mut machine = Machine::<Checked>::new(RunConfig::default().control, host);
    let mut run = |text: &str| {
        let loaded = machine.load(&parse(text)).unwrap();
        machine.run(loaded.entry).map(|finished| finished.stats)
    };
    let before = run(first).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(before.peak_objects, 4, "{before:?}");
    assert_eq!(
        run(fail).unwrap_err().to_string(),
        "division by zero in `fail`"
    );
    let after = run(third).unwrap_or_else(|error| panic!("{error}"));
    // `peak_stack` だけはその実行の値である
    let mut expected = before;
    expected.peak_stack = after.peak_stack;
    assert_eq!(after, expected);
    machine.close().unwrap_or_else(|error| panic!("{error}"));
}

/// 根の値は `Unr` なので、検証を通ったプログラムの根は `FileIn` に届かない。検証を通らない手書きの LIR で `FileIn` を根に
/// 置き、続けられる誤りで止めると、回復は `FileIn` を写せずに失敗する。その誤りは元の誤りと回復できない理由を持ち、
/// セッションを使えなくする (docs/spec/vm.md の「追記」)。
#[test]
fn a_failed_recovery_breaks_the_session() {
    let root = std::env::temp_dir().join(format!("eml-vm-unrecovered-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.txt"), "abc").unwrap();
    let text = "\
fn main() -> unit {
  let p.0: obj = const \"a.txt\"
  let h.1: obj lin = extern \"Std.FileIn.open\"(p.0)
  let u.2: unit = prim root_set[1](h.1)
  let n.3: int = extern \"Prelude.Div Int.%\"(1, 0)
  return ()
}
";
    let mut errors = Vec::new();
    for debug_heap in [true, false] {
        let (host, _) = capture_host(root.clone());
        let config = RunConfig::default().with_debug_heap(debug_heap);
        let mut machine = AnyMachine::new(&config, host);
        let loaded = machine.load(&parse(text)).unwrap();
        errors.push(machine.run(loaded.entry).unwrap_err());
    }
    std::fs::remove_dir_all(&root).unwrap();
    for error in errors {
        assert!(
            matches!(
                &error,
                RuntimeError::Unrecovered {
                    cause,
                    reason: HeapError::Unrecoverable,
                } if matches!(
                    **cause,
                    RuntimeError::Fault {
                        fault: VmFault::Extern(Fault::Arith(ArithFault::DivisionByZero)),
                        ..
                    }
                )
            ),
            "{error:?}"
        );
        assert!(error.is_broken());
        assert_eq!(
            error.to_string(),
            "division by zero in `main`\n\
             recovery failed: a session root reaches an object that recovery cannot copy"
        );
    }
}

/// 文字列を返す入口。`run_shown` が結果を写して手放すので、検査モードの `close` はリークを報告しない
/// (docs/spec/vm.md の「追記」)。
const SHOWN: &str = "\
fn shown() -> obj {
  let s.0: obj = const \"x = \"
  let n.1: obj = extern \"Prelude.Show Int.show\"(42)
  let t.2: obj = extern \"Prelude.++\"(s.0, n.1)
  let u.3: obj = const \"printed\"
  let v.4: unit = extern Prelude.println(u.3)
  return t.2
}
";

#[test]
fn a_shown_entry_returns_its_text_after_the_output() {
    for debug_heap in [true, false] {
        let (host, out) = capture_host(Default::default());
        let config = RunConfig::default().with_debug_heap(debug_heap);
        let mut machine = AnyMachine::new(&config, host);
        let program = parse(SHOWN);
        eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
        let loaded = machine.load(&program).unwrap();
        let (_, text) = machine.run_shown(loaded.entry, 100).unwrap();
        assert_eq!(text.as_deref(), Some("x = 42"));
        // 入口が書いた行は、結果を返す前に書き出してある
        assert_eq!(out.contents(), "printed\n");
        let (_, text) = machine.run_shown(loaded.entry, 3).unwrap();
        assert_eq!(text.as_deref(), Some("x ="));
        machine.write_line("x = 42 : Int").unwrap();
        assert_eq!(out.contents(), "printed\nprinted\nx = 42 : Int\n");
        machine.close().unwrap_or_else(|error| panic!("{error}"));
    }
}

/// `Unit` を返す入口は文字列を返さないので、`run_shown` は内部の誤りにする。速いモードで文字列でない語を読まないため
/// である。
#[test]
fn a_shown_entry_must_return_an_object() {
    let (host, _) = capture_host(Default::default());
    let mut machine = Machine::<Fast>::new(RunConfig::default().control, host);
    let loaded = machine
        .load(&parse("fn main() -> unit {\n  return ()\n}\n"))
        .unwrap();
    let error = machine.run_shown(loaded.entry, 10).unwrap_err();
    assert!(
        matches!(
            error,
            RuntimeError::Fault {
                fault: VmFault::Internal(_),
                ..
            }
        ),
        "{error:?}"
    );
}
