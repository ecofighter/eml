-- 誰も送らない channel の `recv` は待ち続け、どのタスクも進めなくなると実行時エラー `deadlock` で止まる
-- (docs/spec/stdlib.md の「`Std.Chan`」、docs/spec/vm.md の「時計と driver」)。
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
    println (show (Chan.recv c))
    drop t)
  println "unreachable"
