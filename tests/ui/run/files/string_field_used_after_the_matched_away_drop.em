-- 葉が `unr` の `String` のフィールド `name` を束縛し、持ち主の `R` を drop した後で使う。持ち主は drop に要るので、
-- Perceus は `name` を `dup` で取る (docs/spec/core-ir.md の「決定木の葉の `drop`」)。
import FileIn

data R =
  | R { name : String, files : List FileIn.FileIn }

close_all : List FileIn.FileIn -> <IO> Unit
close_all [] = println "closed"
close_all (f :: rest) =
  FileIn.close f
  close_all rest

report : R -> <IO> Unit
report r =
  match r with
    | R { name, files = [] } -> println name
    | R { name, files } ->
        println name
        close_all files

main : Unit -> <IO> Unit
main () =
  report (R { name = "none", files = [] })
  report (R { name = "one", files = [FileIn.open "input.txt"] })
