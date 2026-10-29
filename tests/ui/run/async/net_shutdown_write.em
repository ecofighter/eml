-- `Net.shutdown_write` は送る側だけを閉じる。相手は残りを読みきってから EOF を受け、まだ書いて返せる。閉じた側の
-- 書きは種類 `ConnectionReset` の IO の失敗になる (docs/spec/stdlib.md の「`Std.Net`」)。
import Async (Async)
import Async
import Bytes
import Net
import String

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

-- 接続した側と受けた側の2つのストリームを返す
connected : Unit -> <IO, Panic, Async> (Net.TcpStream, Net.TcpStream)
connected () =
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let t = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    s)
  let _ = spin 40
  let c = Net.connect "127.0.0.1" port
  (c, Async.await t)

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let (a, b) = connected ()
  let a = Net.shutdown_write (Net.write "ping" a)
  let (buf, b) = Net.read_chunk (Bytes.empty ()) b
  println (String.decode buf)
  let (rest, b) = Net.read_chunk (Bytes.empty ()) b
  if Bytes.length rest == 0 then println "eof" else println "data"
  Net.close (Net.write "pong" b)
  let (buf, a) = Net.read_chunk (Bytes.empty ()) a
  println (String.decode buf)
  match try_io (fn () -> Net.write "late" a) with
    | Ok a ->
      Net.close a
      println "wrote"
    | Err e -> println "\{show e.kind}: \{e}")
