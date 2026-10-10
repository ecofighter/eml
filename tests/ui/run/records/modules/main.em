-- Records of another module are built and taken apart by field name, with the constructor imported or qualified.
import Shapes (Shape(..))

area : Shape -> Int
area s = match s with
  | Circle { radius } -> 3 * radius * radius
  | Shapes.Rect { height, width } -> width * height

main : Unit -> <IO> Unit
main () =
  println (show (area (Circle { radius = 1 })))
  println (show (area (Shapes.Rect { width = 2, height = 3 })))
