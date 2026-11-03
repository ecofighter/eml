-- `compute` は仕事の後でだけ働く取り消しの地点である。取り消したスコープの中でも仕事は最後まで動き、`shield` の
-- 中では結果が返り、外では結果を落として `Cancelled` を投げる。スコープを離れた後は取り消しが届かない
-- (docs/spec/stdlib.md の「`compute`」)。
import Async (Async)
import MutArray

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () -> spin 40)
  let _ = spin 40
  Async.scope (fn s ->
    Async.cancel_scope s
    let x = Async.shield (fn () -> Async.compute (fn () -> spin 40 + 5))
    println "shielded \{show x}"
    match try_io (fn () -> Async.compute (fn () -> MutArray.make 2 (spin 40))) with
      | Ok a ->
        drop a
        println "unreachable"
      | Err e -> println "compute: \{show e.kind}: \{e}")
  println "requested \{show (Async.cancel_requested ())}"
  println (show (Async.await t))

main : Unit -> <IO, Panic> Unit
main () = Async.run body
