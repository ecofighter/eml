-- E2002: `compute`の仕事のrowは`Panic`だけなので、`IO`は入らない (docs/spec/stdlib.mdの「`compute`」)。
import Async (Async)

main : Unit -> <IO, Panic, Async> Unit
main () =
  Async.compute (fn () -> println "job")
