-- `main` may list `IO` and `Panic` in either order.
main : Unit -> <Panic, IO> Unit
main () = println (show (1 + 2))
