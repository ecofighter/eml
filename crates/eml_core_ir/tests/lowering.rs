//! Core IR から LIR への変換は、構造を写すだけである
//! (docs/spec/core-ir.md の「Core IR から LIR への変換」)。2つの IR に共通する命令は
//! 同じ表示になるので、EP の後の Core IR と、それを変換した LIR の表示は同じ文字列になる。

use eml_core_ir::{Pass, parse, pretty_with_positions, to_lir};

use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use crate::common::programs::{UiProgram, ui_programs};
use crate::common::{core_text, function};

/// プログラムどうしは独立なので、コアの数のスレッドで分けて流す。`Session` は `Send + Sync` である
/// (docs/implementation/architecture.md の「CLI と lib API」)。
#[test]
fn every_program_lowers_to_the_same_text() {
    let programs = ui_programs();
    let next = AtomicUsize::new(0);
    let workers = thread::available_parallelism().map_or(1, NonZeroUsize::get);
    thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some(program) = programs.get(next.fetch_add(1, Ordering::Relaxed)) {
                    lowers_to_the_same_text(program);
                }
            });
        }
    });
}

fn lowers_to_the_same_text(program: &UiProgram) {
    let core = eml_test_support::core_until(
        eml_test_support::with_modules(&program.entry, &program.module_refs()),
        Pass::Evidence,
    );
    let shown = pretty_with_positions(&core);
    let lir = to_lir(core);
    assert_eq!(
        eml_lir::pretty_with_positions(&lir),
        shown,
        "{}",
        program.name
    );
    if let Err(error) = eml_lir::verify_lowered(&lir) {
        panic!("{}: {error}", program.name);
    }
}

