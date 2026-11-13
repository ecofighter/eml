import Float

main : Unit -> <IO, Panic> Unit
main () =
  println (show (Float.truncate 2.5 + Float.floor 1.5 + Float.ceil 1.5))
  println (show (Float.sqrt 4.0 + Float.abs (0.0 - 1.0)))
  println (show (Float.sin 0.0 + Float.cos 0.0 + Float.exp 0.0 + Float.log 1.0))
