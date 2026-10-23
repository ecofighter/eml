-- 戻らない呼び出しの後ろの、実行が届かない所で消費する `FileIn` は暗黙に手放さない。呼び出しをまたいで生きているので、
-- 巻き戻しが解放する。
import Fail (Fail(..))
import FileIn

after : Unit -> <Fail String, IO> (Int, Unit)
after () =
  let f = FileIn.open "input.txt"
  (Fail.fail "x", FileIn.close f)

main : Unit -> <IO> Unit
main () =
  println (show (Fail.to_result after))
