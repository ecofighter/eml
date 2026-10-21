-- E1034: an instance must be in the module of its class or of its head type, and this module
-- imports both.
import Size (Size(..))
import Color (Color(..))

instance Size Color where
  size _ = 1

main : Unit -> <IO> Unit
main () = ()
