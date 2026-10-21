-- 借りた文字列を渡した extern が失敗しても、文字列は呼ぶ側に残る。検査モードの漏れの検査が数えの合わないものを断る。
main : Unit -> <IO> Unit
main () =
  let t = "ab" ++ "c"
  println (show (try_panic (fn () -> String.slice 0 9000 t)))
  println (show (try_panic (fn () -> String.byte_at 9000 t)))
  println t
