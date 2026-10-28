-- 呼んだタスクが外から取り消されていれば、期限が過ぎていても `timeout` は `None` を返さずに `Cancelled` を起こし直す。
-- `shield` の中で開いた `timeout` には外の取り消しが届かず、期限は届く (docs/spec/stdlib.md の「時間」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

after : Int -> a -> <IO, Async> a
after ms x =
  Async.sleep_ms ms
  x

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
  let waiting = try_io (fn () ->
    Async.scope (fn s ->
      Async.spawn_in s (fn () ->
        Async.sleep_ms 20
        Async.cancel_scope s)
      let _ = spin 40
      Async.timeout 50 (fn () -> after 100 1)))
  match waiting with
    | Ok r -> println (show r)
    | Err e -> println "cancelled while waiting: \{show e.kind}"
  let past = try_io (fn () ->
    Async.scope (fn s ->
      Async.cancel_scope s
      Async.timeout 0 (fn () -> after 10 2)))
  match past with
    | Ok r -> println (show r)
    | Err e -> println "cancelled with a past deadline: \{show e.kind}"
  let shielded = try_io (fn () ->
    Async.scope (fn s ->
      Async.cancel_scope s
      let a = Async.shield (fn () -> Async.timeout 100 (fn () -> after 50 3))
      let b = Async.shield (fn () -> Async.timeout 50 (fn () -> after 100 4))
      println "shielded: \{show a} \{show b}"
      Async.yield_now ()))
  match shielded with
    | Ok () -> println "unreachable"
    | Err e -> println "after the shield: \{show e.kind}"
  bit_first (fn () ->
    Async.shield (fn () ->
      Async.yield_now ()
      let a = Async.timeout 100 (fn () -> after 10 1)
      let b = Async.timeout 10 (fn () -> after 100 2)
      println "bit first: \{show a} \{show b}")
    Async.yield_now ())

main : Unit -> <IO, Panic> Unit
main () = Async.run body
