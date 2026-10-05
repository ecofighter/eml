-- Option values built by constructors and taken apart by `match`.
data Option a =
  | None
  | Some a

describe : Option Int -> String
describe o = match o with
  | None -> "none"
  | Some n -> "some " ++ show_int n

safe_div : Int -> Int -> Option Int
safe_div a b = if b == 0 then None else Some (a / b)

main : Unit -> <IO> Unit
main () =
  println (describe (safe_div 10 2))
  println (describe (safe_div 1 0))
  println (describe (Some 42))
