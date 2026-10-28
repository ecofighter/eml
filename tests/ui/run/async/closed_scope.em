-- 閉じたスコープの `Scope` を外へ出すと、`spawn_in` と `start_in` は Panic `the scope is closed` を起こし、
-- `cancel_scope` は何もしない (docs/spec/stdlib.md の「スコープ」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let s = Async.scope (fn s ->
    Async.spawn_in s (fn () ->
      let _ = spin 40
      println "in scope")
    let _ = spin 40
    s)
  Async.cancel_scope s
  match try_panic (fn () -> Async.spawn_in s (fn () -> println "unreachable")) with
    | Ok () -> println "unreachable"
    | Err m -> println "panicked: \{m}"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
