-- E1037: an instance can only define the methods of its class.
class Size a where
  size : a -> Int

data Color =
  | Red
  | Green

instance Size Color where
  size _ = 1
  weight _ = 2

main : Unit -> <IO> Unit
main () = ()
