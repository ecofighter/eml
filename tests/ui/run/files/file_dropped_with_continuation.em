-- A file kept alive across a `once` operation is released with the continuation when the clause drops it.
effect Ask where
  ask : Unit -> Int

read_after : File -> <Ask, IO> Unit
read_after f =
  let n = ask ()
  close f

main : Unit -> <IO> Unit
main () =
  let f = open "input.txt"
  handle read_after f with
    | ask () k ->
        drop k
        println "dropped"
