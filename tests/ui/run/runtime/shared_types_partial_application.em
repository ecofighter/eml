-- 長い `ident` の連鎖の部分適用。型の木の大きさが `ident` ごとに2倍になるので、型検査と Core IR が型の部分を
-- 共有しなければならない。
ident : a -> a
ident x = x

main : Unit -> <IO> Unit
main () =
  let g = ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident ident
  println (show_int (g 5))
