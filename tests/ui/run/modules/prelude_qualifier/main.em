-- `Prelude` works as a qualifier in every module: for a value here, and for a value, constructors, and an effect in
-- a row inside `Greet`.
import Greet

main : Unit -> <IO> Unit
main () =
  Greet.greet (Prelude.not False)
  Greet.greet False
