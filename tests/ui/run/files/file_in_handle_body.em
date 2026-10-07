-- The body of a handle runs at most once, so it can capture a file.
effect Ask where
  ask : Unit -> Int

use_file : File -> <Ask, IO> Int
use_file f =
  close f
  ask ()

main : Unit -> <IO> Unit
main () =
  let f = open "input.txt"
  let n =
    handle use_file f with
      | ask () k -> k 41
  println (show_int (n + 1))
