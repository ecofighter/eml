-- `File` を捕まえたローカル関数を呼ばずに `fail` する枝では、持ち上げたローカル関数の捕まえる `File` を手放す。
import Fail (Fail(..))
import Fs

close_or_fail : Bool -> <Fail String, IO> Unit
close_or_fail c =
  let f = Fs.open "input.txt"
  let close_it () = Fs.close f
  if c then close_it () else Fail.fail "kept"

main : Unit -> <IO> Unit
main () =
  println (show (Fail.to_result (fn () -> close_or_fail True)))
  println (show (Fail.to_result (fn () -> close_or_fail False)))
