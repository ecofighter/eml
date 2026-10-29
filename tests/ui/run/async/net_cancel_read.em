-- 待っている読みを取り消すと、driver に取り消しを頼み、完了を待ってから `Cancelled` を投げる。取り消した読みの
-- ストリームは包みが落とすので、相手のストリームの読みは EOF になる。記録の buffer は検査モードの漏れの検査が覆う
-- (docs/spec/stdlib.md の「取り消し」と「`Std.Net`」)。
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
  match Async.timeout 10 (fn () -> Net.read_chunk (Bytes.empty ()) b) with
    | Some (_, b) ->
      Net.close b
      println "read"
    | None -> println "None"
  let (buf, a) = Net.read_chunk (Bytes.empty ()) a
  Net.close a
  if Bytes.length buf == 0 then println "eof" else println "data")
