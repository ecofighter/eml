-- E2012: a constrained type variable that grows on each recursive call would need instances at
-- infinitely many types.
nest : Show a => Int -> a -> <Panic> String
nest n x = if n == 0 then show x else nest (n - 1) (x, x)

main : Unit -> <IO, Panic> Unit
main () = println (nest 2 1)
