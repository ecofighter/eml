import Async (Async)
import Async
import Chan
import Array

consume : Chan.Chan (Array Int) -> Int -> <IO, Panic, Async> Int
consume c n =
  match Chan.recv c with
    | Some a -> consume c (n + Array.length a)
    | None -> n

-- send yields every 128 fast paths and parks when full: bounded
spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

produce : Int -> Chan.Chan (Array Int) -> <IO, Panic, Async> Unit
produce i c =
  if i == 0 then ()
  else
    let _ = spin 1000000
    match Chan.send (Array.make 131072 i) c with
      | Chan.Sent -> produce (i - 1) c
      | Chan.Rejected _ -> ()

main : Unit -> <IO, Panic, Async> Unit
main () =
  let c = Chan.new 4
  let t = Async.start (fn () -> consume c 0)
  produce 1200 c
  Chan.close c
  println (show (Async.await t))
