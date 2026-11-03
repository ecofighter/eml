-- E2020: `compute` の仕事の row は `never` で上限を付けるので、`IO` は入らない (docs/spec/stdlib.md の「`compute`」)。
import Async (Async)

main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () -> Async.compute (fn () -> println "job"))
