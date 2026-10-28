-- 待っている読みを取り消すと、driver に取り消しを頼み、完了を待ってから `Cancelled` を投げる。取り消した読みの端は
-- 包みが落とすので、相手の端の読みは EOF になる。返した buffer は検査モードの漏れの検査が覆う
-- (docs/spec/stdlib.md の「取り消し」、「`Std.IoTest`」)。
import Async
import Bytes
import IoTest

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let (a, b) = IoTest.pair ()
  match Async.timeout 10 (fn () -> IoTest.read_chunk (Bytes.empty ()) b) with
    | Some (_, b) ->
      drop b
      println "read"
    | None -> println "None"
  let (buf, a) = IoTest.read_chunk (Bytes.empty ()) a
  drop a
  if Bytes.length buf == 0 then println "eof" else println "data")
