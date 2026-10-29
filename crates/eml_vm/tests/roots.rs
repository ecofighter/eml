//! セッションの根を読み書きする基本操作 (docs/spec/lir.md の「基本操作」)。根に置いた値は実行の終わりに VM が
//! 手放すので、検査モードのリークの検査も通る。

use eml_rt::{Fault, HeapError};
use eml_vm::{AnyMachine, RunConfig, RuntimeError, Sched, VmFault, capture_host};

use crate::common::{disasm, parse, run_lir};

/// 根に置いた文字列と整数を読み直して表示する。整数は `tobj` に box して置き、文字列はそのまま `obj` で読む。
#[test]
fn a_value_set_in_a_root_is_read_back() {
    let text = "\
fn main() -> unit {
  let s.0: obj = const \"kept\"
  let u.1: unit = prim root_set[0](s.0)
  let b.2: tobj = box 42
  let u.3: unit = prim root_set[7](b.2)
  let t.4: obj = prim root_get[0]()
  let v.5: unit = extern Prelude.println(t.4)
  let c.6: tobj = prim root_get[7]()
  let n.7: int = unbox c.6
  decref c.6
  let r.8: obj = extern \"Prelude.Show Int.show\"(n.7)
  let w.9: unit = extern Prelude.println(r.8)
  return w.9
}
";
    assert_eq!(run_lir(text), ("kept\n42\n".to_string(), Ok(())));
}

/// 空の根は、どちらのモードでも読めない。
#[test]
fn reading_an_empty_root_is_an_error() {
    let text = "\
fn main() -> unit {
  let t.0: tobj = prim root_get[3]()
  decref t.0
  return ()
}
";
    assert_eq!(
        run_lir(text).1,
        Err(RuntimeError::Fault {
            fault: VmFault::Extern(Fault::Heap(HeapError::RootMisuse)),
            function: "main".to_string(),
            at: None,
        })
    );
}

#[test]
fn root_primitives_name_their_root() {
    insta::assert_snapshot!(disasm("\
fn main() -> unit {
  let b.0: tobj = box 1
  let u.1: unit = prim root_set[2](b.0)
  let t.2: tobj = prim root_get[2]()
  decref t.2
  return u.1
}
"), @"
    fn main (arity 0, frame 2)
         3  box_const s0, c0
         4  root_set s0, r2, s0
         5  root_get s1, r2
         6  decref s1
         7  ret s0
    ");
}

/// 組の構築子。宣言はプログラムの先頭に要る。
const LAYOUT: &str = "layout (,) { (,)(tobj, tobj) }\n";

/// 根 0 と根 2 の組を読む関数。エントリは最初の関数なので、入力の後に置く。どの入力も、同じ名前の関数を同じコードで
/// 持つ。
const READERS: &str = "\
fn first(p.0: unit) -> tobj {
  let a.1: obj = prim root_get[0]()
  let (x.2: tobj, y.3: tobj) = unbox a.1 (,)
  dup x.2
  decref a.1
  return x.2
}
fn third(p.0: unit) -> tobj {
  let a.1: obj = prim root_get[2]()
  let (x.2: tobj, y.3: tobj) = unbox a.1 (,)
  dup x.2
  decref a.1
  return x.2
}
fn show_pair(t.0: tobj) -> unit {
  let (a.1: tobj, b.2: tobj) = unbox t.0 (,)
  let m.3: int = unbox a.1
  let n.4: int = unbox b.2
  decref t.0
  let s.5: obj = extern \"Prelude.Show Int.show\"(m.3)
  let u.6: unit = extern Prelude.println(s.5)
  let s.7: obj = extern \"Prelude.Show Int.show\"(n.4)
  let u.8: unit = extern Prelude.println(s.7)
  return u.8
}
";

/// REPL の `let` のように、組を根 0 に置く入力。組はヒープの物体で、main の ID が持つ。
const KEEP: &str = "\
fn keep() -> unit {
  let a.0: obj = con (,) #0(#5, #6)
  let u.1: unit = prim root_set[0](a.0)
  return u.1
}
";

/// 根 2 に組を置く入力。根 0 より先に置く。
const KEEP_LATER_ROOT: &str = "\
fn keep_later() -> unit {
  let a.0: obj = con (,) #0(#7, #8)
  let u.1: unit = prim root_set[2](a.0)
  return u.1
}
";

/// 両方の子が根 2 を読む入力。
const READ_LATER_ROOT: &str = "\
fn read_later() -> unit {
  let t.0: obj = par (,) #0(&third, &third)
  tail call show_pair(t.0)
}
";

