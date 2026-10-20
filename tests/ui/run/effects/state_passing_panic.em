-- 状態を渡す形に特殊化した経路の奥で `Panic` が起き、handler の外の `try_panic` が受ける。状態はヒープのリストを
-- 持ったまま捨てられ、検査モードで漏れない (docs/spec/core-ir.md の「handler の特殊化で意味が変わらない理由」)。
import Std.State (State(..))

push_until : Int -> <State (List Int), Panic> Unit
push_until i =
  put (100 / (3 - i) :: get ())
  push_until (i + 1)

main : Unit -> <IO, Panic> Unit
main () =
  match try_panic (fn () -> State.exec [] (fn () -> push_until 0)) with
    | Ok xs -> println (show xs)
    | Err message -> println message
