-- `IoTest.checksum` はプールの仕事でバイトの和を求める。並んだ2つの仕事の結果は `await` の順に受け取る。仕事の遅れは
-- 本物の時計でだけ効き、テストの driver は仕事を次の決着でその場で動かすので、仕事の間に仮想の時間は進まない。
-- そのため 300 ms の遅れの仕事も 20 ms の期限に間に合う (docs/spec/stdlib.md の「`Std.IoTest`」、
-- docs/implementation/testing.md の「UI テスト」)。
import Async
import Bytes
import IoTest

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  println (show (IoTest.checksum 0 (Bytes.of_string "ab")))
  let t = Async.start (fn () -> IoTest.checksum 0 (Bytes.of_string "abc"))
  let u = Async.start (fn () -> IoTest.checksum 0 (Bytes.of_string "xyz"))
  println (show (Async.await u))
  println (show (Async.await t))
  match Async.timeout 20 (fn () -> IoTest.checksum 300 (Bytes.of_string "ab")) with
    | Some n -> println "Some \{show n}"
    | None -> println "None")
