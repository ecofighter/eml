-- `Async.timeout` で囲んだファイルの読みは、走り出したプールの仕事が終わってからでないと戻らない。仮想の時計は、どの
-- タスクも待っていて決着させる操作もないときだけ進むので、期限の前に仕事が終わって `Some` で返る。過ぎた期限は読みを
-- 受け付ける前に取り消しを投げ、`None` になる。囲んだ読み手は、どちらでも閉じる (docs/spec/stdlib.md の「`Std.Stream`」)。
import Async (Async)
import Async
import Bytes
import Stream

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

read_some : Int -> <IO, Panic, Async> Unit
read_some ms =
  let r = Stream.open "stream_timeout_after_job.em"
  let read = Async.timeout ms (fn () ->
    let (b, r) = Stream.read_chunk (Bytes.empty ()) r
    Stream.close r
    Bytes.length b > 0)
  println (show read)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () -> spin 40)
  let _ = spin 40
  read_some 10
  read_some 0
  println (show (Async.await t))

main : Unit -> <IO, Panic> Unit
main () = Async.run body
