-- chunked の本文を読む。大文字の `Chunked`、chunk の拡張、16進の大文字と小文字、読みをまたぐ 100 000 バイトの chunk、
-- trailer を確かめる。`Content-Length` が一緒にあっても chunked で読み、trailer は `headers` に入れない
-- (docs/spec/stdlib.md の「`Std.Http`」)
import Async (Async)
import Async
import Bytes
import List
import Net
import String
import Http

skip_request : Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
skip_request s =
  let (line, s) = Net.read_line s
  match line with
    | Some "" -> s
    | Some _ -> skip_request s
    | None -> s

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    let s = skip_request s
    let s = Net.write "HTTP/1.1 200 OK\r\nTransfer-Encoding: Chunked\r\nContent-Length: 3\r\n\r\n" s
    let s = Net.write "5;name=value\r\nhello\r\nb \r\n, chunked! \r\n186a0\r\n" s
    let (_, s) = Net.write_bytes (Bytes.make 100000 120u8) s
    Net.close (Net.write "\r\n1F\r\n and the last one, of 31 bytes.\r\n0\r\nX-Trailer: t\r\n\r\n" s))
  let r = Http.get "http://localhost:\{show port}/"
  println "status: \{show r.status}"
  List.each (fn h -> println "header: \{h.0}: \{h.1}") r.headers
  let n = Bytes.length r.body
  println "length: \{show n}"
  println "head: \{String.decode (Bytes.slice 0 16 r.body)}"
  println "tail: \{String.decode (Bytes.slice (n - 32) n r.body)}"
  Async.await server)
