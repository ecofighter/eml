-- `FileIn` を捕まえた `once` の閉包を、1回だけ呼ぶ高階関数に渡す。特殊化した複製は捕獲を引数として受け取り、`FileIn` を
-- 1回だけ閉じる。checked mode でもリークは出ない
import FileIn

call_once : (once Unit -> <IO> Unit) -> <IO> Unit
call_once g = g ()

main : Unit -> <IO> Unit
main () =
  let f = FileIn.open "input.txt"
  call_once (fn () -> FileIn.close f)
  println "closed"
