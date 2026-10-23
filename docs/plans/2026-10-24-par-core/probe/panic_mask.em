import Std.Par

foo : (Unit -> <never e> Int) -> <Panic | e> Int
foo h = h ()

main : Unit -> <IO, Panic> Unit
main () =
  let r = try_panic (fn () -> try_panic (fn () -> Par.par (fn () -> foo (fn () -> panic "x")) (fn () -> 1)))
  println (show r)
