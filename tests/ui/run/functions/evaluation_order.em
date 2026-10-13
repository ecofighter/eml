-- 呼び出しは、呼ばれる式がどれでも、括弧があってもなくても、引数をすべて評価してから矢印を順に適用する
-- (docs/spec/expressions.md の「関数適用」)。そのため `g` の出力が `f` と `w` の本体の出力より先に出る。パイプの左の値は
-- 最初に評価する。
f : Int -> <IO> (Int -> <IO> Int)
f a =
  println "f"
  fn b -> a + b

g : Unit -> <IO> Int
g () =
  println "g"
  1

w : Int -> <IO> (Int -> <IO> Int)
w = fn a ->
  println "w"
  fn b -> a + b

main : Unit -> <IO> Unit
main () =
  println (show ((f 1) (g ())))
  println (show (f 1 (g ())))
  println (show (w 1 (g ())))
  let k = f
  println (show (k 1 (g ())))
  println (show ((fn a -> f a) 1 (g ())))
  println (show (g () |> k 1))
