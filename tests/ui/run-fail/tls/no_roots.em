-- テストのdriverでは根はプログラムが渡すCAだけなので、CAを渡さない`Tls.connect`は接続の設定で失敗する
-- (docs/spec/vm.mdの「時計と driver」)
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
