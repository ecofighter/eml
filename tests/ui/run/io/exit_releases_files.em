-- `exit` passes through frames holding files, a `try_io` and a handler whose state is a file; everything they hold is
-- released once, and neither `try_io` nor the `return` clause runs.
import Fs

effect Tick where
  tick : Unit -> Unit

deep : Int -> <Tick, IO, Panic> Int
deep n =
  let f = Fs.open "input.txt"
  tick ()
  let r = if n == 0 then exit 4 else deep (n - 1) + 1
  Fs.close f
  r

main : Unit -> <IO, Panic> Unit
main () =
  let g = Fs.open "input.txt"
  let result =
    handle try_io (fn () -> deep 2) from g with
      | ctl tick () k st -> k () st
      | return x st ->
        Fs.close st
        x
  match result with
    | Ok n -> println "ok \{n}"
    | Err e -> println "caught: \{e}"
