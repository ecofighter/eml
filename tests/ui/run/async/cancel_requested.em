-- `cancel_requested` はスコープの旗も読み、`shield` の中では偽を返す。`check_cancelled` は旗で投げ、スコープを出た
-- 後は偽に戻る (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

inner : Async.Scope -> <IO, Panic, Async> Unit
inner s =
  Async.spawn_in s (fn () ->
    let _ = spin 40
    ())
  let _ = spin 40
  println "before \{show (Async.cancel_requested ())}"
  Async.cancel_scope s
  println "after \{show (Async.cancel_requested ())}"
  let shielded = Async.shield (fn () -> Async.cancel_requested ())
  println "in shield \{show shielded}"
  Async.check_cancelled ()
  println "unreachable"

body : Unit -> <IO, Panic, Async> Unit
body () =
  match try_io (fn () -> Async.scope inner) with
    | Ok () -> println "unreachable"
    | Err e -> println "scope: \{show e.kind}: \{e}"
  println "outside \{show (Async.cancel_requested ())}"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
