-- Two imports with the same qualifier merge, so `T` looks in both modules. Importing the same module twice is not an
-- error.
import Text.Upper as T
import Text.Lower as T
import Text.Upper

main : Unit -> <IO> Unit
main () =
  println (T.shout "a")
  println (T.whisper "b")
  println (Upper.shout "c")