/// 右の子がもう一度 fork し、その2つの子が根 0 を読む入力。内側の右の子は、右の子の ID から出した ID で読む。
const NESTED: &str = "\
fn nested() -> unit {
  let t.0: obj = par (,) #0(&first, &inner)
  let (a.1: tobj, b.2: tobj) = unbox t.0 (,)
  dup b.2
  decref t.0
  tail call show_pair(b.2)
}
fn inner(p.0: unit) -> tobj {
  let t.1: obj = par (,) #0(&first, &first)
  return t.1
}
";

/// 両方の子が根 0 を読む入力。右の子は子の ID で読む。
const READ: &str = "\
fn read() -> unit {
  let t.0: obj = par (,) #0(&first, &first)
  tail call show_pair(t.0)
}
";

/// 右の子が根 0 を読んでから、0 で割って止まる入力。
const FAIL: &str = "\
fn fail() -> unit {
  let t.0: obj = par (,) #0(&first, &divide)
  tail call show_pair(t.0)
}
fn divide(p.0: unit) -> tobj {
  let x.1: tobj = call first(())
  let n.2: int = unbox x.1
  decref x.1
  let m.3: int = extern \"Prelude.Div Int./\"(n.2, 0)
  let b.4: tobj = box m.3
  return b.4
}
";

/// 入力を1つの `Machine` に順に読み込んで実行し、出力と、入力ごとの結果 (誤りは文言) を返す。2つのモードと、逐次と
/// seed の方針で流し、どれも同じになることを確かめる。最後の `close` が漏れを報告しないことも確かめる。
fn run_inputs(inputs: &[&str]) -> (String, Vec<Result<(), String>>) {
    let mut runs = Vec::new();
    for debug_heap in [true, false] {
        for sched in [Sched::Sequential, Sched::Seeded(1)] {
            let (host, out) = capture_host(Default::default());
            let config = RunConfig::default()
                .with_debug_heap(debug_heap)
                .with_sched(sched);
            let mut machine = AnyMachine::new(&config, host);
            let mut results = Vec::new();
            for input in inputs {
                let program = parse(&format!("{LAYOUT}{input}{READERS}"));
                eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
                let loaded = machine
                    .load(&program)
                    .unwrap_or_else(|error| panic!("{error:?}"));
                results.push(machine.run(loaded.entry).map(|_| ()).map_err(|error| {
                    assert!(!error.is_broken(), "{error}");
                    error.to_string()
                }));
            }
            machine.close().unwrap_or_else(|error| panic!("{error}"));
            runs.push(((debug_heap, sched), (out.contents(), results)));
        }
    }
    let (_, first) = runs.remove(0);
    for (run, other) in runs {
        assert_eq!(other, first, "{run:?}");
    }
    first
}

/// 前の入力が根に置いた組を、`par` の右の子が読める。fork が根に共有の印を付けるので、検査モードは子の ID での
/// 読み込みを `WrongOwner` で断らない。
#[test]
fn a_right_child_reads_a_root() {
    assert_eq!(
        run_inputs(&[KEEP, READ]),
        ("5\n5\n".to_string(), vec![Ok(()), Ok(())])
    );
}

/// 子の誤りの後の回復は、共有の印の付いた根を main の ID の局所の物体として写す。次の入力の fork がまた印を付ける
/// ので、同じ `Machine` の `par` の子がまた根を読める。
#[test]
fn a_par_after_a_child_error_reads_the_roots_again() {
    assert_eq!(
        run_inputs(&[KEEP, FAIL, READ]),
        (
            "5\n5\n".to_string(),
            vec![
                Ok(()),
                Err("division by zero in `divide`".to_string()),
                Ok(())
            ]
        )
    );
}

/// 印の付いた根は、次の入力の fork でも共有のままである。印を付け直さなくても、右の子が読める。
#[test]
fn a_root_shared_by_an_earlier_fork_is_read_again() {
    assert_eq!(
        run_inputs(&[KEEP, READ, READ]),
        ("5\n5\n5\n5\n".to_string(), vec![Ok(()), Ok(()), Ok(())])
    );
}

/// 右の子の中の fork でも、根は共有のままなので、内側の右の子が読める。
#[test]
fn a_nested_right_child_reads_a_root() {
    assert_eq!(
        run_inputs(&[KEEP, NESTED]),
        ("5\n5\n".to_string(), vec![Ok(()), Ok(())])
    );
}

/// 印を付け終えた根より前の番号に後から置いた根にも、次の fork が印を付ける。
#[test]
fn a_root_set_below_the_shared_ones_is_shared_at_the_next_fork() {
    assert_eq!(
        run_inputs(&[KEEP_LATER_ROOT, READ_LATER_ROOT, KEEP, READ]),
        (
            "7\n7\n5\n5\n".to_string(),
            vec![Ok(()), Ok(()), Ok(()), Ok(())]
        )
    );
}
