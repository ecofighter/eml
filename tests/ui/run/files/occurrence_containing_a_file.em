-- `(o, 0)` の葉は `h` を中に持つ `Some h` を受け取るので、`h` を drop しない
-- (docs/superpowers/specs/2026-10-13-s8d-kind-marks-design.md の「明示の drop」)。
close_opt : Option Fs.File -> <IO> Unit
close_opt o =
  match o with
    | None -> println "none"
    | Some f ->
        Fs.close f
        println "closed in close_opt"

go : Fs.File -> Int -> <IO> Unit
go h n =
  match (Some h, n) with
    | (o, 0) -> close_opt o
    | (Some h2, _) ->
        Fs.close h2
        println "closed in go"
    | (None, _) -> println "never"

main : Unit -> <IO> Unit
main () =
  go (Fs.open "input.txt") 0
  go (Fs.open "input.txt") 1
