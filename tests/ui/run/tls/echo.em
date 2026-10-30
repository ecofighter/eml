-- テストの driver の `tls.localhost` は TLS を終端し、平文の端を待ち受けているリスナーに渡す。eml の server が平文で
-- 読み書きした行を、クライアントは `Std.Tls` で暗号にして送り、復号して読む (docs/spec/stdlib.md の「`Std.Tls`」)
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
  let server = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    let (addr, s) = Net.peer_addr s
    println "server: peer \{addr}"
    let (line, s) = Net.read_line s
    let s = match line with
      | Some text -> Net.write_line text s
      | None -> s
    Net.close s)
  let c = Tls.connect_with_ca ca "tls.localhost" port
  let c = Tls.write_line "hello" c
  let (line, c) = Tls.read_line c
  match line with
    | Some text -> println "client: Some \{text}"
    | None -> println "client: None"
  Tls.close c
  Async.await server)
