-- `tls-cut.localhost` の終端器は close_notify を送らずに閉じる。クライアントは届いた平文を読みきった後の読みで
-- `ConnectionReset` の失敗を受ける。接続は `Net` で開き、`Tls.wrap_with_ca` で `tls.localhost` の名前の
-- handshake をする (docs/spec/stdlib.md の「`Std.Tls`」、docs/spec/vm.md の「時計と driver」)
import Async (Async)
import Async
import Bytes
import Fs
import Net
import String
import Tls

main : Unit -> <IO, Panic> Unit
main () =
  let r = Async.run (fn () ->
    let ca = Fs.read_text "ca.pem"
    let l = Net.listen "127.0.0.1" 0
    let (port, l) = Net.local_port l
    let server = Async.start (fn () ->
      let (s, l) = Net.accept l
      Net.close_listener l
      Net.close (Net.write "partial" s))
    let c = Tls.wrap_with_ca ca "tls.localhost" (Net.connect "tls-cut.localhost" port)
    let (b, c) = Tls.read_chunk (Bytes.empty ()) c
    println (String.decode b)
    let r = try_io (fn () ->
      let (_, c) = Tls.read_chunk (Bytes.empty ()) c
      Tls.close c)
    Async.await server
    r)
  match r with
    | Ok () -> println "read"
    | Err e ->
      println (show e.kind)
      fail_io e.kind e.message
