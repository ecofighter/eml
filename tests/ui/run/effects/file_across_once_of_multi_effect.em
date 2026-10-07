-- A file can be kept alive across a `once` operation of an effect that also has a `multi` operation, because only
-- the `multi` operations copy the continuation.
effect Mixed where
  single : Unit -> Int
  multi many : Unit -> Int

counted : File -> <Mixed, IO> Int
counted f =
  let n = single ()
  close f
  n + many ()

main : Unit -> <IO> Unit
main () =
  let total =
    handle counted (open "../files/input.txt") with
      | single () k -> k 1
      | many () k -> k 10 + k 20
  println (show_int total)
