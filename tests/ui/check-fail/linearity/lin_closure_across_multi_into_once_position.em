-- E3006: `File` を捕まえたクロージャは `Lin` のまま広がらない。`multi` の操作をまたいで持つと、再開のたびに
-- 閉じることになるので、持ち越し規則が止める (docs/spec/types.md の「値を読む式の矢印の線形性」)。
import FileIn

effect Choice where
  multi choose : Unit -> Bool

run_once : (once Int -> <IO> Unit) -> <IO> Unit
run_once f = f 1

pick : Unit -> <Choice, IO> Unit
pick () =
  let file = FileIn.open "/dev/null"
  let g = fn n -> FileIn.close file
  let c = choose ()
  run_once g

main : Unit -> <IO> Unit
main () = println "x"
