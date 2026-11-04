-- `Std.Stream` で書いたファイルを読み直す。`create` は切り詰め、`append` は末尾に足す。書き手は buffer を持たないので、
-- 書き終えた操作の後はすぐ読める。書いたファイルは `--- written:` に出る (docs/spec/stdlib.md の「`Std.Stream`」)。
import Async (Async)
import Async
import Bytes
import Stream

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

writer : Unit -> <IO, Panic, Async> Unit
writer () =
  let w = Stream.create "out.txt"
  let w = Stream.write_line "discarded" w
  Stream.close_writer w
  let w = Stream.create "out.txt"
  let w = Stream.write_line "alpha" w
  let (b, w) = Stream.write_bytes (Bytes.of_string "beta\n") w
  println "wrote \{show (Bytes.length b)} bytes"
  Stream.close_writer w
  let w = Stream.append "out.txt"
  let w = Stream.write "gamma\n" w
  Stream.close_writer w
  let r = Stream.open "out.txt"
  let (text, r) = Stream.read_all r
  Stream.close r
  println text

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let t = Async.start writer
  let _ = spin 40
  Async.await t)
