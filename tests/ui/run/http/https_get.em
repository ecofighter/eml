-- https の要求。`tls.localhost` の終端器が平文を同じリスナーに渡す。http から https へのリダイレクトは追い、
-- EOF までの本文は close_notify で終わる (docs/spec/stdlib.md の「`Std.Http`」、docs/spec/vm.md の「時計と driver」)
import Async (Async)
import Async
import Bytes
import Fs
import Net
import String
import Http

-- 要求の行を空の行まで印字する。閉じる前に読むので、終端器は handshake を終えてから close_notify を送る
print_request : Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
print_request s =
  let (line, s) = Net.read_line s
  match line with
    | Some "" -> s
    | Some text ->
      println "server: \{text}"
      print_request s
    | None -> s

serve : String -> Net.TcpListener -> <IO, Panic, Async> Net.TcpListener
serve response l =
  let (s, l) = Net.accept l
  Net.close (Net.write response (print_request s))
  l

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let options = { Http.defaults | ca = Fs.read_text "ca.pem" }
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let l = serve "HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nsecure" l
    let l = serve "HTTP/1.1 301 Moved Permanently\r\nLocation: https://tls.localhost:\{show port}/upgraded\r\n\r\n" l
    let l = serve "HTTP/1.1 200 OK\r\n\r\nuntil close_notify" l
    Net.close_listener l)
  let r = Http.request "GET" "https://tls.localhost:\{show port}/" (Bytes.empty ()) options
  println "client: \{show r.status} \{String.decode r.body}"
  let r = Http.request "GET" "http://localhost:\{show port}/plain" (Bytes.empty ()) options
  println "client: \{show r.status} \{String.decode r.body}"
  Async.await server)
