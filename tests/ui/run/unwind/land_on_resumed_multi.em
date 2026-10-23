-- `multi` の節が `k 1 + k 2` と2回再開する。2回目の再開は操作が戻った後に `FileIn` を開き、同じ handler の
-- `never` の操作で中断する。着地の印の所で止まり、`FileIn` は閉じる。
import FileIn

effect Pick where
  multi pick : Unit -> Int
  never stop : Int -> a

body : Unit -> <Pick, IO> Int
body () =
  let n = pick ()
  println "picked \{n}"
  if n == 1 then n else
    let f = FileIn.open "input.txt"
    let r = stop 100
    FileIn.close f
    r

main : Unit -> <IO, Panic> Unit
main () =
  let r = handle body () with
    | ctl pick () k -> k 1 + k 2
    | ctl stop v -> v
    | return x -> x
  println (show r)
  println "after"
