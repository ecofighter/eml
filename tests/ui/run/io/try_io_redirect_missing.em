-- リダイレクトのファイルを開けなければ、子を起動せずに起動の誤りになる。種類はファイルを開いた誤りのもので、
-- `try_io` が捕まえる (docs/spec/stdlib.md の「`Proc`」)
import Proc

main : Unit -> <IO, Panic> Unit
main () =
  match try_io (fn () -> Proc.run (Proc.stdin_file "nope.txt" `eml run try_io_redirect_missing.em child`)) with
    | Ok code -> println "exit \{code}"
    | Err e -> println "\{show e.kind}: \{e}"
