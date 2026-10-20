//! Core IR から LIR への変換は、構造を写すだけである
//! (docs/spec/core-ir.md の「Core IR から LIR への変換」)。2つの IR に共通する命令は
//! 同じ表示になるので、EP の後の Core IR と、それを変換した LIR の表示は同じ文字列になる。

use eml_core_ir::{Pass, parse, pretty_with_positions, to_lir};

use crate::common::programs::ui_programs;

#[test]
fn every_program_lowers_to_the_same_text() {
    for program in ui_programs() {
        let core = eml_test_support::core_until_files(
            &program.entry,
            &program.module_refs(),
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
  let t.1: tobj = mask [State] apply c.0((): unit)
  return t.1
}
",
    )
    .unwrap();
    to_lir(core);
}
