-- `Net.read_line` は改行 (`\n` か `\r\n`) を除いた行を返し、2回の書きにまたがる行も1行にする。改行のない最後の行は
-- そのまま返し、残りのない EOF は `None` である。`read_line` が読みすぎたバイトは、次の `read_chunk` が先に返す。
-- 読む前に書きをすべて終えるので、読みの周回の数はスケジュールによらない (docs/spec/stdlib.md の「`Std.Net`」)。
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

show_line : Option String -> <IO> Unit
show_line r = match r with
  | Some line -> println "Some \{line}"
  | None -> println "None"

lines : Net.TcpStream -> <IO, Panic, Async> Unit
lines b =
  let (r, b) = Net.read_line b
  show_line r
  match r with
    | Some _ -> lines b
    | None -> Net.close b

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let (a, b) = connected ()
  let a = Net.write "x\nyz" a
  let (r, b) = Net.read_line b
  show_line r
  let (buf, b) = Net.read_chunk (Bytes.of_string "> ") b
  println (String.decode buf)
  let a = Net.write "ab" a
  let a = Net.write "c\r\nd\n" a
  let a = Net.write "tail" a
  Net.close a
  let t = Async.start (fn () -> lines b)
  let _ = spin 40
  Async.await t)
