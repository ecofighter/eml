-- ファイルを開き続けるタスクを取り消す。取り込みは host の driver が受け付けで終えるので、開くタスクが待つのは host が
-- ほかの worker のときだけである。待つ間に取り消されたタスクの記録も `connect` と同じく完了からストリームを作り、包みの
-- `finish` がそれを落とすので、ファイルは host の bin を通して閉じる。run は残ったタスクなしで終わり、検査モードで
-- 漏れが出ない (docs/spec/vm.md の「操作の記録」、docs/spec/stdlib.md の「`Std.Stream`」)。
import Async (Async)
import Async
import Stream

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

reopen : Unit -> <IO, Async> Unit
reopen () =
  Stream.close (Stream.open "stream_open_cancelled.em")
  reopen ()

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start reopen
  let _ = spin 40
  Async.yield_now ()
  Async.cancel t

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  println "after run"
