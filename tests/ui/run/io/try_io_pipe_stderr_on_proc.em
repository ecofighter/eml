-- `Proc` は標準エラーの pipe を持たないので、`Exec.pipe_stderr` の `Cmd` を `Proc` で起動すると、子を起動せずに
-- 起動の誤り (`Other`) になり、`try_io` が捕まえる (docs/spec/stdlib.md の「`Proc`」)
import Exec
import Proc

main : Unit -> <IO, Panic> Unit
main () =
  match try_io (fn () -> Proc.run (Exec.pipe_stderr `eml-only`)) with
    | Ok code -> println "exit \{code}"
    | Err e -> println "\{show e.kind}: \{e}"
