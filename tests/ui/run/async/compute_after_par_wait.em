-- worker が1つでも、`par` の join で待つタスクと、ほかのタスクが出した仕事は、同じ実行待ちに並んで動く
-- (docs/spec/stdlib.md の「`compute`」)。
import Async (Async)
import Std.Par

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

sum : Int -> Int -> <Panic> Int
sum acc n = if n == 0 then acc else sum (acc + n) (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () -> spin 40)
  let _ = spin 40
  let u = Async.start (fn () ->
    let (a, b) = Par.par (fn () -> spin 400 + 1) (fn () -> spin 400 + 2)
    a + b)
  let x = Async.compute (fn () -> sum 0 4)
  println (show (Async.await u))
  println (show x)
  println (show (Async.await t))

main : Unit -> <IO, Panic> Unit
main () = Async.run body
