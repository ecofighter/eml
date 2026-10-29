-- signal: terminate at 5
-- 待つタスクのいない `Terminate` は既定の動きをし、run を `terminated` で止める (docs/spec/stdlib.md の「`Std.Signal`」)。
-- タスクは1つだけにして、止まる位置をスケジュールによらず同じにする
import Async

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  Async.run (fn () -> Async.sleep_ms 100)
  println "unreachable"
