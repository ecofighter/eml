-- 渡したCAがテストのdriverの証明書に署名していなければ、handshakeは証明書の拒否で失敗する。
-- 文言にrustlsとOSの詳細は付かない (docs/spec/stdlib.mdの「`Std.Tls`」)
import Async
import Fs
import Net
import Tls

main : Unit -> <IO, Panic> Unit
main () =
  let r = Async.run (fn () ->
    let l = Net.listen "127.0.0.1" 0
    let (port, l) = Net.local_port l
    let r = try_io (fn () -> Tls.close (Tls.connect_with_ca (Fs.read_text "other-ca.pem") "tls.localhost" port))
    Net.close_listener l
    r)
  match r with
    | Ok () -> println "connected"
    | Err e ->
      println (show e.kind)
      fail_io e.kind e.message
