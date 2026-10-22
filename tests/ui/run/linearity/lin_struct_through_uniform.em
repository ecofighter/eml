-- `File` の葉を持つ構造体を一様な位置 (`_`) に通して戻す。`pass` の多相再帰は `pass@[_]` で、直接の呼び出しでも、
-- 関数の値の一様な `apply` でも、box の挿入が構造体を箱に入れる。戻ると構造体に戻る。箱は `lin` のビットを持たないが、
-- `File` はそれぞれちょうど1回閉じ、検査モードでリークもない (docs/spec/lir.md の「構造体の形」)。
data Box a = | Box a

pass : Int -> a -> <Panic> a
pass n x = if n == 0 then x else
  let (y, _) = pass (n - 1) (x, n)
  y

hide : Int -> a -> <Panic> Box a
hide n x = if n <= 0 then Box x else hide (n - 1) x

main : Unit -> <IO, Panic> Unit
main () =
  let (f, k) = pass 3 (Fs.open "input.txt", 7)
  let (text, f) = Fs.read_all f
  Fs.close f
  println (text ++ " " ++ show k)
  let Box again = hide 0 (pass 2)
  let (g, m) = again (Fs.open "input.txt", 8)
  let (text, g) = Fs.read_all g
  Fs.close g
  println (text ++ " " ++ show m)
