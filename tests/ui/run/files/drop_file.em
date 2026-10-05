-- `drop` releases a file like `close` does.
main : Unit -> <IO> Unit
main () =
  let f = open "input.txt"
  drop f
  println "dropped"
