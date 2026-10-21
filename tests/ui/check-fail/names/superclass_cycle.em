-- E1042: two classes that are each other's superclass.
class Right a => Left a where
  left : a -> Int

class Left a => Right a where
  right : a -> Int

main : Unit -> <IO> Unit
main () = ()
