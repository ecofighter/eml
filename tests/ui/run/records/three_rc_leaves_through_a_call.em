-- RC の葉を3つ持つ組を、展開しない関数 (再帰する `pass`) に通し、呼んだ後も元の組を使う。組は構造体なので、3つの文字列の
-- 葉は箱に入らずに引数と結果の値として渡り、共有した組の葉は葉ごとに `dup` する。その回数 (`rc_increments`) は
-- `crates/eml_vm/tests/bench.rs` の `trade_offs` が固定する
-- (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「足すテスト」)。
pass : Int -> (String, String, String) -> <Panic> (String, String, String)
pass n t = if n == 0 then t else
  let (a, b, c) = t
  pass (n - 1) (b, c, a)

main : Unit -> <IO, Panic> Unit
main () =
  let t = (show 9, show 12, show 345)
  let (a, b, c) = pass 7 t
  println (a ++ b ++ c)
  let (d, e, f) = t
  println (d ++ "," ++ e ++ "," ++ f)
