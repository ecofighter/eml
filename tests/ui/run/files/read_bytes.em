-- `Bytes` reads accept any bytes; `read_chunk` appends to the given `Bytes`
import Bytes
import Fs
import String

main : Unit -> <IO, Panic> Unit
main () =
  let (b, f) = Fs.read_all_bytes (Fs.open "not_utf8.txt")
  Fs.close f
  println "all \{Bytes.length b}"
  Fs.write_text "lines.txt" "ab\r\nc\n"
  let f = Fs.open "lines.txt"
  let (l, f) = Fs.read_line_bytes f
  match l with
    | Some x -> println "line \{Bytes.length x}"
    | None -> println "none"
  let (rest, f) = Fs.read_chunk (Bytes.of_string ">") f
  println (show (String.from_utf8 rest))
  let (more, f) = Fs.read_chunk (Bytes.empty ()) f
  println "more \{Bytes.length more}"
  Fs.close f
