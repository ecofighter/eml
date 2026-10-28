-- 空の端の読みは、相手が書くまで待つ。待つ間は buffer に空きを足さず、読めるようになってから足して貸す
-- (docs/spec/stdlib.md の「`Std.IoTest`」、docs/spec/vm.md の「操作の記録」)。
import Async (Async)
import Async
import Bytes
import IoTest
import String

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let (a, b) = IoTest.pair ()
  let t = Async.start (fn () ->
    let (buf, b) = IoTest.read_chunk (Bytes.empty ()) b
    drop b
    println "read \{String.decode buf}")
  let _ = spin 40
  Async.yield_now ()
  println "writing"
  let (_, a) = IoTest.write (Bytes.of_string "late") a
  drop a
  Async.await t)
