-- Every combination of three choices runs. The copied closures of the continuation capture a string and the
-- evidence of the `Name` handler installed inside the captured part, and each copy releases what it holds.
effect Choice where
  multi choose : Unit -> Bool

effect Name where
  name : Unit -> String

bit : Bool -> Int
bit b = if b then 1 else 0

describe : Unit -> <Choice, Name, Panic> String
describe () =
  let prefix = name ()
  let a = bit (choose ())
  let b = bit (choose ())
  let c = bit (choose ())
  prefix ++ ":" ++ show (a * 4 + b * 2 + c)

labelled : Unit -> <Choice, Panic> String
labelled () =
  handle describe () with
    | ctl name () k -> k "bits"

every : Unit -> <Panic> String
every () =
  handle labelled () with
    | ctl choose () k -> k False ++ " " ++ k True

paths : Unit -> <Panic> Int
paths () =
  handle labelled () with
    | ctl choose () k -> k False + k True
    | return s -> 1

main : Unit -> <IO, Panic> Unit
main () =
  println (every ())
  println (show (paths ()))
