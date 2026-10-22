-- E3001: the function in the result of `mk` is handed to the performer, which may call it any number of times. The
-- clause must then give a function that may be called any number of times, but this lambda captures a file.
import FileIn

effect Mk where
  mk : Unit -> (Unit -> <IO> Unit, Int)

use_it : Unit -> <Mk, IO> Unit
use_it () =
  let (g, n) = mk ()
  g ()
  g ()

main : Unit -> <IO> Unit
main () =
  handle use_it () with
    | ctl mk () k ->
        let h = FileIn.open "input.txt"
        k (fn () -> FileIn.close h, 1)
    | return u -> u
