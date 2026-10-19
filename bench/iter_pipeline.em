-- `Iter.generate` の列を `Iter.filter` / `Iter.map` / `Iter.fold` に通す。
import Yield (Yield(..))
import Iter

nums : Int -> Unit -> <Yield Int, Panic> Unit
nums n () =
  let go i =
    if i > n then ()
    else
      yield i
      go (i + 1)
  go 1

main : Unit -> <IO, Panic> Unit
main () =
  let it = Iter.map (fn x -> x * x) (Iter.filter (fn x -> x % 2 == 0) (Iter.generate (nums 100000)))
  println "\{Iter.fold (fn a b -> a + b) 0 it}"
