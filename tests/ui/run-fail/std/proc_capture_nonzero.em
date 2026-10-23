-- windows: skip (子のプロセスに POSIX のツールを使う)
-- `try_io` で捕まえない `Proc.capture` の 0 でない終了は、実行時エラーになる (docs/spec/stdlib.md の「実行時エラー」)
import Proc

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  println (Proc.capture `sh -c 'echo partial; exit 3'`)
