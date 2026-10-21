-- E2006: an instance of `Ord` needs an instance of `Eq`, its superclass, for the same type.
data Color =
  | Red
  | Green

instance Ord Color where
  compare _ _ = EQ

main : Unit -> <IO> Unit
main () = ()
