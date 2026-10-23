-- `(n, [])` の葉は `unr` の `n` だけを束縛するので、`(Int, List FileIn)` の値を drop してから `n` を返す
-- (docs/spec/core-ir.md の「決定木の葉の `drop`」)。
import FileIn

close_all : List FileIn.FileIn -> <IO> Unit
close_all [] = println "closed"
close_all (f :: rest) =
  FileIn.close f
  close_all rest

count : (Int, List FileIn.FileIn) -> <IO> Int
count p =
  match p with
    | (n, []) -> n
    | (n, f :: rest) ->
        FileIn.close f
        close_all rest
        n

main : Unit -> <IO> Unit
main () =
  println (show (count (1, [FileIn.open "input.txt"])))
  println (show (count (2, [])))
