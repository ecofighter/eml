-- `start` と `await`。待つ順はタスクが終わる順によらず、`run` は本体の値を返す (docs/spec/stdlib.md の「`Std.Async`」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Int
body () =
  let a = Async.start (fn () -> spin 40 + 1)
  let b = Async.start (fn () -> spin 40 + 2)
  let s = spin 40
  let y = Async.await b
  let x = Async.await a
  y * 10 + x + s

main : Unit -> <IO, Panic> Unit
main () = println (show (Async.run body))
