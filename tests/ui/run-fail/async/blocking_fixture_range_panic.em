-- fixtures: only runnable through the test harness
-- fixture ids: 1210..1211
-- 範囲の外の `Int as int` の引数は、`blocking` の呼び出しでも仕事を積む前に `Panic` を上げる。捕まえないと、その文言と
-- 呼び出しの位置を持つ実行時エラーになる。

import Async (Async)

extern "C" blocking symbol "eml_fixture_add_i32" add : Int as int -> Int as int -> <IO, Panic, Async> Int as int

body : Unit -> <IO, Panic, Async> Unit
body () =
  println "before"
  println (show (add 3000000000 1))

main : Unit -> <IO, Panic> Unit
main () = Async.run body
