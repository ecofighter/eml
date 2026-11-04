-- ファイルを読み続けるタスクを取り消す。走っているファイルの仕事は取り消せないので、読みのタスクは仕事が終わるのを
-- 待ってから `Cancelled` を投げ、包みが読み手を落として閉じる。検査モードで漏れが出ない
-- (docs/spec/stdlib.md の「`Std.Stream`」、docs/spec/stdlib.md の「取り消し」)。
import Async (Async)
import Async
import Bytes
import Stream

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

-- ファイルを終わりまで読み、開き直して、また読む
reread : Stream.Reader -> <IO, Async> Unit
reread r =
  let (b, r) = Stream.read_chunk (Bytes.empty ()) r
  if Bytes.length b == 0 then
    Stream.close r
    reread (Stream.open "stream_cancelled_reader.em")
  else reread r

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () -> reread (Stream.open "stream_cancelled_reader.em"))
  let _ = spin 40
  Async.yield_now ()
  Async.cancel t

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  println "after run"
