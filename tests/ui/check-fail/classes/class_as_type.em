-- E1043: a class in the type position of a signature.
class Size a where
  size : a -> Int

measure : Size -> Int
measure _ = 0

main : Unit -> <IO> Unit
main () = ()
