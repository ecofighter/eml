-- `mask` 付きの呼び出しの中で `fail` すると、内側の `Fail.to_result` を飛ばして外側の `Fail.to_result` へ中断する。
-- 途中のフレームが持つ `File` は閉じる。
import Fail (Fail(..))
import FileIn

masked : (Unit -> <e> a) -> <Fail String | e> a
masked f = f ()

boom : Unit -> <Fail String> Int
boom () = Fail.fail "skipped the inner handler"

main : Unit -> <IO> Unit
main () =
  let r = Fail.to_result (fn () ->
    let f = FileIn.open "input.txt"
    let inner = Fail.to_result (fn () -> masked boom)
    FileIn.close f
    inner)
  println (show r)
