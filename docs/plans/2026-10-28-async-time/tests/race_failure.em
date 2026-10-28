-- `race` の本体の失敗はもう一方を取り消し、`race` から起こし直される。種類 `Cancelled` で終わった本体は勝たず、
-- もう一方を取り消さない。両方がそうなら `race` は1つ目の本体の失敗を起こし直す。呼んだタスクが取り消されると、
-- 両方を取り消して `Cancelled` を起こし直す (docs/spec/stdlib.md の「時間」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

after : Int -> a -> <IO, Async> a
after ms x =
  Async.sleep_ms ms
  x

failing : Unit -> <IO, Panic, Async> Int
failing () =
  let _ = spin 40
  Async.sleep_ms 50
  panic "arm failed"

other : Unit -> <IO, Panic, Async> Int
other () =
  let _ = spin 40
  match try_io (fn () -> Async.sleep_ms 100) with
    | Ok () -> 1
    | Err e ->
      println "other: \{show e.kind}"
      2

gives_up : String -> Int -> Unit -> <IO, Panic, Async> Int
gives_up name ms () =
  let _ = spin 40
  Async.sleep_ms ms
  fail_io Cancelled name

report : String -> (Unit -> <IO, Panic, Async> Either Int Int) -> <IO, Panic, Async> Unit
report name f =
  match Async.try_all f with
    | Ok r -> println "\{name}: \{show r}"
    | Err e -> println "\{name}: \{show e}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  report "panic" (fn () -> Async.race failing other)
  report "one gives up" (fn () -> Async.race (gives_up "a" 10) (fn () -> after 50 2))
  report "both give up" (fn () -> Async.race (gives_up "a" 20) (gives_up "b" 10))
  report "caller cancelled" (fn () ->
    Async.scope (fn s ->
      Async.spawn_in s (fn () ->
        Async.sleep_ms 20
        Async.cancel_scope s)
      let _ = spin 40
      Async.race (fn () -> after 50 1) (fn () -> after 100 2)))

main : Unit -> <IO, Panic> Unit
main () = Async.run body
