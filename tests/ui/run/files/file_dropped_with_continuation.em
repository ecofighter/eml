-- A file kept alive across a `once` operation is released with the continuation when the clause drops it.
import FileIn

effect Ask where
  ask : Unit -> Int

read_after : FileIn.FileIn -> <Ask, IO> Unit
read_after f =
  let n = ask ()
  FileIn.close f

main : Unit -> <IO> Unit
main () =
  let f = FileIn.open "input.txt"
  handle read_after f with
    | ctl ask () k ->
        drop k
        println "dropped"
