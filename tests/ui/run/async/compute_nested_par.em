-- 仕事の中の `par` は、仕事の中で fork して join する (docs/spec/stdlib.md の「`compute`」)。
import Async (Async)
import Std.Par

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () -> spin 40)
  let _ = spin 40
  let n = Async.compute (fn () ->
    let (a, b) = Par.par (fn () -> spin 40 + 3) (fn () -> spin 40 + 5)
    a + b)
  println (show n)
  println (show (Async.await t))

main : Unit -> <IO, Panic> Unit
main () = Async.run body
