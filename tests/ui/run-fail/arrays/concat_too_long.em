-- つないだ長さを確保できなければ、`Array.make`と同じくメモリの上限の誤りで実行が終わる。`try_panic`は捕まえない (docs/spec/stdlib.mdの「`Std.Array`」)。
import Array

main : Unit -> <IO, Panic> Unit
main () =
  let chunk = Array.make 65536 0
  match try_panic (fn () -> Array.length (Array.concat (Array.make 65536 chunk))) with
    | Ok n -> println "unreachable \{show n}"
    | Err m -> println "unreachable: \{m}"
