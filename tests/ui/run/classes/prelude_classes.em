-- The Prelude classes Eq, Ord and Show on the built-in types, a user instance of Eq that keeps the
-- default `!=`, and a function constrained by Ord.
data Color =
  | Red
  | Green

instance Eq Color where
  Red == Red = True
  Green == Green = True
  _ == _ = False

order : Ordering -> String
order LT = "less"
order EQ = "equal"
order GT = "greater"

max_of : Ord a => a -> a -> a
max_of x y = if x < y then y else x

main : Unit -> <IO> Unit
main () =
  println (show (1 == 1) ++ " " ++ show ("a" != "b") ++ " " ++ show (True == False))
  println (show (Red != Green))
  println (order (compare 1 2) ++ " " ++ order (compare "b" "a") ++ " " ++ order (compare True True))
  println (show (max_of 3 7) ++ " " ++ max_of "pear" "apple")
  println (show "say \"hi\"\n")
  println (show_prec 7 (-5) ++ " " ++ show_prec 0 (-5) ++ " " ++ show 42)
  println (show LT ++ " " ++ show True)
