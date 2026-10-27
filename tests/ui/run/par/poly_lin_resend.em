-- `lin` ビットのない入れ物が `Lin` の `MutArray` を持ち、入れ子の `par` で2回送られる。`(pick 2) arr` は VM が
-- 部分適用で作る閉包で、元が即値の関数なので `lin` ビットを持たない (docs/spec/object-model.md の「クロージャ」)。
-- `outer` と `inner` の中の子の閉包も、引数の関数の Kind が決まらないので `poly` の印で作る。外の fork は閉包に
-- 共有の印を付け、中の `MutArray` を子へ移す。内の fork では、閉包は rc == -1 で届き、検査モードは中の `MutArray`
-- を2つ目の受け手の持ち物にする (docs/spec/object-model.md の「`share`」)。子が書いて返した `MutArray` を親がまた書く。
import Std.Par
import Std.MutArray
import Std.Array

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

put : MutArray Int -> Unit -> <Panic> MutArray Int
put arr u = MutArray.write arr 0 7

pick : Int -> <Panic> (MutArray Int -> Unit -> <Panic> MutArray Int)
pick n = if n == 0 then put else pick (n - 1)

inner : (Unit -> <Panic> MutArray Int) -> <Panic> MutArray Int
inner g =
  let (j, arr) = Par.par (fn () -> spin 40) (fn () -> g ())
  MutArray.write arr 1 (j + 2)

outer : (Unit -> <Panic> MutArray Int) -> <Panic> (Int, MutArray Int)
outer g = Par.par (fn () -> spin 40 + 1) (fn () -> inner g)

main : Unit -> <IO, Panic> Unit
main () =
  let g = (pick 2) (MutArray.make 3 0)
  let (i, arr) = outer g
  let arr = MutArray.write arr 2 (i + 10)
  let b = MutArray.freeze arr
  println (show (Array.get b 0, Array.get b 1, Array.get b 2))
