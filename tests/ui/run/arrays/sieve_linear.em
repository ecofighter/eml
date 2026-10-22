-- `MutArray` を線形に受け渡して書いたエラトステネスのふるい。100 までの素数を書く。
import Array
import MutArray

cross_out : Int -> Int -> Int -> MutArray Bool -> <Panic> MutArray Bool
cross_out step n j marks =
  if j >= n then marks
  else cross_out step n (j + step) (MutArray.write marks j False)

sieve_from : Int -> Int -> MutArray Bool -> <Panic> MutArray Bool
sieve_from i n marks =
  if i * i >= n then marks
  else
    let (prime, marks) = MutArray.read marks i
    if prime then sieve_from (i + 1) n (cross_out i n (i * i) marks)
    else sieve_from (i + 1) n marks

primes : Int -> Array Bool -> List Int -> <Panic> List Int
primes i marks acc =
  if i < 2 then acc
  else primes (i - 1) marks (if Array.get marks i then i :: acc else acc)

main : Unit -> <IO, Panic> Unit
main () =
  let n = 101
  let marks = MutArray.freeze (sieve_from 2 n (MutArray.make n True))
  println (show (primes (n - 1) marks Nil))
