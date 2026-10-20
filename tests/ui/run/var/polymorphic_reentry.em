-- トップレベルの関数が、多相な再帰で自分の `var x` の地点に入り直し、前の回の `x` を読む閉包を渡す。閉包は
-- 作った回の `x` を読む (呼び出しの mask が外側の回の handler を選ぶ)
g : Int -> (Unit -> <e> Int) -> <Panic | e> Int
g n read_outer =
  var x = n * 100
  if n == 0 then read_outer () + x
  else
    x <- x + 1
    g (n - 1) (fn () -> x) + read_outer ()

main : Unit -> <IO, Panic> Unit
main () = println (show (g 2 (fn () -> 7)))
