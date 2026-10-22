-- `(o, 0)` の葉は `h` を中に持つ `Some h` を受け取るので、`h` を drop しない
-- (docs/spec/core-ir.md の「決定木の葉の `drop`」)。
import FileIn

close_opt : Option FileIn.FileIn -> <IO> Unit
close_opt o =
  match o with
    | None -> println "none"
    | Some f ->
        FileIn.close f
        println "closed in close_opt"

go : FileIn.FileIn -> Int -> <IO> Unit
go h n =
  match (Some h, n) with
    | (o, 0) -> close_opt o
    | (Some h2, _) ->
        FileIn.close h2
        println "closed in go"
    | (None, _) -> println "never"

main : Unit -> <IO> Unit
main () =
  go (FileIn.open "input.txt") 0
  go (FileIn.open "input.txt") 1
