import Fail (Fail(..))
import FileIn (FileIn)

fail_with_file : String -> Unit -> <IO, Fail FileIn> Unit
fail_with_file path () = Fail.fail (FileIn.open path)

main : Unit -> <IO> Unit
main () =
  let r = Fail.to_option (fail_with_file "x")
  println "done"
