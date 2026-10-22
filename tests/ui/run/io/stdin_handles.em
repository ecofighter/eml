-- A `Stdin` handle and a later one read one buffer in program order; `Bytes` and `String` reads mix
import Bytes
import Stdin
import String

main : Unit -> <IO, Panic> Unit
main () =
  let h = Stdin.open ()
  let (first, h) = Stdin.read_line h
  println (show first)
  Stdin.close h
  let g = Stdin.open ()
  let (second, g) = Stdin.read_line_bytes g
  match second with
    | Some b -> println "bytes \{Bytes.length b}"
    | None -> println "none"
  let (rest, g) = Stdin.read_chunk (Bytes.of_string ">") g
  println (show (String.from_utf8 rest))
  let (more, g) = Stdin.read_chunk (Bytes.empty ()) g
  println "more \{Bytes.length more}"
  let (text, g) = Stdin.read_all g
  println (show text)
  let (last, g) = Stdin.read_line g
  println (show last)
  Stdin.close g
