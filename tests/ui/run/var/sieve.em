-- `var` で書いたエラトステネスのふるい。2000 より小さい素数の数を書く
import Array
import MutArray

sieve : Int -> <Panic> Array Bool
sieve n =
  var marks = MutArray.make n True
  for 2 n (fn i ->
    if marks[i] then
      var j = i * i
      while (fn () -> j < n) (fn () ->
        marks[j] <- False
        j <- j + i))
  MutArray.freeze marks

count : Array Bool -> <Panic> Int
count marks =
  var c = 0
  for 2 (Array.length marks) (fn i -> if marks[i] then c <- c + 1)
  c

main : Unit -> <IO, Panic> Unit
main () = println (show (count (sieve 2000)))
