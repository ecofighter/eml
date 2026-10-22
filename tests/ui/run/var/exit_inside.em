-- 配列の変数の範囲の中で `exit` を呼ぶ。検査モードでも物体が残らない
import MutArray

main : Unit -> <IO, Panic> Unit
main () =
  var a = MutArray.make 4 0
  a[0] <- 1
  println "before"
  if a[0] == 1 then exit 3 else ()
  println "unreachable"
  drop a
