-- `IoTest.read_chunk_within` は、期限までに読めなければ `None` と端を返し、端は続けて使える。期限の前に読めれば、
-- 期限を付けても `Some` を返す。テストの driver は終わる操作がないときだけ時計を進めるので、どの seed でも同じ結果
-- になる (docs/spec/stdlib.md の「`Std.IoTest`」)。
import Async (Async)
import Async
import Bytes
import IoTest
import String

show_read : Option Bytes -> <IO, Panic> Unit
show_read r = match r with
  | Some buf -> println "Some \{String.decode buf}"
  | None -> println "None"

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

reads : Unit -> <IO, Panic, Async> Unit
reads () =
  let (a, b) = IoTest.pair ()
  let (r, b) = IoTest.read_chunk_within 50 (Bytes.empty ()) b
  show_read r
  let (_, a) = IoTest.write (Bytes.of_string "hello") a
  let (buf, b) = IoTest.read_chunk (Bytes.empty ()) b
  println (String.decode buf)
  let (_, a) = IoTest.write (Bytes.of_string "again") a
  let (r, b) = IoTest.read_chunk_within 100 (Bytes.of_string "> ") b
  show_read r
  drop a
  drop b

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let t = Async.start reads
  let _ = spin 40
  Async.await t)
