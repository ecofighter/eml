-- `kind` tells apart a file and a standard input that are not UTF-8.
import Fs
import Stdin

report : Result IOError String -> <IO> Unit
report (Ok text) = println "ok \{text}"
report (Err e) = println "\{show e.kind}: \{e}"

main : Unit -> <IO, Panic> Unit
main () =
  report (try_io (fn () -> Fs.read_text "not_utf8.txt"))
  report (try_io (fn () -> Stdin.read_text ()))
