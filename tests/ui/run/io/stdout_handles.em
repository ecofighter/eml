-- `println` and `Stdout` handles write into one buffer, so the output keeps the program order
import Bytes
import Stderr
import Stdout

main : Unit -> <IO, Panic> Unit
main () =
  let out = Stdout.open ()
  println "one"
  let out = Stdout.write_string "two" out
  let out = Stdout.write (Bytes.of_string ", three\n") out
  let out = out |> Stdout.write_line "four" |> Stdout.flush
  Stdout.close out
  let out = Stdout.open ()
  let out = Stdout.write_line "five" out
  Stdout.close out
  println "six"
  let err = Stderr.open ()
  let err = Stderr.write_string "e" err
  eprintln "1"
  let err = Stderr.write (Bytes.of_string "e2\n") err
  Stderr.close err
