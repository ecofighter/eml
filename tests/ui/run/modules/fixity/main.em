-- Imported operators bring their `pub` fixities: `<**>` binds tighter than `<+>`. With the default `infixl 9` for
-- both, the first line would print 9.
import Ops ((<+>), (<**>))

main : Unit -> <IO> Unit
main () =
  println (show_int (1 <+> 2 <**> 3))
  println (show_int (2 <**> 3 <+> 4))
