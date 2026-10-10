-- E2001: list elements must share one type; with an expected type, each element is checked against it.
names : List String
names = ["a", 1]

main : Unit -> <IO> Unit
main () =
  let mixed = ["a", 1]
  println "done"
