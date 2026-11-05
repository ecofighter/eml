-- 親が作った `UnboxedArray` を両方の子が読む。右の子の `thaw` は、共有された配列を写してから `fill` で書くので、
-- 親が join の後に読む配列は変わらない (docs/spec/vm.md の「`par` の fork と join」)。
import Float
import MutUnboxedArray
import Std.Par
import UnboxedArray

sum_range : UnboxedArray Float -> Int -> Int -> Float -> <Panic> Float
sum_range a i end acc = if i == end then acc else sum_range a (i + 1) end (acc + UnboxedArray.get a i)

main : Unit -> <IO, Panic> Unit
main () =
  let a = UnboxedArray.tabulate 1000 Float.of_int
  let (l, r) =
    Par.par
      (fn () -> sum_range a 0 500 0.0)
      (fn () ->
        let ones = MutUnboxedArray.freeze (MutUnboxedArray.fill (UnboxedArray.thaw a) 1.0)
        sum_range a 500 1000 0.0 + sum_range ones 0 1000 0.0)
  println (show (l, r, UnboxedArray.get a 999))
