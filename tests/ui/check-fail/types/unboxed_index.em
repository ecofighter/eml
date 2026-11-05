-- 添字の式 `e[i]` は `Array` だけに使えるので、`UnboxedArray` への添字は E2001
import UnboxedArray

main : Unit -> <IO, Panic> Unit
main () = println (show (UnboxedArray.make 3 1.0)[0])
