-- `Float.parse (show x)` は `x` を返す。Python と照らした値の一覧のどれもで成り立つ。NaN は `==` で比べられない
-- ので `is_nan` で確かめる。
import Float
import List

round_trips : Float -> Bool
round_trips x =
  match Float.parse (show x) with
    | Some y -> if Float.is_nan x then Float.is_nan y else y == x && compare y x == EQ
    | None -> False

main : Unit -> <IO, Panic> Unit
main () =
  let values =
    [ 0.1, 1.0 / 3.0, 1e16, 1.5e16, 1e15, 123456.0, 1e-5, 0.0001, -0.0, 1.0 / 0.0, -1.0 / 0.0
    , 0.0 / 0.0, 1e22, 5e-324, 1.7976931348623157e308, 2.5, 3.5, -1.5, 0.5, -0.5
    , 7.120236347223045e-307, 6.189700196426902e26
    ]
  println (show (List.map round_trips values))
