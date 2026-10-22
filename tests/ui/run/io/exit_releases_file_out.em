-- `exit` releases a `FileOut` that is never closed, and the release writes out its buffer
import FileOut

main : Unit -> <IO> Unit
main () =
  let f = FileOut.write_line "kept" (FileOut.create "out.txt")
  exit 3
