-- 根に足す CA を渡さなければ、テストの driver の証明書は既定の根でたどれず、handshake は証明書の拒否で失敗する。
-- 文言に rustls の詳細は付かない (docs/spec/stdlib.md の「`Std.Tls`」)
import Async
import Net
import Tls

main : Unit -> <IO, Panic> Unit
main () =
  let r = Async.run (fn () ->
    let l = Net.listen "127.0.0.1" 0
    let (port, l) = Net.local_port l
    let r = try_io (fn () -> Tls.close (Tls.connect "tls.localhost" port))
    Net.close_listener l
    r)
  match r with
    | Ok () -> println "connected"
    | Err e ->
      println (show e.kind)
      fail_io e.kind e.message
