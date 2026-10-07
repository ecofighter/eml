-- E2001: a `Main.Color` is passed where a `Paint.Color` is expected. Two modules define `Color`, so both are shown
-- qualified by their module.
import Paint

data Color =
  | Blue

main : Unit -> <IO> Unit
main () = println (Paint.name Blue)
