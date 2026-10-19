-- `x |> f a` evaluates `x` before the arguments of `f a`.
say : String -> <IO> Int
say s =
  println s
  1

add : Int -> Int -> <Panic> Int
add a b = a + b

main : Unit -> <IO, Panic> Unit
main () =
  let n = say "left" |> add (say "right")
  println (show n)
