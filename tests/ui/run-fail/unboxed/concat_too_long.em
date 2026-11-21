-- つないだ長さを確保できなければ、`UnboxedArray.make`と同じくメモリの上限の誤りで実行が終わる。`try_panic`は捕まえない (docs/spec/stdlib.mdの「`Std.UnboxedArray`」)。
import Array
import UnboxedArray

main : Unit -> <IO, Panic> Unit
main () =
  let chunk = UnboxedArray.make 65536 0
  match try_panic (fn () -> UnboxedArray.length (UnboxedArray.concat (Array.make 65536 chunk))) with
    | Ok n -> println "unreachable \{show n}"
    | Err m -> println "unreachable: \{m}"
