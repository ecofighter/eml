deep : Int -> Int -> <IO> Int
deep n d =
  if n == 0 then 1 / d
  else
    let s = show_int n
    let r = deep (n - 1) d
    println s
    r + 1

main : Unit -> <IO> Unit
main () = println (show_int (deep 1000 0))
