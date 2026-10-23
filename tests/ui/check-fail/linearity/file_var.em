-- `var` の変数は `Unr` か `MutArray` でなければならない。`FileIn` は E3007
import FileIn

main : Unit -> <IO> Unit
main () =
  var f = FileIn.open "input.txt"
  println "done"
