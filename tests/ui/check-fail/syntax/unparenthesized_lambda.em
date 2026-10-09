-- ラムダと `drop` は、引数に使うとき括弧で囲む必要がある。どちらも E0012 を1回ずつ報告する。
each : Int -> (Int -> <e> Unit) -> <e> Unit
each n f =
  if n > 0 then
    f n
    each (n - 1) f

after : Unit -> Int
after () = 1

effect Ask where
  ask : Unit -> Int

main : Unit -> <IO> Unit
main () =
  each 3 fn n ->
    println (show n)
  let v = handle ask () + 1 with
    | ask () k -> after drop k
  println (show v)
