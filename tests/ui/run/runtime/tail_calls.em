-- Tail calls do not push frames: a loop of ten thousand iterations, and a call in tail position
-- whose result is applied to the remaining argument.
loop : Int -> Int -> <Panic> Int
loop n acc = if n == 0 then acc else loop (n - 1) (acc + 1)

add : Int -> Int -> <Panic> Int
add a b = a + b

adder : Int -> Int -> <Panic> Int
adder x = add x

sum : Int -> Int -> <Panic> Int
sum a b = adder a b

main : Unit -> <IO, Panic> Unit
main () =
  println (show (loop 10000 0))
  println (show (sum 3 4))
