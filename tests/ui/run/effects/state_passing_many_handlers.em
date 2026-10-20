-- 状態つきの3つの handler を重ねたループ。どの handler も状態を渡す形で特殊化され、内側から順に複製を重ねる。
-- 3回目の複製は3つの状態を1つのタプルで返す (docs/spec/core-ir.md の「handler の複製」)。
effect A where
  bump_a : Unit -> Unit

effect B where
  bump_b : Unit -> Unit

effect C where
  bump_c : Unit -> Unit

-- 再帰するので融合で展開されず、複製のまま残る
step : Int -> <A, B, C, Panic> Unit
step k =
  if k == 0 then ()
  else
    bump_a ()
    bump_b ()
    bump_c ()
    step (k - 1)

loop : Int -> <A, B, C, Panic> Unit
loop i =
  if i == 0 then ()
  else
    step 1
    loop (i - 1)

run_a : (Unit -> <A | e> x) -> <Panic | e> (x, Int)
run_a action =
  handle action () from 0 with
    | fn bump_a () s -> ((), s + 1)
    | return x s -> (x, s)

run_b : (Unit -> <B | e> x) -> <Panic | e> (x, Int)
run_b action =
  handle action () from 0 with
    | fn bump_b () s -> ((), s + 1)
    | return x s -> (x, s)

run_c : (Unit -> <C | e> x) -> <Panic | e> (x, Int)
run_c action =
  handle action () from 0 with
    | fn bump_c () s -> ((), s + 1)
    | return x s -> (x, s)

main : Unit -> <IO, Panic> Unit
main () =
  let (((_, c), b), a) = run_a (fn () -> run_b (fn () -> run_c (fn () -> loop 1000)))
  println "\{a} \{b} \{c}"
