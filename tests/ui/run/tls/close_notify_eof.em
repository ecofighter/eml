-- server が閉じると終端器は close_notify を送り、クライアントの `read_chunk` は届いた平文の後で空の読みを返す。
-- EOF の後の読みも空で、`close` は正常に終わる (docs/spec/stdlib.md の「`Std.Tls`」)
import Async (Async)
import Async
import Bytes
import Fs
import Net
import String
import Tls

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let ca = Fs.read_text "ca.pem"
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    Net.close (Net.write "bye" s))
  let c = Tls.connect_with_ca ca "tls.localhost" port
  let (b, c) = Tls.read_chunk (Bytes.empty ()) c
  println (String.decode b)
  let (b, c) = Tls.read_chunk (Bytes.empty ()) c
  if Bytes.length b == 0 then println "eof" else println "more"
  let (b, c) = Tls.read_chunk (Bytes.empty ()) c
  if Bytes.length b == 0 then println "eof" else println "more"
  Tls.close c
  println "closed"
  Async.await server)
