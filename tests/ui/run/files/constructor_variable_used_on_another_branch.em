-- `v` を `if` の両方の枝で使うので、`match v` は `v` の唯一の使用でない。決定木は `v` を開かずに分解し、`xs` を2か所で
-- 使わない (docs/spec/core-ir.md の「translate が決める印」)。
import FileIn

close_all : List FileIn.FileIn -> <IO> Unit
close_all [] = println "closed"
close_all (f :: rest) =
  FileIn.close f
  close_all rest

close_opt : Option (List FileIn.FileIn) -> <IO> Unit
close_opt o =
  match o with
    | None -> println "none"
    | Some l -> close_all l

go : Bool -> List FileIn.FileIn -> <IO> Unit
go c xs =
  let v = Some xs
  println "between"
  if c then
    match v with
      | Some l -> close_all l
      | None -> println "none"
  else close_opt v

main : Unit -> <IO> Unit
main () =
  go True [FileIn.open "input.txt"]
  go False [FileIn.open "input.txt"]
