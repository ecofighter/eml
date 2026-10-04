-- The continuation `k` of a `once` operation is saved across a `multi` operation, so resuming the `multi`
-- continuation twice copies `k` and resumes it on each path. The carry-over rule of stage 5 will reject this
-- program statically; until then the runtime copies `k` with its frames and releases every copy.
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
