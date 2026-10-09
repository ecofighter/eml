-- E2012: the method's own constrained type variable `b` grows on each recursive call of the
-- instance method, so the instance would need `Show` at infinitely many types.
class Measure a where
  measure : Show b => a -> Int -> b -> String

instance Measure Int where
  measure x n y = if n == 0 then show y else measure x (n - 1) (y, y)

main : Unit -> <IO> Unit
main () = println (measure (1 : Int) 2 "a")
