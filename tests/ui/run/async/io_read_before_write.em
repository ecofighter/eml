-- 空の端の読みは、相手が書くまで待つ。仮想の時計は、走れるタスクがなく、終わる操作もないときにだけ進むので、本体の
-- `sleep_ms` が戻るときには、読むタスクはもう空の端で待っている。出力の順はどの方針と seed でも同じである
-- (docs/spec/stdlib.md の「`Std.IoTest`」、docs/spec/vm.md の「時計と driver」)。
import Async
import Bytes
import IoTest
import String

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let (a, b) = IoTest.pair ()
  let t = Async.start (fn () ->
    println "reading"
    let (buf, b) = IoTest.read_chunk (Bytes.empty ()) b
    drop b
    println "read \{String.decode buf}")
  Async.sleep_ms 10
  println "writing"
  let (_, a) = IoTest.write (Bytes.of_string "late") a
  drop a
  Async.await t)
