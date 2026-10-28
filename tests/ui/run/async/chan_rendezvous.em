-- 容量 0 の channel は待ち合わせである。待つ受け手がいなければ `try_send` は `Full` を返し、`send` は受け手が来るまで
-- 待ってから `Sent` を返す (docs/spec/stdlib.md の「`Std.Chan`」)。
import Async (Async)
import Async
import Chan

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let c = Chan.new 0
  println (show (Chan.try_send 1 c))
  let r = Async.start (fn () -> Chan.recv c)
  let _ = spin 40
  println (show (Chan.send 2 c))
  println (show (Async.await r))

main : Unit -> <IO, Panic> Unit
main () = Async.run body
