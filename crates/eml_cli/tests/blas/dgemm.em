-- CBLAS の `cblas_dgemm` で、行優先の 2x2 の行列 [1,2,3,4] と [5,6,7,8] の積を求める。
-- `pure` の呼び出しでも、`MutUnboxedArray` の引数を持つ行は、結果の `Unit` を捨てても消えない。
link on linux "openblas"
link on macos framework "Accelerate"
link on windows "mkl_rt"

import Float
import MutUnboxedArray
import UnboxedArray

extern "C" pure symbol "cblas_dgemm" dgemm : Int as int -> Int as int -> Int as int -> Int as int -> Int as int -> Int as int -> Float -> UnboxedArray Float -> Int as int -> UnboxedArray Float -> Int as int -> Float -> MutUnboxedArray Float -> Int as int -> <Panic> (Unit, MutUnboxedArray Float)

row_major : Int
row_major = 101

no_trans : Int
no_trans = 111

main : Unit -> <IO, Panic> Unit
main () =
  let a = UnboxedArray.from_list [1.0, 2.0, 3.0, 4.0]
  let b = UnboxedArray.from_list [5.0, 6.0, 7.0, 8.0]
  let (_, c) = dgemm row_major no_trans no_trans 2 2 2 1.0 a 2 b 2 0.0 (MutUnboxedArray.make 4 0.0) 2
  let product = MutUnboxedArray.freeze c
  let show_at i = println (show (Float.round (UnboxedArray.get product i)))
  show_at 0
  show_at 1
  show_at 2
  show_at 3
