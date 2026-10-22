-- `FileOut.append` keeps what the file already holds and writes after it
import FileOut
import Fs

main : Unit -> <IO> Unit
main () =
  Fs.write_text "log.txt" "1\n"
  FileOut.close (FileOut.write_line "2" (FileOut.append "log.txt"))
