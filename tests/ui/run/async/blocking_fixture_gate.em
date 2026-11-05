-- clock: real (waits for another task inside a C call)
-- fixtures: only runnable through the test harness
-- fixture ids: 1020..1022
-- `blocking` の C の関数が鍵を待つ間も、同じ worker のほかのタスクは動く。鍵を開けるのは別のタスクなので、
-- `a: done` は、どの順で動いても `b: opening` の後に出る。

import Async (Async)

extern "C" blocking symbol "eml_fixture_gate_wait" gate_wait : Int -> <IO, Async> Int
extern "C" symbol "eml_fixture_gate_open" gate_open : Int -> <IO> Unit

body : Unit -> <IO, Panic, Async> Unit
body () =
  let a = Async.start (fn () ->
    println "a: calling"
    let r = gate_wait 1020
    println "a: done \{show r}")
  let b = Async.start (fn () ->
    println "b: opening"
    gate_open 1020)
  Async.await a
  Async.await b

main : Unit -> <IO, Panic> Unit
main () = Async.run body
