-- 引数はすべて評価してから矢印を適用する (docs/spec/expressions.md の「関数適用」)。`pick 1` が `multi` の操作を起こす
-- 間、評価済みの `FileIn.open` の値を持つので、持ち越し規則が E3006 にする。
import FileIn

effect Choose where
  multi choose : Unit -> Bool

closer : Int -> FileIn.FileIn -> <IO> Int
closer n f =
  FileIn.close f
  n

pick : Int -> <Choose, IO, Panic> (FileIn.FileIn -> <IO> Int)
pick n = if choose () then closer n else closer (n + 1)

body : Unit -> <Choose, IO, Panic> Int
body () = pick 1 (FileIn.open "input.txt")

main : Unit -> <IO, Panic> Unit
main () =
  let r = handle body () with
    | ctl choose () k -> k True + k False
  println (show r)
