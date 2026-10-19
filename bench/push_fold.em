-- `Yield.fold` に畳み込みのラムダを渡して、`Yield` の生産者を合計する。
import Yield (Yield(..))

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
  println "\{Yield.fold (fn acc x -> acc + x) 0 (nums 100000)}"
