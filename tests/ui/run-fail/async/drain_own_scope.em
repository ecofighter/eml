-- `s` に属するタスクが `shield` の中で `drain_within` を呼ぶと、自分の終わりを待つことになり、どのタスクも進めない。
-- 実行時エラー `deadlock` で止まる (docs/spec/stdlib.md の「時間」、docs/spec/vm.md の「時計と driver」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  Async.run (fn () ->
    Async.scope (fn s ->
      Async.spawn_in s (fn () -> Async.shield (fn () -> Async.drain_within 10 s))
      let _ = spin 40
      ()))
  println "unreachable"
