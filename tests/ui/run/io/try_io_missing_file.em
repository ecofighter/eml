-- `try_io` turns a failed `Fs.read_text` into `Err`, whose kind and message can be shown; a success is `Ok`.
main : Unit -> <IO> Unit
main () =
  match try_io (fn () -> Fs.read_text "missing.txt") with
    | Ok text -> println "read \{text}"
    | Err e -> println "\{show e.kind}: \{e}"
  match try_io (fn () -> Fs.read_text "input.txt") with
    | Ok text -> println "read \{String.byte_length text} bytes"
    | Err e -> println "\{e}"
