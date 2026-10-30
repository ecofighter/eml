-- (b) 大きい転送。1 本の接続で `mib` MiB を `chunk` KiB ずつ write_bytes で送り、相手は read_chunk で EOF まで読む。
-- 引数: mib chunk_kib wmode rmode [port]
--   wmode: fresh なら書くたびに Bytes.make で新しい buffer を作る、shared なら同じ buffer を何度も渡す (共有のまま貸す)
--   rmode: fresh なら読むたびに Bytes.empty () を渡す、reuse なら返った buffer を truncate 0 で空にして渡し直す、
--          accum なら1つの buffer に足し続ける
--   port: 0 なら同じ run の中で送り手と読み手を動かす。正なら送り手だけになり 127.0.0.1:port へ送る。負なら読み手だけになり
--         127.0.0.1:(-port) へ接続して EOF まで読む
import Async (Async)
import Async
import Bytes
import Net
import Env
import String

drain_fresh : Int -> Net.TcpStream -> <IO, Panic, Async> Int
drain_fresh total s =
  let (b, s) = Net.read_chunk (Bytes.empty ()) s
  let n = Bytes.length b
  if n == 0 then
    Net.close s
    total
  else drain_fresh (total + n) s

drain_reuse : Int -> Bytes -> Net.TcpStream -> <IO, Panic, Async> Int
drain_reuse total b s =
  let (b, s) = Net.read_chunk (Bytes.truncate 0 b) s
  let n = Bytes.length b
  if n == 0 then
    Net.close s
    total
  else drain_reuse (total + n) b s

drain_accum : Int -> Bytes -> Net.TcpStream -> <IO, Panic, Async> Int
drain_accum before b s =
  let (b, s) = Net.read_chunk b s
  let n = Bytes.length b
  if n == before then
    Net.close s
    n
  else drain_accum n b s

drain : String -> Net.TcpStream -> <IO, Panic, Async> Int
drain rmode s =
  if rmode == "reuse" then drain_reuse 0 (Bytes.empty ()) s
  else if rmode == "accum" then drain_accum 0 (Bytes.empty ()) s
  else drain_fresh 0 s

send_fresh : Int -> Int -> Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
send_fresh n size s =
  if n == 0 then s
  else
    let (_, s) = Net.write_bytes (Bytes.make size 'x'u8) s
    send_fresh (n - 1) size s

send_shared : Int -> Bytes -> Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
send_shared n b s =
  if n == 0 then s
  else
    let (_, s) = Net.write_bytes b s
    send_shared (n - 1) b s

send : String -> Int -> Int -> Net.TcpStream -> <IO, Panic, Async> Unit
send wmode n size s =
  if wmode == "shared" then Net.close (send_shared n (Bytes.make size 'x'u8) s)
  else Net.close (send_fresh n size s)

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let mib = int_arg 0 100
  let size = int_arg 1 64 * 1024
  let wmode = str_arg 2 "fresh"
  let rmode = str_arg 3 "fresh"
  let port = int_arg 4 0
  let n = mib * 1024 * 1024 / size
  if port == 0 then
    let l = Net.listen "127.0.0.1" 0
    let (port, l) = Net.local_port l
    let t = Async.start (fn () ->
      let (s, l) = Net.accept l
      Net.close_listener l
      drain rmode s)
    send wmode n size (Net.connect "127.0.0.1" port)
    let got = Async.await t
    println "bulk done \{show got}"
  else if port > 0 then
    send wmode n size (Net.connect "127.0.0.1" port)
    println "bulk sent \{show (n * size)}"
  else
    let got = drain rmode (Net.connect "127.0.0.1" (0 - port))
    println "bulk done \{show got}")

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
