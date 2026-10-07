-- E1030: `Prelude` and `Main` are reserved module names.
import Prelude
import Main

main : Unit -> <IO> Unit
main () = println "x"
