-- メインのworkerのタスクのPanicは、ほかのタスクの失敗と同じく`await`で起こし直され、
-- `run`が文言を保って起こし直す (docs/spec/stdlib.mdの「`Std.Async`」の「メインのworker」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let n = Async.run_on_main (fn () ->
    let _ = spin 40
    (panic "boom on main" : Int))
  println "unreachable \{show n}"

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  Async.run body
  println "unreachable"
