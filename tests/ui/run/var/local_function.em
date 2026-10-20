-- 範囲の中の局所関数が変数を使う。再帰する局所関数も変数を捕まえずに使える
main : Unit -> <IO, Panic> Unit
main () =
  var n = 0
  let bump k = n := n + k
  bump 3
  bump 4
  let go i =
    if i < 5 then
      n := n + i
      go (i + 1)
    else ()
  go 0
  println (show n)
