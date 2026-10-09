-- E1028: `pick` comes from two imports that bring different definitions; both imports are shown.
import Left (pick)
import Right (pick)

main : Unit -> <IO> Unit
main () = println (show (pick 1))
