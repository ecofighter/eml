-- One application passes two arguments to a function that performs an operation after the first.
effect Ask where
  ask : Unit -> Int

adder : Int -> <Ask> (Int -> <Panic> Int)
adder x =
  let n = ask ()
  fn y -> x + y + n

run : (Int -> <Ask> (Int -> <Panic> Int)) -> <Ask, Panic> Int
run h = h 1 2

main : Unit -> <IO, Panic> Unit
main () =
  let r =
    handle run adder with
      | ctl ask () k ->
        let v = k 10
        v * 2
  println (show r)
