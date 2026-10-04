-- E2005: a row variable cannot occur in the type argument of a label of its own row. `g` performs `Store` with `g`
-- itself as the type argument, so the row of `g` would be infinite.
effect Store s where
  put : s -> Unit

seq : Unit -> Int -> Int
seq _ n = n

main : Unit -> <IO> Unit
main () =
  let h = fn g -> seq (put g) (g ())
  println "x"
