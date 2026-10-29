-- 誰も書かない端の読みは待ち続け、どのタスクも進めなくなると実行時エラー `deadlock` で止まる。driver に保留の
-- 操作があっても、テストの driver は外からの入力なしに終わる操作がなければ進まないと答える
-- (docs/spec/vm.md の「時計と driver」)。
import Async
import Bytes
import IoTest

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  Async.run (fn () ->
    let (a, b) = IoTest.pair ()
    let t = Async.start (fn () ->
      let (buf, a) = IoTest.read_chunk (Bytes.empty ()) a
      drop a
      Bytes.length buf)
    let _ = spin 40
    println (show (Async.await t))
    drop b)
  println "unreachable"
