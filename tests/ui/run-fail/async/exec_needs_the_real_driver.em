-- 仮想の時計のテストの driver は子を扱わないので、`Exec.spawn` はどの worker でも、driver に触れる前に
-- `NotAvailable` で失敗する。捕まえなければ run の失敗である (docs/spec/stdlib.md の「`Std.Exec`」)。子は起動しない
import Async (Async)
import Async
import Exec

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

runner : Unit -> <IO, Async> Unit
runner () = drop (Exec.spawn `eml --version`)

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let t = Async.start runner
  let _ = spin 40
  Async.await t)
