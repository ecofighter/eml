-- `Tls.wrap_with_ca` は、既にある `TcpStream` の上で handshake する。`Tls.shutdown_write` は close_notify を送って
-- 送る側を閉じ、server は要求を EOF まで読みきってから応える。クライアントはその後も読める
-- (docs/spec/stdlib.md の「`Std.Tls`」)
import Async (Async)
import Async
import Bytes
import Fs
import Net
import String
import Tls

drain : Bytes -> Net.TcpStream -> <IO, Async> (Bytes, Net.TcpStream)
drain b s =
  let n = Bytes.length b
  let (b, s) = Net.read_chunk b s
  if Bytes.length b == n then (b, s) else drain b s

drain_tls : Bytes -> Tls.Stream -> <IO, Async> (Bytes, Tls.Stream)
drain_tls b c =
  let n = Bytes.length b
  let (b, c) = Tls.read_chunk b c
  if Bytes.length b == n then (b, c) else drain_tls b c

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let ca = Fs.read_text "ca.pem"
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    let (b, s) = drain (Bytes.empty ()) s
    println "server: \{String.decode b}"
    Net.close (Net.write "done" s))
  let c = Tls.wrap_with_ca ca "tls.localhost" (Net.connect "tls.localhost" port)
  let c = Tls.write "req" c
  let c = Tls.shutdown_write c
  let (b, c) = drain_tls (Bytes.empty ()) c
  println "client: \{String.decode b}"
  Tls.close c
  Async.await server)
