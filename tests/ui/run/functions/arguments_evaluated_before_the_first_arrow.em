-- `first 1 (second ())` は、`first` の引数の数を超える `second ()` も先に評価してから、矢印を順に適用する
-- (docs/spec/expressions.md の「関数適用」)。そのため `second` の出力が `first` の本体の出力より先に出る。
first : Int -> <IO> (Int -> <IO> Int)
first x =
  println "first"
  fn y -> x + y

second : Unit -> <IO> Int
second () =
  println "second"
  2

main : Unit -> <IO> Unit
main () = println (show (first 1 (second ())))
