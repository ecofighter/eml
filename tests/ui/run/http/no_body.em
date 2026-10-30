-- `HEAD` の応答と、204 と 304 の応答は、`Content-Length` があっても本文を読まない。1xx の中間の応答は読んで捨て、
-- 次の応答を返す (docs/spec/stdlib.md の「`Std.Http`」)
import Async (Async)
import Async
import Bytes
import List
import Net
import Http

skip_request : Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
skip_request s =
  let (line, s) = Net.read_line s
  match line with
    | Some "" -> s
    | Some _ -> skip_request s
    | None -> s

-- 1つの接続を受けて応答を書く。本文を読まないことを確かめるため、接続はクライアントが閉じるまで閉じない
serve : String -> Net.TcpListener -> <IO, Panic, Async> Net.TcpListener
serve response l =
  let (s, l) = Net.accept l
  let s = Net.write response (skip_request s)
  let (_, s) = Net.read_chunk (Bytes.empty ()) s
  Net.close s
  l

report : Http.Response -> <IO, Panic> Unit
report r =
  println "status: \{show r.status}"
  List.each (fn h -> println "header: \{h.0}: \{h.1}") r.headers
  println "length: \{show (Bytes.length r.body)}"

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let l = serve "HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n" l
    let l = serve "HTTP/1.1 204 No Content\r\nContent-Length: 5\r\n\r\n" l
    let l = serve "HTTP/1.1 304 Not Modified\r\nContent-Length: 5\r\n\r\n" l
    let l = serve "HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 103 Early Hints\r\nLink: </a.css>\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok" l
    Net.close_listener l)
  let url = "http://localhost:\{show port}/"
  report (Http.request "HEAD" url (Bytes.empty ()) Http.defaults)
  report (Http.get url)
  report (Http.get url)
  report (Http.get url)
  Async.await server)
