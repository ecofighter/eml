-- `return` の節が `fail` して外側の `Fail.to_result` へ中断する。途中のフレームが持つ `File` は閉じる。
import Fail (Fail(..))
import Fs

effect Tick where
  tick : Unit -> Unit

ticked : Unit -> <Fail String> Int
ticked () =
  handle tick () with
    | ctl tick () k -> k ()
    | return x -> Fail.fail "from the return clause"

outer : Unit -> <Fail String, IO> Int
outer () =
  let f = Fs.open "input.txt"
  let r = ticked ()
  Fs.close f
  r

main : Unit -> <IO> Unit
main () =
  println (show (Fail.to_result outer))
