-- `(n, [])` の葉は `unr` の `n` だけを束縛するので、`(Int, List File)` の値を drop してから `n` を返す
-- (docs/superpowers/specs/2026-10-13-s8d-kind-marks-design.md の「明示の drop」)。
close_all : List Fs.File -> <IO> Unit
close_all [] = println "closed"
close_all (f :: rest) =
  Fs.close f
  close_all rest

count : (Int, List Fs.File) -> <IO> Int
count p =
  match p with
    | (n, []) -> n
    | (n, f :: rest) ->
        Fs.close f
        close_all rest
        n

main : Unit -> <IO> Unit
main () =
  println (show (count (1, [Fs.open "input.txt"])))
  println (show (count (2, [])))
