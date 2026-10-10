-- A comma in the middle of a line, such as one in an effect row, does not close the block opened by `->`.
effect Log where
  log : String -> Unit

apply : (Unit -> <IO> Unit) -> <IO> Unit
apply f = f ()

main : Unit -> <IO> Unit
main () =
  apply (fn () ->
    let g : Unit -> <IO, Log> Unit = fn () -> println "hi"
    let pair = (1, 2)
    println "ok")
