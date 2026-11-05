-- fixtures: only runnable through the test harness
-- fixture ids: 2..3
-- ハンドルを返す C の関数が NULL を返すと、物体を作らずに `Panic` を上げる。捕まえないので `frees` は読まない。

extern "C" data Handle drop "eml_fixture_handle_free"
extern "C" symbol "eml_fixture_handle_new" handle_new : Int -> Int -> <IO, Panic> Handle
extern "C" symbol "eml_fixture_handle_get" handle_get : Handle -> <IO> (Int, Handle)

main : Unit -> <IO, Panic> Unit
main () =
  let h = handle_new 2 (0 - 1)
  let (v, h) = handle_get h
  drop h
  println (show v)
