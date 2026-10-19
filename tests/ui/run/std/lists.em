-- List の関数 (docs/spec/stdlib.md)。効果のある関数は要素の順に呼ぶ
logged : Int -> <IO, Panic> Int
logged x =
  println "visit \{x}"
  x * 10

main : Unit -> <IO, Panic> Unit
main () =
  let xs = [1, 2, 3, 4]
  println (show (List.map logged xs))
  println (show (List.filter (fn x -> x % 2 == 0) xs))
  println (show (List.filter_map (fn x -> if x > 2 then Some (x + 100) else None) xs))
  println (show (List.fold (fn acc x -> acc * 10 + x) 0 xs))
  List.each (fn x -> println "each \{x}") xs
  println (show (List.length xs))
  println (show (List.reverse xs))
  println (show (List.append xs [5, 6]))
  println (show (List.map (fn x -> x) (Nil : List Int)))
