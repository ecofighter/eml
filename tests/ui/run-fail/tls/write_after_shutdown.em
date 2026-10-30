-- `Tls.shutdown_write` の後の書きは、`Net` の書きと同じ失敗になる (docs/spec/stdlib.md の「`Std.Tls`」)
import Async (Async)
import Async
import Bytes
import Fs
import Net
import Tls

drain : Net.TcpStream -> <IO, Async> Net.TcpStream
drain s =
  let (b, s) = Net.read_chunk (Bytes.empty ()) s
  if Bytes.length b == 0 then s else drain s

main : Unit -> <IO, Panic> Unit
main () =
  let r = Async.run (fn () ->
    let ca = Fs.read_text "ca.pem"
    let l = Net.listen "127.0.0.1" 0
    let (port, l) = Net.local_port l
    let server = Async.start (fn () ->
      let (s, l) = Net.accept l
      Net.close_listener l
      Net.close (drain s))
    let c = Tls.connect_with_ca ca "tls.localhost" port
    let c = Tls.shutdown_write c
    let r = try_io (fn () -> Tls.close (Tls.write "late" c))
    Async.await server
    r)
  match r with
    | Ok () -> println "written"
    | Err e ->
      println (show e.kind)
      fail_io e.kind e.message
