-- 仕事の `Lin` の結果は、呼んだタスクに移って届く。検査モードで漏れが出ない (docs/spec/stdlib.md の「`compute`」)。
import Async (Async)
import MutArray

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () -> spin 40)
  let _ = spin 40
  let a = Async.compute (fn () -> MutArray.make 3 (spin 40 + 7))
  let (n, a) = MutArray.length a
  let (x, a) = MutArray.read a 0
  drop a
  println "\{show n} \{show x}"
  println (show (Async.await t))

main : Unit -> <IO, Panic> Unit
main () = Async.run body
