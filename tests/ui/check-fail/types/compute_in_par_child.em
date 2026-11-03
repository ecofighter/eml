-- E2020: `compute` は `IO` と `Async` を行うので、`par` の子の中では呼べない (docs/spec/stdlib.md の「`Std.Par`」)。
import Std.Par
import Async (Async)

main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let (a, b) = Par.par (fn () -> Async.compute (fn () -> 1)) (fn () -> 2)
    println (show (a + b)))
