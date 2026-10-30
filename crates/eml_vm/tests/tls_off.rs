//! `tls` を外した `eml_vm`。`Std.Tls` の接続は `NotAvailable` を投げる (docs/spec/stdlib.md の「`Std.Tls`」)。

const SRC: &str = "import Async
import Tls

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  match try_io (fn () -> Tls.connect \"tls.localhost\" 1) with
    | Ok s -> Tls.close s
    | Err e -> println (show e))
";

#[test]
fn std_tls_is_not_available() {
    let (out, result) = eml_test_support::run(SRC);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        out,
        "IOError { kind = NotAvailable, message = \"cannot set up TLS for tls.localhost: not available\" }\n"
    );
}
