-- 子の標準入出力のリダイレクト (docs/spec/stdlib.md の「`Cmd`」)。`stdin_file` は `< in`、`stdout_file` は `> out`、
-- `stdout_append` は `>> out` で、相対パスは `FileIn.open` と同じ基準である。標準出力をリダイレクトした `output` は
-- 何も読まない。子はこのファイルを `child` の引数で流した eml で、標準入力の各行に `> ` を付けて書く
import Env
import FileIn
import List
import Proc
import Stdin
import String

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | ["child"] -> List.each (fn line -> println "> \{line}") (String.lines (Stdin.read_text ()))
    | _ -> parent ()

parent : Unit -> <IO, Panic> Unit
parent () =
  let child = `eml run proc_redirect.em child`
  println "run \{Proc.run (child |> Proc.stdin_file "proc_redirect.input" |> Proc.stdout_file "out.txt")}"
  println "run \{Proc.run (child |> Proc.stdin_text "third\n" |> Proc.stdout_append "out.txt")}"
  println (show (Proc.output (child |> Proc.stdin_text "fourth\n" |> Proc.stdout_append "out.txt")))
  println (show (Proc.output (Proc.no_stdin child)))
  let (text, f) = FileIn.read_all (FileIn.open "out.txt")
  FileIn.close f
  println (String.trim text)
