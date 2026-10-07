-- A handler whose state is `()` still resumes with three arguments.
effect Ask where
  ask : Unit -> Int

main : Unit -> <IO> Unit
main () =
  let n =
    handle ask () + ask () from () with
      | ask () k s -> k 20 s
      | return x s -> x + 1
  println (show_int n)
