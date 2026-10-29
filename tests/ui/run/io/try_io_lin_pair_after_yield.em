-- 待ちの後の継続で、`lin` の値を含む組を返す関数が IO の失敗を起こしても、`try_io` が受け取る。戻らない呼び出しの
-- 組の束縛は、使われない葉のダミーを `unr` にするので、手放す命令が verifier を通る (docs/spec/lir.md の「開く」)。
import Async (Async)
import Async
import FileIn

grab : Bool -> FileIn.FileIn -> <IO, Async> (Int, FileIn.FileIn)
grab keep h =
  Async.yield_now ()
  if keep then (1, h)
  else
    FileIn.close h
    fail_io Prelude.Other "gave up"

attempt : Bool -> <IO, Async> Unit
attempt keep =
  match try_io (fn () -> grab keep (FileIn.open "input.txt")) with
    | Ok (n, h) ->
      FileIn.close h
      println "kept \{show n}"
    | Err e -> println "\{show e.kind}: \{e}"

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  attempt True
  attempt False)
