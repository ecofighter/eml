-- `compute f` は `f ()` を仕事として動かし、呼んだタスクだけが待って結果を受け取る。始めたタスクの中の `compute` も
-- 同じく動く (docs/spec/stdlib.md の「`compute`」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

sum : Int -> Int -> <Panic> Int
sum acc n = if n == 0 then acc else sum (acc + n) (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () -> spin 40)
  let _ = spin 40
  println "a \{show (Async.compute (fn () -> sum 0 10))}"
  let u = Async.start (fn () -> Async.compute (fn () -> sum 0 100))
  println "t \{show (Async.await u)}"
  println (show (Async.await t))

main : Unit -> <IO, Panic> Unit
main () = Async.run body
