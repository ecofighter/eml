-- An IO failure under `try_io` releases the handles on the way: the file whose read failed is closed by the
-- unwinding, and the `Stdout` handle is released without losing what it wrote
import Bytes
import FileIn
import Stdout

read_bad : Stdout.Stdout -> <IO, Panic> Stdout.Stdout
read_bad out =
  let out = Stdout.write_line "before" out
  let (text, f) = FileIn.read_all (FileIn.open "not_utf8.txt")
  FileIn.close f
  Stdout.write_line text out

main : Unit -> <IO, Panic> Unit
main () =
  match try_io (fn () -> Stdout.close (read_bad (Stdout.open ()))) with
    | Ok () -> println "ok"
    | Err e -> println "caught: \{e}"
  let (b, f) = FileIn.read_all_bytes (FileIn.open "not_utf8.txt")
  FileIn.close f
  println "bytes \{Bytes.length b}"
