import Yield (Yield(..))
import List

count_to : Int -> Unit -> <Yield Int> Unit
count_to n () =
  let go i =
    if i > n then ()
    else
      yield i
      go (i + 1)
  go 1

qualified : Unit -> <Yield String> Unit
qualified () =
  Yield.yield "a"
  Yield.yield "b"

print_item : Int -> <IO> Unit
print_item x = println (show x)

with_use : Unit -> <IO> Unit
with_use () =
  use x <- Yield.for_each (count_to 2)
  println "got \{x}"

main : Unit -> <IO> Unit
main () =
  Yield.for_each (count_to 3) print_item
  with_use ()
  println (show (Yield.collect (count_to 4)))
  println (show (Yield.collect qualified))
  println (show (Yield.fold (fn acc x -> acc + x) 0 (count_to 10)))
