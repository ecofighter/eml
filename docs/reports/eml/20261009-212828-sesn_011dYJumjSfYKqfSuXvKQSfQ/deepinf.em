deep : Int -> <IO> Int
deep n =
  let s = show_int n
  let r = deep (n + 1)
  println s
  r + 1

main : Unit -> <IO> Unit
main () = println (show_int (deep 0))
