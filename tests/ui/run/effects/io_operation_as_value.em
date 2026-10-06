-- `IO` の操作を値として渡しても、その場で実行される。
each : (String -> <IO> Unit) -> <IO> Unit
each f =
  f "a"
  f "b"

main : Unit -> <IO> Unit
main () = each println
