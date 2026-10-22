-- E2001: a file opened for writing is not a file opened for reading
import FileIn
import FileOut

main : Unit -> <IO> Unit
main () = FileIn.close (FileOut.create "out.txt")
