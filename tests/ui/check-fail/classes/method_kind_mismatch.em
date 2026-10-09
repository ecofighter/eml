-- E2011: the signature of `pair_with` leaves `b` free to be linear, but the instance uses the
-- value of `b` twice.
class Pairable a where
  pair_with : a -> b -> (b, b)

data Box = | Box Int

instance Pairable Box where
  pair_with _ x = (x, x)

main : Unit -> <IO> Unit
main () = ()
