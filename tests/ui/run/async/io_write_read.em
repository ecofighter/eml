-- `IoTest.read_chunk` は、書いたバイトを読んで buffer の後ろに足す。書き手の端を落とした後の読みは EOF で、何も
-- 足さない (docs/spec/stdlib.md の「`Std.IoTest`」)。
import Async (Async)
import Async
import Bytes
import IoTest
import String

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

echo : Unit -> <IO, Panic, Async> Unit
echo () =
  let (a, b) = IoTest.pair ()
  let (_, a) = IoTest.write (Bytes.of_string "hello") a
  let (buf, b) = IoTest.read_chunk (Bytes.of_string "> ") b
  println (String.decode buf)
  drop a
  let (rest, b) = IoTest.read_chunk (Bytes.empty ()) b
  drop b
  if Bytes.length rest == 0 then println "eof" else println "data"

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let t = Async.start echo
  let _ = spin 40
  Async.await t)
