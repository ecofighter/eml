import Std.Par
import Fail (Fail(..))

-- 本体の row は `<Fail String | e>`。par の結果の row `<e>` を含めると Fail の mask が付くはず
foo : (Unit -> <never e> Int) -> <Fail String | e> (Int, Int)
foo h = Par.par h h

main : Unit -> <IO, Panic> Unit
main () =
  let r = Fail.to_result (fn () -> Fail.to_result (fn () -> foo (fn () -> Fail.fail 7)))
  println (show r)
