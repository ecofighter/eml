//! `gui`を外した`eml_rows`。`Gui.run`は`NotAvailable`を投げる (docs/spec/stdlib.mdの「`Std.Gui`」)。

const SRC: &str = "import Async
import Gui

data Msg =
  | Quit

main : Unit -> <IO, Panic, Async.Async> Unit
main () =
  match try_io (fn () -> Async.scope (fn _ -> Gui.run (fn () -> Gui.wait ()))) with
    | Ok Quit -> println \"opened\"
    | Err e -> println (show e)
";

#[test]
fn std_gui_is_not_available() {
    let (out, result) = eml_test_support::run(SRC);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        out,
        "IOError { kind = NotAvailable, message = \"cannot open the GUI: not available\" }\n"
    );
}
