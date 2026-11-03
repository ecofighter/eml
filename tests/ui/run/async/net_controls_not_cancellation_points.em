-- `Net.set_nodelay` と `Net.shutdown_write` は取り消しの地点でない。取り消したスコープの中でも投げずに終わり、相手は
-- EOF を読む。届いていた取り消しは、その後の最初の取り消しの地点で投げる (docs/spec/stdlib.md の「`Std.Net`」)
import Async (Async)
import Async
import Bytes
import Net

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
  let r = try_io (fn () -> Async.scope (fn sc ->
    Async.cancel_scope sc
    let a = Net.shutdown_write (Net.set_nodelay True a)
    println "configured"
    Net.close a
    Async.yield_now ()
    println "unreachable"))
  match r with
    | Ok () -> println "ok"
    | Err e -> println "\{show e.kind}: \{e}"
  let (rest, b) = Net.read_chunk (Bytes.empty ()) b
  if Bytes.length rest == 0 then println "eof" else println "data"
  Net.close b)
