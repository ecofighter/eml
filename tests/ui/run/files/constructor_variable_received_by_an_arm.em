-- `p` を束縛した後で検査し、`other` の枝が `p` そのものを受け取る。決定木は `p` を開かずに分解するので、`xs` と
-- `ys` を2か所で使わない (docs/spec/core-ir.md の「translate が決める印」)。
close_all : List Fs.File -> <IO> Unit
close_all [] = println "closed"
close_all (f :: rest) =
  Fs.close f
  close_all rest

take : (List Fs.File, List Fs.File) -> <IO> Unit
take p =
  match p with
    | (a, b) ->
        close_all a
        close_all b

go : List Fs.File -> List Fs.File -> <IO> Unit
go xs ys =
  let p = (xs, ys)
  println "between"
  match p with
    | ([], b) -> close_all b
    | other -> take other

main : Unit -> <IO> Unit
main () =
  go [] [Fs.open "input.txt"]
  go [Fs.open "input.txt"] []
