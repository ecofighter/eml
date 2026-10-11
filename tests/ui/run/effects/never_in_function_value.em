-- A function that fails with a `never` operation is passed as a value and fails inside `mapl`.
effect Fail where
  never fail : String -> a

check : Int -> <Fail> Int
check n = if n < 0 then fail "neg" else n

mapl : (a -> <e> b) -> List a -> <e> List b
mapl f xs =
  match xs with
    | [] -> []
    | x :: rest -> f x :: mapl f rest

main : Unit -> <IO> Unit
main () =
  let r =
    handle mapl check [1, -2, 3] with
      | fail _ -> []
      | return x -> x
  println (show r)
