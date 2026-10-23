-- E3002: `FileIn` を捕まえたクロージャは `Lin` なので、読む式で広がらず、`once` の位置へ2回渡すと誤りになる
-- (docs/spec/types.md の「値を読む式の矢印の線形性」)。
import FileIn

run_once : (once Unit -> <IO> Unit) -> <IO> Unit
run_once f = f ()

main : Unit -> <IO> Unit
main () =
  let file = FileIn.open "/dev/null"
  let g = fn () -> FileIn.close file
  run_once g
  run_once g
