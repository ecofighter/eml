-- E3001: a `once` continuation captured by a lambda stays linear when the lambda reaches an operation through the
-- type argument of an effect label. Passing `inc` first makes the label `Store (Int -> Int)` unrestricted, so the
-- second `put` must not accept a lambda that resumes `k`; otherwise `f` would resume `k` twice.
effect Ask where
  ask : Unit -> Int

effect Store s where
  put : s -> Unit

inc : Int -> Int
inc x = x + 1

seq : Unit -> Int -> Int
seq _ n = n

main : Unit -> <IO> Unit
main () =
  let r =
    handle ask () with
      | ask () k ->
          handle seq (put inc) (seq (put (fn x -> resume k x)) 0) with
            | put f k2 -> f 1 + f 2 + resume k2 ()
  println (show_int r)
