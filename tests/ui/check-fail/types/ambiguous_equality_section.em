-- E2009: an equality reference whose argument type is never fixed cannot choose an instance of Eq.
f : Int -> Int
f x =
  let eq = (==)
  x

main : Unit -> <IO> Unit
main () = ()
