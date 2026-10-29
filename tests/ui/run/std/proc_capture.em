-- 出力を取る起動 (docs/spec/stdlib.md の「`Proc`」)。`output` は終了コードを値で返し、0 でなくても失敗しない。
-- `capture` は 0 で終わった子の標準出力を返す。標準入力は空なので、標準入力を写す子はすぐに終わる。子はこのファイルを
-- `child` の引数で流した eml である
import Env
import Proc
import Stdin
import Stdout
import String

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | ["child", "partial"] ->
      println "partial"
      exit 5
    | ["child", "cat"] -> Stdout.close (Stdout.write (Stdin.read_text ()) (Stdout.open ()))
    | "child" :: words -> println (String.join " " words)
    | _ -> parent ()

parent : Unit -> <IO, Panic> Unit
parent () =
  let child = `eml run proc_capture.em child`
  println (show (Proc.capture (Proc.arg "hi" child)))
  println (show (Proc.output (Proc.arg "partial" child)))
  println (show (Proc.capture (Proc.arg "cat" child)))
  let cmd = child |> Proc.arg "one" |> Proc.args ["two", "three"]
  println (display cmd)
  println (show (Proc.capture cmd))
