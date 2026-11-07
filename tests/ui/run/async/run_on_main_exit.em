-- メインのworkerのタスクの`exit 4`は、待っている本体を捨てて、その終了コードでrunを終える
-- (docs/spec/stdlib.mdの「`exit`」、「`Std.Async`」の「メインのworker」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let w = Async.start (fn () -> spin 40 + 1)
  let _ = spin 40
  let t = Async.start_on_main (fn () ->
    let _ = spin 40
    println "exiting"
    (exit 4 : Unit))
  let _ = spin 40
  Async.await t
  let _ = Async.await w
  println "unreachable body"

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  Async.run body
  println "unreachable"
