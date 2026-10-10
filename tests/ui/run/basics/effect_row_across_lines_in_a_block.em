-- A comma that ends a line inside an effect row does not close the block opened by `->`, because the next line is deeper than the block.
effect Log where
  log : String -> Unit

apply : (Unit -> <IO> Unit) -> <IO> Unit
apply f = f ()

main : Unit -> <IO> Unit
main () =
  apply (fn () ->
    let g : Unit -> <IO,
      Log> Unit = fn () -> println "hi"
    println "ok")
