-- channel の品物には channel も `Task` のハンドルも置ける。返事の channel を送って答えを受け取り、`Task` のハンドルを
-- 別のタスクに渡してそこで待つ (docs/spec/stdlib.md の「`Std.Chan`」)。
import Async (Async)
import Async
import Chan

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

put : a -> Chan.Chan a -> <IO, Async> Unit
put x c =
  match Chan.send x c with
    | Chan.Sent -> ()
    | Chan.Rejected x -> drop x

-- 要求ごとに、送られてきた返事の channel へ 42 を送る
server : Chan.Chan (Chan.Chan Int) -> <IO, Async> Unit
server requests =
  match Chan.recv requests with
    | Some reply ->
      put 42 reply
      server requests
    | None -> ()

waiter : Chan.Chan (Async.Task Int) -> <IO, Panic, Async> Int
waiter handles =
  match Chan.recv handles with
    | Some t -> Async.await t
    | None -> 0

body : Unit -> <IO, Panic, Async> Unit
body () =
  let requests = Chan.new 0
  Async.spawn (fn () -> server requests)
  let _ = spin 40
  let reply = Chan.new 1
  put reply requests
  println "reply: \{show (Chan.recv reply)}"
  Chan.close reply
  Chan.close requests
  let handles = Chan.new 1
  let t = Async.start (fn () -> 5)
  let _ = spin 40
  put t handles
  let w = Async.start (fn () -> waiter handles)
  let _ = spin 40
  println "awaited: \{show (Async.await w)}"
  Chan.close handles

main : Unit -> <IO, Panic> Unit
main () = Async.run body
