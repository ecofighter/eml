-- 借用の extern を値として使う。`$extern` の包みが、借りた引数を呼ぶ側に返す。
import String

main : Unit -> <IO, Panic> Unit
main () =
  let s = "hello" ++ "!"
  let f = String.byte_at 3
  println (show (f s))
  println (show (f s))
  let g = String.slice 1
  println (show (g 4 s))
  println s
