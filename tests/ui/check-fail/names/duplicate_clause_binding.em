-- E1017: an operation clause binds `n` twice, once as the argument and once as the continuation.
effect Ask where
  ask : Int -> Int

run : Unit -> <Ask> Int
run () = ask 1

main : Unit -> <IO> Unit
main () =
  let r = handle run () with
    | ask n n -> n 1
  println (show_int r)
