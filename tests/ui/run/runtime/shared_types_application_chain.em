-- `ident` を次の `ident` に続けて適用すると、木として書き下した型の大きさが適用ごとに2倍になる。型検査と
-- Core IR が型の部分を共有しなければ、指数の時間とメモリがかかる。
ident : a -> a
ident x = x

main : Unit -> <IO> Unit
main () =
  println (show (ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident 5))
