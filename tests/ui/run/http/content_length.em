-- `Content-Length` の本文を読む。要求の行と順、応答のヘッダーの名前を小文字にすること、obs-fold をつなぐこと、
-- 同じ名前のヘッダーを並べたまま残すこと、`Content-Length` の後のバイトを読まないことを確かめる
-- (docs/spec/stdlib.md の「`Std.Http`」)
import Async (Async)
import Async
import List
import Net
import String
import Http

-- 要求の行を空の行まで印字する
print_request : Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
print_request s =
  let (line, s) = Net.read_line s
  match line with
    | Some "" -> s
    | Some text ->
      println "server: \{text}"
      print_request s
    | None -> s

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    let s = print_request s
    Net.close (Net.write "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nX-Multi: a\r\nX-MULTI: b\r\nX-Folded: one\r\n \t two \r\nContent-Length:  5 \r\n\r\nhelloEXTRA" s))
  let r = Http.get "http://localhost:\{show port}/index.html?q=1"
  println "status: \{show r.status}"
  List.each (fn h -> println "header: \{h.0}: \{h.1}") r.headers
  println "body: \{String.decode r.body}"
  Async.await server)
