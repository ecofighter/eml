-- E2002: `Std.Par`の集まりの関数のコールバックは`IO`を起こせない
import Std.Array
import Std.Par

main : Unit -> <IO, Panic> Unit
main () =
  let xs = Par.tabulate 10 (fn i -> i)
  let ys = Par.map (fn x ->
    println (show x)
    x) xs
  println (show (Array.length ys))
