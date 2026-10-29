-- `Net.read_line_within` の期限は1行の全体に付く。期限までに改行が来なければ `None` を返し、読んだ途中の行は
-- ストリームの buffer に残って、次の読みが続きと合わせて1行にする (docs/spec/stdlib.md の「`Std.Net`」)。
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

show_read : Option (Option String) -> <IO> Unit
show_read r = match r with
  | Some (Some line) -> println "Some \{line}"
  | Some None -> println "eof"
  | None -> println "None"

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let (a, b) = connected ()
  let a = Net.write "hel" a
  let (r, b) = Net.read_line_within 50 b
  show_read r
  let t = Async.start (fn () ->
    let (r, b) = Net.read_line_within 100 b
    show_read r
    b)
  let _ = spin 40
  let a = Net.write "lo\n" a
  Net.close a
  let b = Async.await t
  let (r, b) = Net.read_line_within 100 b
  show_read r
  Net.close b)
