-- `Shape(..)` brings the type and its constructors in unqualified, in expressions and in patterns.
import Shapes (Shape(..))

area : Shape -> Int
area s = match s with
  | Square n -> n * n
  | Rect w h -> w * h

main : Unit -> <IO> Unit
main () =
  println (show (area (Square 4)))
  println (show (area (Rect 2 3)))
