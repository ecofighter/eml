-- 要素の型変数は `Unr` に固定されるので、`Lin` の `File` の配列に `Array.get` は使えない。
import Array
import Fs

first_file : Array Fs.File -> <Panic> Fs.File
first_file a = Array.get a 0

main : Unit -> <IO, Panic> Unit
main () = ()
