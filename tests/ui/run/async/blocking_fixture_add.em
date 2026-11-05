-- fixtures: only runnable through the test harness
-- fixture ids: 1000..1001
-- `blocking` の C の関数は、呼んだタスクを止めて仕事を worker 0 の driver に預け、答えをタスクの持ち主の worker で
-- 受ける。タスクが複数あっても、部分適用した関数を通しても、結果はスケジュールによらない。

import Async (Async)

extern "C" blocking symbol "eml_fixture_add_i32" add : Int as int -> Int as int -> <IO, Panic, Async> Int as int

body : Unit -> <IO, Panic, Async> Unit
body () =
  let a = Async.start (fn () -> add 1 2)
  let add10 = add 10
  let b = Async.start (fn () -> add10 5)
  let c = Async.start (fn () -> add (add10 1) 100)
  let x = Async.await a
  let y = Async.await b
  let z = Async.await c
  println (show x)
  println (show y)
  println (show z)

main : Unit -> <IO, Panic> Unit
main () = Async.run body
