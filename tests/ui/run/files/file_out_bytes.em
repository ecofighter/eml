-- `FileOut.write_bytes` writes any byte; a written file that is not UTF-8 is shown escaped
import Bytes
import FileOut

main : Unit -> <IO, Panic> Unit
main () = FileOut.close (FileOut.write_bytes (Bytes.make 1 255u8) (FileOut.create "b.bin"))
