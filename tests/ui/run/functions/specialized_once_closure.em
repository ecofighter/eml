-- `File` を捕まえた `once` の閉包を、1回だけ呼ぶ高階関数に渡す。特殊化した複製は捕獲を引数として受け取り、`File` を
-- 1回だけ閉じる。checked mode でもリークは出ない
import Fs

call_once : (once Unit -> <IO> Unit) -> <IO> Unit
call_once g = g ()

main : Unit -> <IO> Unit
main () =
  let f = Fs.open "input.txt"
  call_once (fn () -> Fs.close f)
  println "closed"
