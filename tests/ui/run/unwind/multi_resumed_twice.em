-- `multi` の操作の `k` を2回再開し、2回目に再開した区間が `File` を持ったまま外側の `Fail.to_result` へ中断する。
-- 再開した区間のフレームも普通のフレームとして巻き戻す。
import Fail (Fail(..))
import Fs

effect Choice where
  multi choose : Unit -> Bool

pick : Unit -> <Choice, Fail String, IO> Int
pick () =
  let b = choose ()
  println "picked \{b}"
  let f = Fs.open "input.txt"
  let r = if b then 1 else Fail.fail "second"
  Fs.close f
  r

main : Unit -> <IO, Panic> Unit
main () =
  let r = Fail.to_result (fn () ->
    handle pick () with
      | ctl choose () k -> k True + k False)
  println (show r)
