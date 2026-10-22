-- `Option (List File)` の `Some []` を検査する葉は、`File` を何も束縛しないので、調べた値を drop する。`Some []` は
-- `lin` の値なので、`lin` の受け手に渡せる (docs/spec/core-ir.md の「決定木の葉の `drop`」)。
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
    | Some [] -> println "empty"
    | Some (f :: rest) ->
        FileIn.close f
        close_all rest

mk : Unit -> Option (List FileIn.FileIn)
mk () = Some []

main : Unit -> <IO> Unit
main () =
  close_opt (Some [FileIn.open "input.txt"])
  close_opt (Some [])
  close_opt (mk ())
  close_opt None
