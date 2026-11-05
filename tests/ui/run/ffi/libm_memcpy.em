-- `MutUnboxedArray` の引数は C が書く配列で、C の関数はそれを借りて組で返す。`UnboxedArray` は C が読むだけの配列である。
link on linux "libm.so.6"
link on macos "m"
link on windows "ucrtbase"

import MutUnboxedArray
import UnboxedArray

extern "C" memcpy : MutUnboxedArray Float -> UnboxedArray Float -> Int as size_t -> <IO, Panic> (Int, MutUnboxedArray Float)
extern "C" pure memcmp : UnboxedArray Float -> UnboxedArray Float -> Int as size_t -> <Panic> Int as int

main : Unit -> <IO, Panic> Unit
main () =
  let source = UnboxedArray.from_list [1.5, 2.5, 3.5]
  let (_, target) = memcpy (MutUnboxedArray.make 3 0.0) source 24
  let copied = MutUnboxedArray.freeze target
  println (show (UnboxedArray.to_list copied))
  println (show (memcmp copied source 24))
