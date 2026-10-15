//! 入力をまたいで `CodeBase` と `Ctx` を持つ `Machine` (docs/spec/vm.md の「追記」)。手書きの LIR のプログラムを1つの
//! `Machine` に続けて読み込み、名前で照らした関数の番号、定数、エフェクト、位置のファイルの引き直しを確かめる。

use eml_rt::{ArithFault, Checked, Fast, Fault};
use eml_vm::{
    AnyMachine, CompileError, Machine, RunConfig, RuntimeError, SourceLocation, VmFault,
    capture_host, disassemble,
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
  let w.4: tobj = prim evv_mask[B, A](w.3)
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
    assert!(g.contains("evv_mask s0, [0, 1]"), "{g}");
}

/// 位置のファイルはパスで引く。2つ目のプログラムの `Program.files` では `b.em` が 0 番だが、誤りの位置は `b.em` を指す。
#[test]
fn files_are_remapped_by_path() {
    let first = "\
fn main() -> unit {
  let n.0: int = extern \"Prelude.+\"(1, 2) @\"a.em\":1:1
  return ()
}
";
    let second = "\
fn second() -> unit {
  let n.0: int = extern \"Prelude./\"(1, 0) @\"b.em\":2:3
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

/// 続けられる誤りの後は、evv と yield の状態を空に戻すので、次の実行は終わりの確かめを通る。スタックと evv に
/// 残った物体はヒープに残り、検査モードでは閉じるときにリークとして数える (S11b の根からの走査が回収する)。
#[test]
fn a_continuable_error_empties_the_evv_for_the_next_run() {
    let first = "\
effect A { a(unit) -> unit }
fn main() -> unit {
  let w.0: tobj = prim evv_get()
  dup w.0
  let e.1: obj = prim ev_new[A]((), w.0)
  let w.2: tobj = prim evv_push(w.0, e.1)
  let u.3: unit = prim evv_set(w.2)
  let n.4: int = extern \"Prelude.%\"(1, 0)
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
        let closed = machine.close();
        assert_eq!(
            matches!(closed, Err(RuntimeError::Leak(_))),
            debug_heap,
            "{closed:?}"
        );
    }
}
