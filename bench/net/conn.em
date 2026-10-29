-- (d) accept と connect の速さ。`par` 個のタスクが、それぞれ接続を張って閉じるのを順に `n / par` 回する。サーバーは
-- 受けたらすぐ閉じ、クライアントは EOF を読んでから閉じる。EOF を待つのは、クライアントが backlog を埋めて SYN の
-- 再送 (1 秒) を待つのを避けるためで、同時に張る接続は `par` 本までになる
-- 引数: n par [port]。port が 0 なら同じ run の中でサーバーを動かす。正ならクライアントだけで 127.0.0.1:port へ
import Async (Async)
import Async
import Bytes
import Env
import Net
import String

accept_loop : Int -> Net.TcpListener -> <IO, Panic, Async> Unit
accept_loop n l =
  if n == 0 then Net.close_listener l
  else
    let (s, l) = Net.accept l
    Net.close s
    accept_loop (n - 1) l

connect_loop : Int -> Int -> <IO, Panic, Async> Unit
connect_loop n port =
  if n == 0 then ()
  else
    let (_, c) = Net.read_chunk (Bytes.empty ()) (Net.connect "127.0.0.1" port)
    Net.close c
    connect_loop (n - 1) port

clients : Async.Scope -> Int -> Int -> Int -> <IO, Panic, Async> Unit
clients sc par each port =
  if par == 0 then ()
  else
    Async.spawn_in sc (fn () -> connect_loop each port)
    clients sc (par - 1) each port

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let n = int_arg 0 10000
  let par = int_arg 1 1
  let port = int_arg 2 0
  let each = n / par
  if port == 0 then
    let l = Net.listen "127.0.0.1" 0
    let (port, l) = Net.local_port l
    let t = Async.start (fn () -> accept_loop (each * par) l)
    if par == 1 then connect_loop each port else Async.scope (fn sc -> clients sc par each port)
    Async.await t
    println "conn done \{show (each * par)}"
  else
    if par == 1 then connect_loop each port else Async.scope (fn sc -> clients sc par each port)
    println "conn done \{show (each * par)}")

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
