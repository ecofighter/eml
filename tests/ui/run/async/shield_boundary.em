-- `shield` の中で開いたスコープには、外の取り消しが届かない。そのスコープ自身の旗は、`shield` の中でも本体と子に
-- 届く。タスクのビットが `shield` の中でスコープを開く前に立っていても、同じである。`shield` を出た後の最初の
-- 取り消しの地点が、外の取り消しで投げる (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

worker : String -> Int -> Unit -> <IO, Panic, Async> Unit
worker name ms () =
  match try_io (fn () -> Async.sleep_ms ms) with
    | Ok () -> println "\{name}: done"
    | Err e -> println "\{name}: \{show e.kind}"

-- 子を `s` に始め、子が `shield` の中で待つ間に `s` を取り消す。子のビットは、子が `shield` の中でスコープを開く前に
-- 立つ
bit_first : (Unit -> <IO, Panic, Async> Unit) -> <IO, Panic, Async> Unit
bit_first child =
  Async.scope (fn s ->
    Async.spawn_in s (fn () ->
      match try_io child with
        | Ok () -> println "unreachable"
        | Err e -> println "after the shield: \{show e.kind}")
    let _ = spin 40
    Async.yield_now ()
    Async.cancel_scope s)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let outer = try_io (fn () ->
    Async.scope (fn s ->
      Async.cancel_scope s
      Async.shield (fn () ->
        Async.scope (fn inner ->
          Async.spawn_in inner (worker "child" 10)
          let _ = spin 40
          Async.yield_now ()
          println "the shielded body runs"))
      println "back outside"
      Async.yield_now ()))
  match outer with
    | Ok () -> println "unreachable"
    | Err e -> println "after the shield: \{show e.kind}"
  let inner = try_io (fn () ->
    Async.shield (fn () ->
      Async.scope (fn inner ->
        Async.cancel_scope inner
        Async.yield_now ()
        println "unreachable")))
  match inner with
    | Ok () -> println "unreachable"
    | Err e -> println "inner flag: \{show e.kind}"
  bit_first (fn () ->
    Async.shield (fn () ->
      Async.yield_now ()
      Async.scope (fn _ ->
        Async.check_cancelled ()
        Async.scope (fn _ ->
          Async.check_cancelled ()
          println "nested scopes in the shield run")))
    Async.yield_now ())

main : Unit -> <IO, Panic> Unit
main () = Async.run body
