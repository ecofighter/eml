-- 改行のないまま 1 MiB を超えた行は、`Net.read_line` と同じ上限の失敗になる (docs/spec/stdlib.md の「`Std.Tls`」)
import Async (Async)
import Async
import Bytes
import Fs
import Net
import Tls

main : Unit -> <IO, Panic> Unit
main () =
  let r = Async.run (fn () ->
    let ca = Fs.read_text "ca.pem"
    let l = Net.listen "127.0.0.1" 0
    let (port, l) = Net.local_port l
    let server = Async.start (fn () ->
      let (s, l) = Net.accept l
      Net.close_listener l
      -- クライアントは上限を超えたところで接続を落とすので、書きの失敗は捨てる
      let _ = try_io (fn () ->
        let (_, s) = Net.write_bytes (Bytes.make 1100000 97u8) s
        Net.close s)
      ())
    let c = Tls.connect_with_ca ca "tls.localhost" port
    let r = try_io (fn () ->
      let (_, c) = Tls.read_line c
      Tls.close c)
    Async.await server
    r)
  match r with
    | Ok () -> println "read"
    | Err e ->
      println (show e.kind)
      fail_io e.kind e.message
