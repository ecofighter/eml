-- `Child` は `FileIn` と別の型で、`FileIn` の関数には渡せない (docs/spec/stdlib.md の「ハンドル」)
import Proc
import FileIn

main : Unit -> <IO, Panic> Unit
main () =
  let (text, c) = FileIn.read_all (Proc.spawn `cat`)
  println text
  FileIn.close c
