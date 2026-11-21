import Async (Async)
import Async
import Chan
import Array

consume : Chan.Chan (Array Int) -> Int -> <IO, Panic, Async> Int
consume c n =
  match Chan.recv c with
    | Some a -> consume c (n + Array.length a)
    | None -> n

-- try_send never yields: the producer (main, worker 0) reaches no reclaim point
push : Array Int -> Chan.Chan (Array Int) -> <IO, Panic> Unit
push a c =
  match Chan.try_send a c with
    | Chan.Accepted -> ()
    | Chan.Full a -> push a c
    | Chan.Refused a -> ()

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

produce : Int -> Chan.Chan (Array Int) -> <IO, Panic> Unit
produce i c =
  if i == 0 then ()
  else
    let _ = spin 1000000
    push (Array.make 131072 i) c
    produce (i - 1) c

main : Unit -> <IO, Panic, Async> Unit
main () =
  let c = Chan.new 4
  let t = Async.start (fn () -> consume c 0)
  produce 1200 c
  Chan.close c
  println (show (Async.await t))
