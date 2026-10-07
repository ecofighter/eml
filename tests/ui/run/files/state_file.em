-- A handler keeps a file as its state, reads it in a clause, and closes it in the `return` clause.
effect Load where
  load : Unit -> String

main : Unit -> <IO> Unit
main () =
  let text =
    handle load () from Fs.open "input.txt" with
      | load () k f ->
          let (f, s) = Fs.read_all f
          k s f
      | return x f ->
          Fs.close f
          x
  println text
