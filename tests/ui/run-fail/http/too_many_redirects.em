-- `max_redirects` を超えるリダイレクトは IO の失敗になる。上限が 2 なら、3つ目の 302 で失敗する
-- (docs/spec/stdlib.md の「`Std.Http`」)
import Async (Async)
import Async
import Bytes
import Net
import Http

skip_request : Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
skip_request s =
  let (line, s) = Net.read_line s
  match line with
    | Some "" -> s
    | Some _ -> skip_request s
    | None -> s

serve : Net.TcpListener -> <IO, Panic, Async> Net.TcpListener
serve l =
  let (s, l) = Net.accept l
  Net.close (Net.write "HTTP/1.1 302 Found\r\nLocation: /again\r\n\r\n" (skip_request s))
  l

main : Unit -> <IO, Panic> Unit
main () =
  let r = Async.run (fn () ->
    let l = Net.listen "127.0.0.1" 0
    let (port, l) = Net.local_port l
    let server = Async.start (fn () -> Net.close_listener (serve (serve (serve l))))
    let options = { Http.defaults | max_redirects = 2 }
    let r = try_io (fn () -> Http.request "GET" "http://localhost:\{show port}/" (Bytes.empty ()) options)
    Async.await server
    r)
  match r with
    | Ok _ -> println "followed"
    | Err e ->
      println (show e.kind)
      fail_io e.kind e.message
