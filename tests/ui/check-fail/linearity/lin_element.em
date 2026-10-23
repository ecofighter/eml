-- 配列の変数の要素は `Unr` でなければならない。`FileIn` を書き込むと E3001
import FileIn
import MutArray

main : Unit -> <IO, Panic> Unit
main () =
  var a = MutArray.empty ()
  a[0] <- FileIn.open "input.txt"
  drop a
