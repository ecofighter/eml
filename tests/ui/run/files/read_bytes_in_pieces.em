import Bytes
import FileIn

main : Unit -> <IO, Panic> Unit
main () =
  let f = FileIn.open "bytes_input.txt"
  let (chunk, f) = FileIn.read_chunk (Bytes.empty ()) f
  println "chunk \{Bytes.length chunk > 0}"
  FileIn.close f
  let f = FileIn.open "bytes_input.txt"
  let (line, f) = FileIn.read_line_bytes f
  match line with
    | Some l -> println "line \{Bytes.length l}"
    | None -> println "no line"
  let (rest, f) = FileIn.read_all_bytes f
  println "rest \{Bytes.length rest}"
  let (more, f) = FileIn.read_line_bytes f
  match more with
    | Some l -> println "more \{Bytes.length l}"
    | None -> println "end"
  FileIn.close f
