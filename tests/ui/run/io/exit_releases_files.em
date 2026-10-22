-- `exit` passes through frames holding files, a `try_io` and a handler whose state is a file; everything they hold is
-- released once, and neither `try_io` nor the `return` clause runs.
import FileIn

effect Tick where
  tick : Unit -> Unit

deep : Int -> <Tick, IO, Panic> Int
deep n =
  let f = FileIn.open "input.txt"
  tick ()
  let r = if n == 0 then exit 4 else deep (n - 1) + 1
  FileIn.close f
  r

main : Unit -> <IO, Panic> Unit
main () =
  let g = FileIn.open "input.txt"
  let result =
    handle try_io (fn () -> deep 2) from g with
      | ctl tick () k st -> k () st
      | return x st ->
        FileIn.close st
        x
  match result with
    | Ok n -> println "ok \{n}"
    | Err e -> println "caught: \{e}"
