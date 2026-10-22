-- The body of a handle runs at most once, so it can capture a file.
import FileIn

effect Ask where
  ask : Unit -> Int

use_file : FileIn.FileIn -> <Ask, IO> Int
use_file f =
  FileIn.close f
  ask ()

main : Unit -> <IO, Panic> Unit
main () =
  let f = FileIn.open "input.txt"
  let n =
    handle use_file f with
      | ctl ask () k -> k 41
  println (show (n + 1))
