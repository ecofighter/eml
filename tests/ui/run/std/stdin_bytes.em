import Bytes
import Stdin

main : Unit -> <IO, Panic> Unit
main () =
  let s = Stdin.open ()
  let (line, s) = Stdin.read_line_bytes s
  match line with
    | Some l -> println "line \{Bytes.length l}"
    | None -> println "no line"
  let (chunk, s) = Stdin.read_chunk (Bytes.empty ()) s
  println "chunk \{Bytes.length chunk > 0}"
  let (rest, s) = Stdin.read_all_bytes s
  println "total \{Bytes.length chunk + Bytes.length rest}"
  Stdin.close s
