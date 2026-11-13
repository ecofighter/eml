import MutUnboxedArray
import UnboxedArray

main : Unit -> <IO, Panic> Unit
main () =
  let a : UnboxedArray Int = UnboxedArray.make 3 1
  let a = UnboxedArray.set a 0 4
  println (show (UnboxedArray.get a 0 + UnboxedArray.length a))
  let f : UnboxedArray Float = UnboxedArray.make 2 0.5
  let f = UnboxedArray.set f 1 1.5
  println (show (UnboxedArray.get f 1))
  let m = UnboxedArray.thaw f
  let m = MutUnboxedArray.write m 0 2.5
  let (x, m) = MutUnboxedArray.read m 0
  let m = MutUnboxedArray.fill m x
  let (n, m) = MutUnboxedArray.length m
  println (show n)
  println (show (UnboxedArray.length (MutUnboxedArray.freeze m)))
  let i : MutUnboxedArray Int = MutUnboxedArray.make 2 3
  let i = MutUnboxedArray.write i 0 4
  let (y, i) = MutUnboxedArray.read i 0
  let i = MutUnboxedArray.fill i y
  let (k, i) = MutUnboxedArray.length i
  println (show k)
  let e : MutUnboxedArray Float = MutUnboxedArray.empty ()
  let fm : MutUnboxedArray Float = MutUnboxedArray.make 1 0.0
  let (z, fm) = MutUnboxedArray.read fm 0
  println (show z)
  println (show (UnboxedArray.length (MutUnboxedArray.freeze e)))
  println (show (UnboxedArray.length (MutUnboxedArray.freeze fm)))
  println (show (UnboxedArray.length (MutUnboxedArray.freeze i)))
