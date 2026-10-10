-- The Prelude's List, Option and Result: building with `::` and `Nil`, Show, derived Eq and Ord, and `::` as a
-- section and as an operator reference.
data Wrap =
  | Wrap (List Int)
  deriving Show

main : Unit -> <IO> Unit
main () =
  let xs = 1 :: 2 :: 3 :: Nil
  println (show xs)
  println (show (Nil : List Int))
  println (show ("a" :: Nil))
  println (show (-1 :: Nil))
  println (show (Some (1 :: Nil)))
  println (show (Some (-1) :: Nil))
  println (show ((1 :: Nil) :: Nil :: Nil))
  println (show (Wrap xs))
  println (show (xs == 1 :: 2 :: 3 :: Nil))
  println (show (compare (1 :: Nil) (1 :: 2 :: Nil)))
  println (show (compare (2 :: Nil) (1 :: 2 :: Nil)))
  println (show (compare Nil (1 :: Nil)))
  println (show (Some 1 < None))
  println (show (Ok 1 : Result String Int))
  println (show (Err "e" < (Ok 0 : Result String Int)))
  println (show ((::) 1 Nil))
  println (show ((1 ::) Nil))
  println (show ((:: Nil) 1))
  println (show (-1 :: xs))
  println (show (0 :: xs == [0, 1, 2, 3]))
