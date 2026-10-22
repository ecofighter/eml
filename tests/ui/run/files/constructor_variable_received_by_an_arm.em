-- `p` を束縛した後で検査し、`other` の枝が `p` そのものを受け取る。決定木は `p` を開かずに分解するので、`xs` と
-- `ys` を2か所で使わない (docs/spec/core-ir.md の「translate が決める印」)。
import FileIn

close_all : List FileIn.FileIn -> <IO> Unit
close_all [] = println "closed"
close_all (f :: rest) =
  FileIn.close f
  close_all rest

take : (List FileIn.FileIn, List FileIn.FileIn) -> <IO> Unit
take p =
  match p with
    | (a, b) ->
        close_all a
        close_all b

go : List FileIn.FileIn -> List FileIn.FileIn -> <IO> Unit
go xs ys =
  let p = (xs, ys)
  println "between"
  match p with
    | ([], b) -> close_all b
    | other -> take other

main : Unit -> <IO> Unit
main () =
  go [] [FileIn.open "input.txt"]
  go [FileIn.open "input.txt"] []
