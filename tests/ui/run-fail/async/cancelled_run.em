-- `run` の本体の `Cancelled` は、ほかの IO の失敗と同じく `run` から出る。捕まえなければ実行時エラーになる
-- (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()

body : Unit -> <IO, Panic, Async> Int
body () =
  Async.scope (fn s ->
    Async.spawn_in s (fn () ->
      let _ = spin 400
      Async.cancel_scope s)
    let _ = spin 40
    let u = Async.start forever
    let _ = spin 40
    Async.await u)

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  let n = Async.run body
  println "unreachable \{show n}"
