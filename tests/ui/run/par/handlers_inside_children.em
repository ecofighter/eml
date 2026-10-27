-- 子は自分の handler を持てる。子の中の `Fail.to_result` は子の中断を子の中で捕まえる。再開する節が継続を末尾で
-- ない位置で呼ぶ handler は、子が yield している途中に方針が横取りを求めても、yield を終えてから切り替える
-- (docs/spec/vm.md の「タスクとスケジュール」)。
import Std.Par
import Fail (Fail(..))

effect Gen where
  emit : Int -> Unit

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

count : Int -> <Gen, Panic> Unit
count n =
  if n == 0 then ()
  else
    emit n
    count (n - 1)

gather : (Unit -> <Gen, Panic> Unit) -> <Panic> List Int
gather producer =
  handle producer () with
    | ctl emit x k -> x :: k ()
    | return _ -> []

sum : List Int -> <Panic> Int
sum xs = match xs with
  | [] -> 0
  | x :: rest -> x + sum rest

check : Int -> <Fail String, Panic> Int
check n = if n > 2 then Fail.fail "big" else n

main : Unit -> <IO, Panic> Unit
main () =
  let (a, b) = Par.par (fn () -> spin 40 + sum (gather (fn () -> count 30))) (fn () -> sum (gather (fn () -> count 20)))
  println (show (a, b))
  let (r, s) = Par.par (fn () -> Fail.to_result (fn () -> spin 40 + check 3)) (fn () -> Fail.to_result (fn () -> check 2))
  println (show (r, s))
