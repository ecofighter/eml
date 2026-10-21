-- `multi` の操作の継続 `k` は `Unr` なので、`once` の位置に渡した後でも、もう一度再開できる
-- (docs/spec/types.md の「値を読む式の矢印の線形性」)。
effect Choice where
  multi choose : Unit -> Int

run_once : (once Int -> <IO> List Int) -> <IO> List Int
run_once f = f 1

main : Unit -> <IO> Unit
main () =
  let r =
    handle [choose ()] with
      | ctl choose () k -> List.append (run_once k) (k 2)
  println "\{r}"
