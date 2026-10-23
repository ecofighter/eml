import Std.Par

effect Ask where
  ask : Unit -> Int

effect Mixed where
  never stop : Unit -> a
  get : Unit -> Int

main : Unit -> <IO, Panic> Unit
main () =
  let (a, b) = handle Par.par (fn () -> ask ()) (fn () -> 3) with
    | fn ask () -> 1
  println (show (a + b))
  let (c, d) = handle Par.par (fn () -> stop ()) (fn () -> 3) with
    | ctl stop () -> (0, 0)
    | fn get () -> 1
  println (show (c + d))
