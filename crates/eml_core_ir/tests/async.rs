//! `Std.Async` の預かる行を、`Lin` と `Int` の結果の instance で LIR まで下ろす。下ろした後の verifier が、預かる引数の
//! 印と Perceus の規則を確かめる (docs/spec/declarations.md の「`extern`」)。出力は長いので、スナップショットにせず、
//! 預かる行の呼び出しと結果の束縛だけを見る。

use crate::common::{core_text, function};
use eml_core_ir::Pass;

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

/// rowに`Async`を持つ`main`のエントリの関数は、`main`の値を`Std.Async.root`に渡す。rowに`Async`を持たない`main`の
/// プログラムには、`Std.Async`を読み込んでいても`root`のinstanceがない (docs/spec/core-ir.md の「変換の規則」)。
#[test]
fn an_async_main_is_wrapped_in_root() {
    let text = "import Async (Async)\nimport Async\n\nmain : Unit -> <IO, Async> Unit\nmain () = Async.spawn (fn () -> println \"child\")\n";
    let shown = core_text(text, Pass::Translate);
    insta::assert_snapshot!(function(&shown, "entry$main"), @r#"
    fn entry$main() -> unit {
      let t.0: unit = call "Std.Async.root@[Unit]"(&main)
      return t.0
    }
    "#);
    let plain = core_text(
        "import Async\n\nmain : Unit -> <IO> Unit\nmain () = println \"plain\"\n",
        Pass::Translate,
    );
    assert!(!plain.contains("fn \"Std.Async.root@"), "{plain}");
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
