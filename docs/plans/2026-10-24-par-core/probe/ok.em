import Std.Par
import Fail (Fail(..))

left : Unit -> <Fail String> Int
left () = Fail.fail "no"

main : Unit -> <IO, Panic> Unit
main () =
  let (a, b) = Par.par (fn () -> 1 + 2) (fn () -> 3)
  println (show (a + b))
  let r = Fail.to_result (fn () -> Par.par left (fn () -> 4))
  println (show r)
