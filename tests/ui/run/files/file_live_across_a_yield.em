-- `f` は `ask ()` の yield をまたいで生きている。EP が作る続きのクロージャは `f` を捕まえるので、印が `lin` になる
-- (docs/spec/core-ir.md の「EP が足す変数の印」)。節は再開の後に続きがあるので、
-- 実行時にもクロージャを作り、再開すると `f` を閉じる。
import FileIn

effect Ask where
  ask : Unit -> Int

use_it : FileIn.FileIn -> <Ask, IO> Unit
use_it f =
  let n = ask ()
  println (show n)
  FileIn.close f
  println "closed"

main : Unit -> <IO> Unit
main () =
  handle use_it (FileIn.open "input.txt") with
    | ctl ask () k ->
      let u = k 1
      println "resumed"
      u
