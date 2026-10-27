-- E2020: `par` の子の row に `Async` は入らない (docs/spec/stdlib.md の「`Std.Par`」)。
-- 子の row を `<Async>` だけにして、診断が `Async` を名指すことを確かめる。
import Std.Par
import Async (Async)

child : Unit -> <Async> Unit
child () = ()

main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let (a, b) = Par.par child (fn () -> 1)
    println (show b))
