-- 文字列のリテラルの `match` で、`default` の枝が値そのものを使う。`switch` は値を読むだけで、`default` の枝が値を
-- 受け取り、ほかの枝が値を手放す。
shout : String -> String
shout s =
  match s with
    | "a" -> "A"
    | other -> other ++ "!"

main : Unit -> <IO> Unit
main () =
  println (shout "a")
  println (shout "b")
