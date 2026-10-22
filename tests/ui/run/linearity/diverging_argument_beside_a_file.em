-- タプルの要素として先に評価した `File` は、戻らない呼び出しをまたいで一時に持つので、巻き戻しが解放する。
import Fail (Fail(..))
import FileIn

pair : Unit -> <Fail String, IO> (FileIn.FileIn, Int)
pair () =
  let f = FileIn.open "input.txt"
  (f, Fail.fail "x")

main : Unit -> <IO> Unit
main () =
  match Fail.to_result pair with
    | Ok (f, n) ->
      FileIn.close f
      println (show n)
    | Err message -> println "Err \{show message}"
