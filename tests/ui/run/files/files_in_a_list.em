-- A list of linear files is linear: it is taken apart with a list pattern and every file is closed.
import FileIn

main : Unit -> <IO> Unit
main () =
  let files = [FileIn.open "input.txt", FileIn.open "input.txt"]
  match files with
    | [a, b] ->
        FileIn.close a
        FileIn.close b
        println "closed two"
    | rest -> close_all rest

close_all : List FileIn.FileIn -> <IO> Unit
close_all [] = println "closed"
close_all (f :: rest) =
  FileIn.close f
  close_all rest
