-- `Option (List File)` の `Some []` を検査する葉は、`File` を何も束縛しないので、調べた値を drop する。`Some []` は
-- `lin` の値なので、`lin` の受け手に渡せる (docs/spec/core-ir.md の「決定木の葉の `drop`」)。
import Fs

close_all : List Fs.File -> <IO> Unit
close_all [] = println "closed"
close_all (f :: rest) =
  Fs.close f
  close_all rest

close_opt : Option (List Fs.File) -> <IO> Unit
close_opt o =
  match o with
    | None -> println "none"
    | Some [] -> println "empty"
    | Some (f :: rest) ->
        Fs.close f
        close_all rest

mk : Unit -> Option (List Fs.File)
mk () = Some []

main : Unit -> <IO> Unit
main () =
  close_opt (Some [Fs.open "input.txt"])
  close_opt (Some [])
  close_opt (mk ())
  close_opt None
