-- A handler keeps a file as its state, reads it in a clause, and closes it in the `return` clause.
import FileIn

effect Load where
  load : Unit -> String

main : Unit -> <IO> Unit
main () =
  let text =
    handle load () from FileIn.open "input.txt" with
      | ctl load () k f ->
          let (s, f) = FileIn.read_all f
          k s f
      | return x f ->
          FileIn.close f
          x
  println text
