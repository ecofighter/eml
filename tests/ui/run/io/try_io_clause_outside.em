-- A clause runs outside its handle, so the `try_io` inside the handle body does not catch an IO failure in the
-- clause; the `try_io` outside the handle does.
effect Load where
  load : String -> String

body : Unit -> <Load, IO> String
body () =
  match try_io (fn () -> load "missing.txt") with
    | Ok text -> "body ok: \{text}"
    | Err e -> "body caught: \{e}"

run : Unit -> <IO> String
run () =
  handle body () with
    | load path k -> k (Fs.read_text path)

main : Unit -> <IO> Unit
main () =
  match try_io run with
    | Ok text -> println text
    | Err e -> println "outside caught: \{e}"
