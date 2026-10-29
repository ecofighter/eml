-- 子のプロセスとプログラムが標準エラーに書いた行は、標準出力とは別に捕まえる。子はこのファイルを `child` の引数で
-- 流した eml である
import Env
import Proc

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | ["child"] ->
      println "out"
      eprintln "err"
    | _ ->
      println "before"
      eprintln "error before"
      let code = Proc.run `eml run child_stderr.em child`
      println "exit \{code}"
      eprintln "error after"
