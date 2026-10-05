-- Operators defined by the user, with and without a fixity declaration, and one that hides `&&` and so evaluates its right operand eagerly.
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

(&&) : Bool -> Bool -> Bool
a && b = if a then b else False

loud : Unit -> <IO> Bool
loud () =
  println "loud evaluated"
  True

main : Unit -> <IO> Unit
main () =
  println ("a" <+> "b" <+> "c")
  println (show_int (10 <-> 3 <-> 2))
  println (show_int (total (1 :+ 2 :+ 3 :+ E)))
  if False && loud () then println "yes" else println "no"
