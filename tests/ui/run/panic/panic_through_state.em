-- `State.run` の本体で `panic` が起き、handler の外の `try_panic` が捕まえる。巻き戻しが handler の状態を手放すので、
-- 検査モードの漏れの検査も通る。
import State (State(..))

count : Unit -> <State (List String), Panic> Unit
count () =
  State.put ("a" :: State.get ())
  State.put ("b" :: State.get ())
  panic "stop with \{show (State.get ())}"

main : Unit -> <IO> Unit
main () =
  match try_panic (fn () -> State.run [] count) with
    | Ok (_, xs) -> println "ok \{show xs}"
    | Err e -> println "caught: \{e}"
