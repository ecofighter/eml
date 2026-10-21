-- `var` の変数は `Unr` か `MutArray` でなければならない。`File` は E3007
main : Unit -> <IO> Unit
main () =
  var f = Fs.open "input.txt"
  println "done"
