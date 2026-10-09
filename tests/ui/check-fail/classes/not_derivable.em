-- E1038: only Eq, Ord and Show can be derived. E2006: a derived instance needs an instance for
-- every field.
class Size a where
  size : a -> Int

data Box = | Box Int deriving Size

data Fn = | Fn (Int -> Int) deriving Show

main : Unit -> <IO> Unit
main () = ()
