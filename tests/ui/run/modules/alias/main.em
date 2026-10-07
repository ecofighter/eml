-- `import Text.Pad as P` makes only the qualifier `P`.
import Text.Pad as P

main : Unit -> <IO> Unit
main () = println (P.pad "x")
