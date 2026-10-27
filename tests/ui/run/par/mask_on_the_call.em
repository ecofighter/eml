-- `par` の呼び出しそのものに mask が付く。`foo` の row は `Fail String` を足すので、子の `Fail` の中断は `foo` の
-- 内側の `to_result` を飛ばして外側に届く (docs/spec/core-ir.md の「`par` の呼び出し」)。
import Std.Par
import Fail (Fail(..))

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

foo : (Unit -> <never e> Int) -> <Fail String | e> (Int, Int)
foo h = Par.par h h

main : Unit -> <IO, Panic> Unit
main () =
  let r = Fail.to_result (fn () -> Fail.to_result (fn () -> foo (fn () -> spin 40 + Fail.fail 7)))
  println (show r)
