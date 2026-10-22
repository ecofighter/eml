-- リストの要素が失敗する経路は、開いた `File` を `fail` の前に手放す。戻る経路は `File` を閉じる。
import Fail (Fail(..))
import FileIn

build : Bool -> Unit -> <IO, Fail String> List Int
build c () =
  let f = FileIn.open "input.txt"
  let xs = [1, (if c then 2 else Fail.fail "no"), 3]
  FileIn.close f
  xs

main : Unit -> <IO> Unit
main () =
  println (show (Fail.to_result (build True)))
  println (show (Fail.to_result (build False)))
