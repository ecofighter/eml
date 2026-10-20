-- ブロックが `let` で終わり、最後の式がないので `a` を取り出せない。E3003
fill : Unit -> <Panic> Unit
fill () =
  var a = MutArray.make 3 0
  a[0] <- 1
  let n = a[0]

main : Unit -> <IO, Panic> Unit
main () =
  fill ()
  println "done"
