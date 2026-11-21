import Array

-- one phase: about 48 MB of small arrays of k elements, all dropped at the end of the phase
phase : Int -> <Panic> Int
phase k =
  let n = 6000000 / (k + 2)
  let a = Array.tabulate n (fn i -> Array.make k i)
  Array.length (Array.get a (n - 1))

phases : List Int -> Int -> <Panic> Int
phases ks acc =
  match ks with
    | Nil -> acc
    | k :: rest -> phases rest (acc + phase k)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  let r = phases [2, 4, 6, 8, 10, 12, 14, 18, 22, 26, 30, 38] 0
  let _ = spin 1500000000
  println (show r)
