-- An instance method that calls another method of the same instance at a larger type is not
-- polymorphic recursion: the reference resolves to that method's function directly, so no E2012.
data Box a =
  | Box a

instance Eq a => Eq (Box a) where
  (Box x) == (Box y) = x == y
  p != q = not (Box p == Box q)

main : Unit -> <IO> Unit
main () =
  let one = Box (1 : Int)
  println (show (one != Box 2))
  println (show (one != Box 1))
  println (show (Box (Box "a") != Box (Box "a")))
