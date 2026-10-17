-- A row-polymorphic function is used both without handlers and under a handler in one program.
effect Ask where
  ask : Unit -> Int

mapl : (a -> <e> b) -> List a -> <e> List b
mapl f xs =
  match xs with
    | [] -> []
    | x :: rest -> f x :: mapl f rest

main : Unit -> <IO> Unit
main () =
  println (show (mapl (fn x -> x + 1) [1, 2, 3]))
  let r =
    handle mapl (fn x -> x + ask ()) [1, 2, 3] with
      | ctl ask () k -> k 10
  println (show r)
