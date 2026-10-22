-- 同じ形の `Some []` が2つあっても、`Option (List File)` の値は `lin` なので、それぞれ別の `con` で作る
-- (docs/spec/core-ir.md の「translate が決める印」)。
import Fs

close_opt : Option (List Fs.File) -> <IO> Unit
close_opt o =
  match o with
    | None -> println "none"
    | Some [] -> println "empty"
    | Some (f :: rest) ->
        Fs.close f
        close_opt (Some rest)

main : Unit -> <IO> Unit
main () =
  let (a, b) = (Some [], Some [])
  close_opt a
  close_opt b
