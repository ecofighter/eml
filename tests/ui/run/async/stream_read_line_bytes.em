-- `Stream.read_line_bytes` は `read_line` と同じ1行を、終わりの改行 (`\n` か `\r\n`) を除いたバイト列で返し、UTF-8 で
-- ない行も失敗にしない。同じ行を `read_line` で読むと種類 `NotUtf8` の失敗になり、文言の前にはパスが付く
-- (docs/spec/stdlib.md の「`Std.Stream`」)。
import Async (Async)
import Async
import Byte
import Bytes
import Stream

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

show_line : Option Bytes -> String
show_line line = match line with
  | Some b -> "\{show (Bytes.length b)} bytes"
  | None -> "(eof)"

-- `ok\r\n`、`0xff 0xfe\n`、改行のない `end` を書く
write_lines : Unit -> <IO, Panic, Async> Unit
write_lines () =
  let bad = Bytes.push (Byte.of_int 10) (Bytes.push (Byte.of_int 254) (Bytes.push (Byte.of_int 255) (Bytes.empty ())))
  let w = Stream.create "lines.bin"
  let w = Stream.write "ok\r\n" w
  let (_, w) = Stream.write_bytes bad w
  let w = Stream.write "end" w
  Stream.close_writer w

reader : Unit -> <IO, Panic, Async> Unit
reader () =
  write_lines ()
  let r = Stream.open "lines.bin"
  let (a, r) = Stream.read_line_bytes r
  let (b, r) = Stream.read_line_bytes r
  let (c, r) = Stream.read_line_bytes r
  let (d, r) = Stream.read_line_bytes r
  Stream.close r
  println (show_line a)
  println (show_line b)
  println (show_line c)
  println (show_line d)
  let r = Stream.open "lines.bin"
  let (_, r) = Stream.read_line r
  match try_io (fn () ->
    let (_, r) = Stream.read_line r
    Stream.close r) with
    | Ok _ -> println "read"
    | Err e -> println "\{show e.kind}: \{e.message}"

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let t = Async.start reader
  let _ = spin 40
  Async.await t)
