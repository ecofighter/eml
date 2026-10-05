-- E1024: a `use` at the end of a block has nothing to wrap.
wrap : (Unit -> <IO> Unit) -> <IO> Unit
wrap k = k ()

main : Unit -> <IO> Unit
main () =
  println "start"
  use wrap
