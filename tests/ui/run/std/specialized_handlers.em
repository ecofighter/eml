-- 同じ `Yield.for_each` を、生産者と消費者の違う2組で呼ぶ。特殊化した2つの複製は、それぞれ自分の節の関数を持つ
import Yield (Yield(..))

nums : Int -> Unit -> <Yield Int> Unit
nums n () =
  let go i = if i > n then () else
    yield i
    go (i + 1)
  go 1

other : Int -> Unit -> <Yield Int> Unit
other n () =
  yield (n * 10)
  yield (n * 20)

main : Unit -> <IO> Unit
main () =
  Yield.for_each (nums 3) (fn x -> println "\{x}")
  Yield.for_each (other 3) (fn x -> println "got \{x}")
