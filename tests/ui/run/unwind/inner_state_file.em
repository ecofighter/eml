-- 内側の handler の状態が `File` のまま、本体が外側の `Fail.to_result` へ中断する。巻き戻しが内側の handler を外すときに状態を取り出して手放し、`File` を閉じる。
import Fail (Fail(..))
import FileIn

effect Tick where
  tick : Unit -> Unit

body : Unit -> <Tick, Fail String> Int
body () =
  tick ()
  Fail.fail "stop"

inner : Unit -> <Fail String, IO> Int
inner () =
  handle body () from FileIn.open "input.txt" with
    | ctl tick () k f -> k () f
    | return x f ->
      FileIn.close f
      x

main : Unit -> <IO> Unit
main () =
  println (show (Fail.to_result inner))
