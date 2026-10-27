-- `Lin` の `MutArray` が子と親の間を移る (docs/spec/object-model.md の「`share`」と「`transfer_back`」)。左の子は
-- 親の worker ID で動くので、1つ目の左の子が捕まえて書いて使い切る `MutArray` と、2つ目の左の子が作って返す
-- `MutArray` は、親の持ち物のまま動かない。3つ目では、右の子が捕まえた `MutArray` が fork で右の子へ移り、右の子が
-- 書いて返すと join で親へ戻る。4つ目では、右の子が作って返した `MutArray` が join で親へ戻る。親は戻った
-- `MutArray` をその場で書く。所有者を移し損ねると、検査モードが `WrongOwner` か `LinShared` で断る。
import Std.Par
import Std.MutArray
import Std.Array

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  let m = MutArray.make 3 0
  let (frozen, n) = Par.par (fn () -> MutArray.freeze (MutArray.write m 0 5)) (fn () -> spin 40 + 3)
  println (show (Array.get frozen 0 + n))
  let (made, k) = Par.par (fn () -> MutArray.make 3 4) (fn () -> spin 40 + 3)
  let (x, made) = MutArray.read made 0
  println (show (x + k + Array.length (MutArray.freeze made)))
  let a = MutArray.make 2 0
  let (j, a) = Par.par (fn () -> spin 40 + 1) (fn () -> MutArray.write a 0 7)
  let a = MutArray.write a 1 (j + 8)
  let b = MutArray.freeze a
  println (show (Array.get b 0, Array.get b 1))
  let (k2, fresh) = Par.par (fn () -> spin 40 + 2) (fn () -> MutArray.make 2 6)
  let fresh = MutArray.write fresh 1 k2
  let c = MutArray.freeze fresh
  println (show (Array.get c 0, Array.get c 1))
