-- `await` は子の Panic を起こし直す。失敗はスコープの失敗のままで、`run` が文言を保って起こし直す
-- (docs/spec/stdlib.md の「`Std.Async`」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    let _ = spin 40
    (panic "boom" : Int))
  let _ = spin 40
  let n = Async.await t
  println "unreachable \{show n}"

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  Async.run body
  println "unreachable"
