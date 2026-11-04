-- `Std.Stream` で通常のファイルを読む。`read_line` が読みすぎたバイトは読み手の buffer に残り、続く `read_chunk` が
-- driver の操作を出さずに先に返す。EOF の後の `read_all` は空の文字列を、`read_line` は `None` を返す。ファイルの読みは
-- driver のプールの仕事で、仮想の時計ではテストの driver が待ちの区切りで読む (docs/spec/stdlib.md の「`Std.Stream`」)。
import Async (Async)
import Async
import Bytes
import Stream
import String

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

show_line : Option String -> String
show_line line = match line with
  | Some s -> s
  | None -> "(eof)"

reader : Unit -> <IO, Panic, Async> Unit
reader () =
  let r = Stream.open "stream_read_file.txt"
  let (first, r) = Stream.read_line r
  println (show_line first)
  let (chunk, r) = Stream.read_chunk (Bytes.empty ()) r
  println "chunk: \{show (Bytes.length chunk)} bytes"
  let (rest, r) = Stream.read_all r
  println "rest: \{show (String.byte_length rest)} bytes"
  let (last, r) = Stream.read_line r
  println (show_line last)
  Stream.close r
  let r = Stream.open "stream_read_file.txt"
  let (all, r) = Stream.read_all r
  Stream.close r
  println "all: \{show (String.byte_length all)} bytes"

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let t = Async.start reader
  let _ = spin 40
  Async.await t)
