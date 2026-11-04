-- clock: real (the test driver has no processes)
-- `Proc.stdin_text` は `Std.Exec` でも、文字列を子の標準入力にする (docs/spec/stdlib.md の「`Std.Exec`」)。子は
-- このファイルを `cat` の引数で流した eml で、標準入力をそのまま標準出力に写す。pipe の buffer より大きい 200 KiB を
-- 渡し、`Exec.output` が driver の pipe で読み切った文字列と比べる。`Exec.run` の子は受け継いだ標準出力に書く
import Async (Async)
import Env
import Exec
import List
import Proc
import Stdin
import Stdout
import String

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | ["cat"] -> Stdout.close (Stdout.write (Stdin.read_text ()) (Stdout.open ()))
    | ["count"] ->
      let text = Stdin.read_text ()
      println "child: bytes \{show (String.byte_length text)} lines \{show (List.length (String.lines text))}"
    | _ -> Async.run parent

-- 1行は64バイトで、3200 行で 200 KiB になる
build : Int -> String -> <Panic> String
build n acc =
  if n == 0 then acc
  else build (n - 1) (acc ++ "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcde\n")

parent : Unit -> <IO, Panic, Async> Unit
parent () =
  let text = build 3200 ""
  let (out, code) = Exec.output (Proc.stdin_text text `eml run exec_stdin_text.em cat`)
  println "bytes \{show (String.byte_length out)} same \{show (out == text)} exit \{show code}"
  let code = Exec.run (Proc.stdin_text "a\nb\nc\n" `eml run exec_stdin_text.em count`)
  println "run exit \{show code}"
