-- A runtime error raised in an imported module reports the position in that module's file.
import Ratio

main : Unit -> <IO> Unit
main () = println (show (Ratio.ratio 1 0))
