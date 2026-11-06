-- `Tls.connect_with_ca`に渡したCAがPEMとして読めないと、バイトを送る前に固定の文言でTLSの設定が失敗する
-- (docs/spec/stdlib.mdの「`Std.Tls`」)
import Async
import Net
import Tls

main : Unit -> <IO, Panic> Unit
main () =
  let r = Async.run (fn () ->
    let l = Net.listen "127.0.0.1" 0
    let (port, l) = Net.local_port l
    let r = try_io (fn () -> Tls.close (Tls.connect_with_ca "garbage" "tls.localhost" port))
    Net.close_listener l
    r)
  match r with
    | Ok () -> println "connected"
    | Err e ->
      println (show e.kind)
      fail_io e.kind e.message
