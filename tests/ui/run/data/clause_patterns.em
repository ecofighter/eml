-- Constructor patterns in an operation clause parameter and in the return clause take boxes apart.
data Box a =
  | Box a

effect Ask where
  ask : Box Int -> Int

program : Unit -> <Ask> (Box Int)
program () =
  let a = ask (Box 20)
  let b = ask (Box 1)
  Box (a + b)

answer : Unit -> Int
answer () =
  handle program () with
    | ask (Box q) k -> k (q * 2)
    | return (Box r) -> r

main : Unit -> <IO> Unit
main () =
  println (show_int (answer ()))
