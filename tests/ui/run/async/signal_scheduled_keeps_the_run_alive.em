-- signal: terminate at 50
-- 予定したシグナルは時間の輪の項なので、シグナルだけを待つ run もデッドロックにならず、時計が予定の時刻まで進む
-- (docs/spec/vm.md の「時間の待ち」)。
import Async
import Signal

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let t = Async.start (fn () -> Signal.wait Signal.Terminate)
    let _ = spin 40
    Async.await t)
  println "terminate came"
