-- An operation whose parameter is an effectful function; the clause builds a lambda over that row.
effect Run where
  run : (Unit -> <e> Int) -> Int

body : Unit -> <Run> Int
body () = run (fn () -> 1)

main : Unit -> <IO> Unit
main () =
  let r =
    handle body () with
      | run f k ->
        let g = fn () -> f () + 1
        k 7
  println (show r)
