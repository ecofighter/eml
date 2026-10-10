-- Any constructor may have named fields, mixed with positional ones; patterns may omit fields.
data Shape =
  | Circle { radius : Int }
  | Rect { width : Int, height : Int }
  | Dot Int
  | Empty

area : Shape -> Int
area s = match s with
  | Circle { radius } -> 3 * radius * radius
  | Rect { width, height } -> width * height
  | Dot _ -> 0
  | Empty -> 0

width_of : Shape -> Int
width_of s = match s with
  | Rect { width } -> width
  | _ -> 0

main : Unit -> <IO> Unit
main () =
  println (show (area (Circle { radius = 2 })))
  println (show (area (Rect { width = 3, height = 4 })))
  println (show (width_of (Rect { height = 1, width = 7 })))
