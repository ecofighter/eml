-- 子のプロセス (docs/spec/stdlib.md)。子の出力は、それまでの出力の後に並ぶ
main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  let code = Proc.run "sh" ["-c", "echo \"child $1\"; exit 3", "sh", "arg with space"]
  println "exit \{code}"
  println "exit \{Proc.run "sh" ["-c", "exit 0"]}"
