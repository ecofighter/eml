import Async (Async)
import Async
import Atomic
import Array

wait : Int -> Atomic.Counter -> <IO> Unit
wait i k = if Atomic.load k >= i then () else wait i k

work : Array Int -> Atomic.Counter -> <IO> Unit
work a k =
  drop a
  let _ = Atomic.add 1 k
  ()

produce : Int -> Int -> Atomic.Counter -> <IO, Panic, Async> Unit
produce i n k =
  if i == n then ()
  else
    let a = Array.make 100 i
    Async.spawn (fn () -> work a k)
    wait (i + 1) k
    produce (i + 1) n k

main : Unit -> <IO, Panic, Async> Unit
main () =
  let k = Atomic.new 0
  produce 0 200000 k
  println (show (Atomic.load k))
