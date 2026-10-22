-- A `FileOut` that is never closed writes out its buffer when `try_panic` or `try_io` unwinds past it
import FileIn
import FileOut
import Fs

main : Unit -> <IO> Unit
main () =
  let caught =
    try_panic (fn () ->
      let f = FileOut.write_line "panic" (FileOut.create "a.txt")
      panic "stop")
  match caught with
    | Ok () -> println "ok"
    | Err e -> println "caught: \{e}"
  print_file "a.txt"
  let failed =
    try_io (fn () ->
      let f = FileOut.write_line "io" (FileOut.create "b.txt")
      FileIn.close (FileIn.open "missing.txt")
      FileOut.close f)
  match failed with
    | Ok () -> println "ok"
    | Err e -> println "caught: \{e}"
  print_file "b.txt"

print_file : String -> <IO> Unit
print_file path = println (show (Fs.read_text path))
