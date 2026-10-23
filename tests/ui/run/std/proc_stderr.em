-- windows: skip (子のプロセスに POSIX のツールを使う)
-- 標準エラーの向き (docs/spec/stdlib.md の「`Proc`」)。既定は受け継ぎ、`merge_stderr` は標準出力と同じ先に、
-- `discard_stderr` はどこにも出さない
import Proc

main : Unit -> <IO, Panic> Unit
main () =
  let c = `sh -c 'echo out; echo err >&2'`
  println "inherit \{show (Proc.capture c)}"
  println "merge_stderr \{show (Proc.capture (Proc.merge_stderr c))}"
  println "discard_stderr \{show (Proc.capture (Proc.discard_stderr c))}"
  println "run \{Proc.run (Proc.merge_stderr c)}"
  println "run \{Proc.run (Proc.discard_stderr c)}"
