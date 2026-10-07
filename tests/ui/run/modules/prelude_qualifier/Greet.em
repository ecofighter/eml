pub greet : Bool -> <Prelude.IO> Unit
greet b = match b with
  | Prelude.True -> Prelude.println "yes"
  | Prelude.False -> println "no"
