-- 節を手放し、再開できる操作を呼び、節を作る関数の継続を2回再開する。token は yield の地点をまたがないので、2回の
-- 再開はそれぞれ別の節を作る。
effect Choice where
  multi choose : Unit -> Bool

data P = | P Int Int deriving (Show)

make : Int -> <Panic> P
make n = if n == 0 then P 1 2 else make (n - 1)

flip : P -> <Choice, Panic> P
flip p =
  match p with
    | P a b -> if choose () then P b a else P a (b + 10)

main : Unit -> <IO, Panic> Unit
main () =
  let ps =
    handle [flip (make 3)] with
      | ctl choose () k -> List.append (k True) (k False)
  println "\{ps}"
