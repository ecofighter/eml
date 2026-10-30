-- (c) 多数の接続。クライアントは `c` 本の接続を1本ずつ順に張り (同時に backlog に入るのは1本まで)、張り終えてから
-- 接続ごとのタスクで 16 バイトの要求を書いて返事を読み、閉じる。サーバーは受けた接続ごとにすぐタスクを始め、要求を
-- 読んで返し、相手の EOF を読んで閉じる。張る間はサーバーの側に読みが張った本数だけ保留になり、張り終えた後は
-- `c` 本の読みがほぼ同時に読めるようになる。
-- 引数: c [port]。port が 0 なら同じ run の中でサーバーとクライアントを動かす。正ならクライアントだけで 127.0.0.1:port へ
import Async (Async)
import Async
import Bytes
import Env
import Net
import String

answer : Net.TcpStream -> <IO, Panic, Async> Unit
answer s =
  let (req, s) = Net.read_chunk (Bytes.empty ()) s
  let (_, s) = Net.write_bytes req s
  let (_, s) = Net.read_chunk (Bytes.empty ()) s
  Net.close s

accept_loop : Async.Scope -> Int -> Net.TcpListener -> <IO, Panic, Async> Unit
accept_loop sc n l =
  if n == 0 then Net.close_listener l
  else
    let (s, l) = Net.accept l
    Async.spawn_in sc (fn () -> answer s)
    accept_loop sc (n - 1) l

connect_all : Int -> Int -> List Net.TcpStream -> <IO, Panic, Async> List Net.TcpStream
connect_all n port acc =
  if n == 0 then acc else connect_all (n - 1) port (Net.connect "127.0.0.1" port :: acc)

ask : Net.TcpStream -> <IO, Panic, Async> Unit
ask c =
  let (_, c) = Net.write_bytes (Bytes.make 16 'q'u8) c
  let (_, c) = Net.read_chunk (Bytes.empty ()) c
  Net.close c

ask_all : Async.Scope -> List Net.TcpStream -> <IO, Panic, Async> Unit
ask_all sc xs =
  match xs with
    | Nil -> ()
    | c :: rest ->
      Async.spawn_in sc (fn () -> ask c)
      ask_all sc rest

clients : Int -> Int -> <IO, Panic, Async> Unit
clients c port =
  let xs = connect_all c port Nil
  Async.scope (fn sc -> ask_all sc xs)

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let c = int_arg 0 1000
  let port = int_arg 1 0
  if port == 0 then
    let l = Net.listen "127.0.0.1" 0
    let (port, l) = Net.local_port l
    let server = Async.start (fn () -> Async.scope (fn sc -> accept_loop sc c l))
    clients c port
    Async.await server
    println "many done \{show c}"
  else
    clients c port
    println "many done \{show c}")

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
