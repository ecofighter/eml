-- A list of linear files is linear: it is taken apart with a list pattern and every file is closed.
main : Unit -> <IO> Unit
main () =
  let files = [Fs.open "input.txt", Fs.open "input.txt"]
  match files with
    | [a, b] ->
        Fs.close a
        Fs.close b
        println "closed two"
    | rest -> close_all rest

close_all : List Fs.File -> <IO> Unit
close_all [] = println "closed"
close_all (f :: rest) =
  Fs.close f
  close_all rest
