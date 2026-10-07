-- Tail calls do not push frames: a loop of ten thousand iterations, and a call in tail position
-- whose result is applied to the remaining argument.
loop : Int -> Int -> Int
loop n acc = if n == 0 then acc else loop (n - 1) (acc + 1)

add : Int -> Int -> Int
add a b = a + b

adder : Int -> Int -> Int
adder x = add x

sum : Int -> Int -> Int
sum a b = adder a b

main : Unit -> <IO> Unit
main () =
  println (show_int (loop 10000 0))
  println (show_int (sum 3 4))
