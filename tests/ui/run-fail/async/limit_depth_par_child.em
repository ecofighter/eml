-- max-depth: 4096
-- タスクの中でも、`par` の右の子の深さの超過は run を止める。子の evv には IO の evidence がないので、
-- 外に `try_all` があっても `ResourceLimit` にならない (docs/spec/vm.md の「深さ」)。
import Async (Async)
import Std.Par
import String

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

dive : Int -> <Panic> Int
dive n =
  let label = show n
  let below = dive (n + 1)
  below + String.byte_length label

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    Async.try_all (fn () ->
      let (a, b) = Par.par (fn () -> spin 40) (fn () -> dive 0)
      a + b))
  let _ = spin 40
  match Async.await t with
    | Ok n -> println "unreachable \{show n}"
    | Err _ -> println "unreachable failure"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
