-- signal: terminate at 50
-- 誰も送らない `Chan.recv` で待つ run は、予定したシグナルがあるのでデッドロックにならず、時計が予定の時刻まで
-- 進む。待つタスクのいない `Terminate` が run を `terminated` で止める (docs/spec/vm.md の「時間の待ち」)。タスクは
-- 1つだけにして、止まる位置をスケジュールによらず同じにする
import Async
import Chan

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  Async.run (fn () ->
    let c : Chan.Chan Int = Chan.new 0
    let _ = Chan.recv c
    Chan.close c)
  println "unreachable"
