-- フィールドを持つ値が `match` の `_` に進む。値を分解せずに手放し、漏れも二重の解放もない。
data Shape = | Dot | Line String | Box String String

name : Shape -> String
name s =
  match s with
    | Dot -> "dot"
    | _ -> "other"

main : Unit -> <IO> Unit
main () =
  println (name Dot)
  println (name (Line "a"))
  println (name (Box "b" "c"))
