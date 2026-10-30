-- 本文のある要求。`Content-Length` は本文が空でないときと、`POST`、`PUT`、`PATCH` で付く。呼び出し側のヘッダーは
-- 書いた順と綴りのまま後ろに並び、`User-Agent` を渡すと既定の `User-Agent: eml` の代わりになる
-- (docs/spec/stdlib.md の「`Std.Http`」)
import Async (Async)
import Async
import Bytes
import Net
import String
import Http

-- 要求の行を印字し、`Content-Length` の本文を読んで印字してから、応答を書いて閉じる
serve : Net.TcpListener -> <IO, Panic, Async> Net.TcpListener
serve l =
  let (s, l) = Net.accept l
  let (n, s) = print_head 0 s
  let (body, s) = read_body n (Bytes.empty ()) s
  println "server: body \{String.decode body}"
  Net.close (Net.write "HTTP/1.1 201 Created\r\nContent-Length: 4\r\n\r\ndone" s)
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
    let l = serve (serve (serve (serve l)))
    Net.close_listener l)
  let url = "http://localhost:\{show port}/items"
  let headers = [("Content-Type", "application/json"), ("x-trace", "a b")]
  let r = Http.request "POST" url (Bytes.of_string "{\"name\": \"eml\"}") { Http.defaults | headers }
  println "client: \{show r.status} \{String.decode r.body}"
  let r = Http.request "POST" url (Bytes.empty ()) Http.defaults
  println "client: \{show r.status} \{String.decode r.body}"
  let r = Http.request "PUT" url (Bytes.of_string "x") { Http.defaults | headers = [("User-Agent", "probe/1")] }
  println "client: \{show r.status} \{String.decode r.body}"
  let r = Http.request "DELETE" url (Bytes.empty ()) Http.defaults
  println "client: \{show r.status} \{String.decode r.body}"
  Async.await server)
