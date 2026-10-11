-- A continuation captured by one handler is resumed after that handle returns, inside another handler of the same effect.
-- The resumed computation reinstalls the captured handler on top, so its second `ask` goes back to that handler.
effect Ask where
  multi ask : Unit -> Int

data Saved =
  | Saved (Int -> Saved)
  | Value Int

capture : Unit -> Saved
capture () =
  handle Value (ask () + ask ()) with
    | ask () k -> Saved k

describe : Saved -> String
describe s =
  match s with
    | Saved _ -> "saved"
    | Value n -> show n

main : Unit -> <IO> Unit
main () =
  match capture () with
    | Saved k ->
      let r =
        handle k 1 with
          | ask () k2 -> k2 100
      println (describe r)
      match r with
        | Saved k3 -> println (describe (k3 2))
        | Value n -> println (show n)
    | Value n -> println (show n)
