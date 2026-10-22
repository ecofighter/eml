-- 位置と要素で引く List の関数 (docs/spec/stdlib.md の「`List`」)。範囲の外は切り詰めるか `None` にする
import List

main : Unit -> <IO, Panic> Unit
main () =
  let xs = [1, 2, 3, 4]
  println (show (List.take 2 xs, List.take 0 xs, List.take 9 xs, List.take (-1) xs))
  println (show (List.split_at 2 xs, List.split_at (-1) xs, List.split_at 9 xs))
  println (show (List.nth 0 xs, List.nth 3 xs, List.nth 4 xs, List.nth (-1) xs))
  println (show (List.zip xs ["a", "b"], List.zip (Nil : List Int) xs))
  println (show (List.index_of 3 xs, List.index_of 7 xs, List.index_of 1 [2, 1, 1]))
  println (show (List.contains 3 xs, List.contains 7 xs))
  println (show (List.lookup "b" [("a", 1), ("b", 2), ("b", 3)], List.lookup "z" [("a", 1)]))
