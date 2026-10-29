-- `Net.write` は buffer をすべて書いてストリームを返す。相手が閉じた後の書きは、種類 `ConnectionReset` の IO の
-- 失敗になる (docs/spec/stdlib.md の「`Std.Net`」)。
import Async (Async)
import Async
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
  let a = Net.write_line "hello" a
  println "wrote"
  Net.close b
  match try_io (fn () -> Net.write "late" a) with
    | Ok a ->
      Net.close a
      println "wrote again"
    | Err e -> println "\{show e.kind}: \{e}")