#[test]
fn lowering_keeps_names_marks_and_tables() {
    let core = parse(
        "\
layout Option { None, Some(tobj) }
effect State { get(unit) -> tobj, put(tobj) -> unit }
fn entry$main() -> unit {
  let t.0: tobj = call f(())
  let s.1: obj = const \"x\"
  let u.2: unit = extern Prelude.println(s.1) @\"lib/Util.em\":3:7
  return u.2
}
internal yields fn f(p.0: unit) -> tobj {
  return #0
}
internal yields clause State.get fn g(e.0: obj, u.1: unit) -> tobj {
  let s.2: tobj = prim ev_take_state(e.0)
  return s.2
}
",
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let lir = to_lir(core.clone());
    assert_eq!(lir.entry, core.entry);
    assert_eq!(lir.strings, core.strings);
    assert_eq!(lir.layouts, core.layouts);
    assert_eq!(lir.effects, core.effects);
    assert_eq!(lir.files, core.files);
    let marks = |name: &str, internal: bool, yields: bool, clause: Option<(u32, u32)>| {
        (name.to_string(), internal, yields, clause)
    };
    let lowered: Vec<_> = lir
        .functions
        .iter()
        .map(|function| {
            marks(
                &function.name,
                function.internal,
                function.yields,
                function.clause,
            )
        })
        .collect();
    assert_eq!(
        lowered,
        vec![
            marks("entry$main", false, false, None),
            marks("f", true, true, None),
            marks("g", true, true, Some((0, 0))),
        ]
    );
    assert_eq!(lir.functions[1].vars, core.functions[1].vars);
}

#[test]
#[should_panic(expected = "internal error: an effect instruction reaches the lowering to LIR")]
fn lowering_rejects_a_perform() {
    let core = parse(
        "\
effect State { get(unit) -> tobj }
fn f() -> tobj {
  let p.0: tobj = perform State.get(())
  return p.0
}
",
    )
    .unwrap();
    to_lir(core);
}

#[test]
#[should_panic(expected = "internal error: a mask reaches the lowering to LIR")]
fn lowering_rejects_a_mask() {
    let core = parse(
        "\
effect State { get(unit) -> tobj }
fn f(c.0: tobj) -> tobj {
  let t.1: tobj = mask [State] apply c.0(())
  return t.1
}
",
    )
    .unwrap();
    to_lir(core);
}

/// ランタイムが名前で呼ぶエントリは、何も呼ばなくても最適化を越えて残る (docs/spec/core-ir.md の「最適化」)。
#[test]
fn runtime_entries_stay_when_nothing_calls_them() {
    let lir = eml_test_support::lir("import Async\nmain : Unit -> Unit\nmain () = ()");
    for &entry in eml_extern::RuntimeEntry::ALL {
        assert!(
            lir.functions
                .iter()
                .any(|function| function.name == entry.name()),
            "`{}` stays",
            entry.name()
        );
    }
}

/// `Std.Async` を読み込まなければ、エントリは根にならない。
#[test]
fn runtime_entries_are_absent_without_the_import() {
    let lir = eml_test_support::lir("main : Unit -> Unit\nmain () = ()");
    for &entry in eml_extern::RuntimeEntry::ALL {
        assert!(
            lir.functions
                .iter()
                .all(|function| function.name != entry.name()),
            "`{}` is absent",
            entry.name()
        );
    }
}

/// `blocking` の C の関数と、それを呼ぶ関数。`count` は文字列を借り、`fill` は配列を借りて組で返す。
const BLOCKING: &str = "\
import Async (Async)
import MutUnboxedArray

extern \"C\" blocking count : String -> Int -> <IO, Async> Int
extern \"C\" blocking fill : Int -> MutUnboxedArray Float -> <IO, Async> (Unit, MutUnboxedArray Float)

go : String -> MutUnboxedArray Float -> <IO, Async> (Int, MutUnboxedArray Float)
go s a =
  let n = count s 3
  let ((), a) = fill n a
  (n, a)

main : Unit -> <IO, Panic> Unit
main () =
  let a : MutUnboxedArray Float = MutUnboxedArray.empty ()
  let (n, a) = Async.run (fn () -> go \"x\" a)
  drop a
  println (show n)
";

/// `blocking` の呼び出しは、受け付け、`Std.Async.io_wait`、組の `unpack`、取り出しの4つの文になる。取り出しは記録の
/// 番号と `Wake` に続けて物体の引数を借り、`lin` の引数は組で返る (docs/spec/core-ir.md の「組を返す extern」)。
#[test]
fn a_blocking_call_waits_between_submit_and_take() {
    insta::assert_snapshot!(function(&core_text(BLOCKING, Pass::Translate), "go"), @"
    yields fn go(s.0: obj, a.1: obj lin) -> (,){int, obj lin} lin {
      let op.2: int = extern submit Main.count(s.0, 3)
      let r.3: (,){enum, int} lin = call Std.Async.io_wait(op.2)
      unpack r.3 (,) #0(w.4: enum, op.5: int)
      let t.6: int = extern take Main.count(op.5, w.4, s.0)
      let op.7: int = extern submit Main.fill(t.6, a.1)
      let r.8: (,){enum, int} lin = call Std.Async.io_wait(op.7)
      unpack r.8 (,) #0(w.9: enum, op.10: int)
      let t.11: unit = extern take Main.fill(op.10, w.9, a.1)
      let d.12: (,){unit, obj lin} lin = con (,) #0(t.11, a.1)
      unpack d.12 (,) #0(x.13: unit, a.14: obj lin)
      let d.15: (,){int, obj lin} lin = con (,) #0(t.6, a.14)
      return d.15
    }
    ");
}

/// 値として使う `blocking` の C の関数を包む関数は、受け付けと取り出しの間で止まるので yield する。
#[test]
fn a_blocking_wrapper_yields() {
    let text = BLOCKING.replace("let n = count s 3", "let f = count s\n  let n = f 3");
    insta::assert_snapshot!(function(&core_text(&text, Pass::Translate), "go$extern0"), @"
    internal yields uniform from 0 fn go$extern0(p.0: obj, p.1: int) -> int {
      let op.2: int = extern submit Main.count(p.0, p.1)
      let r.3: (,){enum, int} lin = call Std.Async.io_wait(op.2)
      unpack r.3 (,) #0(w.4: enum, op.5: int)
      let t.6: int = extern take Main.count(op.5, w.4, p.0)
      return t.6
    }
    ");
}
