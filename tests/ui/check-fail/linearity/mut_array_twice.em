-- `MutArray` は `Lin` なので、2 度使えない。
main : Unit -> <IO, Panic> Unit
main () =
  let m = MutArray.make 2 0
  let a = MutArray.freeze m
  let b = MutArray.freeze m
  println (show (Array.length a + Array.length b))
