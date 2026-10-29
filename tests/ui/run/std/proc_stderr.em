-- 標準エラーの向き (docs/spec/stdlib.md の「`Proc`」)。既定は受け継ぎ、`merge_stderr` は標準出力と同じ先に、
-- `discard_stderr` はどこにも出さない。子はこのファイルを `child` の引数で流した eml である
import Env
import Proc
import Stdout

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | ["child"] -> child ()
    | _ -> parent ()

-- 標準出力を書き出してから標準エラーに書き、合わせた pipe の中の順を決める
child : Unit -> <IO, Panic> Unit
child () =
  Stdout.close (Stdout.flush (Stdout.write_line "out" (Stdout.open ())))
  eprintln "err"

parent : Unit -> <IO, Panic> Unit
parent () =
  let c = `eml run proc_stderr.em child`
  println "inherit \{show (Proc.capture c)}"
  println "merge_stderr \{show (Proc.capture (Proc.merge_stderr c))}"
  println "discard_stderr \{show (Proc.capture (Proc.discard_stderr c))}"
  println "run \{Proc.run (Proc.merge_stderr c)}"
  println "run \{Proc.run (Proc.discard_stderr c)}"
