-- `FileOut` writes a string, a line and bytes into one file; `close` writes them out
import Bytes
import FileOut
import Fs

main : Unit -> <IO> Unit
main () =
  let f = FileOut.create "out.txt"
  let f = FileOut.write "a" f
  let f = FileOut.write_line "b" f
  let f = FileOut.write_bytes (Bytes.of_string "c\n") f
  FileOut.close f
  println (Fs.read_text "out.txt")
