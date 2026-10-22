-- 節の本体が捕まえた `File` を、一方の枝で閉じ、もう一方の枝で `fail` する。`drop` は持ち上げた節の関数の中に置く。
-- 操作の節が捕まえる値は `Unr` に限られるので、1回だけ動く `return` の節で書く。
import Fail (Fail(..))
import Fs

effect Ask where
  ask : Unit -> Int

close_later : Int -> <Fail String, IO> Int
close_later n =
  let f = Fs.open "input.txt"
  handle ask () with
    | ctl ask () k -> k n
    | return x ->
      if x > 0 then
        Fs.close f
        x
      else Fail.fail "not positive"

main : Unit -> <IO> Unit
main () =
  println (show (Fail.to_result (fn () -> close_later 1)))
  println (show (Fail.to_result (fn () -> close_later 0)))
