-- https から http へのリダイレクトは断る。リダイレクトの先には接続しない (docs/spec/stdlib.md の「`Std.Http`」)
import Async (Async)
import Async
import Bytes
import Fs
import Net
import Http

skip_request : Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
skip_request s =
  let (line, s) = Net.read_line s
  match line with
    | Some "" -> s
    | Some _ -> skip_request s
    | None -> s

main : Unit -> <IO, Panic> Unit
main () =
  let r = Async.run (fn () ->
    let options = { Http.defaults | ca = Fs.read_text "ca.pem" }
    let l = Net.listen "127.0.0.1" 0
    let (port, l) = Net.local_port l
    let server = Async.start (fn () ->
      let (s, l) = Net.accept l
      Net.close_listener l
      let response = "HTTP/1.1 301 Moved Permanently\r\nLocation: http://localhost:\{show port}/plain\r\n\r\n"
      Net.close (Net.write response (skip_request s)))
    let r = try_io (fn () -> Http.request "GET" "https://tls.localhost:\{show port}/" (Bytes.empty ()) options)
    Async.await server
    r)
  match r with
    | Ok _ -> println "followed"
    | Err e ->
      println (show e.kind)
      fail_io e.kind e.message
