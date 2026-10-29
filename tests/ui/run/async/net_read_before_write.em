-- データのないストリームの読みは、相手が書くまで待つ。仮想の時計は、走れるタスクがなく、終わる操作もないときにだけ
-- 進むので、本体の `sleep_ms` が戻るときには、読むタスクはもう待っている。出力の順はどの方針と seed でも同じである
-- (docs/spec/stdlib.md の「`Std.Net`」、docs/spec/vm.md の「時計と driver」)。
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
  let t = Async.start (fn () ->
    println "reading"
    let (buf, b) = Net.read_chunk (Bytes.empty ()) b
    Net.close b
    println "read \{String.decode buf}")
  let _ = spin 40
  Async.sleep_ms 10
  println "writing"
  Net.close (Net.write "late" a)
  Async.await t)
