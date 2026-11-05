-- fixtures: only runnable through the test harness
-- fixture ids: 1..2
-- ハンドルは C の側の物体を指す。借りて読んだ後、捨てると dtor が1回だけ走る。

extern "C" data Handle drop "eml_fixture_handle_free"
extern "C" symbol "eml_fixture_handle_new" handle_new : Int -> Int -> <IO, Panic> Handle
extern "C" symbol "eml_fixture_handle_get" handle_get : Handle -> <IO> (Int, Handle)
extern "C" symbol "eml_fixture_frees" frees : Int -> <IO> Int

main : Unit -> <IO, Panic> Unit
main () =
  let h = handle_new 1 7
  let (v, h) = handle_get h
  let (w, h) = handle_get h
  drop h
  println (show (v + w))
  println (show (frees 1))
