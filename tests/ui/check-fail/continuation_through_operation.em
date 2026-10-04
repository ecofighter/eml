-- E3001: an operation has no body, so the kinds of the type variables and arrows in its parameter types are
-- fixed to `Unr`. A continuation, or a lambda that captures one, cannot be passed to such a parameter: the
-- clause of the operation may use the argument more than once or not at all.
effect Ask where
  ask : Unit -> Int

effect Pair where
  pair : a -> (a -> a -> b) -> b

effect Sink where
  sink : a -> Unit

inner : Unit -> <Pair> Int
inner () =
  handle ask () + 100 with
    | ask () k ->
        let f = pair k (fn c1 c2 -> fn u -> resume c1 1 + resume c2 2)
        f ()

main : Unit -> <IO> Unit
main () =
  let n =
    handle inner () with
      | pair x g k2 -> resume k2 (g x x)
  println (show_int n)

sunk : Unit -> <Sink> Int
sunk () =
  handle ask () with
    | ask () k ->
        sink k
        0
