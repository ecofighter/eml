-- `await` の周りで失敗を捕まえても、スコープの失敗は残り `run` が起こし直す。IO の失敗の種類は `await` を通っても
-- 残る (docs/spec/stdlib.md の「`Std.Async`」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    let _ = spin 40
    (fail_io TimedOut "too slow" : Int))
  let _ = spin 40
  match try_io (fn () -> Async.await t) with
    | Ok n -> println "ok \{show n}"
    | Err e -> println "caught \{show e.kind}: \{e}"

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  Async.run body
  println "unreachable"
