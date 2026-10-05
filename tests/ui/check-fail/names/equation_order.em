-- E1018, E1019 and E1020: equations follow their signature, stay together and take the same number of arguments.
f : Int -> Int

g : Int
g = 1

f 0 = 1

h : Int
h = 2

f n = n

k : Int -> Int -> Int
k 0 y = y
k x = x

main : Unit -> <IO> Unit
main () = ()
