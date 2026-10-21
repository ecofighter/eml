-- 添字の読み出し `a[0]` と `Array.get` の結果の関数は、読む式で広がるので `once` の位置に渡せる
-- (docs/spec/types.md の「値を読む式の矢印の線形性」)。
import Std.Array

run_once : (once Unit -> String) -> String
run_once f = f ()

hello : Unit -> String
hello () = "hello"

main : Unit -> <IO, Panic> Unit
main () =
  let a = Array.make 2 hello
  println (run_once a[0])
  println (run_once (Array.get a 1))
