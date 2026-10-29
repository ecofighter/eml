-- 子のプロセス (docs/spec/stdlib.md)。子の出力は、それまでの出力の後に並ぶ。子はこのファイルを `child` の引数で
-- 流した eml である
import Env
import Proc

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | ["child", arg] ->
      println "child \{arg}"
      exit 3
    | ["child"] -> ()
    | _ ->
      println "before"
      let code = Proc.run `eml run processes.em child 'arg with space'`
      println "exit \{code}"
      println "exit \{Proc.run `eml run processes.em child`}"
