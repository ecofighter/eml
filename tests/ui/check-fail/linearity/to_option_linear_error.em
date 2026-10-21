import Fail (Fail(..))
import Fs (File)

fail_with_file : String -> Unit -> <IO, Fail File> Unit
fail_with_file path () = Fail.fail (Fs.open path)

main : Unit -> <IO> Unit
main () =
  let r = Fail.to_option (fail_with_file "x")
  println "done"
