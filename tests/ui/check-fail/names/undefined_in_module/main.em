-- E1002 and E1001 for names that `Lib` does not define; the messages name the module.
import Lib

two : Unit -> <Panic> Lib.Count
two () = Lib.one + Lib.uno

main : Unit -> <IO, Panic> Unit
main () = println (show (two ()))
