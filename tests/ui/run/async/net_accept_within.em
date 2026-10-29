-- `Net.accept_within` は、期限までに接続が来なければ `None` とリスナーを返し、リスナーは続けて使える。接続が来れば
-- `Some` を返す。テストの driver は終わる操作がないときだけ時計を進めるので、どの seed でも同じ結果になる
-- (docs/spec/stdlib.md の「`Std.Net`」)。
import Async
import Net

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let (r, l) = Net.accept_within 50 l
  match r with
    | Some s ->
      Net.close s
      println "Some"
    | None -> println "None"
  let t = Async.start (fn () -> Net.connect "127.0.0.1" port)
  let _ = spin 40
  let (r, l) = Net.accept_within 1000 l
  match r with
    | Some s ->
      let (peer, s) = Net.peer_addr s
      Net.close s
      println "Some \{peer}"
    | None -> println "None"
  Net.close (Async.await t)
  Net.close_listener l)
