-- タスクの中の入れ子の `run` は自分の根のスコープを開き、外の `run` の後に2つ目の `run` も流せる
-- (docs/spec/stdlib.md の「`Std.Async`」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    let n = Async.run (fn () ->
      let c = Async.start (fn () -> spin 40 + 2)
      let _ = spin 40
      Async.await c * 10)
    println "inner \{show n}"
    n + 1)
  let _ = spin 40
  let m = Async.await t
  println "outer \{show m}"

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  println (show (Async.run (fn () -> 3)))
