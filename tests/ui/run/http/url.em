-- URL の読み方と、接続の前に投げる要求の誤り。scheme と host は大文字小文字を区別せず小文字にし、空の path は `/`
-- にし、fragment は送らない。既定のポートでなければ `Host` にポートを書く (docs/spec/stdlib.md の「`Std.Http`」)
import Async (Async)
import Async
import Bytes
import List
import Net
import Http

-- 要求の行と `Host` の行を印字し、空の本文で応える
serve : Net.TcpListener -> <IO, Panic, Async> Net.TcpListener
serve l =
  let (s, l) = Net.accept l
  let (line, s) = Net.read_line s
  let (host, s) = Net.read_line s
  match (line, host) with
    | (Some a, Some b) -> println "server: \{a} | \{b}"
    | _ -> println "server: no request"
  let s = skip_request s
  Net.close (Net.write "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n" s)
  l

skip_request : Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
skip_request s =
  let (line, s) = Net.read_line s
  match line with
    | Some "" -> s
    | Some _ -> skip_request s
    | None -> s

attempt : String -> Bytes -> Http.Options -> String -> <IO, Panic, Async> Unit
attempt method body options url =
  match try_io (fn () -> Http.request method url body options) with
    | Ok r -> println "client: \{show r.status}"
    | Err e -> println "client: \{show e.kind}: \{e.message}"

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let l = serve (serve (serve l))
    Net.close_listener l)
  let get = attempt "GET" (Bytes.empty ()) Http.defaults
  get "HTTP://LocalHost:\{show port}"
  get "http://localhost:\{show port}?a=1&b=2#top"
  get "http://127.0.0.1:\{show port}/a/b/../c?x#y#z"
  List.each get [
    "ftp://localhost/",
    "localhost/",
    "http://",
    "http:///path",
    "http://user:pw@localhost/",
    "http://[::1]:8080/",
    "http://localhost:0/",
    "http://localhost:65536/",
    "http://localhost:/",
    "http://localhost:8a/",
    "http://local host/",
    "http://localhost/caf\u{e9}",
    "http://local%68ost/",
    "http://nowhere.invalid/"]
  attempt "GET POST" (Bytes.empty ()) Http.defaults "http://localhost/"
  attempt "GET" (Bytes.empty ()) { Http.defaults | headers = [("Bad Name", "x")] } "http://localhost/"
  attempt "GET" (Bytes.empty ()) { Http.defaults | headers = [("X-Split", "a\r\nInjected: b")] } "http://localhost/"
  attempt "GET" (Bytes.empty ()) { Http.defaults | headers = [("content-LENGTH", "3")] } "http://localhost/"
  attempt "GET" (Bytes.empty ()) { Http.defaults | headers = [("Host", "elsewhere")] } "http://localhost/"
  Async.await server)
