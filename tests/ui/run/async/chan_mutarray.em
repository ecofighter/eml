-- `Lin` の品物は持ち主ごと受け手のタスクへ移る。受け取った `MutArray` には書き込め、送り返した配列も漏れない
-- (docs/spec/stdlib.md の「`Std.Chan`」、docs/spec/object-model.md の「論理的な worker ID」)。
import Async (Async)
import Chan
import MutArray

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

put : a -> Chan.Chan a -> <IO, Async> Unit
put x c =
  match Chan.send x c with
    | Chan.Sent -> ()
    | Chan.Rejected x -> drop x

-- 受け取った配列の 1 番に 7 を書いて送り返す
worker : Chan.Chan (MutArray Int) -> Chan.Chan (MutArray Int) -> <IO, Panic, Async> Unit
worker inbox outbox =
  match Chan.recv inbox with
    | Some a -> put (MutArray.write a 1 7) outbox
    | None -> ()

body : Unit -> <IO, Panic, Async> Unit
body () =
  let inbox = Chan.new 0
  let outbox = Chan.new 1
  Async.spawn (fn () -> worker inbox outbox)
  let _ = spin 40
  put (MutArray.make 3 0) inbox
  match Chan.recv outbox with
    | Some a ->
      let (x, a) = MutArray.read a 1
      let a = MutArray.write a 2 8
      let (y, a) = MutArray.read a 2
      drop a
      println "\{show x} \{show y}"
    | None -> println "unreachable"
  Chan.close inbox
  Chan.close outbox

main : Unit -> <IO, Panic> Unit
main () = Async.run body
