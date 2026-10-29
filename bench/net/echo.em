-- (a) ループバックの echo。1 本の接続で `size` バイトの往復を `n` 回する。
-- 引数: n size mode [port]
--   mode: fresh なら書くたびに Bytes.make で作り、読むたびに Bytes.empty () を渡す。reuse なら1つの buffer を
--         Bytes.truncate 0 で空にして読みに渡し直し、書きには同じ buffer を渡す (共有なので submit_write が写す)
--   port: 0 なら同じ run の中のタスクが相手になる。正なら 127.0.0.1:port の echo サーバーへ接続する
import Async (Async)
import Async
import Bytes
import Env
import Net
import String

serve_fresh : Net.TcpStream -> <IO, Panic, Async> Unit
serve_fresh s =
  let (buf, s) = Net.read_chunk (Bytes.empty ()) s
  if Bytes.length buf == 0 then Net.close s
  else serve_fresh (Net.write_bytes buf s)

serve_reuse : Bytes -> Net.TcpStream -> <IO, Panic, Async> Unit
serve_reuse b s =
  let (b, s) = Net.read_chunk (Bytes.truncate 0 b) s
  if Bytes.length b == 0 then Net.close s
  else serve_reuse b (Net.write_bytes b s)

-- `want` バイト集まるまで読む
read_exact : Int -> Bytes -> Net.TcpStream -> <IO, Panic, Async> (Bytes, Net.TcpStream)
read_exact want b s =
  if Bytes.length b >= want then (b, s)
  else
    let (b, s) = Net.read_chunk b s
    read_exact want b s

rounds_fresh : Int -> Int -> Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
rounds_fresh n size s =
  if n == 0 then s
  else
    let s = Net.write_bytes (Bytes.make size 'x'u8) s
    let (_, s) = read_exact size (Bytes.empty ()) s
    rounds_fresh (n - 1) size s

rounds_reuse : Int -> Int -> Bytes -> Bytes -> Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
rounds_reuse n size msg b s =
  if n == 0 then s
  else
    let s = Net.write_bytes msg s
    let (b, s) = read_exact size (Bytes.truncate 0 b) s
    rounds_reuse (n - 1) size msg b s

client : Int -> Int -> String -> Int -> <IO, Panic, Async> Unit
client n size mode port =
  let c = Net.set_nodelay True (Net.connect "127.0.0.1" port)
  if mode == "reuse" then Net.close (rounds_reuse n size (Bytes.make size 'x'u8) (Bytes.empty ()) c)
  else Net.close (rounds_fresh n size c)

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let n = int_arg 0 10000
  let size = int_arg 1 16
  let mode = str_arg 2 "fresh"
  let port = int_arg 3 0
  if port == 0 then
    let l = Net.listen "127.0.0.1" 0
    let (port, l) = Net.local_port l
    let t = Async.start (fn () ->
      let (s, l) = Net.accept l
      Net.close_listener l
      let s = Net.set_nodelay True s
      if mode == "reuse" then serve_reuse (Bytes.empty ()) s else serve_fresh s)
    client n size mode port
    Async.await t
    println "echo done \{show n}"
  else
    client n size mode port
    println "echo done \{show n}")

-- 引数の読み取り。各プログラムが1ファイルで動くように、同じ定義を写して持つ
int_arg : Int -> Int -> <IO, Panic> Int
int_arg i default =
  match nth i (Env.args ()) with
    | Some s ->
      match String.to_int s with
        | Some n -> n
        | None -> default
    | None -> default

str_arg : Int -> String -> <IO, Panic> String
str_arg i default =
  match nth i (Env.args ()) with
    | Some s -> s
    | None -> default

nth : Int -> List String -> <Panic> Option String
nth i xs =
  match xs with
    | Nil -> None
    | x :: rest -> if i == 0 then Some x else nth (i - 1) rest
