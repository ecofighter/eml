-- 末尾再帰で書いた List の関数は、長いリストでも呼び出しの深さの上限に当たらない (docs/spec/stdlib.md の「`List`」)
import List

count_down : Int -> List Int -> <Panic> List Int
count_down n acc = if n == 0 then acc else count_down (n - 1) (n :: acc)

main : Unit -> <IO, Panic> Unit
main () =
  let xs = count_down 100000 Nil
  println (show (List.length (List.take 99999 xs)))
  let (front, back) = List.split_at 50000 xs
  println (show (List.length front, List.length back))
  println (show (List.nth 99999 xs, List.index_of 100000 xs, List.contains 100001 xs))
  println (show (List.length (List.zip xs xs)))
  println (show (List.lookup 100000 (List.zip xs xs)))
  let sorted = List.sort_by (fn x y -> compare y x) xs
  println (show (List.take 3 sorted))
