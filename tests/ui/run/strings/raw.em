-- Raw strings have no escapes or holes, may span lines, and keep their indentation.
main : Unit -> <IO> Unit
main () =
  println r"C:\tmp\{x}"
  println r#"say "hi""#
  println r"line one
  line two"
