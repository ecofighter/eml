-- 接続ごとのタスクを `try_all` で囲むと、1つの接続の失敗はそのタスクで止まり、ほかの接続とサーバーは続く。読まずに
-- 閉じた相手への書きは種類 `ConnectionReset` の IO の失敗になる。サーバーのスコープは接続のタスクがすべて終わって
-- から閉じる (docs/spec/stdlib.md の「`Std.Net`」と「`Failure` と `try_all`」)。
import Async (Async)
import Async
import Bytes
import Net
import String

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

-- 相手が書いたものを1回読み、前に `echo ` を付けて返す
serve : Net.TcpStream -> <IO, Panic, Async> Unit
serve s =
  let (buf, s) = Net.read_chunk (Bytes.empty ()) s
  Net.close (Net.write "echo \{String.decode buf}" s)

-- 接続を `n` 個受け、それぞれを `s` のタスクで受け持つ
accept_loop : Async.Scope -> Int -> Net.TcpListener -> <IO, Panic, Async> Unit
accept_loop sc n l =
  if n == 0 then Net.close_listener l
  else
    let (s, l) = Net.accept l
    Async.spawn_in sc (fn () ->
      match Async.try_all (fn () -> serve s) with
        | Ok () -> ()
        | Err (Async.IOFailed e) -> println "a connection failed: \{show e.kind}: \{e}"
        | Err (Async.Panicked m) -> println "a connection panicked: \{m}")
    let _ = spin 40
    accept_loop sc (n - 1) l

-- `msg` を書いて返事を読む
ask : Int -> String -> <IO, Panic, Async> String
ask port msg =
  let c = Net.write msg (Net.connect "127.0.0.1" port)
  let (buf, c) = Net.read_chunk (Bytes.empty ()) c
  Net.close c
  String.decode buf

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () -> Async.scope (fn sc -> accept_loop sc 3 l))
  let _ = spin 40
  println (ask port "a")
  Net.close (Net.connect "127.0.0.1" port)
  let c = ask port "c"
  Async.await server
  println c
  println "server finished")
