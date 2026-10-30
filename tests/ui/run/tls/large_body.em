-- 200 KiB の本文は、TLS のレコードと `Net` の 64 KiB の読みの境目をいくつもまたぐ。クライアントは `read_chunk` を
-- 空の読みまで繰り返し、バイトを1つも落とさずに受ける (docs/spec/stdlib.md の「`Std.Tls`」)
import Async (Async)
import Async
import Bytes
import Fs
import Net
import Tls

total : Int -> Tls.Stream -> <IO, Panic, Async> (Int, Tls.Stream)
total n c =
  let (b, c) = Tls.read_chunk (Bytes.empty ()) c
  let k = Bytes.length b
  if k == 0 then (n, c) else total (n + k) c

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let ca = Fs.read_text "ca.pem"
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    let (_, s) = Net.write_bytes (Bytes.make 204800 120u8) s
    Net.close s)
  let c = Tls.connect_with_ca ca "tls.localhost" port
  let (n, c) = total 0 c
  println (show n)
  Tls.close c
  Async.await server)
