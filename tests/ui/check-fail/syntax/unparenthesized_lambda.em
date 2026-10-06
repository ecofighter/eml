-- A lambda or a `resume` used as an argument must be parenthesized; each reports E0012 once.
each : Int -> (Int -> <e> Unit) -> <e> Unit
each n f =
  if n > 0 then
    f n
    each (n - 1) f

inc : Int -> Int
inc n = n + 1

effect Ask where
  ask : Unit -> Int

main : Unit -> <IO> Unit
main () =
  each 3 fn n ->
    println (show_int n)
  let v = handle ask () + 1 with
    | ask () k -> inc resume k 1
  println (show_int v)
