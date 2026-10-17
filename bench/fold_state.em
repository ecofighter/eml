-- State.run の中で Yield.fold に純粋なラムダを渡す。fold の節がその場で動き、yield しないことを固定する (S13d)
import Yield (Yield(..))
import Std.State (State(..))

count_to : Int -> Unit -> <Yield Int> Unit
count_to n () =
  let go i =
    if i > n then ()
    else
      yield i
      go (i + 1)
  go 1

main : Unit -> <IO> Unit
main () =
  let (t, _) = State.run 0 (fn () -> Yield.fold (fn acc x -> acc + x) 0 (count_to 100000))
  println (show t)
