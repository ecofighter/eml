-- E2006: an equality reference whose argument type is never fixed cannot choose how to compare.
f : Int -> Int
f x =
  let eq = (==)
  x

main : Unit -> <IO> Unit
main () = ()
