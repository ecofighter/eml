-- Imported operators bring their `pub` fixities: `<**>` binds tighter than `<+>`. With the default `infixl 9` for
-- both, the first line would print 9.
import Ops ((<+>), (<**>))

main : Unit -> <IO> Unit
main () =
  println (show (1 <+> 2 <**> 3))
  println (show (2 <**> 3 <+> 4))
