-- `push_fold.em` の畳み込みを、`acc + x` を書き込んだ専用の `sum` handler にした手書きの版。
import Yield (Yield(..))

nums : Int -> Unit -> <Yield Int> Unit
nums n () =
  let go i =
    if i > n then ()
    else
      yield i
      go (i + 1)
  go 1

sum : (Unit -> <Yield Int | e> Unit) -> <e> Int
sum producer =
  handle producer () from 0 with
    | fn yield x acc -> ((), acc + x)
    | return _ acc -> acc

main : Unit -> <IO> Unit
main () =
  println "\{sum (nums 100000)}"
