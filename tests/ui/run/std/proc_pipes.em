import Bytes
import Env
import Proc
import Stdin
import String

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | ["child"] ->
      let text = Stdin.read_text ()
      println "got \{String.trim text}"
      println "second"
      println "third"
      println "fourth"
    | _ -> parent ()

parent : Unit -> <IO, Panic> Unit
parent () =
  let c = Proc.spawn `eml run proc_pipes.em child`
  let c = Proc.write "a" c
  let c = Proc.write_bytes (Bytes.of_string "b\n") c
  let c = Proc.close_stdin c
  let (line, c) = Proc.read_line c
  match line with
    | Some l -> println l
    | None -> println "no line"
  let (line, c) = Proc.read_line_bytes c
  match line with
    | Some l -> println "bytes \{Bytes.length l}"
    | None -> println "no line"
  let (chunk, c) = Proc.read_chunk (Bytes.empty ()) c
  let (rest, c) = Proc.read_all_bytes c
  println "tail \{Bytes.length chunk + Bytes.length rest}"
  println "exit \{Proc.wait c}"
