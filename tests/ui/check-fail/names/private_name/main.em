-- E1029: `hidden` is not `pub` in `Lib`, in the import list and qualified. The use of the listed name reports
-- nothing more.
import Lib (hidden)

main : Unit -> <IO> Unit
main () =
  println (show (Lib.hidden 1))
  println (show (hidden 2))
