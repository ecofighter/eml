-- `Unr` のクロージャは、`multi` の操作をまたいで持てる。呼んだ後で `once` の位置にも渡せ、再開のたびに
-- 同じ結果になる (docs/spec/types.md の「値を読む式の矢印の線形性」)。
effect Choice where
  multi choose : Unit -> Bool

run_once : (once Int -> String) -> String
run_once f = f 1

pick : Unit -> <Choice> String
pick () =
  let s = "a" ++ "b"
  let g = fn n -> s
  let c = choose ()
  if c then run_once g else g 2

main : Unit -> <IO> Unit
main () =
  let r =
    handle [pick ()] with
      | ctl choose () k -> List.append (k True) (k False)
  println "\{r}"
