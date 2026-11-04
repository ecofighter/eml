-- `Proc.stdin_text` は文字列を子の標準入力にする (docs/spec/stdlib.md の「`Proc`」)。pipe の buffer より大きい 2 MiB を
-- `capture` で渡し、3行を `output` で渡す。子はこのファイルを `child` の引数で流した eml で、標準入力のバイト数と行数を
-- 書く
import Env
import List
import Proc
import Stdin
import String

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | ["child"] ->
      let text = Stdin.read_text ()
      println "bytes \{String.byte_length text} lines \{List.length (String.lines text)}"
    | _ -> parent ()

-- 1行は64バイトで、32768 行で 2 MiB になる
build : Int -> String -> <Panic> String
build n acc =
  if n == 0 then acc
  else build (n - 1) (acc ++ "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcde\n")

parent : Unit -> <IO, Panic> Unit
parent () =
  let child = `eml run proc_stdin_text.em child`
  println (String.trim (Proc.capture (Proc.stdin_text (build 32768 "") child)))
  let (out, code) = Proc.output (Proc.stdin_text "a\nb\nc\n" child)
  println (String.trim out)
  println "exit \{code}"
