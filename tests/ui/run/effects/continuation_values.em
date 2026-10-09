-- A continuation is a value: it can be passed to a function and captured by a closure before it is resumed.
effect Ask where
  ask : Unit -> Int

add_two : Unit -> <Ask> Int
add_two () = ask () + ask ()

main : Unit -> <IO> Unit
main () =
  let passed =
    handle add_two () with
      | ask () k ->
          let go = fn cont value -> cont value
          go k 20
  println (show passed)
  let captured =
    handle add_two () with
      | ask () k ->
          let later = fn n -> k n
          later 7
  println (show captured)
