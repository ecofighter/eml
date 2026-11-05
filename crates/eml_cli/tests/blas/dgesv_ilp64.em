-- Accelerate の ILP64 版の LAPACK `dgesv` で、A = [[2,1],[1,3]] (列優先)、b = [3,5] の連立方程式を解く。
-- Fortran の引数はすべてポインタで渡すので、スカラーは長さ 1 の配列にする。ILP64 の整数は 64 bit の `Int` である。
link on macos framework "Accelerate"

import Float
import MutUnboxedArray
import UnboxedArray

extern "C" symbol "dgesv$NEWLAPACK$ILP64" dgesv : UnboxedArray Int -> UnboxedArray Int -> MutUnboxedArray Float -> UnboxedArray Int -> MutUnboxedArray Int -> MutUnboxedArray Float -> UnboxedArray Int -> MutUnboxedArray Int -> <IO> (Unit, MutUnboxedArray Float, MutUnboxedArray Int, MutUnboxedArray Float, MutUnboxedArray Int)

main : Unit -> <IO, Panic> Unit
main () =
  let n = UnboxedArray.from_list [2]
  let nrhs = UnboxedArray.from_list [1]
  let ldab = UnboxedArray.from_list [2]
  let a = UnboxedArray.from_list [2.0, 1.0, 1.0, 3.0]
  let b = UnboxedArray.from_list [3.0, 5.0]
  let (_, factors, pivots, solution, info) = dgesv n nrhs (UnboxedArray.thaw a) ldab (MutUnboxedArray.make 2 0) (UnboxedArray.thaw b) ldab (MutUnboxedArray.make 1 0)
  -- LU 分解の結果と pivot は使わないが、`Lin` の値なので `freeze` で凍らせて捨てる
  let _ = MutUnboxedArray.freeze factors
  let _ = MutUnboxedArray.freeze pivots
  let x = MutUnboxedArray.freeze solution
  println (show (Float.round (UnboxedArray.get x 0 * 10.0)))
  println (show (Float.round (UnboxedArray.get x 1 * 10.0)))
  println (show (UnboxedArray.get (MutUnboxedArray.freeze info) 0))
