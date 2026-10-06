-- E0004: list patterns, which come with lists in stage S2.
first : Int -> Int
first x =
  match x with
    | y :: ys -> y
    | _ -> 0

main : Unit -> <IO> Unit
main () = println "done"
