-- 誰も書かないストリームの読みは待ち続け、どのタスクも進めなくなると実行時エラー `deadlock` で止まる。driver に
-- 保留の操作があっても、テストの driver は外からの入力なしに終わる操作がなければ進まないと答える
-- (docs/spec/vm.md の「時計と driver」)。
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
main () =
  println "before"
  Async.run (fn () ->
    let (a, b) = connected ()
    let t = Async.start (fn () ->
      let (buf, a) = Net.read_chunk (Bytes.empty ()) a
      Net.close a
      Bytes.length buf)
    let _ = spin 40
    println (show (Async.await t))
    Net.close b)
  println "unreachable"
