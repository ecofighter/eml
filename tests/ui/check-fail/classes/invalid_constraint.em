-- E1040: a constraint on a type variable that does not appear in the type, a context on an
-- operation, and a method that does not mention the class variable.
class Size a where
  size : a -> Int
  zero : Int

count : Size a => Int -> Int
count n = n

effect Measure where
  measure : Size a => a -> Int

main : Unit -> <IO> Unit
main () = ()
