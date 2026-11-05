-- 負の長さの `MutUnboxedArray.tabulate` は、`make` と同じ実行時エラーになる。位置は std の中の `Panic` を起こす所である。
import Float
import MutUnboxedArray
import UnboxedArray

main : Unit -> <IO, Panic> Unit
main () =
  let a = MutUnboxedArray.freeze (MutUnboxedArray.tabulate (-1) (fn i -> Float.of_int i))
  println (show (UnboxedArray.length a))
