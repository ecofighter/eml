-- `tls-cut.localhost` の終端器は close_notify を送らずに閉じる。EOF までの本文を読むクライアントは、`Std.Tls` の
-- `ConnectionReset` の失敗をそのまま受ける (docs/spec/stdlib.md の「`Std.Http`」、docs/spec/vm.md の「時計と driver」)
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
      Net.close (Net.write "HTTP/1.1 200 OK\r\n\r\npartial" (skip_request s)))
    let r = try_io (fn () -> Http.request "GET" "https://tls-cut.localhost:\{show port}/" (Bytes.empty ()) options)
    Async.await server
    r)
  match r with
    | Ok _ -> println "read"
    | Err e ->
      println (show e.kind)
      fail_io e.kind e.message
