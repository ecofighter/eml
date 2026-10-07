-- E1002 and E1001 for names that `Lib` does not define; the messages name the module.
import Lib

two : Lib.Count
two = Lib.one + Lib.uno

main : Unit -> <IO> Unit
main () = println (show_int two)
