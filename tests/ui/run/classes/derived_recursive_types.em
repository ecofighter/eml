-- Derived instances of a recursive type and of two mutually recursive types call themselves through
-- instance resolution.
data List a =
  | Nil
  | Cons a (List a)
  deriving (Eq, Ord, Show)

data Tree =
  | Leaf Int
  | Node Forest
  deriving (Eq, Show)

data Forest =
  | Empty
  | More Tree Forest
  deriving (Eq, Show)

main : Unit -> <IO> Unit
main () =
  println (show (Cons 1 (Cons 2 Nil)))
  println (show (Cons 1 Nil == Cons 1 Nil) ++ " " ++ show (compare (Cons 1 Nil) (Cons 1 (Cons 0 Nil))))
  println (show (Node (More (Leaf 1) Empty)))
  println (show (Node Empty == Node (More (Leaf 1) Empty)))
