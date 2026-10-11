-- Every combination of three choices runs. The copied closures of the continuation capture a string and the
-- evidence of the `Name` handler installed inside the captured part, and each copy releases what it holds.
effect Choice where
  multi choose : Unit -> Bool

effect Name where
  name : Unit -> String

bit : Bool -> Int
bit b = if b then 1 else 0

describe : Unit -> <Choice, Name> String
describe () =
  let prefix = name ()
  let a = bit (choose ())
  let b = bit (choose ())
  let c = bit (choose ())
  prefix ++ ":" ++ show (a * 4 + b * 2 + c)

labelled : Unit -> <Choice> String
labelled () =
  handle describe () with
    | name () k -> k "bits"

every : Unit -> String
every () =
  handle labelled () with
    | choose () k -> k False ++ " " ++ k True

paths : Unit -> Int
paths () =
  handle labelled () with
    | choose () k -> k False + k True
    | return s -> 1

main : Unit -> <IO> Unit
main () =
  println (every ())
  println (show (paths ()))
