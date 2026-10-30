-- 307 と 308 は同じメソッドと本文で送り直す。301 と 302 は `GET` にするが、`HEAD` は `HEAD` のままにする。
-- 回数は `max_redirects` まで追え、`max_redirects` が 0 ならリダイレクトの応答をそのまま返す
-- (docs/spec/stdlib.md の「`Std.Http`」)
import Async (Async)
import Async
import Bytes
import Net
import String
import Http

serve : String -> Net.TcpListener -> <IO, Panic, Async> Net.TcpListener
serve response l =
  let (s, l) = Net.accept l
  let (n, s) = print_head 0 s
  let (body, s) = read_body n (Bytes.empty ()) s
  if n > 0 then println "server: body \{String.decode body}" else ()
  Net.close (Net.write response s)
  l

-- 要求の行と `Content-Length` の行だけを印字する
print_head : Int -> Net.TcpStream -> <IO, Panic, Async> (Int, Net.TcpStream)
print_head n s =
  let (line, s) = Net.read_line s
  match line with
    | Some "" -> (n, s)
    | Some text ->
      if String.starts_with "Content-Length: " text then
        println "server: \{text}"
        match String.to_int (String.slice 16 (String.byte_length text) text) with
          | Some m -> print_head m s
          | None -> print_head n s
      else if String.contains " HTTP/1.1" text then
        println "server: \{text}"
        print_head n s
      else print_head n s
    | None -> (n, s)

read_body : Int -> Bytes -> Net.TcpStream -> <IO, Panic, Async> (Bytes, Net.TcpStream)
read_body n b s =
  if Bytes.length b >= n then (b, s)
  else
    let (b, s) = Net.read_chunk b s
    read_body n b s

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let l = serve "HTTP/1.1 307 Temporary Redirect\r\nLocation: /b\r\n\r\n" l
    let l = serve "HTTP/1.1 308 Permanent Redirect\r\nLocation: /c\r\n\r\n" l
    let l = serve "HTTP/1.1 302 Found\r\nLocation: /d\r\n\r\n" l
    let l = serve "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok" l
    let l = serve "HTTP/1.1 301 Moved Permanently\r\nLocation: /f\r\n\r\n" l
    let l = serve "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n" l
    let l = serve "HTTP/1.1 302 Found\r\nLocation: /h\r\nContent-Length: 4\r\n\r\nmove" l
    Net.close_listener l)
  let options = { Http.defaults | max_redirects = 3 }
  let r = Http.request "PUT" "http://localhost:\{show port}/a" (Bytes.of_string "data") options
  println "client: \{show r.status} \{String.decode r.body}"
  let r = Http.request "HEAD" "http://localhost:\{show port}/e" (Bytes.empty ()) options
  println "client: \{show r.status} \{show (Bytes.length r.body)}"
  let r = Http.request "GET" "http://localhost:\{show port}/g" (Bytes.empty ()) { Http.defaults | max_redirects = 0 }
  println "client: \{show r.status} \{String.decode r.body}"
  Async.await server)
