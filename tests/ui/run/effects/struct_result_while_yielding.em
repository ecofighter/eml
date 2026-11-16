-- 構造体を返す関数はyieldする間ダミーを返す。yieldの道ではダミーを`unbox`し、`release unboxed`で手放す。
-- `release unboxed`はダミーには何もしない (docs/spec/lir.md の「ダミーの値」)。
effect Pick where
  pick : Unit -> Int

data Pair =
  | Pair String Int

pair_of : Int -> <Pick, Panic> Pair
pair_of n = Pair (show n) (n + pick ())

twice_of : Int -> <Pick, Panic> Pair
twice_of n = Pair (show (n * 2)) (n * pick ())

run : List (Int -> <Pick, Panic> Pair) -> Int -> <Pick, Panic> String
run fs n =
  match fs with
    | [] -> "."
    | f :: rest ->
      match f n with
        | Pair s m -> s ++ " " ++ show m ++ " " ++ run rest n

picked : Int -> <Panic> String
picked n =
  handle run [pair_of, twice_of] n with
    | ctl pick () k -> "picked " ++ k 10

main : Unit -> <IO, Panic> Unit
main () =
  println (picked 3)
