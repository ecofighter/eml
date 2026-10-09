-- Derived Show of an infix constructor places parentheses by the constructor's fixity, as Haskell's
-- derived Show does.
infixr 5 :+

data Chain =
  | End
  | Int :+ Chain
  deriving (Eq, Show)

data Wrap = | Wrap Chain deriving Show

main : Unit -> <IO> Unit
main () =
  println (show (1 :+ 2 :+ End))
  println (show (Wrap (1 :+ End)))
  println (show ((-1) :+ End))
