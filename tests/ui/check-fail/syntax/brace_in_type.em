-- E0011: a type cannot be written with braces; records are declared as constructors of a `data`.
name_of : { name : String } -> String
name_of p = "x"

main : Unit -> <IO> Unit
main () = println "done"
