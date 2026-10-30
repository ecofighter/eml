-- `read_line` が読みすぎた平文は `Conn` に残り、次の `read_chunk` がソケットを読まずに先に返す。`read_line` と
-- `read_chunk` を交互に呼んでも、バイトは落ちず順も変わらない。CRLF も LF も行を切り、close_notify の EOF では改行
-- のない最後の行も返る (docs/spec/stdlib.md の「`Std.Tls`」)
import Async (Async)
import Async
import Bytes
import Fs
import Net
import String
import Tls

print_line : Option String -> <IO> Unit
print_line line = match line with
  | Some l -> println "line: Some \{l}"
  | None -> println "line: None"

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let ca = Fs.read_text "ca.pem"
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    let s = Net.write "one\r\ntwo\nth" s
    -- 後半は、クライアントが前半の読み残しを持っている間に送る
    let (_, s) = Net.read_line s
    Net.close (Net.write "ree\r\nfour" s))
  let c = Tls.connect_with_ca ca "tls.localhost" port
  let (line, c) = Tls.read_line c
  print_line line
  let (line, c) = Tls.read_line c
  print_line line
  let c = Tls.write_line "next" c
  let (b, c) = Tls.read_chunk (Bytes.empty ()) c
  println "chunk: \{String.decode b}"
  let (line, c) = Tls.read_line c
  print_line line
  let (line, c) = Tls.read_line c
  print_line line
  let (line, c) = Tls.read_line c
  print_line line
  Tls.close c
  Async.await server)
