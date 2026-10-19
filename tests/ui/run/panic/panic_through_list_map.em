-- std の `List.map` に渡した関数で誤りが起きる。`List.map` の row 変数が `Panic` を運び、`try_panic` が捕まえる。
import List

main : Unit -> <IO> Unit
main () =
  println (show (try_panic (fn () -> List.map (fn x -> 12 / x) [1, 2, 0, 4])))
  println (show (try_panic (fn () -> List.map (fn x -> if x > 3 then panic "big \{x}" else x) [1, 5])))
  println (show (try_panic (fn () -> List.map (fn x -> 12 / x) [1, 2, 3])))
