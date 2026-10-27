//! `Std.Async` の預かる行を、`Lin` と `Int` の結果の instance で LIR まで下ろす。下ろした後の verifier が、預かる引数の
//! 印と Perceus の規則を確かめる (docs/spec/declarations.md の「`extern`」)。出力は長いので、スナップショットにせず、
//! 預かる行の呼び出しと結果の束縛だけを見る。

fn program(result: &str, import: &str) -> String {
    format!(
        "import Async\n{import}\nmain : Unit -> <IO, Panic> Unit\nmain () =\n  Async.run (fn () ->\n    let t = Async.start (fn () -> {result})\n    let x = Async.await t\n    let y = Async.run (fn () -> {result})\n    drop x\n    drop y)\n"
    )
}

fn assert_held_calls(text: &str) {
    for call in [
        "extern Std.Async.finish_ok(",
        "extern Std.Async.task_cell(",
        "extern Std.Async.take_result(",
    ] {
        assert!(text.contains(call), "no `{call}` in\n{text}");
    }
}

#[test]
fn held_rows_lower_for_a_lin_instance() {
    let lir = eml_test_support::lir(&program("MutArray.empty ()", "import MutArray"));
    let text = eml_lir::pretty(&lir);
    assert_held_calls(&text);
    assert!(
        text.contains(": obj lin = extern Std.Async.take_result("),
        "{text}"
    );
}

#[test]
fn held_rows_lower_for_an_int_instance() {
    let lir = eml_test_support::lir(&program("1", ""));
    let text = eml_lir::pretty(&lir);
    assert_held_calls(&text);
    assert!(
        text.contains(": tobj = extern Std.Async.take_result("),
        "{text}"
    );
}
