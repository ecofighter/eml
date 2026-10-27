-- 種類 `Cancelled` の IO の失敗で終わったタスクは、スコープの失敗にならない。待たないタスクなら報告されず、`await` は
-- 種類と文言のまま起こし直す (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  Async.spawn (fn () ->
    let _ = spin 40
    fail_io Prelude.Cancelled "gave up")
  let _ = spin 40
  let t = Async.start (fn () -> (fail_io Prelude.Cancelled "stopped" : Int))
  let _ = spin 40
  match try_io (fn () -> Async.await t) with
    | Ok n -> println "unreachable \{show n}"
    | Err e -> println "await: \{show e.kind}: \{e}"

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  println "after run"
