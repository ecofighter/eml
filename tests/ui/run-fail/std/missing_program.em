main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  println "exit \{Proc.run "eml-no-such-program" []}"
