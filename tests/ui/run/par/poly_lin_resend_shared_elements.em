-- `poly` の組 `(String, MutArray String)` を入れ子の `par` で2回送る。`MutArray` の要素 0 は親も持つ文字列で、
-- `lin` の物体の下にある rc > 1 の物体である。外の fork の `share` がそれに印を付けるので、内の fork の送り直しの
-- 所有者だけのたどりは、印の付いた要素で止まる。`lin` の部分に残るのは rc 1 の物体だけである
-- (docs/spec/object-model.md の「`share`」)。内の子が要素を読んで書き、親が凍結して読む。
import Std.Par
import Std.MutArray
import Std.Array

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

touch : (String, MutArray String) -> <Panic> (String, MutArray String)
touch p =
  let (s, arr) = p
  let (x, arr) = MutArray.read arr 0
  let arr = MutArray.write arr 1 (x ++ s)
  (s, arr)

inner : (String, MutArray String) -> <Panic> (Int, (String, MutArray String))
inner p = Par.par (fn () -> spin 30) (fn () -> touch p)

outer : (String, MutArray String) -> <Panic> (Int, (Int, (String, MutArray String)))
outer p = Par.par (fn () -> spin 30) (fn () -> inner p)

main : Unit -> <IO, Panic> Unit
main () =
  let s = "ab" ++ show 12
  let arr = MutArray.make 2 s
  let arr = MutArray.write arr 1 (s ++ "z")
  let p = (s, arr)
  let (i, (j, (t, arr))) = outer p
  let b = MutArray.freeze arr
  println (show (i, j, t, Array.get b 0, Array.get b 1, s))
