-- A point-free `main`: the equation has no parameters and its value is a function, which is applied to `()`.
greet : String -> <IO> Unit
greet name = println name

main : Unit -> <IO> Unit
main = fn () -> greet "point-free"
