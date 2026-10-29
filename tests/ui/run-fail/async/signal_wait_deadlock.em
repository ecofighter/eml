-- テストの driver の run では、予定のないシグナルは来ないので、シグナルだけを待つ run はデッドロックになる
-- (docs/spec/vm.md の「時計と driver」)。
import Async
import Signal

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  Async.run (fn () ->
    let t = Async.start (fn () -> Signal.wait Signal.Terminate)
    let _ = spin 40
    Async.await t)
  println "unreachable"
