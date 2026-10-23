-- `FileIn` を持つ `data` の、フィールドのない case の葉は、調べた値を drop する
-- (docs/spec/core-ir.md の「決定木の葉の `drop`」)。
import FileIn

data Res =
  | Empty
  | Has FileIn.FileIn

fin : Res -> <IO> Unit
fin r =
  match r with
    | Empty -> println "empty"
    | Has f ->
        FileIn.close f
        println "closed"

main : Unit -> <IO> Unit
main () =
  fin Empty
  fin (Has (FileIn.open "input.txt"))
