-- 捕まえない `Stream.open` の失敗は run の失敗になる。ないファイルは `FileIn.open` と同じく ``cannot open `p`: not found``
-- で、記録を作らずに投げる (docs/spec/stdlib.md の「`Std.Stream`」)。
import Async (Async)
import Async
import Stream

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

reader : Unit -> <IO, Async> Unit
reader () = Stream.close (Stream.open "missing.txt")

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let t = Async.start reader
  let _ = spin 40
  Async.await t)
