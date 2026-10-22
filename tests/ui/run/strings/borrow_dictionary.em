-- `Eq a` の関数を `String` で使う。辞書の `==` は借用の extern を指し、同じ文字列を比べ続けても参照の数が合う。
import List
import String

same : Eq a => a -> a -> Bool
same x y = x == y

count_equal : Eq a => a -> List a -> <Panic> Int
count_equal x ys = List.length (List.filter (fn y -> same x y) ys)

main : Unit -> <IO, Panic> Unit
main () =
  let s = "abc" ++ "d"
  println (show (same s s))
  println (show (same s "abc"))
  println (show (count_equal s ["abcd", "x", s, s]))
  println (show (same (String.slice 1 3 s) "bc"))
