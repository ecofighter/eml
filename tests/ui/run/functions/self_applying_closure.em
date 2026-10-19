-- 自分を引数に受けて呼ぶ閉包。融合の段は、書き換えで生まれた呼び出しを展開するたびに既知の閉包への `apply` をまた
-- 見つけるが、展開の回数の上限 (`MAX_INLINE_ROUNDS`) で止まる。止まらなければコンパイルが終わらない
data Fix = | Fix (Fix -> Int -> <Panic> Int)

fix_apply : Fix -> Int -> <Panic> Int
fix_apply (Fix f) x = f (Fix f) x

main : Unit -> <IO, Panic> Unit
main () =
  let lam = fn self n -> if n == 0 then 0 else fix_apply self (n - 1) + 2
  println (show (fix_apply (Fix lam) 10))
