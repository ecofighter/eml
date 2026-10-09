-- A class and an instance in different modules: the instance lives with its type, and the method
-- operator keeps the fixity its module declared.
import Shapes (Area(..))
import Squares (Square(..))

main : Unit -> <IO> Unit
main () = println (show (Square 2 <+> Square 3 + 1))
