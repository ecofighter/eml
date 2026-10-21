-- 範囲の値に `x` を使う閉包が入るので E2019
main : Unit -> <IO> Unit
main () =
  let g =
    var x = 1
    fn () -> x
  println "\{g ()}"
