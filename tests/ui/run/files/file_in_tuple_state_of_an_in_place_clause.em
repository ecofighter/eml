-- すぐに再開する節が、`File` を含むタプルの状態を分解して組み立て直す。入口は `ev_take_state` で取り出した状態を
-- `match` で分解し、末尾の再開を `ev_set_state` に替える。
import FileIn

effect Tick where
  tick : Unit -> Int

body : Unit -> <Tick, Panic> Int
body () = tick () + tick ()

main : Unit -> <IO, Panic> Unit
main () =
  let r = handle body () from (FileIn.open "input.txt", 0) with
    | ctl tick () k st ->
        match st with
          | (f, n) -> k n (f, n + 1)
    | return x st ->
        match st with
          | (f, n) ->
              FileIn.close f
              x + n
  println (show r)
