-- `Proc.capture` の 0 でない終了は、`kind` が `Other` の IO の失敗で、`try_io` が捕まえる (docs/spec/stdlib.md の
-- 「`try_io`」)。子はこのファイルを `child` の引数で流した eml である
import Env
import Proc

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | ["child"] ->
      println "partial"
      exit 3
    | _ ->
      match try_io (fn () -> Proc.capture `eml run try_io_cmd_exit.em child`) with
        | Ok out -> println "ok \{show out}"
        | Err e -> println "\{show e.kind}: \{e}"
