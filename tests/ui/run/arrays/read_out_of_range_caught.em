-- 範囲外の `MutArray.read` を `try_panic` が捕まえても、借りていた配列は巻き戻しが手放す。
-- 要素が文字列の配列は、手放し損ねると検査モードの漏れとして見える。
import MutArray

main : Unit -> <IO, Panic> Unit
main () =
  let r = try_panic (fn () ->
    let m = MutArray.make 3 "x"
    let (v, m) = MutArray.read m 7
    let _ = MutArray.freeze m
    v)
  println (show r)
  let m = MutArray.make 2 "y"
  let (v, m) = MutArray.read m 1
  let _ = MutArray.freeze m
  println v
