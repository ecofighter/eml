-- An uncaught runtime error releases a `FileOut` that is never closed, and the release writes out its buffer
import FileOut

main : Unit -> <IO, Panic> Unit
main () =
  let f = FileOut.write_line "kept" (FileOut.create "out.txt")
  panic "stop"
