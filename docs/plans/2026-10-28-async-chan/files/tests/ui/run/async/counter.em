-- `Counter` はタスクの間で共有する数である。`add` は足した後の値を返し、`store` は値を置き換える
-- (docs/spec/stdlib.md の「`Std.Atomic`」)。
import Async (Async)
import Async
import Atomic

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

bump : Int -> Atomic.Counter -> <IO, Panic, Async> Unit
bump n c =
  if n == 0 then ()
  else
    let _ = Atomic.add 1 c
    Async.yield_now ()
    bump (n - 1) c

body : Unit -> <IO, Panic, Async> Unit
body () =
  let c = Atomic.new 0
  let a = Async.start (fn () -> bump 10 c)
  let _ = spin 40
  let b = Async.start (fn () -> bump 10 c)
  let _ = spin 40
  bump 10 c
  Async.await a
  Async.await b
  println (show (Atomic.load c))
  Atomic.store 5 c
  println (show (Atomic.add 2 c))

main : Unit -> <IO, Panic> Unit
main () = Async.run body
