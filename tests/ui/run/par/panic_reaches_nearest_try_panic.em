-- extern のエフェクトは mask されないので、子の `Panic` は `foo` の row によらず、いちばん内側の `try_panic` に届く
-- (docs/spec/types.md の「推論」)。外側の `try_panic` は内側の結果を値として受け取る。
import Std.Par

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

foo : (Unit -> <never e> Int) -> <Panic | e> Int
foo h = h ()

main : Unit -> <IO, Panic> Unit
main () =
  let r = try_panic (fn () -> try_panic (fn () -> Par.par (fn () -> spin 40 + foo (fn () -> panic "x")) (fn () -> 1)))
  println (show r)
