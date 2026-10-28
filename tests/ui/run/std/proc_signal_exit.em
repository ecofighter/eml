-- windows: skip (シグナルで終わる子は unix にだけある)
-- シグナルで終わった子の終了コードは、シェルと同じく 128 + シグナルの番号である (docs/spec/stdlib.md の「`Proc`」)
import Proc

main : Unit -> <IO, Panic> Unit
main () =
  match try_io (fn () -> Proc.capture `sh -c 'kill -TERM $$'`) with
    | Ok out -> println "ok \{show out}"
    | Err e -> println "\{show e.kind}: \{e}"
