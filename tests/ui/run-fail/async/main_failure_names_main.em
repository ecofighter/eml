-- 根のタスクの捕まえられなかった失敗は、位置を持たず、`main`の名前を付けて報告する。`main`自身で起きても子のタスクで
-- 起きても同じである (docs/spec/vm.md の「誤り、止めること、終わり」)。
import Async (Async)
import Async

main : Unit -> <IO, Panic, Async> Unit
main () =
  Async.spawn (fn () ->
    Async.sleep_ms 10
    panic "boom")
  println "before"
