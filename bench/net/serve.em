-- 接続ごとのタスクで答える echo のサーバー。`conns` 個のクライアントのタスクが、それぞれ1本の接続で `size` バイトの
-- 往復を `n / conns` 回する
-- 引数: n size conns
import Async (Async)
import Async
import Bytes
import Env
import Net
import String

serve : Bytes -> Net.TcpStream -> <IO, Panic, Async> Unit
serve b s =
  let (b, s) = Net.read_chunk (Bytes.truncate 0 b) s
  if Bytes.length b == 0 then Net.close s
  else
    let (b, s) = Net.write_bytes b s
    serve b s

accept_loop : Async.Scope -> Int -> Net.TcpListener -> <IO, Panic, Async> Unit
accept_loop sc k l =
  if k == 0 then Net.close_listener l
  else
    let (s, l) = Net.accept l
    Async.spawn_in sc (fn () -> serve (Bytes.empty ()) (Net.set_nodelay True s))
    accept_loop sc (k - 1) l

read_exact : Int -> Bytes -> Net.TcpStream -> <IO, Panic, Async> (Bytes, Net.TcpStream)
read_exact want b s =
  if Bytes.length b >= want then (b, s)
  else
    let (b, s) = Net.read_chunk b s
    read_exact want b s

rounds : Int -> Int -> Bytes -> Bytes -> Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
rounds n size msg b s =
  if n == 0 then s
  else
    let (msg, s) = Net.write_bytes msg s
    let (b, s) = read_exact size (Bytes.truncate 0 b) s
    rounds (n - 1) size msg b s

clients : Async.Scope -> Int -> Int -> Int -> Int -> <IO, Panic, Async> Unit
clients sc k each size port =
  if k == 0 then ()
  else
    Async.spawn_in sc (fn () ->
      let c = Net.set_nodelay True (Net.connect "127.0.0.1" port)
      Net.close (rounds each size (Bytes.make size 'x'u8) (Bytes.empty ()) c))
    clients sc (k - 1) each size port

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let n = int_arg 0 20480
  let size = int_arg 1 16
  let conns = int_arg 2 32
  let each = n / conns
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () -> Async.scope (fn sc -> accept_loop sc conns l))
  Async.scope (fn sc -> clients sc conns each size port)
  Async.await server
  println "serve done \{show (each * conns)}")

int_arg : Int -> Int -> <IO, Panic> Int
int_arg i default =
  match nth i (Env.args ()) with
    | Some s ->
      match String.to_int s with
        | Some n -> n
        | None -> default
    | None -> default

nth : Int -> List String -> <Panic> Option String
nth i xs =
  match xs with
    | Nil -> None
    | x :: rest -> if i == 0 then Some x else nth (i - 1) rest
