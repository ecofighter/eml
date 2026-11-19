-- rowに`Async`を持つ`main`は、runの根のタスクである。runは、始めたタスクが待たれていない子も含めてすべて終わってから
-- 終わる (docs/spec/stdlib.md の「`Std.Async`」)。
import Async (Async)
import Async

main : Unit -> <IO, Panic, Async> Unit
main () =
  let t = Async.start (fn () -> 41 + 1)
  Async.spawn (fn () ->
    Async.sleep_ms 10
    println "late child")
  println "answer \{show (Async.await t)}"
  println "main returns"
