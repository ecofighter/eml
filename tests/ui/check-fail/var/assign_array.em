-- 配列の変数に `:=` で代入すると古い配列を捨てることになるので E3008
main : Unit -> <IO, Panic> Unit
main () =
  var a = MutArray.make 3 0
  a := MutArray.make 4 0
  drop a
