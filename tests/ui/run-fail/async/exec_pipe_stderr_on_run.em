-- 標準エラーの pipe は `Exec.spawn` の最後の段だけが使える。最後でない段の `pipe_stderr` と、`Exec.run` の
-- `pipe_stderr` は、子を起動せずに起動の誤り (`Other`) にする。誤りは `Proc.launch_args` が起こすので、仮想の時計の
-- driver の断りより先に出る (docs/spec/stdlib.md の「`Std.Exec`」)
import Async (Async)
import Async
import Exec
import Proc ((.|))

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  match try_io (fn () -> drop (Exec.spawn (Exec.pipe_stderr `eml-first` .| `eml-second`))) with
    | Ok () -> println "unreachable"
    | Err e -> println "\{show e.kind}: \{e}"
  println "exit \{show (Exec.run (Exec.pipe_stderr `eml-only`))}")
