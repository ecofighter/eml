-- `Net.read_chunk_within` は、期限までに読めなければ `None` とストリームを返し、ストリームは続けて使える。期限の前に
-- 読めれば、期限を付けても `Some` を返す。過ぎた期限 (0 以下) は、データのないストリームでは待たずに `None` になる。
-- テストの driver は終わる操作がないときだけ時計を進めるので、正の期限とデータのないストリームの過ぎた期限は、どの
-- seed でも同じ結果になる (docs/spec/stdlib.md の「`Std.Net`」、docs/implementation/testing.md の「UI テスト」)。
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

show_read : Option Bytes -> <IO, Panic> Unit
show_read r = match r with
  | Some buf -> println "Some \{String.decode buf}"
  | None -> println "None"

reads : Unit -> <IO, Panic, Async> Unit
reads () =
  let (a, b) = connected ()
  let (r, b) = Net.read_chunk_within 50 (Bytes.empty ()) b
  show_read r
  let (r, b) = Net.read_chunk_within 0 (Bytes.empty ()) b
  show_read r
  let a = Net.write "hello" a
  let (buf, b) = Net.read_chunk (Bytes.empty ()) b
  println (String.decode buf)
  let a = Net.write "again" a
  let (r, b) = Net.read_chunk_within 100 (Bytes.of_string "> ") b
  show_read r
  Net.close a
  Net.close b

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let t = Async.start reads
  let _ = spin 40
  Async.await t)
