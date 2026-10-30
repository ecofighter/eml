-- 相手が close_notify を送って閉じた後でも `Tls.close` は正常に戻る。close_notify の書きが失敗しても `close` は
-- その失敗を捨てる (docs/spec/stdlib.md の「`Std.Tls`」)
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
  -- server が閉じきってから閉じる
  Async.await server
  Tls.close c
  println "closed")
