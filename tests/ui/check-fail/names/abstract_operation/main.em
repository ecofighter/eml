-- E1058: 抽象エフェクトの操作は、外のモジュールで perform できず、handler の節にも import の並びにも書けない
-- (docs/spec/effects.md の「抽象エフェクト」)。
import Lib
import Lib (ping)

main : Unit -> <IO, Panic> Unit
main () =
  println (show (Lib.answer (fn () -> Lib.ping ())))
  let n = handle Lib.twice () with
    | ctl Lib.ping () k -> k 0
    | return x -> x
  println (show n)
