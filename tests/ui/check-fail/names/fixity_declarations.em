-- E1021 and E1022: one fixity per operator, and only for operators defined in this module.
infixr 5 :+
infixl 6 :+
infixl 6 +

data P =
  | E
  | Int :+ P

main : Unit -> <IO> Unit
main () = ()
