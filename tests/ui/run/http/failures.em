-- 読めない応答と読めない `Location` の IO の失敗。接続ごとに1つの壊れた応答を書き、クライアントは失敗の種類と文言を印字する
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

-- クライアントは誤りを見つけたところで接続を落とすので、書きの失敗は捨てる
serve : Net.TcpListener -> Bytes -> <IO, Panic, Async> Net.TcpListener
serve l response =
  let (s, l) = Net.accept l
  let _ = try_io (fn () ->
    let (_, s) = Net.write_bytes response (skip_request s)
    Net.close s)
  l

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let long = String.decode (Bytes.make 65537 97u8)
  -- `X-Ok: ` の6バイトと合わせて、行の中身がちょうど上限の 65536 バイトになる
  let longest = String.decode (Bytes.make 65530 97u8)
  let responses = [
    "",
    "HTTP/1.1 200 OK\r\nContent-Length: 5\r\n",
    "HTTP/2 200 OK\r\n\r\n",
    "HTTP/1.1 20 OK\r\n\r\n",
    "HTTP/1.1 200 OK\r\nX-Long: \{long}\r\n\r\n",
    "HTTP/1.1 200 OK\r\nBad Name: x\r\n\r\n",
    "HTTP/1.1 200 OK\r\n folded first\r\n\r\n",
    "HTTP/1.1 200 OK\r\nContent-Length: 5, 6\r\n\r\nhello",
    "HTTP/1.1 200 OK\r\nContent-Length: -1\r\n\r\n",
    "HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nhello",
    "HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip, chunked\r\n\r\n",
    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n",
    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabcd\r\n0\r\n\r\n",
    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhel",
    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nX-Trailer: t\r\n",
    "HTTP/1.1 302 Found\r\nLocation: next?u=http://x\r\n\r\n",
    "HTTP/1.1 200 OK\r\nX-Ok: \{longest}\r\nContent-Length: 0\r\n\r\n",
    "HTTP/1.1 302 Found\r\nLocation: \r\n\r\n",
    "HTTP/1.1 200 OK\r\nTransfer-Encoding: \r\n\r\n"]
  -- UTF-8 でない行は文字列では書けないので、バイトで組む
  let not_utf8 = Bytes.append (Bytes.append (Bytes.of_string "HTTP/1.1 200 OK\r\nX-Bad: ") (Bytes.make 1 255u8)) (Bytes.of_string "\r\n\r\n")
  let responses = List.append (List.map Bytes.of_string responses) [not_utf8]
  let server = Async.start (fn () ->
    let l = List.fold serve l responses
    Net.close_listener l)
  let url = "http://localhost:\{show port}/"
  List.each (fn _ ->
    match try_io (fn () -> Http.get url) with
      | Ok r -> println "ok: \{show r.status}"
      | Err e -> println "\{show e.kind}: \{e.message}") responses
  Async.await server)
