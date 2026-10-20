-- 2つの状態つきの handler を重ねたループ (S14c-2)。内側の `Tick` の handler から先に特殊化される
import Std.State (State(..))

effect Tick where
  tick : Unit -> Unit

step : Int -> <State Int, Tick, Panic> Unit
step i =
  tick ()
  put (get () + i)

loop : Int -> Int -> <State Int, Tick, Panic> Unit
loop i n =
  if i > n then ()
  else
    step i
    loop (i + 1) n

count : (Unit -> <Tick | e> a) -> <Panic | e> (a, Int)
count action =
  handle action () from 0 with
    | fn tick () c -> ((), c + 1)
    | return x c -> (x, c)

counted : Unit -> <State Int, Panic> Int
counted () =
  let (_, ticks) = count (fn () -> loop 1 10)
  ticks

main : Unit -> <IO, Panic> Unit
main () =
  let (ticks, total) = State.run 0 counted
  println "\{total} \{ticks}"
