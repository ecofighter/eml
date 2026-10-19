-- E1052: 浮動小数のリテラルはパターンに書けない。負のリテラルも同じである。
f : Float -> Int
f x = match x with
  | 1.5 -> 1
  | -2.5 -> 2
  | _ -> 0

main : Unit -> <IO> Unit
main () = println (show (f 1.5))
