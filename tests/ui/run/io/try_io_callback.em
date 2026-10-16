-- A function that calls its callback inside `try_io` also catches the callback's IO failures: `IO` is never masked.
attempt : (Unit -> <e> a) -> <IO | e> Result IOError a
attempt action = try_io (fn () -> action ())

main : Unit -> <IO> Unit
main () =
  match attempt (fn () -> Fs.read_text "missing.txt") with
    | Ok text -> println text
    | Err e -> println "attempt caught: \{e}"
