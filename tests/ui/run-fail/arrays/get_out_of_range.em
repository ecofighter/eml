-- 範囲外の添字の `Array.get` は、捕まえる `try_panic` がなければ位置つきの実行時エラーになる。
main : Unit -> <IO, Panic> Unit
main () = println (show (Array.get (Array.make 3 0) 3))
