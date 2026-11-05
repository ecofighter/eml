-- fixtures: only runnable through the test harness
-- fixture ids: 1200..1201
-- ハンドルを返す `blocking` の C の関数が NULL を返すと、取り出しの行が物体を作らずに `Panic` を上げる。捕まえないので
-- 実行時エラーになり、`frees` は読まない。

import Async (Async)

extern "C" data Handle drop "eml_fixture_handle_free"
extern "C" blocking symbol "eml_fixture_handle_new" handle_new : Int -> Int -> <IO, Panic, Async> Handle
extern "C" symbol "eml_fixture_handle_get" handle_get : Handle -> <IO> (Int, Handle)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    let h = handle_new 1200 (0 - 1)
    let (v, h) = handle_get h
    drop h
    v)
  println "before"
  println (show (Async.await t))

main : Unit -> <IO, Panic> Unit
main () = Async.run body
