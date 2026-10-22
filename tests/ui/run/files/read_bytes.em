-- `Bytes` reads accept any bytes; `read_chunk` appends to the given `Bytes`
import Bytes
import FileIn
import Fs
import String

main : Unit -> <IO, Panic> Unit
main () =
  let (b, f) = FileIn.read_all_bytes (FileIn.open "not_utf8.txt")
  FileIn.close f
  println "all \{Bytes.length b}"
  Fs.write_text "lines.txt" "ab\r\nc\n"
  let f = FileIn.open "lines.txt"
  let (l, f) = FileIn.read_line_bytes f
  match l with
    | Some x -> println "line \{Bytes.length x}"
    | None -> println "none"
  let (rest, f) = FileIn.read_chunk (Bytes.of_string ">") f
  println (show (String.from_utf8 rest))
  let (more, f) = FileIn.read_chunk (Bytes.empty ()) f
  println "more \{Bytes.length more}"
  FileIn.close f
