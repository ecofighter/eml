-- clock: real (waits for another task inside a C call)
-- fixtures: only runnable through the test harness
-- fixture ids: 1030..1032
-- 走っている `blocking` の呼び出しは、取り消しても止まらない。C の関数が戻ってから、タスクは `Cancelled` で終わり、
-- 引数のハンドルは dtor で1回だけ壊れる。取り消した後に譲っても、鍵を開けるまで `a:` の行は出ない (docs/spec/declarations.md の「`extern "C"`」)。

import Async (Async)

extern "C" data Handle drop "eml_fixture_handle_free"
extern "C" symbol "eml_fixture_handle_new" handle_new : Int -> Int -> <IO, Panic> Handle
extern "C" blocking symbol "eml_fixture_handle_wait" handle_wait : Handle -> Int -> <IO, Async> (Int, Handle)
extern "C" symbol "eml_fixture_gate_open" gate_open : Int -> <IO> Unit
extern "C" symbol "eml_fixture_frees" frees : Int -> <IO> Int

body : Unit -> <IO, Panic, Async> Unit
body () =
  let a = Async.start (fn () ->
    match try_io (fn () ->
      let h = handle_new 1030 7
      let (v, h) = handle_wait h 1031
      drop h
      v) with
      | Ok v -> println "unreachable \{show v}"
      | Err e -> println "a: \{show e.kind}")
  Async.yield_now ()
  Async.cancel a
  Async.yield_now ()
  println "b: opening"
  gate_open 1031

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  println (show (frees 1030))
