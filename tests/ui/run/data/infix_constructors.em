-- An infix constructor declared with a `:` operator, in expressions and patterns. Without a fixity declaration
-- it is `infixl 9`, so a right-nested sequence needs parentheses.
data Seq a =
  | Done
  | a :> Seq a

total : Seq Int -> Int
total s = match s with
  | Done -> 0
  | x :> rest -> x + total rest

main : Unit -> <IO> Unit
main () = println (show_int (total (1 :> (2 :> (3 :> Done)))))
