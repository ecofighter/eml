-- 1歩ごとに閉包が育つ CPS のループ。鍵の入れ子の深さで特殊化を打ち切るので、コンパイルは止まり、値も変わらない
main : Unit -> <IO, Panic> Unit
main () =
  let go k n = if n == 0 then k 0 else go (fn x -> k (x + 1)) (n - 1)
  println (show (go (fn x -> x) 50))
