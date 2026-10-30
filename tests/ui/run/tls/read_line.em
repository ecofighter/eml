-- `Tls.read_line` は `Net.read_line` と同じく CRLF と LF で行を切り、改行のない最後の行も返してから `None` を返す
-- (docs/spec/stdlib.md の「`Std.Tls`」)
import Async (Async)
import Async
import Fs
import Net
import Tls

lines : Tls.Stream -> <IO, Panic, Async> Tls.Stream
lines c =
  let (line, c) = Tls.read_line c
  match line with
    | Some l ->
      println "Some \{l}"
      lines c
    | None ->
      println "None"
      c

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let ca = Fs.read_text "ca.pem"
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    Net.close (Net.write "one\r\ntwo\nthree" s))
  let c = Tls.connect_with_ca ca "tls.localhost" port
  Tls.close (lines c)
  Async.await server)
