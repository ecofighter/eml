-- 一方の枝の `match` だけが参照する `data` の値を、もう一方の枝の戻らない呼び出しの前で暗黙に手放す。
import Fail (Fail(..))

data Box = | Box Fs.File Int

by_fail : Int -> <Fail String, IO> Unit
by_fail n =
  let b = Box (Fs.open "input.txt") n
  if n > 5 then Fail.fail "big"
  else
    match b with
      | Box g _ -> Fs.close g

by_exit : Int -> <IO> Unit
by_exit n =
  let b = Box (Fs.open "input.txt") n
  if n > 5 then exit n
  else
    match b with
      | Box g _ -> Fs.close g

main : Unit -> <IO> Unit
main () =
  println (show (Fail.to_result (fn () -> by_fail 0)))
  println (show (Fail.to_result (fn () -> by_fail 9)))
  by_exit 0
  println "closed"
  by_exit 7
