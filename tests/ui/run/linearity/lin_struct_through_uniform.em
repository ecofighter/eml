-- `FileIn` の葉を持つ構造体を一様な位置 (`_`) に通して戻す。`pass` の多相再帰は `pass@[_]` で、直接の呼び出しでも、
-- 関数の値の一様な `apply` でも、box の挿入が構造体を箱に入れる。戻ると構造体に戻る。箱は `lin` のビットを持たないが、
-- `FileIn` はそれぞれちょうど1回閉じ、検査モードでリークもない (docs/spec/lir.md の「構造体の形」)。
import FileIn

data Box a = | Box a

pass : Int -> a -> <Panic> a
pass n x = if n == 0 then x else
  let (y, _) = pass (n - 1) (x, n)
  y

hide : Int -> a -> <Panic> Box a
hide n x = if n <= 0 then Box x else hide (n - 1) x

main : Unit -> <IO, Panic> Unit
main () =
  let (f, k) = pass 3 (FileIn.open "input.txt", 7)
  let (text, f) = FileIn.read_all f
  FileIn.close f
  println (text ++ " " ++ show k)
  let Box again = hide 0 (pass 2)
  let (g, m) = again (FileIn.open "input.txt", 8)
  let (text, g) = FileIn.read_all g
  FileIn.close g
  println (text ++ " " ++ show m)
