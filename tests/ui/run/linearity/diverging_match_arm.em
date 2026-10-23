-- `fail` で終わる腕と `FileIn` を閉じる腕を持つ `match`、`exit` で終わる枝と `FileIn` を閉じる枝を持つ `if`。戻らない側は
-- 呼び出しの前に `FileIn` を手放す。
import Fail (Fail(..))
import FileIn

pick : Int -> <Fail String, IO> Int
pick n =
  let f = FileIn.open "input.txt"
  match n with
    | 0 -> Fail.fail "zero"
    | _ ->
      FileIn.close f
      n

finish : Int -> <IO> Unit
finish code =
  let f = FileIn.open "input.txt"
  if code == 0 then FileIn.close f else exit code

main : Unit -> <IO> Unit
main () =
  println (show (Fail.to_result (fn () -> pick 1)))
  println (show (Fail.to_result (fn () -> pick 0)))
  finish 0
  println "closed"
  finish 3
