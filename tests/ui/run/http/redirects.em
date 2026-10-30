-- リダイレクトの連鎖。`POST` の後の 303 は本文のない `GET` にして `Content-Type` を落とし、同じオリジンの
-- `Authorization` は残す。`localhost` から `127.0.0.1` へはオリジンが変わるので、`Authorization` と `Cookie` を
-- 落とす。`Location` のない 302 は、そのまま応答として返す (docs/spec/stdlib.md の「`Std.Http`」)
import Async (Async)
import Async
import Bytes
import Net
import String
import Http

-- 要求の行を印字し、`Content-Length` の本文を読んで印字してから、応答を書いて閉じる
serve : String -> Net.TcpListener -> <IO, Panic, Async> Net.TcpListener
serve response l =
  let (s, l) = Net.accept l
  let (n, s) = print_head 0 s
  let (body, s) = read_body n (Bytes.empty ()) s
  if n > 0 then println "server: body \{String.decode body}" else ()
  Net.close (Net.write response s)
  l

print_head : Int -> Net.TcpStream -> <IO, Panic, Async> (Int, Net.TcpStream)
print_head n s =
  let (line, s) = Net.read_line s
  match line with
    | Some "" -> (n, s)
    | Some text ->
      println "server: \{text}"
      let n = if String.starts_with "Content-Length: " text then
          match String.to_int (String.slice 16 (String.byte_length text) text) with
            | Some m -> m
            | None -> n
        else n
      print_head n s
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
    let l = serve "HTTP/1.1 303 See Other\r\nLocation: /see\r\nContent-Length: 4\r\n\r\nskip" l
    let l = serve "HTTP/1.1 301 Moved Permanently\r\nLocation:  http://127.0.0.1:\{show port}/moved \r\n\r\n" l
    let l = serve "HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nhere" l
    let l = serve "HTTP/1.1 302 Found\r\nContent-Length: 11\r\n\r\nno location" l
    Net.close_listener l)
  let headers = [("Content-Type", "text/plain"), ("Authorization", "Bearer t"), ("Cookie", "k=v"), ("Accept", "*/*")]
  let options = { Http.defaults | headers }
  let r = Http.request "POST" "http://localhost:\{show port}/form" (Bytes.of_string "a=1") options
  println "client: \{show r.status} \{String.decode r.body}"
  let r = Http.get "http://localhost:\{show port}/plain"
  println "client: \{show r.status} \{String.decode r.body}"
  Async.await server)
