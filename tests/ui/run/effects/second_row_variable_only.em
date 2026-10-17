-- Only the second row variable `f` reaches the body's row. Used purely, `pick` needs no bit; under a handler with a pure
-- first callback, only the bit of `f` is set, so the instance with just the second bit runs.
effect Ask where
  ask : Unit -> Int

pick : (Unit -> <e> Int) -> (Unit -> <f> Int) -> <f> Int
pick _ second = second () + 1

main : Unit -> <IO> Unit
main () =
  println (show (pick (fn () -> 1) (fn () -> 2)))
  let r =
    handle pick (fn () -> 1) (fn () -> ask () * 10) with
      | ctl ask () k -> k 4
  println (show r)
