-- `Tls.connect_with_ca`が接続を設定している間にタスクを取り消すと、受け付けで取り消しが届いても、設定の仕事が走った後で
-- 取り出しが`Conn`を落としても、どちらの道も同じ`Cancelled`の失敗で終わる (docs/spec/stdlib.mdの「`Std.Tls`」)
import Async (Async)
import Async
import Fs
import Net
import Tls

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let ca = Fs.read_text "ca.pem"
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  Net.close_listener l
  Async.scope (fn s ->
    Async.spawn_in s (fn () -> Async.cancel_scope s)
    match try_io (fn () -> Tls.close (Tls.connect_with_ca ca "tls.localhost" port)) with
      | Ok () -> println "connected"
      | Err e ->
        println (show e.kind)
        fail_io e.kind e.message))
