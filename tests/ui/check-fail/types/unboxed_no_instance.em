-- `Unboxed` の instance は `Float` と `Int` だけなので、`String` の要素の `UnboxedArray.make` は E2006
import UnboxedArray

main : Unit -> <IO, Panic> Unit
main () =
  let a = UnboxedArray.make 3 "x"
  println (show (UnboxedArray.length a))
