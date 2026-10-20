-- `Int` の変数への添字の書き込みは、変数の型を `MutArray` と合わせられないので E2001
main : Unit -> <IO, Panic> Unit
main () =
  var n = 1
  n[0] := 2
  println "\{n}"
