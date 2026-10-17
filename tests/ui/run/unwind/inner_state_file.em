-- 内側の handler の状態が `File` のまま、本体が外側の `Fail.to_result` へ中断する。状態は evidence と一緒に閉じる。
import Fail (Fail(..))

effect Tick where
  tick : Unit -> Unit

body : Unit -> <Tick, Fail String> Int
body () =
  tick ()
  Fail.fail "stop"

inner : Unit -> <Fail String, IO> Int
inner () =
  handle body () from Fs.open "input.txt" with
    | ctl tick () k f -> k () f
    | return x f ->
      Fs.close f
      x

main : Unit -> <IO> Unit
main () =
  println (show (Fail.to_result inner))
