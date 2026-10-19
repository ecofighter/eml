-- `multi` の操作の継続がリストを退避し、2回の再開がそれぞれ同じリストを分解する。
effect Choice where
  multi choose : Unit -> Bool

data List a =
  | Nil
  | Cons a (List a)

sum : List Int -> <Panic> Int
sum xs = match xs with
  | Nil -> 0
  | Cons x rest -> x + sum rest

go : List Int -> <Choice, Panic> Int
go xs =
  let b = choose ()
  match xs with
    | Nil -> 0
    | Cons x rest -> (if b then x else 0) + sum rest

main : Unit -> <IO, Panic> Unit
main () =
  let r = handle go (Cons 1 (Cons 2 (Cons 3 Nil))) with
    | ctl choose () k -> k True + k False
  println (show r)
