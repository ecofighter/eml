-- 待っている送り手を取り消すと、送るはずだった値は巻き戻しが落とし、受け手には届かない。取り消しを受けたタスクは
-- channel の待ちの一覧から外れるので、後から来た受け手が起こすこともない (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)
import Async
import Chan
import MutArray

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

put : a -> Chan.Chan a -> <IO, Async> Unit
put x c =
  match Chan.send x c with
    | Chan.Sent -> ()
    | Chan.Rejected x -> drop x

body : Unit -> <IO, Panic, Async> Unit
body () =
  let c = Chan.new 0
  -- `scope` は取り消した送り手が終わるまで待ってから戻る
  Async.scope (fn s ->
    Async.spawn_in s (fn () -> put (MutArray.make 2 1) c)
    let _ = spin 40
    -- 送り手を先に動かして待たせてから取り消す
    Async.yield_now ()
    Async.cancel_scope s)
  println "try_recv: \{tried_text (Chan.try_recv c)}"
  let r = Async.start (fn () -> Chan.recv c)
  let _ = spin 40
  Chan.close c
  println "recv: \{got_text (Async.await r)}"

tried_text : Chan.TryRecvResult (MutArray Int) -> String
tried_text r =
  match r with
    | Chan.Got a ->
      drop a
      "Got"
    | Chan.Empty -> "Empty"
    | Chan.Drained -> "Drained"

got_text : Option (MutArray Int) -> String
got_text r =
  match r with
    | Some a ->
      drop a
      "Some"
    | None -> "None"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
