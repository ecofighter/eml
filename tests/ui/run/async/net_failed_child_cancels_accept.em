-- 失敗した子は、同じスコープで `accept` を待つタスクを取り消す。取り消した受けの包みはリスナーを落とし、`run` は
-- 最初の失敗を起こし直す (docs/spec/stdlib.md の「取り消し」と「`Std.Net`」)。
import Async (Async)
import Async
import Net

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

acceptor : Net.TcpListener -> <IO, Panic, Async> Unit
acceptor l =
  match try_io (fn () -> Net.accept l) with
    | Ok (s, l) ->
      Net.close s
      Net.close_listener l
      println "unreachable"
    | Err e -> println "acceptor: \{show e.kind}: \{e}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  let l = Net.listen "127.0.0.1" 0
  Async.spawn (fn () -> acceptor l)
  let _ = spin 40
  Async.spawn (fn () ->
    let _ = spin 400
    fail_io TimedOut "too slow")
  let _ = spin 40
  ()

main : Unit -> <IO, Panic> Unit
main () =
  match try_io (fn () -> Async.run body) with
    | Ok () -> println "unreachable"
    | Err e -> println "run: \{show e.kind}: \{e}"
