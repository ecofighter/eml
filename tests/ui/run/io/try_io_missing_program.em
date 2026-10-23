-- When `Proc.run` cannot start the child, `spawn_raw` fails with `NotFound` and the abort lands in `try_io`, which
-- returns the error. The failing extern's result slot is never read.
import Proc

main : Unit -> <IO, Panic> Unit
main () =
  match try_io (fn () -> Proc.run `eml-no-such-program`) with
    | Ok code -> println "exit \{code}"
    | Err e -> println "\{show e.kind}: \{e}"
