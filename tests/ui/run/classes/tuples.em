-- Tuples and Unit have structural instances of Eq, Ord and Show.
main : Unit -> <IO> Unit
main () =
  println (show ((1, "a") == (1, "a")) ++ " " ++ show ((1, 2) < (1, 3)) ++ " " ++ show (() == ()))
  println (show (1, "two", (True, ())))
  println (show (compare (2, 0) (1, 9)))
