-- 文字列のリテラルの `match` で、`default` の枝が値そのものを使う。`Switch` の前で値を複製し、二重に解放しない。
shout : String -> String
shout s =
  match s with
    | "a" -> "A"
    | other -> other ++ "!"

main : Unit -> <IO> Unit
main () =
  println (shout "a")
  println (shout "b")
