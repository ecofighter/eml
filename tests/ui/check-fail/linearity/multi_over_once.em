-- E3006: the continuation `k` of a `once` operation is kept alive across a `multi` operation. Resuming the `multi`
-- continuation twice would resume `k` twice, so the carry-over rule rejects the program.
effect Ask where
  ask : Unit -> Int

effect Choice where
  multi choose : Unit -> Bool

inner : Unit -> <Choice> Int
inner () =
  handle ask () with
    | ask () k ->
        let b = choose ()
        resume k (if b then 1 else 2)

main : Unit -> <IO> Unit
main () =
  let total =
    handle inner () with
      | choose () c -> resume c True + resume c False
  println (show_int total)
