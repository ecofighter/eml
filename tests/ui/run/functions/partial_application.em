-- Partial application, extra arguments, equations with fewer parameters than arrows,
-- a closure called twice, and a closure dropped without being called.
greet : String -> String -> String
greet greeting name = greeting ++ ", " ++ name

hello : String -> String
hello = greet "hello"

add : Int -> Int -> Int
add a b = a + b

adder : Int -> Int -> Int
adder x = add x

main : Unit -> <IO> Unit
main () =
  let hi = greet "hi"
  println (hi "alice")
  println (hi "bob")
  println (hello "carol")
  let unused = greet "never called"
  println (show (adder 3 4))
  let add_ten = add 10
  println (show (add_ten 5))
