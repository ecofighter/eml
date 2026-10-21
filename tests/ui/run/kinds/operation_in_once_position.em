-- 操作の参照も、読む式で広がるので `once` の位置に渡せる。節 `fn ask () -> 7` は、渡された位置で1回だけ呼ばれる
-- (docs/spec/types.md の「値を読む式の矢印の線形性」)。
effect Ask where
  ask : Unit -> Int

run_once : (once Unit -> <Ask> Int) -> <Ask> Int
run_once f = f ()

main : Unit -> <IO> Unit
main () =
  let n = handle run_once ask with
    | fn ask () -> 7
  println (show n)
