-- 範囲外の添字の `UnboxedArray.get` は、捕まえる `try_panic` がなければ位置つきの実行時エラーになる。
import UnboxedArray

main : Unit -> <IO, Panic> Unit
main () = println (show (UnboxedArray.get (UnboxedArray.make 3 0.0) 3))
