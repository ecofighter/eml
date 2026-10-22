-- handler の状態が `Lin` の `MutArray` であるふるい。状態を渡す形の特殊化では、状態が `if` の合流と、末尾でない
-- `cross` の呼び出しを通って受け渡される。検査モードで漏れも `LinDup` も起きない
-- (docs/spec/core-ir.md の「handler への特殊化」)。
import Array
import MutArray

effect Marks where
  mark : Int -> Unit
  marked : Int -> Bool

cross : Int -> Int -> Int -> <Marks, Panic> Unit
cross step n j =
  if j >= n then ()
  else
    mark j
    cross step n (j + step)

sieve : Int -> Int -> <Marks, Panic> Unit
sieve i n =
  if i >= n then ()
  else
    if marked i then () else cross i n (i * i)
    sieve (i + 1) n

primes : Int -> Array Bool -> List Int -> <Panic> List Int
primes i marks acc =
  if i < 2 then acc
  else primes (i - 1) marks (if Array.get marks i then acc else i :: acc)

main : Unit -> <IO, Panic> Unit
main () =
  let n = 50
  let marks =
    handle sieve 2 n from MutArray.make n False with
      | fn mark i arr -> ((), MutArray.write arr i True)
      | fn marked i arr -> MutArray.read arr i
      | return _ arr -> MutArray.freeze arr
  println (show (primes (n - 1) marks Nil))
