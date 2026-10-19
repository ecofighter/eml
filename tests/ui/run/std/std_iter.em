import Yield (Yield(..))
import Iter

noisy : Int -> Unit -> <IO, Yield Int, Panic> Unit
noisy n () =
  let go i =
    if i > n then ()
    else
      println "produce \{i}"
      yield i
      go (i + 1)
  go 1

nums : Int -> Unit -> <Yield Int, Panic> Unit
nums n () =
  let go i =
    if i > n then ()
    else
      yield i
      go (i + 1)
  go 1

empty : Unit -> <Yield Int> Unit
empty () = ()

show_pairs : List (Int, Int) -> String
show_pairs xs = show xs

laziness : Unit -> <IO, Panic> Unit
laziness () =
  println "build"
  let it = Iter.take 2 (Iter.map (fn x -> x * 10) (Iter.generate (noisy 5)))
  println "built"
  match Iter.next it with
    | None -> println "none"
    | Some (x, rest) ->
      println "first \{x}"
      drop rest

main : Unit -> <IO, Panic> Unit
main () =
  laziness ()
  println (show (Iter.to_list (Iter.generate (nums 3))))
  println (show (Iter.to_list (Iter.generate empty)))
  println (show (Iter.to_list (Iter.take 0 (Iter.generate (nums 3)))))
  println (show (Iter.to_list (Iter.take 2 (Iter.generate (nums 5)))))
  println (show (Iter.to_list (Iter.take 9 (Iter.generate (nums 3)))))
  println (show (Iter.to_list (Iter.from_list [1, 2, 3])))
  println (show (Iter.to_list (Iter.map (fn x -> x + 1) (Iter.from_list [1, 2, 3]))))
  println (show (Iter.to_list (Iter.filter (fn x -> x > 1) (Iter.from_list [1, 2, 3]))))
  println (show (Iter.to_list (Iter.filter_map (fn x -> if x > 1 then Some (x * 2) else None) (Iter.from_list [1, 2, 3]))))
  println (show_pairs (Iter.to_list (Iter.zip (Iter.from_list [1, 2, 3]) (Iter.from_list [4, 5]))))
  println (show_pairs (Iter.to_list (Iter.zip (Iter.from_list [1]) (Iter.from_list [4, 5, 6]))))
  println (show_pairs (Iter.to_list (Iter.enumerate 1 (Iter.from_list [7, 8, 9]))))
  Iter.each (fn x -> println "each \{x}") (Iter.from_list [1, 2])
  println (show (Iter.fold (fn acc x -> acc + x) 0 (Iter.from_list [1, 2, 3, 4])))
  println (show (Yield.collect (fn () -> Iter.yield_all (Iter.from_list [5, 6, 7]))))
  drop (Iter.generate (noisy 3))
  println "dropped"
