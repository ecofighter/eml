-- `x` を読む閉包を外側の `State` の状態に入れると、次の回の本体がそれを取り出して次の回の `x` を読む。E2019
import Std.State

main : Unit -> <IO, Panic> Unit
main () =
  let f = State.eval (fn () -> 0) (fn () ->
    for 0 3 (fn i ->
      var x = i
      State.put (fn () -> x))
    State.get ())
  println "done"
