-- 終端器がクライアントのバイトを1つも受けないうちに server が閉じると、TLS のバイトは届かず、handshake は途中の
-- EOF で失敗する。受けと閉じを1つのタスクで済ませ、ClientHello より先に閉じる (docs/spec/vm.md の「時計と driver」)
import Async
import Net
import Tls

main : Unit -> <IO, Panic> Unit
main () =
  let r = Async.run (fn () ->
    let l = Net.listen "127.0.0.1" 0
    let (port, l) = Net.local_port l
    let c = Net.connect "tls.localhost" port
    let (s, l) = Net.accept l
    Net.close s
    Net.close_listener l
    try_io (fn () -> Tls.close (Tls.wrap "tls.localhost" c)))
  match r with
    | Ok () -> println "connected"
    | Err e ->
      println (show e.kind)
      fail_io e.kind e.message
