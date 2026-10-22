-- `Std.Map` の関数 (docs/spec/stdlib.md の「`Std.Map`」)
import List
import Map

count : Option Int -> <Panic> Int
count old =
  match old with
    | Some n -> n + 1
    | None -> 1

main : Unit -> <IO, Panic> Unit
main () =
  let m = Map.from_list [(3, "c"), (1, "a"), (2, "b"), (1, "z")]
  println (show (Map.to_list m))
  println (show (Map.lookup 1 m, Map.lookup 9 m, Map.member 2 m, Map.member 9 m, Map.size m))
  println (show (Map.to_list (Map.insert 2 "B" m)))
  println (show (Map.to_list (Map.delete 2 m), Map.to_list (Map.delete 9 m)))
  let words = ["b", "a", "b", "c", "b", "a"]
  let counts = List.fold (fn acc w -> Map.upsert w count acc) Map.empty words
  println (show (Map.to_list counts))
  println (Map.fold (fn acc k v -> acc ++ k ++ show v) "" counts)
  println (show (Map.to_list (Map.empty : Map.Map Int Int), Map.size (Map.empty : Map.Map Int Int)))
