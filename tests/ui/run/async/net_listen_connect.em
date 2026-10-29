-- `Net.listen` のポート 0 は空いているポートを使い、`local_port` で読む。`connect` したストリームと `accept` した
-- ストリームは、互いのポートを相手の番地に持つ。テストの driver はポートを 49152 から順に振る
-- (docs/spec/stdlib.md の「`Std.Net`」、docs/spec/vm.md の「時計と driver」)。
import Async
import Net

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  println "listening on \{show port}"
  let t = Async.start (fn () ->
    let (s, l) = Net.accept l
    let (peer, s) = Net.peer_addr s
    Net.close s
    Net.close_listener l
    peer)
  let _ = spin 40
  let c = Net.connect "localhost" port
  let (peer, c) = Net.peer_addr c
  println "connected to \{peer}"
  println "accepted from \{Async.await t}"
  Net.close c)
