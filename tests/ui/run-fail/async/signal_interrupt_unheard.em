-- signal: interrupt at 5
-- 待つタスクのいない `Interrupt` は既定の動きをし、run を `interrupted` で止める (docs/spec/stdlib.md の「`Std.Signal`」)。
-- 2つのタスクがどちらも眠りで待つので、止まる位置はどのタスクが最後に待ったかによらない
import Async

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  Async.run (fn () ->
    let t = Async.start (fn () -> Async.sleep_ms 100)
    let _ = spin 40
    Async.sleep_ms 100
    Async.await t)
  println "unreachable"
