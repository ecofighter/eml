-- `sieve_var.em` のふるいを、`MutArray` を末尾再帰のループに線形に受け渡して手で書いた版
import Array
import MutArray

cross : Int -> Int -> Int -> MutArray Bool -> <Panic> MutArray Bool
cross step n j marks =
  if j < n then cross step n (j + step) (MutArray.write marks j False)
  else marks

sieve_from : Int -> Int -> MutArray Bool -> <Panic> MutArray Bool
sieve_from i n marks =
  if i < n then
    let (prime, marks) = MutArray.read marks i
    if prime then sieve_from (i + 1) n (cross i n (i * i) marks)
    else sieve_from (i + 1) n marks
  else marks

sieve : Int -> <Panic> Array Bool
sieve n = MutArray.freeze (sieve_from 2 n (MutArray.make n True))

count_from : Int -> Int -> Array Bool -> Int -> <Panic> Int
count_from i n marks c =
  if i < n then count_from (i + 1) n marks (if Array.get marks i then c + 1 else c)
  else c

count : Array Bool -> <Panic> Int
count marks = count_from 2 (Array.length marks) marks 0

main : Unit -> <IO, Panic> Unit
main () = println (show (count (sieve 20000)))
