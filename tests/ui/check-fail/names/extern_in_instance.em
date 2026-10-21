-- E1033: `extern` in an instance is only allowed in the standard library. The method still
-- counts as defined, so E1036 is not reported as well.
class Size a where
  size : a -> Int

data Color =
  | Red
  | Green

instance Size Color where
  extern size

main : Unit -> <IO> Unit
main () = ()
