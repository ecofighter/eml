-- 条件つきの `yield` が並ぶ生産者を特殊化する。合流のブロックは継続の関数に写さず、合流のブロックの関数を呼ぶ形に
-- なる (docs/spec/core-ir.md の「handler の複製」)。合流の後でも、前に束縛した変数を読む
import Yield (Yield(..))

gen : Int -> Unit -> <Yield Int> Unit
gen n () =
  let m = n * 10
  if n > 1 then yield 1 else ()
  if n > 2 then yield m else ()
  if n > 3 then yield 3 else yield 4
  yield (m + n)

main : Unit -> <IO> Unit
main () =
  println (show (Yield.fold (fn a b -> a + b) 0 (gen 2)))
  println (show (Yield.fold (fn a b -> a + b) 0 (gen 5)))
  println (show (Yield.fold (fn a b -> a * 10 + b) 0 (gen 3)))
