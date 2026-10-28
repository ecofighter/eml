-- `shield` の中の取り消しの地点は旗を見ず、待ちは最後まで終わる。`shield` を出た後の最初の取り消しの地点が投げる
-- (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

inner : Async.Scope -> <IO, Panic, Async> Unit
inner s =
  Async.cancel_scope s
  let n = Async.shield (fn () ->
    let t = Async.start (fn () -> spin 40 + 7)
    let _ = spin 40
    Async.yield_now ()
    Async.await t)
  println "shielded \{show n}"
  Async.yield_now ()
  println "unreachable"

body : Unit -> <IO, Panic, Async> Unit
body () =
  match try_io (fn () -> Async.scope inner) with
    | Ok () -> println "unreachable"
    | Err e -> println "after shield: \{show e.kind}: \{e}"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
