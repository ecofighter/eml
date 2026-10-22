-- タプルの要素として先に評価した `File` は、戻らない呼び出しをまたいで一時に持つので、巻き戻しが解放する。
import Fail (Fail(..))
import Fs

pair : Unit -> <Fail String, IO> (Fs.File, Int)
pair () =
  let f = Fs.open "input.txt"
  (f, Fail.fail "x")

main : Unit -> <IO> Unit
main () =
  match Fail.to_result pair with
    | Ok (f, n) ->
      Fs.close f
      println (show n)
    | Err message -> println "Err \{show message}"
