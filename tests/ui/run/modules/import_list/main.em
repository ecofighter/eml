-- An import list brings the value `area` and the type `Shape` in unqualified; the constructors stay qualified.
import Shapes (area, Shape)

total : Shape -> Shape -> Int
total a b = area a + area b

main : Unit -> <IO> Unit
main () = println (show_int (total (Shapes.Square 3) (Shapes.Rect 2 5)))
