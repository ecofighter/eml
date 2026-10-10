-- Lists may be written with leading commas, as in Haskell and Elm, also when the bracket starts a statement.
apply_all : List (Int -> Int) -> Int -> List Int
apply_all fs x =
  match fs with
    | [] -> []
    | f :: rest -> f x :: apply_all rest x

fs : List (Int -> Int)
fs =
  [ fn x ->
      x + 1
  , fn x -> x * 2
  ]

main : Unit -> <IO> Unit
main () =
  let xs =
    [ 1
    , 2
    , 3
    ]
  println (show (apply_all fs 10))
  println (show xs)
