-- `Iter.generate` の列を `Iter.fold` で合計するだけの最小の形。`sum_loop.em` との差が、`Iter` の生産者を包む費用である。
import Yield (Yield(..))
import Iter

nums : Int -> Unit -> <Yield Int> Unit
nums n () =
  let go i =
    if i > n then ()
    else
      yield i
      go (i + 1)
  go 1

main : Unit -> <IO> Unit
main () =
  println "\{Iter.fold (fn acc x -> acc + x) 0 (Iter.generate (nums 100000))}"
