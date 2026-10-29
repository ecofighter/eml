-- `Net.write` が書いたバイトを、相手のストリームの `Net.read_chunk` が読んで buffer の後ろに足す。書き手が閉じた後の
-- 読みは EOF で、何も足さない (docs/spec/stdlib.md の「`Std.Net`」)。
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
  let a = Net.write "hello" a
  let (buf, b) = Net.read_chunk (Bytes.of_string "> ") b
  println (String.decode buf)
  Net.close a
  let (rest, b) = Net.read_chunk (Bytes.empty ()) b
  Net.close b
  if Bytes.length rest == 0 then println "eof" else println "data")
