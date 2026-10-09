-- Methods used as values, partially applied and in operator sections, with a fixity declared for
-- the method operator.
infixl 6 <+>

class Join a where
  (<+>) : a -> a -> a

instance Join Int where
  a <+> b = a + b

apply_twice : (Int -> Int) -> Int -> Int
apply_twice f x = f (f x)

main : Unit -> <IO> Unit
main () =
  println (show_int (apply_twice (<+> 10) 1))
  println (show_int (apply_twice ((<+>) 5) 0))
  let join = (<+>)
  println (show_int (join 2 3 <+> 4))
