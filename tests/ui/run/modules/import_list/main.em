-- An import list brings the value `area` and the type `Shape` in unqualified; the constructors stay qualified.
import Shapes (area, Shape)

total : Shape -> Shape -> <Panic> Int
total a b = area a + area b

main : Unit -> <IO, Panic> Unit
main () = println (show (total (Shapes.Square 3) (Shapes.Rect 2 5)))
