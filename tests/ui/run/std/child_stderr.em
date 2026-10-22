-- 子のプロセスとプログラムが標準エラーに書いた行は、標準出力とは別に捕まえる
import Proc

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  eprintln "error before"
  let code = Proc.run "sh" ["-c", "echo out; echo err >&2"]
  println "exit \{code}"
  eprintln "error after"
