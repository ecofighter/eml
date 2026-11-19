-- E2001: `par`の子のrowに`Async`は入らない (docs/spec/stdlib.mdの「`Std.Par`」)。
-- 子の row を `<Async>` だけにして、診断が `Async` を名指すことを確かめる。
import Std.Par
import Async (Async)

child : Unit -> <Async> Unit
child () = ()

main : Unit -> <IO, Panic, Async> Unit
main () =
  let (a, b) = Par.par child (fn () -> 1)
  println (show b)
