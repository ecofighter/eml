-- clock: real (waits for another task inside a C call)
-- fixtures: only runnable through the test harness
-- fixture ids: 1100..1141
-- 40 のタスクが、それぞれ別の鍵を `blocking` の呼び出しの中で待つ。プールが同時に走らせる数を超えても、41 番目のタスクが
-- 開けた鍵の順によらず、すべてが終わる。

import Async (Async, Task)
import List

extern "C" blocking symbol "eml_fixture_gate_wait" gate_wait : Int -> <IO, Async> Int
extern "C" symbol "eml_fixture_gate_open" gate_open : Int -> <IO> Unit

waiter : Int -> <IO, Panic, Async> Task Int
waiter k = Async.start (fn () -> gate_wait (1100 + k))

open_all : Int -> <IO, Panic> Unit
open_all k =
  if k < 40 then
    gate_open (1100 + k)
    open_all (k + 1)
  else ()

count_ok : List Int -> Int -> <Panic> Int
count_ok rs n =
  match rs with
    | [] -> n
    | r :: rest -> count_ok rest (if r == 0 then n + 1 else n)

start_all : Int -> List (Task Int) -> <IO, Panic, Async> List (Task Int)
start_all k ts = if k == 40 then List.reverse ts else start_all (k + 1) (waiter k :: ts)

await_all : List (Task Int) -> List Int -> <IO, Panic, Async> List Int
await_all ts acc =
  match ts with
    | [] -> acc
    | t :: rest -> await_all rest (Async.await t :: acc)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let ts = start_all 0 []
  let opener = Async.start (fn () -> open_all 0)
  let rs = await_all ts []
  Async.await opener
  println (show (count_ok rs 0))

main : Unit -> <IO, Panic> Unit
main () = Async.run body
