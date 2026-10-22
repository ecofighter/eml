-- With nested `try_io`, the inner one catches the failure and the outer one returns `Ok`.
import Fs

inner : Unit -> <IO> String
inner () =
  match try_io (fn () -> Fs.read_text "missing.txt") with
    | Ok _ -> "inner ok"
    | Err e -> "inner caught: \{e}"

main : Unit -> <IO> Unit
main () =
  match try_io inner with
    | Ok text -> println "outer ok, \{text}"
    | Err e -> println "outer caught: \{e}"
