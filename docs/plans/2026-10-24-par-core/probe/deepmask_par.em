import Std.Par
import Fail (Fail(..))

foo : (Unit -> <never e> Int) -> <Fail String | e> Int
foo h = h ()

main : Unit -> <IO, Panic> Unit
main () =
  let r = Fail.to_result (fn () -> Fail.to_result (fn () -> Par.par (fn () -> foo (fn () -> Fail.fail 7)) (fn () -> 1)))
  println (show r)
