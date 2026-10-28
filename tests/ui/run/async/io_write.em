-- `IoTest.write` は driver の書きを出して完了を待ち、書いたバイトの数を返す。相手の端を落とした後の書きは、IO の
-- 失敗 `Other` になる (docs/spec/stdlib.md の「`Std.IoTest`」)。
import Async (Async)
import Async
import Bytes
import IoTest

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

writer : Unit -> <IO, Panic, Async> Unit
writer () =
  let (a, b) = IoTest.pair ()
  let (n, a) = IoTest.write (Bytes.of_string "hello") a
  println (show n)
  drop b
  match try_io (fn () -> IoTest.write (Bytes.of_string "late") a) with
    | Ok (m, a) ->
      drop a
      println "wrote \{show m}"
    | Err e -> println "\{show e.kind}: \{e}"

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let t = Async.start writer
  let _ = spin 40
  Async.await t)
