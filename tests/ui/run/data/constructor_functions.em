-- Constructors are values: they can be partially applied and passed to functions.
data Pair a b =
  | Pair a b

data Option a =
  | None
  | Some a

apply_to : (a -> b) -> a -> b
apply_to f x = f x

show_pair : Pair Int String -> String
show_pair p = match p with
  | Pair n s -> show n ++ " " ++ s

show_option : Option Int -> String
show_option o = match o with
  | None -> "none"
  | Some n -> show n

main : Unit -> <IO> Unit
main () =
  let with_one = Pair 1
  println (show_pair (with_one "one"))
  println (show_pair (apply_to (Pair 2) "two"))
  println (show_option (apply_to Some 3))
  let make = Some
  println (show_option (make 4))
