-- E0011: an import after a declaration. The import is still read, so `Text.pad` resolves and nothing else is
-- reported.
greeting : String
greeting = "hi"

import Text

main : Unit -> <IO> Unit
main () = println (Text.pad greeting)
