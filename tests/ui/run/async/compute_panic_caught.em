-- 仕事の中の捕まえない `Panic` は、仕事が終わった後に呼んだタスクで起こし直し、`try_all` には `Panicked` として、
-- `try_panic` には文言として届く (docs/spec/stdlib.md の「`compute`」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () -> spin 40)
  let _ = spin 40
  println (show (Async.try_all (fn () -> Async.compute (fn () -> 1 / spin 40))))
  println (show (try_panic (fn () -> Async.compute (fn () -> (panic "boom" : Int)))))
  println (show (Async.await t))

main : Unit -> <IO, Panic> Unit
main () = Async.run body
