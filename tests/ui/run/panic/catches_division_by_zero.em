-- `try_panic` がゼロ除算を捕まえる。`%` も同じである。
divide : Int -> Int -> <Panic> Int
divide a b = a / b

main : Unit -> <IO> Unit
main () =
  println (show (try_panic (fn () -> divide 7 0)))
  println (show (try_panic (fn () -> 7 % 0)))
  println (show (try_panic (fn () -> divide 7 2)))
