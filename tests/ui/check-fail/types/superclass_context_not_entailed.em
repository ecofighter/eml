-- E2006: the `Eq` instance for `Box a` needs `Eq a`, so an `Ord` instance for `Box a` must also require `Eq a` in
-- its context to use it as its superclass instance.
data Box a =
  | Box a

instance Eq a => Eq (Box a) where
  (Box x) == (Box y) = x == y

instance Ord (Box a) where
  compare _ _ = EQ

main : Unit -> <IO> Unit
main () = ()
