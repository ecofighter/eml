-- 要素の型変数は `Unr` に固定されるので、`Lin` の `FileIn` の配列に `Array.get` は使えない。
import Array
import FileIn

first_file : Array FileIn.FileIn -> <Panic> FileIn.FileIn
first_file a = Array.get a 0

main : Unit -> <IO, Panic> Unit
main () = ()
