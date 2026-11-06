-- 接続の設定は取り消しの地点で、期限が0 msなら仕事を走らせる前に期限が来る。仮想の時計では期限が仕事の途中に
-- 来ることはないので、1 msなら接続できる (docs/spec/stdlib.mdの「`Std.Tls`」)
import Async (Async)
import Async
import Fs
import Net
import Tls

attempt : String -> Int -> Int -> <IO, Panic, Async> Unit
attempt ca port ms =
  match Async.timeout ms (fn () -> Tls.connect_with_ca ca "tls.localhost" port) with
    | Some s ->
      println "\{show ms}: connected"
      Tls.close s
    | None -> println "\{show ms}: timed out"

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let ca = Fs.read_text "ca.pem"
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  attempt ca port 0
  attempt ca port 1
  Net.close_listener l)
