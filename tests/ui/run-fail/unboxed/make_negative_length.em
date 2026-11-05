-- 負の長さの `UnboxedArray.make` は、捕まえる `try_panic` がなければ位置つきの実行時エラーになる。
import UnboxedArray

main : Unit -> <IO, Panic> Unit
main () = println (show (UnboxedArray.length (UnboxedArray.make (-1) 0.0)))
