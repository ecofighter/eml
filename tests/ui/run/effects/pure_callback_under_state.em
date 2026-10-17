import Yield (Yield(..))
import List
import Std.State (State(..))

count_to : Int -> Unit -> <Yield Int> Unit
count_to n () =
  let go i =
    if i > n then ()
    else
      yield i
      go (i + 1)
  go 1

produce : Int -> Unit -> <Yield (List Int)> Unit
produce n () =
  let go i =
    if i > n then ()
    else
      yield [i, i + 1, i + 2]
      go (i + 1)
  go 1

sum_lengths : (Unit -> <Yield (List Int) | e> Unit) -> <e> Int
sum_lengths p = Yield.fold (fn acc xs -> acc + List.length xs) 0 p

map_lists : (a -> b) -> (Unit -> <Yield (List a) | e> Unit) -> <Yield (List b) | e> Unit
map_lists f producer =
  handle producer () with
    | fn yield xs -> yield (List.map f xs)

main : Unit -> <IO> Unit
main () =
  let (total, _) = State.run 0 (fn () -> Yield.fold (fn acc x -> acc + x) 0 (count_to 1000))
  println (show total)
  let (_, n) = State.run 0 (fn () ->
    Yield.for_each (produce 5) (fn xs -> println (show (List.length (List.map (fn x -> x * 2) xs))))
    put 1)
  println (show n)
  let (m, _) = State.run 0 (fn () -> sum_lengths (fn () -> map_lists (fn x -> x * 2) (produce 1000)))
  println (show m)
