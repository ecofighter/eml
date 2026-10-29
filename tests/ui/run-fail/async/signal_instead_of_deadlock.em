-- signal: terminate at 50
-- 誰も送らない `Chan.recv` で待つ run は、予定したシグナルがあるのでデッドロックにならず、時計が予定の時刻まで
-- 進む。待つタスクのいない `Terminate` が run を `terminated` で止める (docs/spec/vm.md の「時間の待ち」)。2つの
-- タスクがどちらも `Chan.recv` で待つので、止まる位置はどのタスクが最後に待ったかによらない
import Async
import Chan

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  Async.run (fn () ->
    let c : Chan.Chan Int = Chan.new 0
    let t = Async.start (fn () -> Chan.recv c)
    let _ = spin 40
    let _ = Chan.recv c
    let _ = Async.await t
    Chan.close c)
  println "unreachable"
