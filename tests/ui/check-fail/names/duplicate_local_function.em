-- E1017: one `let … and …` group defines `f` twice; references go to the first.
main : Unit -> <IO, Panic> Unit
main () =
  let f x = x + 1
  and f y = y + 2
  println (show (f 1))
