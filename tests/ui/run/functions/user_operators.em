-- Operators defined by the user, with and without a fixity declaration, and one that hides `&&`.
infixr 5 <+>

(<+>) : String -> String -> String
a <+> b = a ++ "/" ++ b

(<->) : Int -> Int -> Int
a <-> b = a - b

data P =
  | E
  | Int :+ P

infixr 5 :+

total : P -> Int
total p = match p with
  | E -> 0
  | n :+ rest -> n + total rest

main : Unit -> <IO> Unit
main () =
  println ("a" <+> "b" <+> "c")
  println (show_int (10 <-> 3 <-> 2))
  println (show_int (total (1 :+ 2 :+ 3 :+ E)))
