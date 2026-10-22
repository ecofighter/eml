-- A `FileOut` keeps what it writes in its buffer until `flush` writes it out
import FileOut
import Fs

main : Unit -> <IO> Unit
main () =
  let f = FileOut.write_line "x" (FileOut.create "f.txt")
  println (show (Fs.read_text "f.txt"))
  let f = FileOut.flush f
  println (show (Fs.read_text "f.txt"))
  FileOut.close f
