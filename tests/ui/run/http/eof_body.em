-- `Content-Length` も `Transfer-Encoding` もない応答は、EOF までを本文にする。server は本文を2回に分けて書く
-- (docs/spec/stdlib.md の「`Std.Http`」)
import Async (Async)
import Async
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
    let s = Net.write "HTTP/1.0 200 OK\r\nServer: test\r\n\r\nfirst part, " s
    Net.close (Net.write "second part" s))
  let r = Http.get "http://localhost:\{show port}"
  println "status: \{show r.status}"
  println "body: \{String.decode r.body}"
  Async.await server)
