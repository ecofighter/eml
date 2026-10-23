-- `Proc.capture` の 0 でない終了は、`kind` が `Other` の IO の失敗で、`try_io` が捕まえる (docs/spec/stdlib.md の
-- 「`try_io`」)
import Proc

main : Unit -> <IO, Panic> Unit
main () =
  match try_io (fn () -> Proc.capture `sh -c 'echo partial; exit 3'`) with
    | Ok out -> println "ok \{show out}"
    | Err e -> println "\{show e.kind}: \{e}"
  match try_io (fn () -> Proc.capture `sh -c 'kill -TERM $$'`) with
    | Ok out -> println "ok \{show out}"
    | Err e -> println "\{show e.kind}: \{e}"
