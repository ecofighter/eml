import Async (Async)
import Async
import Array

-- spawn uses no budget: main (worker 0) never switches while the tasks on worker 1 drop its arrays
spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

produce : Int -> <IO, Panic, Async> Unit
produce i =
  if i == 0 then ()
  else
    let _ = spin 1000000
    let a = Array.make 131072 i
    Async.spawn (fn () -> if Array.length a == 0 then println "empty" else ())
    produce (i - 1)

main : Unit -> <IO, Panic, Async> Unit
main () =
  produce 1200
  println "done"
