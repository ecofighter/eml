import Async (Async)
import Async
import Chan
import Atomic
import Array

-- the consumer drops the item before it counts it, so a counted item's free has arrived
consume : Chan.Chan (Array Int) -> Atomic.Counter -> <IO, Panic, Async> Unit
consume c k =
  match Chan.recv c with
    | Some a ->
      drop a
      let _ = Atomic.add 1 k
      consume c k
    | None -> ()

push : Array Int -> Chan.Chan (Array Int) -> <IO, Panic> Unit
push a c =
  match Chan.try_send a c with
    | Chan.Accepted -> ()
    | Chan.Full a -> push a c
    | Chan.Refused a -> ()

wait : Int -> Atomic.Counter -> <IO> Unit
wait i k = if Atomic.load k >= i then () else wait i k

produce : Int -> Int -> Chan.Chan (Array Int) -> Atomic.Counter -> <IO, Panic> Unit
produce i n c k =
  if i == n then ()
  else
    push (Array.make 100 i) c
    wait (i + 1) k
    produce (i + 1) n c k

main : Unit -> <IO, Panic, Async> Unit
main () =
  let c = Chan.new 4
  let k = Atomic.new 0
  let t = Async.start (fn () -> consume c k)
  produce 0 200000 c k
  Chan.close c
  Async.await t
  println (show (Atomic.load k))
