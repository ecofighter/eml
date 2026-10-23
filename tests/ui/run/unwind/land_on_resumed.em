-- 節が `ctl` で `k` を呼んで再開した後、再開された本体が同じ handler の `never` の操作を呼ぶ。中断の行き先は
-- 再開で作り直された evidence で、`resume$` の着地の印の所で止まる。途中のフレームの `FileIn` は閉じる。
import FileIn

effect Ctl where
  ask : Unit -> Int
  never stop : Int -> a

body : Unit -> <Ctl, IO, Panic> Int
body () =
  let a = ask ()
  println "asked \{a}"
  let f = FileIn.open "input.txt"
  let r = stop 7
  FileIn.close f
  a + r

main : Unit -> <IO, Panic> Unit
main () =
  let r = handle body () with
    | ctl ask () k -> k 10 + 1
    | ctl stop v -> v * 2
    | return x -> x
  println (show r)
  println "after"
