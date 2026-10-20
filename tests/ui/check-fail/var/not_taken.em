-- ブロックが `()` で終わり、どこでも `a` を取り出さないので E3003
main : Unit -> <IO, Panic> Unit
main () =
  println "start"
  var a = MutArray.make 3 0
  a[0] <- 1
  ()
