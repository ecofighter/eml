-- 内側の `var x` を使う閉包を、外側の `var f` に代入するので E2019
main : Unit -> <IO> Unit
main () =
  var f = fn () -> 0
  var x = 1
  f := fn () -> x
  println "done"
