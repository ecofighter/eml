-- `0.0` と `-0.0` は `==` では等しいが、ビット列が違うので `compare` で区別でき、`show` も `0.0` と `-0.0` を
-- 書く。2つの定数を定数の表でまとめないことも確かめる。
main : Unit -> <IO, Panic> Unit
main () =
  let z = 0.0
  let n = -0.0
  println (show (n == z))
  println (show (compare n z))
  println (show (compare z n))
  println (show (compare n n))
  println (show (compare (negate z) z))
  println (show (compare (z * (-1.0)) z))
  println (show z)
  println (show n)
