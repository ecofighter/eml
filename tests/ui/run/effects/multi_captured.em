-- The continuation of a `multi` operation is an unrestricted value. A closure can capture it and be called more
-- than once, and a local function can take it and resume it more than once.
effect Ask where
  multi ask : Unit -> Int

plus_one : Unit -> <Ask, Panic> Int
plus_one () = ask () + 1

twice : (Int -> <e> Int) -> <Panic | e> Int
twice f = f 10 + f 20

main : Unit -> <IO, Panic> Unit
main () =
  let captured =
    handle plus_one () with
      | ctl ask () k -> twice (fn n -> k n)
  println (show captured)
  let passed =
    handle plus_one () with
      | ctl ask () k ->
          let go = fn cont value -> cont value
          go k 1 * go k 2
  println (show passed)
