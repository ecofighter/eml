-- 誰も接続しないリスナーの `accept` は待ち続け、どのタスクも進めなくなると実行時エラー `deadlock` で止まる
-- (docs/spec/vm.md の「時計と driver」)。
import Async
import Net

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  Async.run (fn () ->
    let l = Net.listen "127.0.0.1" 0
    let t = Async.start (fn () ->
      let (s, l) = Net.accept l
      Net.close_listener l
      Net.close s)
    let _ = spin 40
    Async.await t)
  println "unreachable"
