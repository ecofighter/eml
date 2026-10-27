-- 取り消したタスクが持っていた `FileIn` は、巻き戻しが落として閉じる。検査モードで漏れが出ない
-- (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)
import FileIn

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()

reader : Unit -> <IO, Panic, Async> Unit
reader () =
  let f = FileIn.open "cancelled_file.em"
  let u = Async.start forever
  let _ = spin 40
  let n = Async.await u
  FileIn.close f
  println "unreachable \{show n}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start reader
  let _ = spin 40
  Async.cancel t

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  println "after run"
