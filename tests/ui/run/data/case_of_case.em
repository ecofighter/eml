-- Case-of-case: known tags jump straight to their arm, and arms with fields stay in the switch. Every path
-- frees what it built.
data Option a =
  | None
  | Some a

data Color =
  | Red
  | Green
  | Blue

describe : Option String -> String
describe o = if (match o with | Some _ -> True | None -> False) then "has a value" else "empty"

color_code : Int -> Int
color_code n =
  let c = if n == 0 then Red else if n == 1 then Green else Blue
  match c with
    | Red -> 10
    | Green -> 20
    | Blue -> 30

pick : Bool -> Bool -> String -> String
pick a b s = match (if a then None else if b then Some s else Some "x") with
  | None -> "none"
  | Some t -> "some " ++ t

main : Unit -> <IO> Unit
main () =
  println (describe (Some "v"))
  println (describe None)
  println (show_int (color_code 0 + color_code 1 + color_code 2))
  println (pick True True "s")
  println (pick False True "s")
  println (pick False False "s")
